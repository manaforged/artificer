use super::*;

pub(crate) fn discard(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let trash = crate::platform::temp_sibling(dir);
    fs::rename(dir, &trash)?;
    fs::remove_dir_all(&trash)?;
    Ok(())
}

pub(super) fn collect_stale(home: &Path, root: &Path) -> Result<u64> {
    let mut bytes = 0;
    if !root.is_dir() {
        return Ok(bytes);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let Some((name, pid)) = crate::platform::temp_owner(&file_name) else {
            continue;
        };
        if crate::platform::alive(pid) {
            continue;
        }
        let Some(_hold) = try_hold(home, name)? else {
            continue;
        };
        let Some(_lease) = try_write(home, name)? else {
            continue;
        };
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            bytes += size(&path)?;
            fs::remove_dir_all(&path)?;
        } else {
            bytes += entry.metadata()?.len();
            fs::remove_file(&path)?;
        }
    }
    Ok(bytes)
}
