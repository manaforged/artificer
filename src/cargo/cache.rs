use super::*;

pub(super) fn disk_meta(home: &Path, key: &str, manifest: &Path) -> Option<Metadata> {
    let p = home
        .join("cargo-meta")
        .join("cache")
        .join(format!("{key}.json"));
    let stamp = p.with_extension("lock");
    let lock_now = lock_digest(manifest);
    let matches = std::fs::read_to_string(&stamp)
        .map(|was| was == lock_now)
        .unwrap_or(false);
    if !matches {
        return None;
    }
    let bytes = std::fs::read(&p).ok()?;
    let mut m = serde_json::from_slice::<Metadata>(&bytes).ok()?;
    reroot(&mut m, manifest).then_some(m)
}

pub(super) fn local_only(meta: &Metadata, root: &Path) -> bool {
    let cargo_home = cargo_home();
    meta.packages.iter().all(|p| {
        p.manifest_path.starts_with(root)
            || (p.source.is_some()
                && ["registry", "git"]
                    .iter()
                    .any(|dir| p.manifest_path.starts_with(cargo_home.join(dir))))
    })
}

pub(super) fn reroot(meta: &mut Metadata, manifest: &Path) -> bool {
    let old = meta.workspace_root.clone();
    if old.as_os_str().is_empty() {
        return false;
    }
    let root_manifest = manifest
        .parent()
        .is_some_and(|d| d.join("Cargo.toml") == manifest && old.join("Cargo.toml") != *manifest)
        && meta
            .packages
            .iter()
            .all(|p| p.source.is_some() || p.manifest_path != *manifest);
    let new = meta
        .packages
        .iter()
        .filter(|p| p.source.is_none())
        .find_map(|p| {
            let rel = p.manifest_path.strip_prefix(&old).ok()?;
            if !manifest.ends_with(rel) {
                return None;
            }
            let mut root = manifest.to_path_buf();
            for _ in 0..rel.components().count() {
                root.pop();
            }
            Some(root)
        })
        .or_else(|| {
            if *manifest == old.join("Cargo.toml") || root_manifest {
                manifest.parent().map(Path::to_path_buf)
            } else {
                None
            }
        });
    let Some(new) = new else {
        return false;
    };
    if new != old {
        let swap = |p: &mut PathBuf| {
            if let Ok(rel) = p.strip_prefix(&old) {
                *p = new.join(rel);
            }
        };
        let from = format!("path+file://{}", file_url(&old));
        let to = format!("path+file://{}", file_url(&new));
        let move_id = |id: &mut String| {
            if let Some(rest) = id.strip_prefix(&from)
                && (rest.starts_with('/') || rest.starts_with('#'))
            {
                *id = format!("{to}{rest}");
            }
        };
        for pkg in &mut meta.packages {
            swap(&mut pkg.manifest_path);
            if let Some(file) = &mut pkg.license_file {
                swap(file);
            }
            for t in &mut pkg.targets {
                swap(&mut t.src_path);
            }
            move_id(&mut pkg.id);
        }
        meta.workspace_members.iter_mut().for_each(move_id);
        if let Some(resolve) = &mut meta.resolve {
            if let Some(root) = &mut resolve.root {
                move_id(root);
            }
            for node in &mut resolve.nodes {
                move_id(&mut node.id);
                for dep in &mut node.deps {
                    move_id(&mut dep.pkg);
                }
            }
        }
        meta.workspace_root = new.clone();
        meta.pkg_ix = OnceLock::new();
        meta.node_ix = OnceLock::new();
        let stale = |id: &String| id.starts_with("path+file://") && !id.starts_with(&to);
        if meta.packages.iter().any(|p| stale(&p.id)) || meta.workspace_members.iter().any(stale) {
            return false;
        }
    }
    local_only(meta, &new)
        && (new.join("Cargo.toml") == manifest
            || meta.packages.iter().any(|p| p.manifest_path == manifest))
}

fn file_url(path: &Path) -> String {
    let path = crate::platform::env_path(path);
    let text = path.to_string_lossy();
    #[cfg(windows)]
    let text = text.replace('\\', "/");
    #[cfg(windows)]
    let text = if let Some(unc) = text.strip_prefix("//") {
        unc.to_string()
    } else {
        format!("/{text}")
    };
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn meta_ram() -> &'static std::sync::Mutex<std::collections::HashMap<String, Metadata>> {
    static M: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Metadata>>> =
        std::sync::OnceLock::new();
    M.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub(super) fn meta_ram_get(key: &str) -> Option<Metadata> {
    meta_ram().lock().ok()?.get(key).cloned()
}

pub(super) fn meta_ram_put(key: &str, meta: Metadata) {
    if let Ok(mut g) = meta_ram().lock() {
        g.insert(key.to_string(), meta);
    }
}

pub(crate) fn meta_key(manifest: &Path, extra: &[&str], host: &str, rustc: &str) -> Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"metadata-v2");
    hasher.update(cargo_home().as_os_str().as_encoded_bytes());
    for path in
        crate::config::config_files(manifest.parent().unwrap_or(Path::new(".")), &cargo_home())
    {
        let bytes = std::fs::read(&path)?;
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    hasher.update(host.as_bytes());
    hasher.update(rustc.as_bytes());
    hasher.update(&std::fs::read(manifest).with_context(|| manifest.display().to_string())?);
    if let Some(lock) = nearest_lock(manifest)
        && let Ok(bytes) = std::fs::read(&lock)
    {
        hasher.update(&bytes);
    }
    if let Some(root) = workspace_dir(manifest) {
        hash_manifests(&mut hasher, &root);
    }
    for e in extra {
        hasher.update(e.as_bytes());
    }
    Ok(hasher.finalize().to_hex()[..32].to_string())
}

pub(crate) fn workspace_dir(manifest: &Path) -> Option<PathBuf> {
    let mut top = None;
    let mut dir = manifest.parent();
    while let Some(d) = dir {
        if d.join("Cargo.toml").is_file() {
            top = Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    top
}

const TARGET_DIRS: [&str; 4] = ["tests", "examples", "benches", "src/bin"];

pub(crate) fn hash_manifests(hasher: &mut blake3::Hasher, root: &Path) {
    let mut found = Vec::new();
    collect_manifests(root, &mut found);
    found.sort();
    for path in found {
        let name = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        hasher.update(&(name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        if let Ok(bytes) = std::fs::read(&path) {
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(&bytes);
        }
        if let Some(pkg_dir) = path.parent() {
            for source in ["src/lib.rs", "src/main.rs"] {
                hasher.update(source.as_bytes());
                hasher.update(&[u8::from(pkg_dir.join(source).is_file())]);
            }
            for sub in TARGET_DIRS {
                let dir = pkg_dir.join(sub);
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                let mut names: Vec<String> = entries
                    .flatten()
                    .filter_map(|e| {
                        if e.path().is_file() {
                            Some(e.file_name().to_string_lossy().into_owned())
                        } else if e.path().join("main.rs").is_file() {
                            Some(format!("{}/main.rs", e.file_name().to_string_lossy()))
                        } else {
                            None
                        }
                    })
                    .collect();
                names.sort();
                hasher.update(sub.as_bytes());
                for n in names {
                    hasher.update(&(n.len() as u64).to_le_bytes());
                    hasher.update(n.as_bytes());
                }
            }
        }
    }
}

fn collect_manifests(dir: &Path, found: &mut Vec<PathBuf>) {
    let manifest = dir.join("Cargo.toml");
    if manifest.is_file() {
        found.push(manifest);
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || matches!(name.as_ref(), "target" | "node_modules") {
            continue;
        }
        collect_manifests(&entry.path(), found);
    }
}
