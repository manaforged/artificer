use std::path::{Path, PathBuf};

pub fn on_path(program: &str) -> Option<PathBuf> {
    let suffix = std::env::consts::EXE_SUFFIX;
    let name = if suffix.is_empty() || program.ends_with(suffix) {
        program.to_string()
    } else {
        format!("{program}{suffix}")
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&name))
        .find(|path| path.is_file())
}

pub fn env_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let raw = p.as_os_str().to_string_lossy();
        if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = raw.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    p.to_path_buf()
}

#[cfg(target_os = "macos")]
const OPEN_FILE_CEILING: libc::rlim_t = 10240;

#[cfg(target_os = "macos")]
fn open_file_target(hard: libc::rlim_t) -> libc::rlim_t {
    hard.min(OPEN_FILE_CEILING)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_file_target(hard: libc::rlim_t) -> libc::rlim_t {
    hard
}

#[cfg(unix)]
pub fn raise_open_file_limit() -> std::io::Result<()> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit writes one complete rlimit into this valid, exclusively borrowed struct.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let wanted = open_file_target(limit.rlim_max);
    if limit.rlim_cur >= wanted {
        return Ok(());
    }
    limit.rlim_cur = wanted;
    // SAFETY: setrlimit reads the valid rlimit above; a soft limit at or below the hard limit needs no privilege.
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
pub fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(windows)]
pub fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    // SAFETY: OpenProcess returns an owned handle or null. CloseHandle releases
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        running
    }
}

const TEMP_TAG: &str = ".artificer-";

pub(crate) fn temp_sibling(dst: &Path) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = dst.file_name().unwrap_or_default().to_string_lossy();
    dst.with_file_name(format!(
        ".{name}{TEMP_TAG}{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
}

pub(crate) fn temp_owner(file_name: &str) -> Option<(&str, u32)> {
    let rest = file_name.strip_prefix('.')?;
    let (name, tail) = rest.rsplit_once(TEMP_TAG)?;
    let (pid, _) = tail.split_once('-')?;
    Some((name, pid.parse().ok()?))
}

pub(crate) fn replace_atomic(
    dst: &Path,
    write: impl FnOnce(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let tmp = temp_sibling(dst);
    let result = write(&tmp).and_then(|()| std::fs::rename(&tmp, dst));
    if result.is_err() {
        if tmp.is_dir() {
            drop(std::fs::remove_dir_all(&tmp));
        } else {
            drop(std::fs::remove_file(&tmp));
        }
    }
    result
}

#[cfg(unix)]
pub(crate) fn copy_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    let out = std::process::Command::new("/bin/cp")
        .arg(src)
        .arg(dst)
        .stdin(std::process::Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "cp {} {}: {}",
            src.display(),
            dst.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    std::fs::set_permissions(dst, std::fs::metadata(src)?.permissions())
}

#[cfg(not(unix))]
pub(crate) fn copy_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::copy(src, dst).map(drop)
}

#[cfg(test)]
#[path = "platform_tests.rs"]
mod tests;
