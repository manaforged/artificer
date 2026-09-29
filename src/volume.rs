use std::path::Path;

pub fn capacity(path: &Path) -> Option<(u64, u64)> {
    imp::capacity(path.ancestors().find(|path| path.exists())?)
}

#[cfg(unix)]
mod imp {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub fn capacity(path: &Path) -> Option<(u64, u64)> {
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `statvfs` is a plain C struct; all-zero bytes are a valid value.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is a NUL-terminated path and `stat` is a valid
        let rc = unsafe { libc::statvfs(c.as_ptr(), &mut stat) };
        if rc != 0 {
            return None;
        }
        let frsize = if stat.f_frsize == 0 {
            stat.f_bsize as u64
        } else {
            stat.f_frsize as u64
        };
        let total = (stat.f_blocks as u64).checked_mul(frsize)?;
        let free = (stat.f_bavail as u64).checked_mul(frsize)?;
        Some((total, free))
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    pub fn capacity(path: &Path) -> Option<(u64, u64)> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        let mut free: u64 = 0;
        let mut total: u64 = 0;
        // SAFETY: `wide` is NUL-terminated and both out-params are valid
        let ok = unsafe {
            GetDiskFreeSpaceExW(wide.as_ptr(), std::ptr::null_mut(), &mut total, &mut free)
        };
        (ok != 0).then_some((total, free))
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use std::path::Path;

    pub fn capacity(_path: &Path) -> Option<(u64, u64)> {
        None
    }
}
