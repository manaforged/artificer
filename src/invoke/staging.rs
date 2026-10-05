use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SCRATCH: [&str; 2] = [".rcgu.o", ".rcgu.dwo"];

pub(crate) struct Staged {
    dir: PathBuf,
    _hold: crate::store::Hold,
}

pub(crate) fn stage(home: &Path, cmd: &Command, out: &Path) -> Result<Option<Staged>> {
    let Some(dir) = out_dir(cmd).filter(|dir| dir.as_path() != out) else {
        return Ok(None);
    };
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let hold = crate::store::hold(home, &claim_name(&dir))?;
    for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        if scratch(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        let cleared = if entry.file_type()?.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        cleared.with_context(|| format!("clear {}", path.display()))?;
    }
    Ok(Some(Staged { dir, _hold: hold }))
}

impl Staged {
    pub(crate) fn publish(&self, out: &Path) -> Result<()> {
        link_tree(&self.dir, out)
    }
}

fn link_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in fs::read_dir(from).with_context(|| format!("read {}", from.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        if scratch(&name) {
            continue;
        }
        let (source, target) = (entry.path(), to.join(&name));
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&target).with_context(|| format!("create {}", target.display()))?;
            link_tree(&source, &target)?;
        } else {
            link_file(&source, &target)?;
        }
    }
    Ok(())
}

pub(crate) fn publish_file(source: &Path, target: &Path) -> Result<()> {
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    link_file(source, target)
}

fn link_file(source: &Path, target: &Path) -> Result<()> {
    crate::platform::replace_atomic(target, |tmp| {
        fs::hard_link(source, tmp).or_else(|_| fs::copy(source, tmp).map(drop))
    })
    .with_context(|| format!("publish {} to {}", source.display(), target.display()))
}

pub(crate) fn try_claim(home: &Path, dir: &Path) -> Result<Option<crate::store::Hold>> {
    crate::store::try_hold(home, &claim_name(dir))
}

fn claim_name(dir: &Path) -> String {
    let mut key = crate::action::Key::new();
    key.feed(crate::resolve_path(dir).as_os_str().as_encoded_bytes());
    format!("compile-{}", key.digest())
}

fn scratch(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    SCRATCH.iter().any(|suffix| name.ends_with(suffix))
}

fn out_dir(cmd: &Command) -> Option<PathBuf> {
    let mut args = cmd.get_args();
    while let Some(arg) = args.next() {
        if arg == "--out-dir" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}
