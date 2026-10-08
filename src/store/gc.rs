use super::*;

pub const AGE: Duration = Duration::from_secs(30 * 24 * 3600);

pub const CAP: u64 = 8 << 30;

pub const CAP_SHARE: u64 = 15;

pub const HEADROOM_SHARE: u64 = 10;

pub const FREE_SHARE: u64 = 5;

pub const FREE_FLOOR: u64 = 8 << 30;

pub fn has_room(home: &Path) -> bool {
    let Some((total, free)) = crate::volume::capacity(home) else {
        return true;
    };
    free > std::cmp::max(total / 100 * FREE_SHARE, FREE_FLOOR)
}

pub fn gc_cap(home: &Path, cap: u64) -> Result<(u32, u64)> {
    let root = home.join("units").join(LAYOUT);
    let mut units: Vec<(std::time::SystemTime, u64, PathBuf, String)> = Vec::new();
    if root.is_dir() {
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with('.') || !entry.file_type()?.is_dir()
            {
                continue;
            }
            let dir = entry.path();
            let Ok(meta) = fs::metadata(dir.join("ok")).or_else(|_| fs::metadata(&dir)) else {
                continue;
            };
            let Ok(mtime) = meta.modified() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            units.push((mtime, size(&dir)?, dir, name));
        }
    }
    let mut freed = super::discard::collect_stale(home, &root)?;
    let mut total = usage(home)?;
    if total <= cap {
        return Ok((0, freed));
    }
    let target = cap.saturating_sub(cap / 100 * HEADROOM_SHARE);
    units.sort_by_key(|u| u.0);
    let mut gone = 0;
    for (_, bytes, dir, name) in units {
        if total <= target {
            break;
        }
        let Some(_hold) = try_hold(home, &name)? else {
            continue;
        };
        let Some(_lease) = try_write(home, &name)? else {
            continue;
        };
        if !dir.is_dir() {
            continue;
        }
        super::discard::discard(&dir)?;
        total = total.saturating_sub(bytes);
        freed += bytes;
        gone += 1;
    }
    Ok((gone, freed))
}

pub(super) fn aged(meta: &fs::Metadata, max_age: Duration) -> bool {
    meta.modified()
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d > max_age)
        .unwrap_or(false)
}

pub fn gc_units(home: &Path, max_age: Duration) -> Result<(u32, u64)> {
    let root = home.join("units").join(LAYOUT);
    let (mut gone, mut bytes) = super::layouts::purge_stale(home, max_age)?;
    bytes += super::discard::collect_stale(home, &root)?;
    if root.is_dir() {
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with('.') || !entry.file_type()?.is_dir()
            {
                continue;
            }
            let dir = entry.path();
            let meta = match fs::metadata(dir.join("ok")).or_else(|_| fs::metadata(&dir)) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !aged(&meta, max_age) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(_hold) = try_hold(home, &name)? else {
                continue;
            };
            let Some(_lease) = try_write(home, &name)? else {
                continue;
            };
            let Ok(meta) = fs::metadata(dir.join("ok")).or_else(|_| fs::metadata(&dir)) else {
                continue;
            };
            if !aged(&meta, max_age) {
                continue;
            }
            bytes += size(&dir)?;
            super::discard::discard(&dir)?;
            gone += 1;
        }
    }
    let cache = home.join("cargo-meta").join("cache");
    if cache.is_dir() {
        for entry in fs::read_dir(&cache)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let meta = entry.metadata()?;
            if aged(&meta, max_age) {
                bytes += meta.len();
                fs::remove_file(entry.path())?;
            }
        }
    }
    let keys = home.join("keys");
    if keys.is_dir() {
        for entry in fs::read_dir(&keys)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let meta = entry.metadata()?;
            if aged(&meta, max_age) {
                bytes += meta.len();
                fs::remove_file(entry.path())?;
            }
        }
    }
    bytes += super::lineage::gc_pointers(home, max_age)?;
    let runs = home.join("rustc-runs");
    if let Ok(meta) = fs::metadata(&runs)
        && meta.is_file()
        && aged(&meta, max_age)
    {
        bytes += meta.len();
        fs::remove_file(&runs)?;
    }
    Ok((gone, bytes))
}

pub fn usage(home: &Path) -> Result<u64> {
    let root = home.join("units").join(LAYOUT);
    if !root.is_dir() {
        return Ok(0);
    }
    size(&root)
}

pub(crate) fn size(dir: &Path) -> Result<u64> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = 0;
    for entry in entries {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_dir() {
            bytes += size(&entry.path())?;
        } else if ft.is_file() {
            bytes += entry.metadata().map_or(0, |meta| meta.len());
        }
    }
    Ok(bytes)
}
