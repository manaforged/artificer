#[cfg(unix)]
use anyhow::Context;
use anyhow::Result;
use std::fs;
#[cfg(unix)]
use std::io;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

pub fn install(home: &Path) -> Result<()> {
    fs::create_dir_all(home)?;
    let tag = home.join("CACHEDIR.TAG");
    if !tag.is_file() {
        fs::write(
            tag,
            "Signature: 8a477f597d28d172789f06886806bc55\n\
             # This file is a cache directory tag created by Artificer.\n\
             # For information about cache directory tags, see:\n\
             #	https://bford.info/cachedir/\n",
        )?;
    }
    install_pool(home)
}

#[cfg(unix)]
fn install_pool(home: &Path) -> Result<()> {
    static INSTALL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _install = INSTALL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fifo = fifo(home);
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);
    if !fifo.exists() {
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .context("mkfifo")?;
        if !status.success() && !fifo.exists() {
            anyhow::bail!("mkfifo {}", fifo.display());
        }
    }
    if parked(&fifo) {
        return write_env(home, &fifo);
    }
    let lease = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.join("jobserver.lock"))?;
    match lease.try_lock() {
        Ok(()) => {
            fill(&fifo, n)?;
            lease.unlock()?;
        }
        Err(fs::TryLockError::WouldBlock) => park(&fifo)?,
        Err(fs::TryLockError::Error(error)) => return Err(error.into()),
    }
    lease.lock_shared()?;
    let mut keep = KEEP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, owner)) = keep.as_mut().and_then(|pools| pools.get_mut(&fifo)) {
        *owner = Some(lease);
    }
    drop(keep);
    write_env(home, &fifo)
}

#[cfg(unix)]
fn write_env(home: &Path, fifo: &Path) -> Result<()> {
    let auth = format!("--jobserver-auth=fifo:{}", fifo.display());
    fs::write(
        home.join("jobserver.env"),
        format!("export MAKEFLAGS='{auth}'\nexport CARGO_MAKEFLAGS='{auth}'\n"),
    )?;
    Ok(())
}

#[cfg(not(unix))]
fn install_pool(_home: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
type Pools = std::collections::HashMap<PathBuf, (fs::File, Option<fs::File>)>;

#[cfg(unix)]
static KEEP: std::sync::Mutex<Option<Pools>> = std::sync::Mutex::new(None);

#[cfg(all(test, unix))]
pub(crate) fn writers(fifo: &Path) -> usize {
    KEEP.lock()
        .expect("jobserver")
        .as_ref()
        .map_or(0, |m| usize::from(m.contains_key(fifo)))
}

#[cfg(unix)]
fn parked(fifo: &Path) -> bool {
    KEEP.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|m| m.contains_key(fifo))
}

#[cfg(unix)]
fn park(fifo: &Path) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(fifo)?;
    let mut keep = KEEP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    keep.get_or_insert_with(std::collections::HashMap::new)
        .entry(fifo.to_path_buf())
        .or_insert((f, None));
    Ok(())
}

#[cfg(unix)]
pub(crate) fn fill(fifo: &Path, n: usize) -> Result<()> {
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(fifo)?;
    let mut buf = [0u8; 128];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(e.into()),
        }
    }
    f.write_all(&vec![b'+'; n])?;
    let mut keep = KEEP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    keep.get_or_insert_with(std::collections::HashMap::new)
        .entry(fifo.to_path_buf())
        .or_insert((f, None));
    Ok(())
}

pub fn isolate(cmd: &mut std::process::Command) {
    cmd.env_remove("MAKEFLAGS");
    cmd.env_remove("CARGO_MAKEFLAGS");
}

pub struct Permit {
    #[cfg(unix)]
    file: Option<fs::File>,
}

pub fn acquire(home: &Path) -> Result<Permit> {
    #[cfg(unix)]
    {
        use std::io::Read;
        let path = fifo(home);
        if !path.exists() {
            return Ok(Permit { file: None });
        }
        let mut file = fs::OpenOptions::new().read(true).write(true).open(&path)?;
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte).context("jobserver token")?;
        Ok(Permit { file: Some(file) })
    }
    #[cfg(not(unix))]
    {
        let _ = home;
        Ok(Permit {})
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(file) = &mut self.file {
            use std::io::Write;
            drop(file.write_all(b"+"));
        }
    }
}

#[cfg(unix)]
pub(crate) fn fifo(home: &Path) -> PathBuf {
    fifo_at(home, &crate::cargo::cargo_home_alias())
}

#[cfg(unix)]
fn fifo_at(home: &Path, cargo_home: &Path) -> PathBuf {
    let alias_home = cargo_home.join("artificer");
    let alias = alias_home.join("jobserver.fifo");
    if !alias.to_string_lossy().contains(char::is_whitespace)
        && crate::resolve_path(&alias_home) == crate::resolve_path(home)
    {
        return alias;
    }
    let direct = home.join("jobserver.fifo");
    if !direct.to_string_lossy().contains(char::is_whitespace) {
        return direct;
    }
    let digest = blake3::hash(crate::resolve_path(home).to_string_lossy().as_bytes());
    let dir = crate::home::control_home().join("jobservers");
    drop(fs::create_dir_all(&dir));
    dir.join(format!("{}.fifo", &digest.to_hex()[..16]))
}

#[cfg(unix)]
pub fn pool_depth(fifo: &Path) -> Result<usize> {
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(fifo)?;
    let mut n = 0;
    let mut buf = [0u8; 128];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(e.into()),
        }
    }
    f.write_all(&vec![b'+'; n])?;
    Ok(n)
}

#[cfg(all(test, unix))]
#[path = "jobs_tests.rs"]
mod tests;
