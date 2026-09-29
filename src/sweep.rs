use anyhow::Result;
use std::fs;
use std::path::Path;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub incremental_dirs: u32,
    pub scratch_dirs: u32,
    pub evicted_units: u32,
    pub evicted_bytes: u64,
}

pub(crate) fn workspace(target_dir: &Path, home: &Path, full: bool) -> Result<Report> {
    let mut report = Report::default();
    let root = crate::resolve_path(target_dir);
    let home = crate::resolve_path(home);
    let mut profiles = vec![root.clone(), root.join("debug"), root.join("release")];
    if let Ok(subs) = fs::read_dir(&root) {
        for sub in subs {
            let sub = sub?;
            if sub.file_type()?.is_dir() {
                profiles.push(sub.path().join("debug"));
                profiles.push(sub.path().join("release"));
            }
        }
    }
    let names: &[&str] = if full {
        &["incremental", "deps", ".fingerprint", "build"]
    } else {
        &["incremental"]
    };
    for profile in profiles {
        for name in names {
            let path = profile.join(name);
            if !path.is_dir() {
                continue;
            }
            let resolved = crate::resolve_path(&path);
            if !resolved.starts_with(&root)
                || resolved.starts_with(&home)
                || home.starts_with(&resolved)
            {
                continue;
            }
            fs::remove_dir_all(path)?;
            report.incremental_dirs += 1;
        }
    }
    report.scratch_dirs = gc_scratch(&home)?;
    Ok(report)
}

pub(crate) fn gc_scratch(home: &Path) -> Result<u32> {
    let scratch = home.join("scratch");
    let units = home.join("units").join(crate::store::LAYOUT);
    if !scratch.is_dir() || !units.is_dir() {
        return Ok(0);
    }
    let mut n = 0;
    for entry in fs::read_dir(&scratch)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        if units.join(&name).join("ok").is_file() {
            let name = name.to_string_lossy();
            let Some(_hold) = crate::store::try_hold(home, &name)? else {
                continue;
            };
            let Some(_lease) = crate::store::try_write(home, &name)? else {
                continue;
            };
            fs::remove_dir_all(entry.path())?;
            n += 1;
        }
    }
    Ok(n)
}
