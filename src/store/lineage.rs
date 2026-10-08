use super::*;

pub const POINTERS: &str = "lineage";

pub const LABEL: &str = "lineage";

pub(crate) fn label(slot: &Slot, lineage: &str) -> Result<()> {
    if slot.hit() {
        return Ok(());
    }
    fs::create_dir_all(&slot.dir)?;
    fs::write(slot.dir.join(LABEL), lineage)?;
    Ok(())
}

pub(crate) fn labelled(slot: &Slot) -> Option<String> {
    let text = fs::read_to_string(slot.dir.join(LABEL)).ok()?;
    let text = text.trim();
    plain(text).then(|| text.to_string())
}

fn plain(name: &str) -> bool {
    !name.is_empty() && Path::new(name).file_name() == Some(std::ffi::OsStr::new(name))
}

pub(crate) fn adopt(home: &Path, lineage: &str, unit_name: &str) {
    if let Err(error) = supersede(home, lineage, unit_name) {
        crate::out::err(format!(
            "artificer: could not evict the unit {unit_name} replaces: {error:#}"
        ));
    }
}

pub(crate) fn supersede(home: &Path, lineage: &str, unit_name: &str) -> Result<()> {
    if !plain(lineage) || !plain(unit_name) {
        return Ok(());
    }
    let dir = home.join(POINTERS);
    fs::create_dir_all(&dir)?;
    let pointer = dir.join(lineage);
    let previous = fs::read_to_string(&pointer).unwrap_or_default();
    let previous = previous.trim();
    if previous == unit_name {
        return Ok(());
    }
    if plain(previous) {
        evict(home, previous)?;
    }
    crate::platform::replace_atomic(&pointer, |tmp| fs::write(tmp, unit_name))?;
    Ok(())
}

fn evict(home: &Path, name: &str) -> Result<()> {
    let Some(_hold) = try_hold(home, name)? else {
        return Ok(());
    };
    let Some(_lease) = try_write(home, name)? else {
        return Ok(());
    };
    super::discard::discard(&Slot::new(home, name).dir)
}

pub(super) fn gc_pointers(home: &Path, max_age: Duration) -> Result<u64> {
    let dir = home.join(POINTERS);
    let mut bytes = 0;
    if !dir.is_dir() {
        return Ok(bytes);
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let meta = entry.metadata()?;
        let unit = fs::read_to_string(entry.path()).unwrap_or_default();
        let unit = unit.trim();
        let live = plain(unit) && Slot::new(home, unit).dir.is_dir();
        if aged(&meta, max_age) || !live {
            bytes += meta.len();
            fs::remove_file(entry.path())?;
        }
    }
    Ok(bytes)
}
