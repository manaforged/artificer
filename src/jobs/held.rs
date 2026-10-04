use anyhow::{Context, Result};
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const RECLAIM_AFTER: Duration = Duration::from_secs(2);
const HELD_SUFFIX: &str = ".held";
const STAGING: &str = ".";
const TOKEN: u8 = b'+';

static SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Token {
    fifo: fs::File,
    path: PathBuf,
    lock: Option<fs::File>,
}

impl Drop for Token {
    fn drop(&mut self) {
        drop(fs::remove_file(&self.path));
        drop(self.lock.take());
        drop(self.fifo.write_all(&[TOKEN]));
    }
}

pub(crate) fn dir(fifo: &Path) -> PathBuf {
    let mut name = fifo.as_os_str().to_owned();
    name.push(HELD_SUFFIX);
    PathBuf::from(name)
}

fn open(fifo: &Path) -> Result<fs::File> {
    Ok(fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(fifo)?)
}

pub(crate) fn take(fifo: &Path) -> Result<Token> {
    let mut file = open(fifo)?;
    let mut byte = [0u8; 1];
    let mut since = Instant::now();
    loop {
        match file.read(&mut byte) {
            Ok(1) => break,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e).context("jobserver token"),
        }
        let left = RECLAIM_AFTER.saturating_sub(since.elapsed());
        if left.is_zero() || !ready(&file, left)? {
            reclaim(fifo)?;
            since = Instant::now();
        }
    }
    match register(fifo) {
        Ok((path, lock)) => Ok(Token {
            fifo: file,
            path,
            lock: Some(lock),
        }),
        Err(error) => {
            drop(file.write_all(&[TOKEN]));
            Err(error)
        }
    }
}

fn ready(file: &fs::File, wait: Duration) -> Result<bool> {
    let mut fd = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = libc::c_int::try_from(wait.as_millis().max(1)).unwrap_or(libc::c_int::MAX);
    // SAFETY: `fd` is a valid pollfd that lives for the call, and the count is 1.
    let n = unsafe { libc::poll(&mut fd, 1, ms) };
    if n < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(true);
        }
        return Err(error.into());
    }
    Ok(n > 0)
}

fn register(fifo: &Path) -> Result<(PathBuf, fs::File)> {
    let dir = dir(fifo);
    fs::create_dir_all(&dir)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!(
        "{}-{}-{nanos}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let staged = dir.join(format!("{STAGING}{name}"));
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&staged)?;
    lock.lock()?;
    let path = dir.join(name);
    if let Err(error) = fs::rename(&staged, &path) {
        drop(fs::remove_file(&staged));
        return Err(error.into());
    }
    Ok((path, lock))
}

pub(crate) fn reclaim(fifo: &Path) -> Result<usize> {
    let entries = match fs::read_dir(dir(fifo)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    let mut n = 0;
    for entry in entries {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(STAGING) {
            continue;
        }
        let path = entry.path();
        let Ok(file) = fs::OpenOptions::new().read(true).open(&path) else {
            continue;
        };
        if file.try_lock().is_err() {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            n += 1;
        }
    }
    if n > 0 {
        open(fifo)?.write_all(&vec![TOKEN; n])?;
    }
    Ok(n)
}
