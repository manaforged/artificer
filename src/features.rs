use crate::cargo::{Metadata, cargo_bin, package};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
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

pub fn selected(
    manifest: &Path,
    roots: &[String],
    meta: &Metadata,
    extra: &[&str],
    dev: bool,
    home: &Path,
) -> Option<HashMap<(String, String), Vec<String>>> {
    if std::env::var_os("ARTIFICER_NO_TREE").is_some() {
        dbg_sel("ARTIFICER_NO_TREE");
        return None;
    }
    let names: Vec<String> = roots
        .iter()
        .filter_map(|id| package(meta, id).ok())
        .map(|p| format!("{}@{}", p.name, p.version))
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
    if !contexts_agree(&text) {
        dbg_sel("a package resolves different features for build scripts and normal code");
        return None;
    }
    let map = parse_tree(&text);
    dbg_sel(&format!("resolved {} packages", map.len()));
    Some(map)
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
        h.update(b"tree-v2");
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
                let mut cmd = Command::new(cargo_bin());
                cmd.current_dir(manifest.parent().unwrap_or(Path::new(".")));
                cmd.env("CARGO", cargo_bin());
                crate::jobs::isolate(&mut cmd);
                cmd.env("CARGO_TARGET_DIR", home.join("cargo-meta"));
                cmd.args([
                    "tree", "--color", "never", "--edges", kinds, "--prefix", "none",
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

fn contexts_agree(text: &str) -> bool {
    let mut seen: HashMap<&str, std::collections::BTreeSet<&str>> = HashMap::new();
    text.lines().all(|line| {
        let Some((head, feats)) = line.split_once('|') else {
            return true;
        };
        let feats = feats.trim_end();
        let feats = feats.strip_suffix("(*)").map_or(feats, str::trim_end);
        let head = head.split(" (").next().unwrap_or(head).trim();
        let set = feats.split(',').filter(|f| !f.is_empty()).collect();
        match seen.entry(head) {
            std::collections::hash_map::Entry::Occupied(was) => *was.get() == set,
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(set);
                true
            }
        }
    })
}

fn parse_tree(text: &str) -> HashMap<(String, String), Vec<String>> {
    let mut map: HashMap<(String, String), Vec<String>> = HashMap::new();
    for line in text.lines() {
        let Some((head, feats)) = line.split_once('|') else {
            continue;
        };
        let feats = feats.trim_end();
        let feats = feats.strip_suffix("(*)").map_or(feats, str::trim_end);
        let head = head.split(" (").next().unwrap_or(head).trim();
        let Some((name, version)) = head.rsplit_once(" v") else {
            continue;
        };
        let feats: Vec<String> = feats
            .split(',')
            .filter(|f| !f.is_empty())
            .map(str::to_string)
            .collect();
        let slot = map
            .entry((name.trim().to_string(), version.trim().to_string()))
            .or_default();
        for f in feats {
            if !slot.contains(&f) {
                slot.push(f);
            }
        }
    }
    map
}

fn dbg_sel(msg: &str) {
    if std::env::var_os("ARTIFICER_DEBUG_SEL").is_some() {
        eprintln!("SEL {msg}");
    }
}

pub fn narrow(meta: &mut Metadata, sel: &HashMap<(String, String), Vec<String>>) {
    let keep: std::collections::HashSet<String> = meta
        .packages
        .iter()
        .filter(|p| sel.contains_key(&(p.name.clone(), p.version.clone())))
        .map(|p| p.id.clone())
        .collect();
    let feats: HashMap<String, Vec<String>> = meta
        .packages
        .iter()
        .filter_map(|p| {
            sel.get(&(p.name.clone(), p.version.clone()))
                .map(|f| (p.id.clone(), f.clone()))
        })
        .collect();
    let Some(resolve) = meta.resolve.as_mut() else {
        return;
    };
    resolve.nodes.retain(|n| keep.contains(&n.id));
    for node in &mut resolve.nodes {
        if let Some(f) = feats.get(&node.id) {
            node.features.clone_from(f);
        }
        node.deps.retain(|d| keep.contains(&d.pkg));
    }
    meta.node_ix = OnceLock::new();
    meta.pkg_ix = OnceLock::new();
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
