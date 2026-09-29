use super::*;

pub(crate) fn lock_file(home: &Path, directory: &str, name: &str) -> Result<fs::File> {
    let dir = home.join(directory).join(LAYOUT);
    fs::create_dir_all(&dir)?;
    Ok(fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(name))?)
}

pub struct Hold {
    _file: fs::File,
}

pub fn hold(home: &Path, name: &str) -> Result<Hold> {
    let file = lock_file(home, "locks", name)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => {
            crate::out::err(format!("artificer: waiting on {name}"));
            file.lock()?;
        }
        Err(fs::TryLockError::Error(error)) => return Err(error.into()),
    }
    Ok(Hold { _file: file })
}

pub fn try_hold(home: &Path, name: &str) -> Result<Option<Hold>> {
    let file = lock_file(home, "locks", name)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Hold { _file: file })),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(error)) => Err(error.into()),
    }
}
