use crate::store::{self, Hold, Slot};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Key(blake3::Hasher);

impl Key {
    #[must_use]
    pub fn new() -> Self {
        Self(blake3::Hasher::new())
    }

    pub fn feed(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.update(&(bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
        self
    }

    pub fn feed_list<I>(&mut self, items: I) -> &mut Self
    where
        I: IntoIterator,
        I::Item: AsRef<[u8]>,
        I::IntoIter: ExactSizeIterator,
    {
        let items = items.into_iter();
        self.0.update(&(items.len() as u64).to_le_bytes());
        for item in items {
            self.feed(item.as_ref());
        }
        self
    }

    pub fn feed_str(&mut self, text: &str) -> &mut Self {
        self.feed(text.as_bytes())
    }

    #[must_use]
    pub fn digest(&self) -> String {
        self.0.finalize().to_hex()[..32].to_string()
    }

    #[must_use]
    pub fn full_digest(&self) -> String {
        self.0.finalize().to_hex().to_string()
    }
}

impl Default for Key {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Unit,
    Test,
    Script,
    ScriptBin,
}

impl Kind {
    pub(crate) fn prefix(self) -> &'static str {
        match self {
            Self::Unit => "u-",
            Self::Test => "test-",
            Self::Script => "script-",
            Self::ScriptBin => "scriptbin-",
        }
    }
}

pub struct Action {
    pub name: String,
    pub slot: Slot,
    pub out: PathBuf,
    home: PathBuf,
    lineage: Option<String>,
    lease: Arc<std::fs::File>,
    writing: std::cell::Cell<bool>,
    _hold: Hold,
}

impl Action {
    pub fn begin(home: &Path, kind: Kind, key: &str) -> Result<Self> {
        let name = format!("{}{key}", kind.prefix());
        let slot = Slot::new(home, &name);
        let hold = store::hold(home, &name)?;
        let lease = store::lease(home, &name)?;
        let out = slot.out_dir();
        Ok(Self {
            name,
            slot,
            out,
            home: home.to_path_buf(),
            lineage: None,
            lease,
            writing: std::cell::Cell::new(false),
            _hold: hold,
        })
    }

    #[must_use]
    pub fn lineage(mut self, lineage: Option<String>) -> Self {
        self.lineage = lineage;
        self
    }

    #[must_use]
    pub fn hit(&self) -> bool {
        self.slot.hit()
    }

    pub(crate) fn prepare(&self) -> Result<()> {
        if self.writing.get() {
            return Ok(());
        }
        match self.lease.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(
                    crate::cargo::Unmodeled(format!("unit {} is in use", self.name)).into(),
                );
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
        self.writing.set(true);
        if !store::has_room(&self.slot.dir) {
            return Err(crate::cargo::Unmodeled("insufficient cache disk space".into()).into());
        }
        Ok(())
    }

    pub(crate) fn lease(&self) -> Result<Arc<std::fs::File>> {
        if self.writing.replace(false) {
            self.lease.unlock()?;
        }
        self.lease.lock_shared()?;
        Ok(Arc::clone(&self.lease))
    }

    pub fn finish(&self) -> Result<()> {
        let Some(lineage) = &self.lineage else {
            return self.slot.mark();
        };
        if let Err(error) = store::label(&self.slot, lineage) {
            crate::out::err(format!(
                "artificer: could not label {}: {error:#}",
                self.name
            ));
        }
        self.slot.mark()?;
        store::adopt(&self.home, lineage, &self.name);
        Ok(())
    }

    pub(crate) fn invalidate(&self) -> Result<()> {
        self.prepare()?;
        if self.slot.dir.exists() {
            std::fs::remove_dir_all(&self.slot.dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
