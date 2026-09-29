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
    if let Some(k) = &key
        && let Some(hit) = disk_meta(home, k, manifest)
    {
        meta_ram_put(&ram_k, hit.clone());
        return Ok(hit);
    }
    let _hold = match &key {
        Some(k) => Some(store::hold(home, &format!("meta-{k}"))?),
        None => None,
    };
    if let Some(k) = &key
        && let Some(hit) = disk_meta(home, k, manifest)
    {
        meta_ram_put(&ram_k, hit.clone());
        return Ok(hit);
    }
    let mut cmd = Command::new(cargo_bin());
    cmd.current_dir(dir);
    cmd.env("CARGO", cargo_bin());
    crate::jobs::isolate(&mut cmd);
    let meta_target = home.join("cargo-meta");
    cmd.env("CARGO_TARGET_DIR", &meta_target);
    cmd.args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(manifest)
        .arg("--filter-platform")
        .arg(&host)
        .args(extra);
    let out =
        crate::out::timed("cargo-metadata", || cmd.output()).context("spawn cargo metadata")?;
    if !out.status.success() {
        bail!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let parsed: Metadata = match serde_json::from_slice(&out.stdout) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Err(Unmodeled(format!("cargo metadata format is not modeled: {error}")).into());
        }
    };
    if let Some(k) = &key
        && local_only(&parsed, &parsed.workspace_root)
    {
        let dir = home.join("cargo-meta").join("cache");
        drop(std::fs::create_dir_all(&dir));
        drop(std::fs::write(dir.join(format!("{k}.json")), &out.stdout));
        drop(std::fs::write(
            dir.join(format!("{k}.lock")),
            lock_digest(manifest),
        ));
    }
    if mods.meta_cache && local_only(&parsed, &parsed.workspace_root) {
        meta_ram_put(&ram_k, parsed.clone());
    }
    Ok(parsed)
}
