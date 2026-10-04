use super::{Location, configured};
use crate::store::{self, LAYOUT, Slot};
use crate::transfer::{TransferReport, complete_units, import_units};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

const RSYNC: &str = "rsync";
const STAGING: &str = "push-staging";
const INBOX: &str = "inbox";
const PUSH_LOG: &str = "push.log";
const PUSH_COMMAND: &str = "push";
const UNITS_FLAG: &str = "--units";
const HOME_ENV: &str = "ARTIFICER_HOME";

static BUILT: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());
static SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) fn note_built(home: &Path, name: &str) {
    BUILT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((home.to_path_buf(), name.to_string()));
}

pub(crate) fn push_built(home: &Path) {
    let names: Vec<String> = {
        let home = crate::resolve_path(home);
        let mut built = BUILT.lock().unwrap_or_else(PoisonError::into_inner);
        let (mine, rest) = built
            .drain(..)
            .partition(|(owner, _)| crate::resolve_path(owner) == home);
        *built = rest;
        mine.into_iter().map(|(_, name)| name).collect::<Vec<_>>()
    };
    if names.is_empty() || !matches!(configured(home), Ok(Some(_))) {
        return;
    }
    if let Err(error) = spawn_push(home, &names) {
        crate::out::err(format!("artificer: background push skipped: {error:#}"));
    }
}

fn batch() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

fn spawn_push(home: &Path, names: &[String]) -> Result<()> {
    let dir = home.join(STAGING);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let list = dir.join(format!("{}.list", batch()));
    fs::write(&list, names.join("\n")).with_context(|| format!("write {}", list.display()))?;
    let log = fs::File::create(home.join(PUSH_LOG))?;
    let exe = crate::install::self_launcher().context("current_exe")?;
    let mut cmd = Command::new(exe);
    cmd.arg(PUSH_COMMAND)
        .arg(UNITS_FLAG)
        .arg(&list)
        .env(HOME_ENV, home)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    super::detach(&mut cmd);
    cmd.spawn().context("spawn artificer push")?;
    Ok(())
}

pub fn push(home: &Path, units: Option<&Path>) -> Result<(Location, TransferReport)> {
    crate::home::claim(home)?;
    let Some((location, _)) = configured(home)? else {
        bail!("no remote is set; run `artificer remote set LOCATION`");
    };
    let names = match units {
        Some(list) => fs::read_to_string(list)
            .with_context(|| format!("read {}", list.display()))?
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
        None => complete_units(home)?,
    };
    let report = match &location {
        Location::Dir(dir) => import_units(dir, home, &names)?,
        Location::Ssh { host, path } => push_ssh(home, host, path, &names)?,
    };
    if let Some(list) = units {
        drop(fs::remove_file(list));
    }
    Ok((location, report))
}

fn push_ssh(home: &Path, host: &str, path: &str, names: &[String]) -> Result<TransferReport> {
    let batch = batch();
    let staging = home.join(STAGING).join(&batch);
    let mut report = TransferReport { units: 0, bytes: 0 };
    for name in names {
        let slot = Slot::new(home, name);
        if !slot.hit() {
            continue;
        }
        Slot::new(&staging, name).copy_from(&slot.dir)?;
        report.units += 1;
        report.bytes += store::size(&slot.dir).unwrap_or(0);
    }
    if report.units > 0 {
        let sent = send(&staging, host, path, &batch);
        drop(fs::remove_dir_all(&staging));
        sent?;
    }
    Ok(report)
}

fn send(staging: &Path, host: &str, path: &str, batch: &str) -> Result<()> {
    let inbox = format!("{path}/{INBOX}");
    remote(host, &format!("mkdir -p {}", quote(&inbox)))?;
    let status = Command::new(RSYNC)
        .args(["-a", "-z", "-e", &super::ssh_command()])
        .arg(format!("{}/units/{LAYOUT}", staging.display()))
        .arg(format!("{host}:{inbox}/{batch}/units/"))
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("run `{RSYNC}`"))?;
    if !status.success() {
        bail!("`{RSYNC}` to {host}:{inbox} failed ({status})");
    }
    let dir = format!("{inbox}/{batch}");
    remote(
        host,
        &format!(
            "mkdir -p {dir_units} && {HOME_ENV}={home} artificer import {dir}; rm -rf {dir}",
            dir_units = quote(&format!("{dir}/units")),
            home = quote(path),
            dir = quote(&dir),
        ),
    )
}

fn remote(host: &str, script: &str) -> Result<()> {
    let ssh = super::ssh_command();
    let mut parts = ssh.split_whitespace();
    let program = parts.next().context("ssh command")?;
    let status = Command::new(program)
        .args(parts)
        .arg(host)
        .arg(script)
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("run ssh {host}"))?;
    if !status.success() {
        bail!("ssh {host} `{script}` failed ({status})");
    }
    Ok(())
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
