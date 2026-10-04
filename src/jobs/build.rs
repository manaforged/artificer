use anyhow::{Context, Result};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const TOKEN: u8 = b'+';
const DIR: &str = "jobservers";

static SEQ: AtomicU64 = AtomicU64::new(0);

pub struct BuildPool {
    path: PathBuf,
    file: fs::File,
}

pub(crate) struct BuildToken<'a> {
    pool: &'a BuildPool,
}

impl BuildPool {
    pub(crate) fn new(home: &Path, tokens: usize) -> Result<Option<Self>> {
        let dir = home.join(DIR);
        if dir.to_string_lossy().contains(char::is_whitespace) {
            return Ok(None);
        }
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("build-{}-{seq}.fifo", std::process::id()));
        drop(fs::remove_file(&path));
        super::make_fifo(&path)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        (&file).write_all(&vec![TOKEN; tokens.max(1)])?;
        Ok(Some(Self { path, file }))
    }

    pub(crate) fn take(&self) -> Result<BuildToken<'_>> {
        let mut byte = [0u8; 1];
        (&self.file)
            .read_exact(&mut byte)
            .context("build job token")?;
        Ok(BuildToken { pool: self })
    }

    pub(crate) fn configure(&self, cmd: &mut Command) {
        super::isolate(cmd);
        cmd.env(
            "CARGO_MAKEFLAGS",
            format!("--jobserver-auth=fifo:{}", self.path.display()),
        );
    }
}

impl Drop for BuildToken<'_> {
    fn drop(&mut self) {
        drop((&self.pool.file).write_all(&[TOKEN]));
    }
}

impl Drop for BuildPool {
    fn drop(&mut self) {
        drop(fs::remove_file(&self.path));
    }
}
