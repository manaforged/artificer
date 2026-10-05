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
    let mut key = crate::action::Key::new();
    key.feed(dir.as_os_str().as_encoded_bytes());
    let hold = crate::store::hold(home, &format!("compile-{}", key.digest()))?;
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
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
        } else if fs::hard_link(&source, &target).is_err() {
            fs::copy(&source, &target)
                .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
        }
    }
    Ok(())
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
