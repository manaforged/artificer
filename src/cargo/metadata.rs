use super::*;

pub fn metadata_extra(manifest: &Path, extra: &[&str], home: &Path) -> Result<Metadata> {
    let dir = manifest.parent().unwrap_or(Path::new("."));
    let rustc = crate::key::rustc_version_in(home, dir)?;
    let host = crate::key::rustc_host(&rustc)?;
    let mods = crate::mods::load(home)?;
    let content_key = meta_key(manifest, extra, &host, &rustc)?;
    let ram_k = format!("{}|{}|{content_key}", home.display(), manifest.display());
    if mods.meta_cache
        && let Some(hit) = meta_ram_get(&ram_k)
    {
        return Ok(hit);
    }
    let key = mods.meta_cache.then_some(content_key);
    if let Some(hit) = cached_on_disk(home, key.as_deref(), manifest, &ram_k) {
        return Ok(hit);
    }
    let _hold = key
        .as_deref()
        .map(|k| store::hold(home, &format!("meta-{k}")))
        .transpose()?;
    if let Some(hit) = cached_on_disk(home, key.as_deref(), manifest, &ram_k) {
        return Ok(hit);
    }
    let (parsed, raw) = run_metadata(manifest, dir, &host, extra, home)?;
    if local_only(&parsed, &parsed.workspace_root) {
        if let Some(k) = &key {
            remember_on_disk(home, k, manifest, &raw);
        }
        if mods.meta_cache {
            meta_ram_put(&ram_k, parsed.clone());
        }
    }
    Ok(parsed)
}

fn cached_on_disk(
    home: &Path,
    key: Option<&str>,
    manifest: &Path,
    ram_k: &str,
) -> Option<Metadata> {
    let hit = disk_meta(home, key?, manifest)?;
    meta_ram_put(ram_k, hit.clone());
    Some(hit)
}

fn run_metadata(
    manifest: &Path,
    dir: &Path,
    host: &str,
    extra: &[&str],
    home: &Path,
) -> Result<(Metadata, Vec<u8>)> {
    let mut cmd = Command::new(cargo_bin());
    cmd.current_dir(dir);
    cmd.env("CARGO", cargo_bin());
    crate::jobs::isolate(&mut cmd);
    cmd.env("CARGO_TARGET_DIR", home.join("cargo-meta"));
    cmd.args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(manifest)
        .arg("--filter-platform")
        .arg(host)
        .args(extra);
    let out = crate::profile::span(crate::profile::SetupPhase::CargoMetadata, || {
        crate::profile::output(&mut cmd)
    })
    .context("spawn cargo metadata")?;
    if !out.status.success() {
        bail!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let parsed: Metadata = serde_json::from_slice(&out.stdout)
        .map_err(|error| Unmodeled(format!("cargo metadata format is not modeled: {error}")))?;
    Ok((parsed, out.stdout))
}

fn remember_on_disk(home: &Path, key: &str, manifest: &Path, raw: &[u8]) {
    let dir = home.join("cargo-meta").join("cache");
    drop(std::fs::create_dir_all(&dir));
    drop(std::fs::write(dir.join(format!("{key}.json")), raw));
    drop(std::fs::write(
        dir.join(format!("{key}.lock")),
        lock_digest(manifest),
    ));
}
