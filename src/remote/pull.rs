use super::{Location, PULL_STAMP, configured};
use crate::store;
use crate::transfer::{TransferReport, import};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

const RSYNC: &str = "rsync";
const PULL_LOCK: &str = "pull.lock";
const PULL_LOG: &str = "pull.log";
const STAGING: &str = "pull-staging";
const EXCLUDES: &str = "pull.exclude";
const PULL_COMMAND: &str = "pull";
const HOME_ENV: &str = "ARTIFICER_HOME";

pub fn pull(home: &Path) -> Result<(Location, TransferReport)> {
    crate::home::claim(home)?;
    let Some((location, _)) = configured(home)? else {
        bail!("no remote is set; run `artificer remote set LOCATION`");
    };
    let Some(_lock) = store::try_hold(home, PULL_LOCK)? else {
        bail!("another pull is running for {}", home.display());
    };
    let report = match &location {
        Location::Dir(dir) => import(home, dir)?,
        Location::Ssh { host, path } => pull_ssh(home, host, path)?,
    };
    fs::write(home.join(PULL_STAMP), "")?;
    Ok((location, report))
}

fn pull_ssh(home: &Path, host: &str, path: &str) -> Result<TransferReport> {
    if cfg!(windows) {
        bail!(
            "pulling from {host}:{path} over ssh is not supported on Windows; set a mounted directory as the remote"
        );
    }
    let staging = home.join(STAGING);
    let dest = staging.join("units").join(store::LAYOUT);
    fs::create_dir_all(&dest).with_context(|| format!("create {}", dest.display()))?;
    let excludes = home.join(EXCLUDES);
    fs::write(&excludes, local_units(home)?)
        .with_context(|| format!("write {}", excludes.display()))?;
    let source = format!("{host}:{path}/units/{}/", store::LAYOUT);
    let output = Command::new(RSYNC)
        .arg("-a")
        .arg("-z")
        .arg("--exclude-from")
        .arg(&excludes)
        .arg(&source)
        .arg(format!("{}/", dest.display()))
        .stdin(Stdio::null())
        .output();
    drop(fs::remove_file(&excludes));
    let output = match output {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!(
                "`{RSYNC}` is not installed or not on PATH; pull from {source} needs rsync and ssh"
            )
        }
        Err(error) => return Err(error).with_context(|| format!("run `{RSYNC}`")),
    };
    if !output.status.success() {
        drop(fs::remove_dir_all(&staging));
        bail!(
            "`{RSYNC} -a -z {source}` failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let report = import(home, &staging)?;
    fs::remove_dir_all(&staging).with_context(|| format!("remove {}", staging.display()))?;
    Ok(report)
}

fn local_units(home: &Path) -> Result<String> {
    let root = home.join("units").join(store::LAYOUT);
    let mut list = String::new();
    if !root.is_dir() {
        return Ok(list);
    }
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join("ok").is_file() {
            list.push_str(&format!("/{}/\n", entry.file_name().to_string_lossy()));
        }
    }
    Ok(list)
}

pub fn spawn_pull(home: &Path) -> Result<()> {
    let log = home.join(PULL_LOG);
    let log = fs::File::create(&log).with_context(|| format!("create {}", log.display()))?;
    let exe = crate::install::self_launcher().context("current_exe")?;
    let mut cmd = Command::new(exe);
    cmd.arg(PULL_COMMAND);
    cmd.env(HOME_ENV, home);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::from(log.try_clone()?));
    cmd.stderr(Stdio::from(log));
    detach(&mut cmd);
    cmd.spawn().context("spawn artificer pull")?;
    Ok(())
}

#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(windows)]
fn detach(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(windows_sys::Win32::System::Threading::DETACHED_PROCESS);
}
