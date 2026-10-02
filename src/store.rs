pub const LAYOUT: &str = "v5";

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Slot {
    pub dir: PathBuf,
}

impl Slot {
    pub fn new(home: &Path, extra: &str) -> Self {
        Self {
            dir: home.join("units").join(LAYOUT).join(extra),
        }
    }

    pub fn hit(&self) -> bool {
        let path = self.dir.join("ok");
        let Ok(meta) = fs::metadata(&path) else {
            return false;
        };
        if meta.is_file() && aged(&meta, Duration::from_secs(24 * 3600)) {
            drop(fs::write(&path, ""));
        }
        meta.is_file()
    }

    pub fn out_dir(&self) -> PathBuf {
        self.dir.join("out")
    }

    pub fn mark(&self) -> Result<()> {
        if self.hit() {
            return Ok(());
        }
        fs::create_dir_all(&self.dir)?;
        fs::write(self.dir.join("ok"), "")?;
        Ok(())
    }

    pub(crate) fn copy_from(&self, src: &Path) -> Result<()> {
        self.publish_with(|tmp| copy_dir(src, tmp))
    }

    fn publish_with(&self, copy: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
        if self.hit() {
            return Ok(());
        }
        if !has_room(&self.dir) {
            return Ok(());
        }
        let parent = self.dir.parent().context("unit parent")?;
        fs::create_dir_all(parent)?;
        let mut failed = None;
        let published = crate::platform::replace_atomic(&self.dir, |tmp| {
            if let Err(error) = copy(tmp) {
                let io = std::io::Error::other(error.to_string());
                failed = Some(error);
                return Err(io);
            }
            fs::write(tmp.join("ok"), "")?;
            if self.dir.exists() && !self.hit() {
                fs::remove_dir_all(&self.dir)?;
            }
            Ok(())
        });
        if let Some(error) = failed {
            return Err(error);
        }
        match published {
            Err(_) if self.hit() => Ok(()),
            result => result.map_err(Into::into),
        }
    }
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            bail!("symlink in unit: {:?}", entry.path());
        } else if ft.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else if ft.is_file() {
            fs::copy(entry.path(), &to)?;
        } else {
            bail!("non-regular file: {:?}", entry.path());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

mod gc;
mod layouts;
use gc::aged;
pub(crate) use gc::size;
pub use gc::{AGE, CAP, CAP_SHARE, gc_cap, gc_units, has_room};

mod locks;
pub use locks::{Hold, hold, try_hold};

mod lease;
pub(crate) use lease::{lease, try_write};

mod stats;
pub(crate) use stats::{build_records, fallback_reasons, fallbacks};
pub use stats::{bump_stats, note_build, note_fallback, stats};
