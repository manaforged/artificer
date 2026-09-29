use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

type ModsCache = HashMap<PathBuf, (Option<std::time::SystemTime>, Mods)>;

fn cache() -> &'static Mutex<ModsCache> {
    static CACHE: OnceLock<Mutex<ModsCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn stamp(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mods {
    pub enabled: bool,
    pub sweep: bool,
    pub cranelift: bool,
    pub rmeta: bool,
    pub slim: bool,
    pub linker: bool,
    #[serde(rename = "meta-cache")]
    pub meta_cache: bool,
    pub threads: bool,
    pub serve: bool,
}

impl Default for Mods {
    fn default() -> Self {
        Self {
            enabled: true,
            sweep: false,
            cranelift: false,
            rmeta: true,
            slim: false,
            linker: false,
            meta_cache: true,
            threads: false,
            serve: false,
        }
    }
}

impl Mods {
    pub fn names() -> &'static [&'static str] {
        &[
            "enabled",
            "sweep",
            "cranelift",
            "rmeta",
            "slim",
            "linker",
            "meta-cache",
            "threads",
            "serve",
        ]
    }

    pub fn get(&self, name: &str) -> Result<bool> {
        Ok(match name {
            "enabled" => self.enabled,
            "sweep" => self.sweep,
            "cranelift" => self.cranelift,
            "rmeta" => self.rmeta,
            "slim" => self.slim,
            "linker" => self.linker,
            "meta-cache" => self.meta_cache,
            "threads" => self.threads,
            "serve" => self.serve,
            other => bail!("unknown mod: {other}"),
        })
    }

    pub fn set(&mut self, name: &str, on: bool) -> Result<()> {
        match name {
            "enabled" => self.enabled = on,
            "sweep" => self.sweep = on,
            "cranelift" => self.cranelift = on,
            "rmeta" => self.rmeta = on,
            "slim" => self.slim = on,
            "linker" => self.linker = on,
            "meta-cache" => self.meta_cache = on,
            "threads" => self.threads = on,
            "serve" if on && cfg!(windows) => {
                bail!("the serve daemon is not available on Windows")
            }
            "serve" => self.serve = on,
            other => bail!("unknown mod: {other}"),
        }
        Ok(())
    }

    pub fn table(&self) -> BTreeMap<&'static str, bool> {
        Mods::names()
            .iter()
            .map(|n| (*n, self.get(n).expect("declared mod")))
            .collect()
    }
}

pub fn enabled(home: &Path) -> Result<bool> {
    if std::env::var_os("ARTIFICER_DISABLED").is_some() {
        return Ok(false);
    }
    Ok(load(home)?.enabled)
}

pub fn path(home: &Path) -> PathBuf {
    home.join("mods.toml")
}

pub fn load(home: &Path) -> Result<Mods> {
    let p = path(home);
    let key = crate::resolve_path(&p);
    let now = stamp(&p);
    if let Ok(map) = cache().lock()
        && let Some((old, hit)) = map.get(&key)
        && *old == now
    {
        return Ok(hit.clone());
    }
    let raw = match fs::read_to_string(&p) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mods = Mods::default();
            if let Ok(mut map) = cache().lock() {
                map.insert(key, (now, mods.clone()));
            }
            return Ok(mods);
        }
        Err(error) => return Err(error).with_context(|| format!("read {}", p.display())),
    };
    let mods = parse(&raw).with_context(|| format!("parse {}", p.display()))?;
    if let Ok(mut map) = cache().lock() {
        map.insert(key, (now, mods.clone()));
    }
    Ok(mods)
}

pub fn save(home: &Path, mods: &Mods) -> Result<()> {
    crate::home::claim(home)?;
    let mut body =
        String::from("# Artificer modes. Use `artificer mods on|off NAME` to change one.\n");
    for name in Mods::names() {
        let on = mods.get(name)?;
        body.push_str(&format!("{name} = {on}\n"));
    }
    fs::write(path(home), body)?;
    let key = crate::resolve_path(&path(home));
    if let Ok(mut map) = cache().lock() {
        map.insert(key, (stamp(&path(home)), mods.clone()));
    }
    Ok(())
}

fn parse(raw: &str) -> Result<Mods> {
    let mut value: toml::Table = toml::from_str(raw)?;
    for name in [
        "source-hash",
        "isolate",
        "src-cache",
        "thin-emit",
        "lock",
        "units",
        "hardlink",
    ] {
        if let Some(value) = value.remove(name) {
            anyhow::ensure!(value.is_bool(), "{name} must be a boolean");
        }
    }
    Ok(toml::Value::Table(value).try_into()?)
}

#[cfg(test)]
#[path = "mods_tests.rs"]
mod tests;
