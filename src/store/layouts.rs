use super::*;

pub(super) fn purge_stale(home: &Path, max_age: Duration) -> Result<(u32, u64)> {
    let units = home.join("units");
    let mut gone = 0;
    let mut bytes = 0;
    let Ok(entries) = fs::read_dir(&units) else {
        return Ok((0, 0));
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name() == LAYOUT {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            let (count, size) = purge_layout(&path, max_age)?;
            gone += count;
            bytes += size;
        } else if aged(&entry.metadata()?, max_age) {
            bytes += entry.metadata()?.len();
            fs::remove_file(&path)?;
        }
    }
    Ok((gone, bytes))
}

fn purge_layout(layout: &Path, max_age: Duration) -> Result<(u32, u64)> {
    let mut gone = 0;
    let mut bytes = 0;
    for entry in fs::read_dir(layout)? {
        let entry = entry?;
        let path = entry.path();
        let Ok(meta) = fs::metadata(path.join("ok")).or_else(|_| entry.metadata()) else {
            continue;
        };
        if !aged(&meta, max_age) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            bytes += size(&path)?;
            fs::remove_dir_all(&path)?;
        } else {
            bytes += meta.len();
            fs::remove_file(&path)?;
        }
        gone += 1;
    }
    drop(fs::remove_dir(layout));
    Ok((gone, bytes))
}
