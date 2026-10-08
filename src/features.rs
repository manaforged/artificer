use crate::cargo::{Metadata, cargo_bin, package};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

pub fn feature_args(all: bool, features: &[String], no_default: bool) -> Vec<String> {
    let mut args = Vec::new();
    if all {
        args.push("--all-features".to_string());
    }
    if no_default {
        args.push("--no-default-features".to_string());
    }
    if !features.is_empty() {
        args.push("--features".to_string());
        args.push(features.join(","));
    }
    args
}

mod split;
mod tree;

pub use split::narrow;
#[cfg(test)]
use tree::{TreePkg, TreeSource};
use tree::{parse_tree, resolve_ids};

const TREE_TAG: &[u8] = b"tree-v4";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Normal,
    Host,
}

#[derive(Debug, Default, Clone)]
pub struct Sides {
    pub normal: Option<Vec<String>>,
    pub host: Option<Vec<String>>,
}

impl Sides {
    fn get_mut(&mut self, side: Side) -> &mut Option<Vec<String>> {
        match side {
            Side::Normal => &mut self.normal,
            Side::Host => &mut self.host,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Selection {
    feats: HashMap<String, Sides>,
    split: HashSet<String>,
}

pub fn selected(
    manifest: &Path,
    roots: &[String],
    meta: &Metadata,
    extra: &[&str],
    dev: bool,
    home: &Path,
) -> Option<Selection> {
    if std::env::var_os("ARTIFICER_NO_TREE").is_some() {
        dbg_sel("ARTIFICER_NO_TREE");
        return None;
    }
    let names: Vec<String> = roots
        .iter()
        .filter_map(|id| package(meta, id).ok())
        .map(|p| p.id.clone())
        .collect();
    if names.is_empty() {
        dbg_sel("no root names");
        return None;
    }
    let all = if dev {
        "normal,build,dev"
    } else {
        "normal,build"
    };
    let text = tree_text(manifest, &names, meta, extra, all, home)?;
    let map = resolve_ids(meta, parse_tree(&text))?;
    let sel = split::plan(meta, map)?;
    dbg_sel(&format!(
        "resolved {} packages, {} split for the host",
        sel.feats.len(),
        sel.split.len()
    ));
    Some(sel)
}

fn tree_text(
    manifest: &Path,
    names: &[String],
    meta: &Metadata,
    extra: &[&str],
    kinds: &str,
    home: &Path,
) -> Option<String> {
    let key = {
        let mut h = blake3::Hasher::new();
        h.update(TREE_TAG);
        h.update(kinds.as_bytes());
        for n in names {
            h.update(n.as_bytes());
        }
        for e in extra {
            h.update(e.as_bytes());
        }
        let dir = manifest.parent().unwrap_or(Path::new("."));
        let version = crate::key::rustc_version_in(home, dir).ok()?;
        let host = crate::key::rustc_host(&version).ok()?;
        h.update(
            crate::cargo::meta_key(manifest, extra, &host, &version)
                .ok()?
                .as_bytes(),
        );
        for pkg in &meta.packages {
            let bytes = std::fs::read(&pkg.manifest_path).ok()?;
            h.update(&(bytes.len() as u64).to_le_bytes());
            h.update(&bytes);
        }
        h.finalize().to_hex()[..32].to_string()
    };
    let cache = home.join("cargo-meta").join("cache");
    let path = cache.join(format!("tree-{key}.txt"));
    if let Some(hit) = tree_ram_get(&key)
        && !parse_tree(&hit).is_empty()
    {
        return Some(hit);
    }
    let cached = read_tree(&path);
    let mut fresh = cached.is_none();
    let text = match cached {
        Some(hit) => hit,
        None => {
            let _hold = crate::store::hold(home, &format!("tree-{key}")).ok();
            if let Some(hit) = read_tree(&path) {
                fresh = false;
                hit
            } else {
                let mut cmd = crate::cargo::real_cargo_command().ok()?;
                cmd.current_dir(manifest.parent().unwrap_or(Path::new(".")));
                cmd.env("CARGO", cargo_bin().ok()?);
                crate::jobs::isolate(&mut cmd);
                cmd.env("CARGO_TARGET_DIR", home.join("cargo-meta"));
                cmd.args([
                    "tree", "--color", "never", "--edges", kinds, "--prefix", "indent",
                ]);
                cmd.arg("--format");
                cmd.arg("{p}|{f}");
                cmd.arg("--manifest-path").arg(manifest);
                for n in names {
                    cmd.arg("-p").arg(n);
                }
                cmd.args(extra);
                let out = match cmd.output() {
                    Ok(out) => out,
                    Err(e) => {
                        dbg_sel(&format!("spawn failed: {e}"));
                        return None;
                    }
                };
                if !out.status.success() {
                    dbg_sel(&format!(
                        "cargo tree exited {:?}: {}",
                        out.status.code(),
                        String::from_utf8_lossy(&out.stderr)
                            .lines()
                            .next()
                            .unwrap_or("")
                    ));
                    return None;
                }
                String::from_utf8(out.stdout).ok()?
            }
        }
    };
    if parse_tree(&text).is_empty() {
        dbg_sel(&format!(
            "empty map after parse: {} bytes, first line {:?}, names {:?}",
            text.len(),
            text.lines().next().unwrap_or(""),
            names,
        ));
        return None;
    }
    if fresh {
        drop(std::fs::create_dir_all(&cache));
        drop(std::fs::write(&path, &text));
    }
    tree_ram_put(&key, text.clone());
    Some(text)
}

fn tree_ram() -> &'static Mutex<HashMap<String, String>> {
    static M: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn tree_ram_get(key: &str) -> Option<String> {
    tree_ram().lock().ok()?.get(key).cloned()
}

fn tree_ram_put(key: &str, text: String) {
    if let Ok(mut g) = tree_ram().lock() {
        g.insert(key.to_string(), text);
    }
}

fn read_tree(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .filter(|text| !parse_tree(text).is_empty())
}

fn dbg_sel(msg: &str) {
    if std::env::var_os("ARTIFICER_DEBUG_SEL").is_some() {
        crate::out::diag(format!("SEL {msg}"));
    }
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
