#[cfg(unix)]
use anyhow::Context;
use anyhow::Result;
use std::fs;
#[cfg(unix)]
use std::io;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
mod build;
#[cfg(unix)]
mod held;

#[cfg(unix)]
pub use build::BuildPool;

#[cfg(not(unix))]
pub struct BuildPool;

#[cfg(not(unix))]
impl BuildPool {
    pub(crate) fn new(_home: &Path, _tokens: usize) -> Result<Option<Self>> {
        Ok(None)
    }

    pub(crate) fn take(&self) -> Result<()> {
        Ok(())
    }

    pub(crate) fn configure(&self, cmd: &mut std::process::Command) {
        isolate(cmd);
    }
}

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
        make_fifo(&fifo)?;
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
fn auth(fifo: &Path) -> String {
    format!("--jobserver-auth=fifo:{}", fifo.display())
}

#[cfg(unix)]
fn write_env(home: &Path, fifo: &Path) -> Result<()> {
    let auth = auth(fifo);
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

#[cfg(unix)]
fn make_fifo(path: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("fifo path {}", path.display()))?;
    // SAFETY: `name` is a valid NUL-terminated path that outlives the call.
    let made = unsafe { libc::mkfifo(name.as_ptr(), 0o600) } == 0;
    if !made && !path.exists() {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("mkfifo {}", path.display()));
    }
    Ok(())
}

pub fn isolate(cmd: &mut std::process::Command) {
    cmd.env_remove("MAKEFLAGS");
    cmd.env_remove("CARGO_MAKEFLAGS");
}

pub struct Permit {
    #[cfg(unix)]
    _token: Option<held::Token>,
}

pub fn acquire(home: &Path) -> Result<Permit> {
    #[cfg(unix)]
    {
        let path = fifo(home);
        if !path.exists() {
            return Ok(Permit { _token: None });
        }
        Ok(Permit {
            _token: Some(held::take(&path)?),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = home;
        Ok(Permit {})
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
