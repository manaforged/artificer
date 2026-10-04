use crate::cargo::{Metadata, Package, cargo_bin, package};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

const PROC_MACRO: &str = " (proc-macro)";
const CRATES_IO: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum TreeSource {
    CratesIo,
    Path(PathBuf),
    Git { url: String, commit: String },
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TreePkg {
    name: String,
    version: String,
    source: TreeSource,
}

impl TreeSource {
    fn parse(text: &str) -> Self {
        if text.is_empty() {
            return Self::CratesIo;
        }
        if Path::new(text).is_absolute() {
            return Self::Path(PathBuf::from(text));
        }
        match text.rsplit_once('#') {
            Some((url, commit)) if url.contains("://") => Self::Git {
                url: url.to_string(),
                commit: commit.to_string(),
            },
            _ => Self::Other(text.to_string()),
        }
    }

    fn matches(&self, pkg: &Package) -> bool {
        match (self, pkg.source.as_deref()) {
            (Self::Path(dir), None) => pkg.manifest_path.parent() == Some(dir.as_path()),
            (Self::Git { url, commit }, Some(src)) => src
                .strip_prefix("git+")
                .and_then(|rest| rest.rsplit_once('#'))
                .is_some_and(|(locator, full)| {
                    locator == url && !commit.is_empty() && full.starts_with(commit.as_str())
                }),
            (Self::CratesIo, Some(src)) => CRATES_IO.contains(&src),
            _ => false,
        }
    }
}

fn tree_line(line: &str) -> Option<(TreePkg, Vec<&str>)> {
    let (head, feats) = line.split_once('|')?;
    let feats = feats.trim_end();
    let feats = feats.strip_suffix("(*)").map_or(feats, str::trim_end);
    let head = head.trim();
    let (ident, rest) = head
        .split_once(" (")
        .map_or((head, ""), |(ident, _)| (ident, &head[ident.len()..]));
    let rest = rest.strip_prefix(PROC_MACRO).unwrap_or(rest);
    let source = match rest.strip_prefix(" (") {
        Some(inner) => inner.strip_suffix(')')?,
        None if rest.is_empty() => "",
        None => return None,
    };
    let (name, version) = ident.rsplit_once(" v")?;
    let key = TreePkg {
        name: name.trim().to_string(),
        version: version.trim().to_string(),
        source: TreeSource::parse(source),
    };
    Some((key, feats.split(',').filter(|f| !f.is_empty()).collect()))
}

fn resolve_ids(
    meta: &Metadata,
    tree: HashMap<TreePkg, Vec<String>>,
) -> Option<HashMap<String, Vec<String>>> {
    let mut ids = HashMap::new();
    for (key, feats) in tree {
        let same: Vec<&Package> = meta
            .packages
            .iter()
            .filter(|p| p.name == key.name && p.version == key.version)
            .collect();
        let pkg = match same.as_slice() {
            [] => continue,
            [only] => *only,
            _ => {
                let mut hit = same.iter().copied().filter(|p| key.source.matches(p));
                let (Some(one), None) = (hit.next(), hit.next()) else {
                    dbg_sel(&format!("no exact package for {key:?}"));
                    return None;
                };
                one
            }
        };
        if ids.insert(pkg.id.clone(), feats).is_some() {
            dbg_sel(&format!("two tree entries map to {}", pkg.id));
            return None;
        }
    }
    Some(ids)
}

pub fn selected(
    manifest: &Path,
    roots: &[String],
    meta: &Metadata,
    extra: &[&str],
    dev: bool,
    home: &Path,
) -> Option<HashMap<String, Vec<String>>> {
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
    if !contexts_agree(&text) {
        dbg_sel("a package resolves different features for build scripts and normal code");
        return None;
    }
    let map = resolve_ids(meta, parse_tree(&text))?;
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
        h.update(b"tree-v3");
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
    let mut seen: HashMap<TreePkg, std::collections::BTreeSet<&str>> = HashMap::new();
    text.lines().filter_map(tree_line).all(|(key, feats)| {
        let set = feats.into_iter().collect();
        match seen.entry(key) {
            std::collections::hash_map::Entry::Occupied(was) => *was.get() == set,
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(set);
                true
            }
        }
    })
}

fn parse_tree(text: &str) -> HashMap<TreePkg, Vec<String>> {
    let mut map: HashMap<TreePkg, Vec<String>> = HashMap::new();
    for (key, feats) in text.lines().filter_map(tree_line) {
        let slot = map.entry(key).or_default();
        for f in feats {
            if !slot.iter().any(|have| have == f) {
                slot.push(f.to_string());
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

pub fn narrow(meta: &mut Metadata, feats: &HashMap<String, Vec<String>>) {
    let Some(resolve) = meta.resolve.as_mut() else {
        return;
    };
    resolve.nodes.retain(|n| feats.contains_key(&n.id));
    for node in &mut resolve.nodes {
        if let Some(f) = feats.get(&node.id) {
            node.features.clone_from(f);
        }
        node.deps.retain(|d| feats.contains_key(&d.pkg));
    }
    meta.node_ix = OnceLock::new();
    meta.pkg_ix = OnceLock::new();
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
