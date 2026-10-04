use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const REMOTE_ENV: &str = "ARTIFICER_REMOTE";
pub const PULL_EVERY: Duration = Duration::from_secs(15 * 60);
const CONFIG_FILE: &str = "remote.toml";
const LOCATION_KEY: &str = "location";
const PULL_STAMP: &str = "pull.stamp";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    Ssh { host: String, path: String },
    Dir(PathBuf),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteFile {
    location: String,
}

impl Location {
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            bail!("a remote location is required");
        }
        if let Some((host, path)) = raw.split_once(':')
            && remote_host(host)
            && path.starts_with('/')
        {
            return Ok(Self::Ssh {
                host: host.to_string(),
                path: path.trim_end_matches('/').to_string(),
            });
        }
        let dir = PathBuf::from(raw);
        if !dir.is_absolute() {
            bail!("remote `{raw}` must be an absolute directory or HOST:/ABSOLUTE/PATH");
        }
        Ok(Self::Dir(dir))
    }
}

fn remote_host(host: &str) -> bool {
    let drive = host.len() == 1 && host.chars().all(|c| c.is_ascii_alphabetic());
    !host.is_empty() && !drive && !host.contains(['/', '\\'])
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ssh { host, path } => write!(f, "{host}:{path}"),
            Self::Dir(dir) => write!(f, "{}", dir.display()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Env,
    Config,
}

pub fn configured(home: &Path) -> Result<Option<(Location, Source)>> {
    if let Some(raw) = std::env::var_os(REMOTE_ENV) {
        let raw = raw
            .to_str()
            .with_context(|| format!("{REMOTE_ENV} must be Unicode"))?;
        if raw.trim().is_empty() {
            return Ok(None);
        }
        return Ok(Some((Location::parse(raw)?, Source::Env)));
    }
    let path = home.join(CONFIG_FILE);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let file: RemoteFile =
        toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Location::parse(&file.location)
        .map(|location| Some((location, Source::Config)))
        .with_context(|| format!("parse {}", path.display()))
}

pub fn set(home: &Path, location: Option<&Location>) -> Result<()> {
    crate::home::claim(home)?;
    let path = home.join(CONFIG_FILE);
    let Some(location) = location else {
        return match fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(error).with_context(|| format!("remove {}", path.display()))
            }
            _ => Ok(()),
        };
    };
    let value = serde_json::to_string(&location.to_string())?;
    fs::write(&path, format!("{LOCATION_KEY} = {value}\n"))
        .with_context(|| format!("write {}", path.display()))
}

pub(crate) fn pull_due(home: &Path) {
    let Ok(Some(_)) = configured(home) else {
        return;
    };
    let stamp = home.join(PULL_STAMP);
    if !crate::maintenance::stale(&stamp, PULL_EVERY) {
        return;
    }
    if fs::write(&stamp, "").is_err() {
        return;
    }
    if let Err(error) = spawn_pull(home) {
        crate::out::err(format!("artificer: background pull skipped: {error:#}"));
    }
}

mod fetch;
mod pull;
pub(crate) use fetch::fetch;
pub use pull::{pull, spawn_pull};
