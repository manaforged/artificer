use super::*;

#[cfg(windows)]
#[path = "windows.rs"]
mod windows;

pub fn stop(home: &Path) -> Result<()> {
    if !home.is_dir() {
        return Ok(());
    }
    let pid = read_pid(home);
    if stale(home) {
        cleanup(home);
        return Ok(());
    }
    drop(probe(
        home,
        &Request {
            token: read_token(home).unwrap_or_default(),
            op: "quit".into(),
            dir: PathBuf::new(),
            packages: Vec::new(),
            json: false,
            workspace: false,
            all_features: false,
            features: Vec::new(),
            no_default: false,
            meta_flags: Vec::new(),
            target_dir: None,
            release: false,
            link: false,
            no_run: false,
            lib: false,
            doc: false,
            only: Vec::new(),
            tests: false,
            all_targets: false,
            args: Vec::new(),
            profile: None,
        },
    ));
    control(home, "serve.stop", b"")?;
    if let Some(addr) = read_addr(home) {
        drop(TcpStream::connect_timeout(&addr, CONNECT));
    }
    finish_stop(home, pid, Duration::from_secs(2))
}

pub(super) fn finish_stop(home: &Path, pid: Option<u32>, wait: Duration) -> Result<()> {
    let start = Instant::now();
    let pending = || {
        home.join("serve.pid").is_file()
            || (cfg!(windows)
                && pid
                    .filter(|pid| *pid != std::process::id())
                    .is_some_and(crate::platform::alive))
    };
    while pending() && start.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(20));
    }
    if pending() {
        let process = pid.map_or_else(|| "unknown PID".to_string(), |pid| format!("pid {pid}"));
        bail!(
            "serve {process} did not stop within {}s; refusing to terminate an unverified process",
            wait.as_secs()
        );
    }
    cleanup(home);
    Ok(())
}

pub fn ping(home: &Path) -> bool {
    let Ok(token) = read_token(home) else {
        return false;
    };
    let req = Request {
        token,
        op: "ping".into(),
        dir: PathBuf::new(),
        packages: Vec::new(),
        json: false,
        workspace: false,
        all_features: false,
        features: Vec::new(),
        no_default: false,
        meta_flags: Vec::new(),
        target_dir: None,
        release: false,
        link: false,
        no_run: false,
        lib: false,
        doc: false,
        only: Vec::new(),
        tests: false,
        all_targets: false,
        args: Vec::new(),
        profile: None,
    };
    probe(home, &req).is_ok_and(|r| r.ok && r.stderr.contains("pong"))
}

pub fn spawn(home: &Path) -> Result<()> {
    if ping(home) {
        return Ok(());
    }
    if stale(home) {
        cleanup(home);
    }
    secure_home(home)?;
    let log = secure_file(&home.join("serve.log"))?;
    let exe = crate::install::self_launcher().context("current_exe")?;
    start(&exe, home, log).context("spawn artificer serve")?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if ping(home) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    bail!("serve did not come up")
}

#[cfg(not(windows))]
fn start(exe: &Path, home: &Path, log: fs::File) -> Result<()> {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(exe);
    cmd.arg("serve");
    if crate::resolve_path(&crate::default_home()) != crate::resolve_path(home) {
        cmd.env("ARTIFICER_HOME", home);
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::from(log.try_clone()?));
    cmd.stderr(Stdio::from(log));
    cmd.spawn()?;
    Ok(())
}

#[cfg(windows)]
use windows::start;

pub fn call(home: &Path, req: &Request) -> Result<Reply> {
    call_within(home, req, Duration::from_secs(30 * 60))
}

fn probe(home: &Path, req: &Request) -> Result<Reply> {
    call_within(home, req, IO)
}

fn call_within(home: &Path, req: &Request, reply_wait: Duration) -> Result<Reply> {
    let addr = read_addr(home).context("no serve.port")?;
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT).context("connect serve")?;
    stream.set_read_timeout(Some(IO))?;
    stream.set_write_timeout(Some(IO))?;
    let line = serde_json::to_string(req)?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.set_read_timeout(Some(reply_wait))?;
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp).context("serve reply")?;
    serde_json::from_str(&resp).context("parse serve reply")
}

pub(super) fn compiler_env_snapshot() -> String {
    let mut values: Vec<_> = std::env::vars_os().collect();
    values.sort();
    let mut key = crate::action::Key::new();
    for (name, value) in values {
        key.feed(name.as_encoded_bytes());
        key.feed(value.as_encoded_bytes());
    }
    key.full_digest()
}

fn served(home: &Path, req: &mut Request) -> Option<(Reply, Instant)> {
    let sent = Instant::now();
    if let Ok(reply) = call(home, req) {
        return Some((reply, sent));
    }
    spawn(home).ok()?;
    if fs::read_to_string(home.join("serve.env")).ok()? != compiler_env_snapshot() {
        return None;
    }
    req.token = read_token(home).ok()?;
    let sent = Instant::now();
    call(home, req).ok().map(|reply| (reply, sent))
}

enum Readiness {
    Use,
    Skip,
    Failed(anyhow::Error),
}

fn readiness(home: &Path) -> Readiness {
    if cfg!(windows)
        || std::env::var_os("ARTIFICER_NOSERVE").is_some()
        || crate::schedule::explicit_jobs()
    {
        return Readiness::Skip;
    }
    if let Ok(recorded) = fs::read_to_string(home.join("serve.env"))
        && recorded != compiler_env_snapshot()
    {
        return Readiness::Skip;
    }
    match crate::mods::load(home) {
        Ok(mods) if !mods.serve => return Readiness::Skip,
        Ok(_) => {}
        Err(error) => return Readiness::Failed(error),
    }
    if fs::read_to_string(home.join("serve.build")).is_ok_and(|b| b != build_stamp()) {
        return match stop(home) {
            Ok(()) => Readiness::Skip,
            Err(error) => Readiness::Failed(error),
        };
    }
    Readiness::Use
}

fn outcome(reply: Reply) -> Option<Result<i32>> {
    if reply.ok {
        if !reply.stdout.is_empty() {
            print!("{}", reply.stdout);
        }
        if !reply.stderr.is_empty() {
            eprint!("{}", reply.stderr);
        }
        return Some(Ok(reply.code));
    }
    if reply.err == "bad token" {
        return None;
    }
    if !reply.stderr.is_empty() {
        eprint!("{}", reply.stderr);
    }
    if reply.err.is_empty() {
        return Some(Ok(reply.code));
    }
    Some(Err(if reply.code == 2 {
        crate::cargo::Unmodeled(reply.err).into()
    } else {
        anyhow::anyhow!("{}", reply.err)
    }))
}

pub fn try_run(home: &Path, req: &mut Request) -> Option<Result<i32>> {
    match readiness(home) {
        Readiness::Use => {}
        Readiness::Skip => return None,
        Readiness::Failed(error) => return Some(Err(error)),
    }
    req.token = read_token(home).unwrap_or_default();
    req.profile = crate::profile::id();
    let (mut reply, sent) =
        crate::profile::span(crate::profile::WrapperPhase::Serve, || served(home, req))?;
    if let Some(part) = reply.profile.take() {
        crate::profile::merge(part, sent);
    }
    outcome(reply)
}
