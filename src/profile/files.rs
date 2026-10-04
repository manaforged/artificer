use super::model::Profile;
use anyhow::{Context, Result, bail};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const DIR: &str = "profiles";
const TIMINGS_DIR: &str = "cargo-timings";
const TIMINGS_LATEST: &str = "cargo-timing.html";
const EXTENSION: &str = "json";
const KEEP: usize = 100;
const MAX_BYTES: u64 = 256 << 20;

fn dir(home: &Path) -> PathBuf {
    home.join(DIR)
}

pub(crate) fn save(home: &Path, profile: &Profile) -> Result<()> {
    if !crate::home::ready(home) {
        return Ok(());
    }
    let dir = dir(home);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let body = serde_json::to_vec(profile)?;
    let path = dir.join(format!("{}.{EXTENSION}", profile.id));
    crate::platform::replace_atomic(&path, |tmp| fs::write(tmp, &body))
        .with_context(|| format!("write {}", path.display()))?;
    prune(&dir)
}

pub(crate) fn list(home: &Path) -> Result<Vec<String>> {
    let mut ids: Vec<String> = entries(&dir(home))?.into_iter().map(|(id, _)| id).collect();
    ids.reverse();
    Ok(ids)
}

pub(crate) fn load(home: &Path, id: &str) -> Result<Profile> {
    let path = dir(home).join(format!("{id}.{EXTENSION}"));
    let body = fs::read(&path).with_context(|| format!("read profile {id}"))?;
    let profile: Profile =
        serde_json::from_slice(&body).with_context(|| format!("parse profile {id}"))?;
    if profile.schema != super::model::SCHEMA {
        bail!(
            "profile {id} has schema {}; this Artificer reads schema {}",
            profile.schema,
            super::model::SCHEMA
        );
    }
    Ok(profile)
}

pub(crate) fn latest(home: &Path) -> Result<Option<Profile>> {
    list(home)?.first().map(|id| load(home, id)).transpose()
}

pub(crate) fn write_timings(profile: &Profile) -> Result<Option<PathBuf>> {
    let Some(target) = profile.target_dir.as_deref() else {
        return Ok(None);
    };
    let dir = Path::new(target).join(TIMINGS_DIR);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let body = super::html::render(profile);
    let path = dir.join(format!(
        "cargo-timing-{}.html",
        stamp(profile.started_at_ms)
    ));
    fs::write(&path, &body).with_context(|| format!("write {}", path.display()))?;
    let latest = dir.join(TIMINGS_LATEST);
    fs::write(&latest, &body).with_context(|| format!("write {}", latest.display()))?;
    Ok(Some(path))
}

fn stamp(unix_ms: u64) -> String {
    let digits: String = super::analyze::utc(unix_ms)
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    let (date, time) = digits.split_at(digits.len().min(8));
    format!("{date}T{time}Z")
}

fn entries(dir: &Path) -> Result<Vec<(String, u64)>> {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", dir.display())),
    };
    let mut out = Vec::new();
    for entry in read {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != EXTENSION) {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if id.starts_with('.') {
            continue;
        }
        out.push((id.to_string(), entry.metadata()?.len()));
    }
    out.sort();
    Ok(out)
}

fn prune(dir: &Path) -> Result<()> {
    let mut kept = 0usize;
    let mut bytes = 0u64;
    for (id, len) in entries(dir)?.into_iter().rev() {
        kept += 1;
        bytes += len;
        if kept <= KEEP && bytes <= MAX_BYTES {
            continue;
        }
        match fs::remove_file(dir.join(format!("{id}.{EXTENSION}"))) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("remove profile {id}")),
        }
    }
    Ok(())
}
