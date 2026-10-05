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
    pub rmeta: bool,
    #[serde(rename = "slim-deps")]
    pub slim_deps: bool,
    #[serde(rename = "meta-cache")]
    pub meta_cache: bool,
    pub threads: bool,
    pub trust: bool,
    pub early: bool,
    pub serve: bool,
}

impl Default for Mods {
    fn default() -> Self {
        Self {
            enabled: true,
            sweep: false,
            rmeta: true,
            slim_deps: true,
            meta_cache: true,
            threads: false,
            trust: true,
            early: false,
            serve: false,
        }
    }
}

impl Mods {
    pub fn names() -> &'static [&'static str] {
        &[
            "enabled",
            "sweep",
            "rmeta",
            "slim-deps",
            "meta-cache",
            "threads",
            "trust",
            "early",
            "serve",
        ]
    }

    fn field(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "enabled" => &mut self.enabled,
            "sweep" => &mut self.sweep,
            "rmeta" => &mut self.rmeta,
            "slim-deps" => &mut self.slim_deps,
            "meta-cache" => &mut self.meta_cache,
            "threads" => &mut self.threads,
            "trust" => &mut self.trust,
            "early" => &mut self.early,
            "serve" => &mut self.serve,
            _ => return None,
        })
    }

    pub fn get(&self, name: &str) -> Result<bool> {
        match self.clone().field(name) {
            Some(on) => Ok(*on),
            None => bail!("unknown mod: {name}"),
        }
    }

    pub fn set(&mut self, name: &str, on: bool) -> Result<()> {
        if name == "serve" && on && cfg!(windows) {
            bail!("the serve daemon is not available on Windows");
        }
        match self.field(name) {
            Some(field) => *field = on,
            None => bail!("unknown mod: {name}"),
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
        "cranelift",
        "linker",
        "slim",
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
