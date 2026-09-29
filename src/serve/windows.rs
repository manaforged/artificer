use anyhow::Result;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle};
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DETACHED_PROCESS, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, InitializeProcThreadAttributeList,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    UpdateProcThreadAttribute,
};

fn inheritable(file: &File) -> io::Result<OwnedHandle> {
    // SAFETY: file and process remain valid during duplication; the returned OwnedHandle exclusively owns the successful duplicate.
    unsafe {
        let mut handle = null_mut();
        let process = GetCurrentProcess();
        if DuplicateHandle(
            process,
            file.as_raw_handle(),
            process,
            &mut handle,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(OwnedHandle::from_raw_handle(handle))
    }
}

pub(super) fn start(exe: &Path, home: &Path, log: File) -> Result<()> {
    let input = inheritable(&File::open("NUL")?)?;
    let output = inheritable(&log)?;
    let handles = [input.as_raw_handle(), output.as_raw_handle()];
    let program: Vec<_> = exe.as_os_str().encode_wide().chain([0]).collect();
    let mut command: Vec<_> = OsStr::new("artificer serve")
        .encode_wide()
        .chain([0])
        .collect();
    let mut environment = Vec::new();
    if crate::resolve_path(&crate::default_home()) != crate::resolve_path(home) {
        let mut values: Vec<_> = std::env::vars_os()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("ARTIFICER_HOME"))
            .collect();
        values.push(("ARTIFICER_HOME".into(), home.as_os_str().to_owned()));
        values.sort_by_cached_key(|(name, _)| name.to_string_lossy().to_uppercase());
        for (name, value) in values {
            environment.extend(name.encode_wide());
            environment.push(b'=' as u16);
            environment.extend(value.encode_wide());
            environment.push(0);
        }
        environment.push(0);
    }
    // SAFETY: buffers stay allocated and aligned, only owned input/output handles are inherited, and successful process handles and the initialized attribute list are each released once.
    unsafe {
        let mut size = 0;
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
        let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
        let list = storage.as_mut_ptr().cast();
        if InitializeProcThreadAttributeList(list, 1, 0, &mut size) == 0 {
            return Err(io::Error::last_os_error().into());
        }
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[1];
        startup.lpAttributeList = list;
        let mut process = PROCESS_INFORMATION::default();
        let result = if UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr().cast(),
            size_of_val(&handles),
            null_mut(),
            null(),
        ) == 0
            || CreateProcessW(
                program.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                DETACHED_PROCESS | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                if environment.is_empty() {
                    null()
                } else {
                    environment.as_ptr().cast()
                },
                null(),
                &startup.StartupInfo,
                &mut process,
            ) == 0
        {
            Err(io::Error::last_os_error())
        } else {
            drop(OwnedHandle::from_raw_handle(process.hProcess));
            drop(OwnedHandle::from_raw_handle(process.hThread));
            Ok(())
        };
        DeleteProcThreadAttributeList(list);
        result?;
    }
    Ok(())
}
