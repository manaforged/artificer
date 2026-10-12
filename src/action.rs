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

const LEASE_POLL: std::time::Duration = std::time::Duration::from_millis(50);

pub struct Action {
    pub name: String,
    pub slot: Slot,
    pub out: PathBuf,
    home: PathBuf,
    lineage: Option<String>,
    lease: Arc<std::fs::File>,
    writing: std::cell::Cell<bool>,
    fetched: std::cell::Cell<bool>,
    from_remote: std::cell::Cell<bool>,
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
            fetched: std::cell::Cell::new(false),
            from_remote: std::cell::Cell::new(false),
            _hold: hold,
        })
    }

    pub(crate) fn compile_out(
        &self,
        settings: &crate::settings::Settings,
        pkg: &crate::cargo::Package,
    ) -> PathBuf {
        settings
            .compile_dir(pkg, self.lineage.as_deref())
            .unwrap_or_else(|| self.out.clone())
    }

    #[must_use]
    pub fn lineage(mut self, lineage: Option<String>) -> Self {
        self.lineage = lineage;
        self
    }

    #[must_use]
    pub fn hit(&self) -> bool {
        if self.slot.hit() {
            return true;
        }
        if !self.fetched.replace(true) && self.fetch().unwrap_or(false) {
            return true;
        }
        self.slot.hit()
    }

    fn fetch(&self) -> Result<bool> {
        if crate::remote::fetch(&self.home, &self.name, &self.slot)? {
            self.from_remote.set(true);
            self.finish()?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) fn prepare(&self) -> Result<()> {
        if self.writing.get() {
            return Ok(());
        }
        self.wait_write()?;
        self.writing.set(true);
        if !store::has_room(&self.slot.dir) {
            return Err(crate::cargo::Unmodeled("insufficient cache disk space".into()).into());
        }
        Ok(())
    }

    fn wait_write(&self) -> Result<()> {
        let limit = std::time::Duration::from_secs(crate::mods::load(&self.home)?.lease_wait_secs);
        let start = std::time::Instant::now();
        let mut told = std::time::Duration::ZERO;
        let who = self.holder();
        loop {
            match self.lease.try_lock() {
                Ok(()) => return Ok(()),
                Err(std::fs::TryLockError::WouldBlock) if start.elapsed() < limit => {
                    if start.elapsed() >= told {
                        say_waiting(&who, start.elapsed());
                        told = start.elapsed() + std::time::Duration::from_secs(5);
                    }
                    std::thread::sleep(LEASE_POLL);
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(crate::cargo::Unmodeled(format!(
                        "unit {} was in use for {}s ({who})",
                        self.name,
                        limit.as_secs()
                    ))
                    .into());
                }
                Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }

    pub(crate) fn lease(&self) -> Result<Arc<std::fs::File>> {
        if self.writing.replace(false) {
            self.lease.unlock()?;
        }
        self.lease.lock_shared()?;
        self.note_holder();
        Ok(Arc::clone(&self.lease))
    }

    fn note_holder(&self) {
        let args = std::env::args()
            .skip(1)
            .take(4)
            .collect::<Vec<_>>()
            .join(" ");
        let path = holder_path(&self.home, &self.name);
        drop(std::fs::write(
            path,
            format!("{} {args}\n", std::process::id()),
        ));
    }

    fn holder(&self) -> String {
        let path = holder_path(&self.home, &self.name);
        let Ok(text) = std::fs::read_to_string(path) else {
            return "another artificer".to_string();
        };
        let mut parts = text.split_whitespace();
        let Some(pid) = parts.next().and_then(|pid| pid.parse::<u32>().ok()) else {
            return "another artificer".to_string();
        };
        if !alive(pid) {
            return "another artificer".to_string();
        }
        let cmd = parts.collect::<Vec<_>>().join(" ");
        if cmd.is_empty() {
            format!("pid {pid}")
        } else {
            format!("pid {pid} (`{cmd}`)")
        }
    }

    pub fn finish(&self) -> Result<()> {
        if !self.from_remote.get() {
            crate::remote::note_built(&self.home, &self.name);
        }
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
        store::discard(&self.slot.dir)
    }
}

fn holder_path(home: &Path, name: &str) -> PathBuf {
    home.join("leases")
        .join(crate::store::LAYOUT)
        .join(format!("{name}.holder"))
}

fn say_waiting(who: &str, elapsed: std::time::Duration) {
    let waited = match elapsed.as_secs() {
        0 => String::new(),
        secs => format!(", {secs}s"),
    };
    crate::out::status(
        crate::out::Status::Waiting,
        format!("cached unit held by {who}{waited}; that process keeps it until it exits"),
    );
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: kill with signal 0 only checks that the pid exists.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
