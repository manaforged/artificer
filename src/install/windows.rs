use anyhow::{Result, bail};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
};

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle came from RegOpenKeyExW and is closed once here.
        unsafe { RegCloseKey(self.0) };
    }
}

fn environment() -> Result<Key> {
    let name = wide("Environment");
    let mut key: HKEY = null_mut();
    // SAFETY: name is NUL-terminated and key is a valid out pointer.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            name.as_ptr(),
            0,
            KEY_READ | KEY_WRITE,
            &mut key,
        )
    };
    if status != 0 {
        bail!("open HKCU\\Environment: error {status}");
    }
    Ok(Key(key))
}

pub(super) struct UserPath {
    pub(super) value: String,
    kind: REG_VALUE_TYPE,
}

pub(super) fn read() -> Result<UserPath> {
    let key = environment()?;
    let name = wide("Path");
    let mut kind: REG_VALUE_TYPE = 0;
    let mut bytes: u32 = 0;
    // SAFETY: a null data pointer asks only for the type and size.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            null(),
            &mut kind,
            null_mut(),
            &mut bytes,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(UserPath {
            value: String::new(),
            kind: REG_EXPAND_SZ,
        });
    }
    if status != 0 {
        bail!("read HKCU\\Environment\\Path: error {status}");
    }
    if kind != REG_SZ && kind != REG_EXPAND_SZ {
        bail!("HKCU\\Environment\\Path is not a string value");
    }
    let mut data = vec![0u16; (bytes as usize).div_ceil(2)];
    // SAFETY: data holds `bytes` bytes and the other pointers are valid.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            null(),
            &mut kind,
            data.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != 0 {
        bail!("read HKCU\\Environment\\Path: error {status}");
    }
    let end = data.iter().position(|&c| c == 0).unwrap_or(data.len());
    let Ok(value) = String::from_utf16(&data[..end]) else {
        bail!(
            "HKCU\\Environment\\Path is not valid UTF-16; refusing to modify it. Add the directory to your user PATH by hand"
        );
    };
    Ok(UserPath { value, kind })
}

pub(super) fn write(path: &UserPath, value: &str) -> Result<()> {
    let key = environment()?;
    let name = wide("Path");
    let data = wide(value);
    let bytes = u32::try_from(data.len() * 2)?;
    // SAFETY: data is a NUL-terminated UTF-16 buffer of `bytes` bytes.
    let status = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            path.kind,
            data.as_ptr().cast(),
            bytes,
        )
    };
    if status != 0 {
        bail!("write HKCU\\Environment\\Path: error {status}");
    }
    let area = wide("Environment");
    let mut result = 0usize;
    // SAFETY: area is NUL-terminated and outlives the call; result is a valid out pointer.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            area.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        )
    };
    Ok(())
}
