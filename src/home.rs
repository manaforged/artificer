use std::path::{Path, PathBuf};

pub fn control_home() -> PathBuf {
    dirs_home().join(".artificer")
}

pub fn env_script() -> String {
    env_script_for(&control_home().join("bin"))
}

pub(crate) fn env_script_for(bin: &Path) -> String {
    #[cfg(unix)]
    {
        let quoted = sh_quote(&bin.display().to_string());
        format!(
            "case \"$PATH\" in {quoted}|{quoted}:*) ;; *) export PATH={quoted}:\"$PATH\" ;; esac\n"
        )
    }
    #[cfg(windows)]
    {
        let quoted = bin.display().to_string().replace('\'', "''");
        format!(
            "if (($env:Path -split ';', 2)[0] -ne '{quoted}') {{ $env:Path = '{quoted};' + $env:Path }}\n"
        )
    }
}

#[cfg(unix)]
pub(crate) fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub(crate) fn resolve_path(p: &Path) -> PathBuf {
    crate::platform::env_path(&p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
}

const TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n# Artificer compile cache. `artificer uninstall --purge` deletes this directory.\n";

const LEGACY_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n";

const LEGACY_MARKERS: [&str; 3] = ["mods.toml", "builds.jsonl", "stat.hits"];

fn legacy(home: &Path) -> bool {
    std::fs::read_to_string(home.join("CACHEDIR.TAG")).is_ok_and(|text| text == LEGACY_TAG)
        && home.join("units").is_dir()
        && LEGACY_MARKERS
            .iter()
            .any(|marker| home.join(marker).is_file())
}

pub(crate) fn owned(home: &Path) -> bool {
    let tag = home.join("CACHEDIR.TAG");
    std::fs::read_to_string(&tag).is_ok_and(|text| text == TAG)
}

fn home_from(env: Option<&str>, cache: &Path) -> PathBuf {
    match env {
        Some(home) => resolve_path(Path::new(home)),
        None => cache.join("artificer"),
    }
}

pub fn default_home() -> PathBuf {
    home_from(
        std::env::var("ARTIFICER_HOME").ok().as_deref(),
        &cache_dir(),
    )
}

pub fn purge(home: &Path) -> std::io::Result<bool> {
    if !owned(home) {
        return Ok(false);
    }
    std::fs::remove_dir_all(home)?;
    Ok(true)
}

pub fn claim(home: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        ready(home),
        "{} is not an Artificer store: it already holds other files",
        home.display()
    );
    Ok(())
}

pub fn ready(home: &Path) -> bool {
    let tag = home.join("CACHEDIR.TAG");
    if std::fs::create_dir_all(home).is_err() {
        return false;
    }
    if owned(home) {
        return true;
    }
    if legacy(home) {
        return write_tag(&tag).is_ok();
    }
    if tag.exists() {
        return false;
    }
    let empty = std::fs::read_dir(home).is_ok_and(|mut entries| entries.next().is_none());
    empty && write_tag(&tag).is_ok()
}

fn write_tag(tag: &Path) -> std::io::Result<()> {
    crate::platform::replace_atomic(tag, |tmp| std::fs::write(tmp, TAG))
}

#[cfg(target_os = "macos")]
fn cache_dir() -> PathBuf {
    dirs_home().join("Library").join("Caches")
}

#[cfg(windows)]
fn cache_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs_home().join("AppData").join("Local"))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| dirs_home().join(".cache"))
}

pub(crate) fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;
