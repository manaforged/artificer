use super::model::Usage;
use std::io::{self, BufRead, BufReader, Read};
use std::process::{Child, Command, ExitStatus, Output, Stdio};

pub(crate) fn status(cmd: &mut Command) -> io::Result<ExitStatus> {
    let child = cmd.spawn()?;
    let (status, usage) = wait(child)?;
    if let Some(usage) = usage {
        super::record::note_usage(usage);
    }
    Ok(status)
}

pub(crate) fn output(cmd: &mut Command) -> io::Result<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (stdout, stderr) = std::thread::scope(|scope| {
        let stdout = scope.spawn(move || read_all(stdout));
        let stderr = scope.spawn(move || read_all(stderr));
        (joined(stdout.join()), joined(stderr.join()))
    });
    let (status, usage) = wait(child)?;
    if let Some(usage) = usage {
        super::record::note_usage(usage);
    }
    Ok(Output {
        status,
        stdout: stdout?,
        stderr: stderr?,
    })
}

pub(crate) fn status_lines(
    cmd: &mut Command,
    mut on_line: impl FnMut(&[u8]) -> io::Result<()> + Send,
) -> io::Result<ExitStatus> {
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stderr = child.stderr.take();
    let (read, waited) = std::thread::scope(|scope| {
        let reader = scope.spawn(move || drain(stderr, &mut on_line));
        let waited = wait(child);
        (joined(reader.join()), waited)
    });
    let (status, usage) = waited?;
    read?;
    if let Some(usage) = usage {
        super::record::note_usage(usage);
    }
    Ok(status)
}

fn drain(
    pipe: Option<impl Read>,
    on_line: &mut impl FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let Some(pipe) = pipe else {
        return Ok(());
    };
    let mut reader = BufReader::new(pipe);
    let mut line = Vec::new();
    let mut failure = None;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        let text = line.strip_suffix(b"\n").unwrap_or(&line);
        let text = text.strip_suffix(b"\r").unwrap_or(text);
        if failure.is_none()
            && let Err(error) = on_line(text)
        {
            failure = Some(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

fn read_all(pipe: Option<impl Read>) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        pipe.read_to_end(&mut bytes)?;
    }
    Ok(bytes)
}

fn joined<T>(result: std::thread::Result<io::Result<T>>) -> io::Result<T> {
    result.unwrap_or_else(|panic| {
        Err(io::Error::other(format!(
            "reading child output panicked: {panic:?}"
        )))
    })
}

#[cfg(unix)]
fn wait(child: Child) -> io::Result<(ExitStatus, Option<Usage>)> {
    use std::os::unix::process::ExitStatusExt;
    let pid = libc::pid_t::try_from(child.id()).map_err(io::Error::other)?;
    let mut raw = 0;
    // SAFETY: rusage is a plain C struct of integers; the all-zero value is valid.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: pid is this process's own unreaped child; raw and usage are valid for writes.
        let reaped = unsafe { libc::wait4(pid, &mut raw, 0, &mut usage) };
        if reaped == pid {
            break;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    drop(child);
    Ok((
        ExitStatus::from_raw(raw),
        Some(Usage {
            user_us: timeval_us(usage.ru_utime),
            system_us: timeval_us(usage.ru_stime),
            peak_rss_bytes: peak_bytes(usage.ru_maxrss),
        }),
    ))
}

#[cfg(unix)]
fn timeval_us(time: libc::timeval) -> u64 {
    let secs = u64::try_from(time.tv_sec).unwrap_or(0);
    let micros = u64::try_from(time.tv_usec).unwrap_or(0);
    secs.saturating_mul(1_000_000).saturating_add(micros)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn peak_bytes(maxrss: libc::c_long) -> u64 {
    u64::try_from(maxrss).unwrap_or(0)
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
fn peak_bytes(maxrss: libc::c_long) -> u64 {
    u64::try_from(maxrss).unwrap_or(0).saturating_mul(1024)
}

#[cfg(windows)]
fn wait(mut child: Child) -> io::Result<(ExitStatus, Option<Usage>)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetProcessTimes;
    let status = child.wait()?;
    let handle = child.as_raw_handle();
    let empty = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (empty, empty, empty, empty);
    // SAFETY: handle is the process handle owned by child, open until child drops at the end of this function; the four pointers are valid FILETIME slots.
    let timed =
        unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) } != 0;
    // SAFETY: PROCESS_MEMORY_COUNTERS is a plain C struct of integers; the all-zero value is valid.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).unwrap_or(0);
    // SAFETY: handle is valid as above; counters is a valid PROCESS_MEMORY_COUNTERS whose size is cb.
    let measured = unsafe { K32GetProcessMemoryInfo(handle, &mut counters, counters.cb) } != 0;
    let usage = timed.then(|| Usage {
        user_us: filetime_us(user),
        system_us: filetime_us(kernel),
        peak_rss_bytes: if measured {
            u64::try_from(counters.PeakWorkingSetSize).unwrap_or(0)
        } else {
            0
        },
    });
    drop(child);
    Ok((status, usage))
}

#[cfg(windows)]
fn filetime_us(time: windows_sys::Win32::Foundation::FILETIME) -> u64 {
    ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) / 10
}
