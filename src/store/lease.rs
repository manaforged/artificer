use super::*;
use std::sync::Arc;

pub(crate) fn lease(home: &Path, name: &str) -> Result<Arc<fs::File>> {
    Ok(Arc::new(super::locks::lock_file(home, "leases", name)?))
}

pub(crate) fn try_write(home: &Path, name: &str) -> Result<Option<Arc<fs::File>>> {
    let file = lease(home, name)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(error)) => Err(error.into()),
    }
}
