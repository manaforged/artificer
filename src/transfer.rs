use crate::store::{self, Slot};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

pub struct TransferReport {
    pub units: u64,
    pub bytes: u64,
}

pub fn export(home: &Path, dest: &Path, days: u64, max_bytes: u64) -> Result<TransferReport> {
    let root = home.join("units").join(store::LAYOUT);
    let mut report = TransferReport { units: 0, bytes: 0 };
    if !root.is_dir() {
        return Ok(report);
    }
    let cutoff = if days == 0 {
        SystemTime::UNIX_EPOCH
    } else {
        SystemTime::now()
            .checked_sub(Duration::from_secs(days.saturating_mul(24 * 3600)))
            .unwrap_or(SystemTime::UNIX_EPOCH)
    };
    let mut units = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let dir = entry.path();
        if !entry.file_type()?.is_dir() || !dir.join("ok").is_file() || !dir.join("out").is_dir() {
            continue;
        }
        let Ok(used) = fs::metadata(dir.join("ok")).and_then(|m| m.modified()) else {
            continue;
        };
        if used < cutoff {
            continue;
        }
        units.push((used, store::size(&dir).unwrap_or(0), dir));
    }
    units.sort_by_key(|(used, ..)| std::cmp::Reverse(*used));

    for (_, _, dir) in units {
        let name = dir
            .file_name()
            .context("unit directory has no name")?
            .to_string_lossy();
        let budget = (max_bytes > 0).then(|| max_bytes.saturating_sub(report.bytes));
        if let Some(bytes) = copy_unit(home, dest, &name, budget)? {
            report.units += 1;
            report.bytes += bytes;
        }
    }
    Ok(report)
}

pub fn import(home: &Path, src: &Path) -> Result<TransferReport> {
    crate::home::claim(home)?;
    let src_units = src.join("units").join(store::LAYOUT);
    let mut report = TransferReport { units: 0, bytes: 0 };
    if !src_units.is_dir() {
        return Ok(report);
    }
    for entry in fs::read_dir(&src_units)? {
        let entry = entry?;
        let dir = entry.path();
        if !entry.file_type()?.is_dir() || !dir.join("ok").is_file() || !dir.join("out").is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(bytes) = copy_unit(src, home, &name, None)? {
            if let Some(lineage) = store::labelled(&Slot::new(home, &name)) {
                store::adopt(home, &lineage, &name);
            }
            report.units += 1;
            report.bytes += bytes;
        }
    }
    Ok(report)
}

fn copy_unit(src: &Path, dest: &Path, name: &str, budget: Option<u64>) -> Result<Option<u64>> {
    let src = crate::resolve_path(src);
    let dest = crate::resolve_path(dest);
    if src == dest {
        return Ok(None);
    }
    let (first, second) = if src < dest {
        (&src, &dest)
    } else {
        (&dest, &src)
    };
    let _first = store::hold(first, name)?;
    let _second = store::hold(second, name)?;
    let from = Slot::new(&src, name);
    let to = Slot::new(&dest, name);
    if !from.hit() || to.hit() {
        return Ok(None);
    }
    let bytes = store::size(&from.dir)?;
    if budget.is_some_and(|budget| bytes > budget) {
        return Ok(None);
    }
    let Some(_lease) = store::try_write(&dest, name)? else {
        return Ok(None);
    };
    to.copy_from(&from.dir)?;
    Ok(to.hit().then_some(bytes))
}

#[cfg(test)]
#[path = "transfer_tests.rs"]
mod tests;
