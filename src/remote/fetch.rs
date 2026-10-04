use super::{Location, configured};
use crate::store::{self, LAYOUT, Slot};
use anyhow::Result;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const RSYNC: &str = "rsync";
const STAGING: &str = "fetch-staging";
const RSYNC_PARTIAL: i32 = 23;

static UNREACHABLE: AtomicBool = AtomicBool::new(false);
static NEXT: AtomicU64 = AtomicU64::new(0);

pub(crate) fn fetch(home: &Path, name: &str, slot: &Slot) -> Result<bool> {
    if UNREACHABLE.load(Ordering::Relaxed) {
        return Ok(false);
    }
    let Some((location, _)) = configured(home)? else {
        return Ok(false);
    };
    crate::profile::span(crate::profile::UnitPhase::Fetch, || match &location {
        Location::Dir(dir) => from_dir(home, dir, name, slot),
        Location::Ssh { host, path } => from_ssh(home, host, path, name, slot),
    })
}

fn from_dir(home: &Path, dir: &Path, name: &str, slot: &Slot) -> Result<bool> {
    if crate::resolve_path(dir) == crate::resolve_path(home) {
        return Ok(false);
    }
    let _hold = store::hold(dir, name)?;
    let from = Slot::new(dir, name);
    if !from.hit() {
        return Ok(false);
    }
    slot.copy_from(&from.dir)?;
    Ok(slot.hit())
}

fn from_ssh(home: &Path, host: &str, path: &str, name: &str, slot: &Slot) -> Result<bool> {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let staging = home
        .join(STAGING)
        .join(format!("{name}-{}-{n}", std::process::id()));
    fs::create_dir_all(&staging)?;
    let source = format!("{host}:{path}/units/{LAYOUT}/{name}/");
    let status = Command::new(RSYNC)
        .args(["-a", "-z", "-e", &ssh_command()])
        .arg(&source)
        .arg(format!("{}/", staging.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let complete = match status {
        Ok(status) if status.success() => staging.join("ok").is_file(),
        Ok(status) if status.code() == Some(RSYNC_PARTIAL) => false,
        _ => {
            UNREACHABLE.store(true, Ordering::Relaxed);
            false
        }
    };
    let fetched = complete && slot.copy_from(&staging).is_ok() && slot.hit();
    drop(fs::remove_dir_all(&staging));
    Ok(fetched)
}

fn ssh_command() -> String {
    let control = crate::home::control_home().join("ssh");
    drop(fs::create_dir_all(&control));
    format!(
        "ssh -o BatchMode=yes -o ConnectTimeout=5 -o ControlMaster=auto -o ControlPersist=60 -o ControlPath={}/%C",
        control.display()
    )
}
