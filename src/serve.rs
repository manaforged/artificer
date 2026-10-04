use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const CONNECT: Duration = Duration::from_millis(200);
const IO: Duration = Duration::from_secs(10);
pub(crate) const GC_EVERY: Duration = Duration::from_secs(24 * 3600);
const MAX_REQUEST: usize = 1024 * 1024;
const MAX_CONNECTIONS: usize = 64;
static CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

pub(crate) const GC_CAP_EVERY: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub token: String,
    pub op: String,
    #[serde(default)]
    pub dir: PathBuf,
    #[serde(default, alias = "package")]
    pub packages: Vec<String>,
    #[serde(default)]
    pub json: bool,
    #[serde(default)]
    pub workspace: bool,
    #[serde(default)]
    pub all_features: bool,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub no_default: bool,
    #[serde(default)]
    pub meta_flags: Vec<String>,
    #[serde(default)]
    pub target_dir: Option<PathBuf>,
    #[serde(default)]
    pub release: bool,
    #[serde(default)]
    pub link: bool,
    #[serde(default)]
    pub no_run: bool,
    #[serde(default)]
    pub lib: bool,
    #[serde(default)]
    pub doc: bool,
    #[serde(default)]
    pub only: Vec<String>,
    #[serde(default)]
    pub tests: bool,
    #[serde(default)]
    pub all_targets: bool,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub err: String,
}

const IDLE: Duration = Duration::from_secs(3600);

fn build_stamp() -> String {
    let Ok(exe) = std::env::current_exe() else {
        return String::new();
    };
    let Ok(m) = fs::metadata(&exe) else {
        return String::new();
    };
    let secs = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    format!("{} {} {secs}", exe.display(), m.len())
}

pub fn listen(home: &Path) -> Result<()> {
    if cfg!(windows) {
        bail!("the serve daemon is not available on Windows; builds run in process");
    }
    crate::home::claim(home)?;
    crate::jobs::install(home)?;
    if ping(home) {
        crate::out::err("artificer: serve already running");
        return Ok(());
    }
    if let Some(pid) = read_pid(home)
        && pid != std::process::id()
        && crate::platform::alive(pid)
    {
        crate::out::err(format!(
            "artificer: serve already running (pid {pid}); `artificer serve stop` first"
        ));
        return Ok(());
    }
    if stale(home) {
        cleanup(home);
    }
    let listener = TcpListener::bind("127.0.0.1:0").context("bind serve")?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let token = make_token()?;
    secure_home(home)?;
    control(home, "serve.port", addr.to_string().as_bytes())?;
    control(home, "serve.token", token.as_bytes())?;
    control(home, "serve.pid", std::process::id().to_string().as_bytes())?;
    control(home, "serve.build", build_stamp().as_bytes())?;
    control(home, "serve.env", compiler_env_snapshot().as_bytes())?;
    match fs::remove_file(home.join("serve.stop")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("remove stale serve.stop"),
    }
    crate::out::err(format!("artificer: serve {addr}"));
    gc(home);
    let mut last_gc = Instant::now();
    let mut last_cap = Instant::now();
    let mut last_seen = Instant::now();
    loop {
        if home.join("serve.stop").is_file() {
            break;
        }
        if last_seen.elapsed() >= IDLE {
            crate::out::err("artificer: serve idle, exiting".to_string());
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let Some(connection) = Connection::claim() else {
                    let mut stream = stream;
                    drop(write_reply(&mut stream, &Reply::fail("server busy")));
                    continue;
                };
                let home = home.to_path_buf();
                let token = token.clone();
                last_seen = Instant::now();
                std::thread::spawn(move || {
                    let _connection = connection;
                    if let Err(e) = handle(stream, &home, &token) {
                        crate::out::err(format!("artificer: serve {e}"));
                    }
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if last_gc.elapsed() >= GC_EVERY {
                    last_gc = Instant::now();
                    gc(home);
                }
                if last_cap.elapsed() >= GC_CAP_EVERY {
                    last_cap = Instant::now();
                    let cap_home = home.to_path_buf();
                    std::thread::spawn(move || {
                        if let Err(e) = crate::maintenance::store_cap(&cap_home)
                            .and_then(|cap| crate::store::gc_cap(&cap_home, cap))
                        {
                            crate::out::err(format!("artificer: serve cap gc {e}"));
                        }
                    });
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                cleanup(home);
                return Err(e.into());
            }
        }
    }
    cleanup(home);
    Ok(())
}

fn make_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).context("generate serve token")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

struct Connection;

impl Connection {
    fn claim() -> Option<Self> {
        CONNECTIONS
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
                (active < MAX_CONNECTIONS).then_some(active + 1)
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        CONNECTIONS.fetch_sub(1, Ordering::Relaxed);
    }
}

fn secure_home(home: &Path) -> Result<()> {
    fs::create_dir_all(home)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn secure_file(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn control(home: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    secure_home(home)?;
    let mut file = secure_file(&home.join(name))?;
    file.write_all(bytes)?;
    Ok(())
}

fn read_addr(home: &Path) -> Option<SocketAddr> {
    fs::read_to_string(home.join("serve.port"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn read_token(home: &Path) -> Result<String> {
    Ok(fs::read_to_string(home.join("serve.token"))?.trim().into())
}

fn read_pid(home: &Path) -> Option<u32> {
    fs::read_to_string(home.join("serve.pid"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn stale(home: &Path) -> bool {
    match read_pid(home) {
        Some(pid) => !crate::platform::alive(pid),
        None => home.join("serve.port").is_file(),
    }
}

fn cleanup(home: &Path) {
    for name in [
        "serve.port",
        "serve.token",
        "serve.pid",
        "serve.stop",
        "serve.build",
    ] {
        drop(fs::remove_file(home.join(name)));
    }
}

fn gc(home: &Path) {
    let home = home.to_path_buf();
    std::thread::spawn(move || {
        let r = crate::store::gc_units(&home, crate::store::AGE).and_then(|(units, bytes)| {
            crate::maintenance::store_cap(&home)
                .and_then(|cap| crate::store::gc_cap(&home, cap))
                .map(|(u2, b2)| (units + u2, bytes + b2))
        });
        match r {
            Ok((gone, bytes)) => crate::out::err(format!(
                "artificer: gc evicted {gone} unit(s), {:.1} MB",
                bytes as f64 / 1_048_576.0
            )),
            Err(e) => crate::out::err(format!("artificer: gc failed: {e}")),
        }
    });
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;

mod client;
use client::compiler_env_snapshot;
pub use client::{ping, stop, try_run};

mod request;
use request::{handle, write_reply};

#[cfg(all(test, not(windows)))]
use client::finish_stop;
