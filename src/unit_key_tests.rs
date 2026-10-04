use super::{from_registry, from_registry_in, registry_only};
use crate::cargo::Package;
use anyhow::{Context, Result};
use std::fs;

fn pkg(source: Option<&str>, manifest: &str) -> Result<Package> {
    serde_json::from_value(serde_json::json!({
        "name": "x",
        "version": "1.0.0",
        "id": "registry+https://github.com/rust-lang/crates.io-index#x@1.0.0",
        "source": source,
        "manifest_path": manifest,
        "targets": [],
    }))
    .context("deserialize package fixture")
}

#[test]
fn vendored_sources_are_not_registry_sources() -> Result<()> {
    let home = crate::cargo::cargo_home();
    let real = home.join("registry/src/index/x-1.0.0/Cargo.toml");
    assert!(from_registry(&pkg(
        Some("registry+..."),
        &real.to_string_lossy()
    )?));
    assert!(!from_registry(&pkg(
        Some("registry+..."),
        "/work/repo/vendor/x/Cargo.toml"
    )?));
    assert!(!from_registry(&pkg(
        None,
        "/work/repo/crates/x/Cargo.toml"
    )?));
    Ok(())
}

#[test]
fn registry_match_survives_a_symlink_cargo_home() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let real = tmp.path().join("cargo-home");
    let src = real.join("registry/src/index/x-1.0.0");
    fs::create_dir_all(&src)?;
    let manifest = src.join("Cargo.toml");
    fs::write(&manifest, "[package]\nname = \"x\"\nversion = \"1.0.0\"\n")?;
    let link = tmp.path().join("dot-cargo");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link)?;
    #[cfg(not(unix))]
    std::os::windows::fs::symlink_dir(&real, &link)?;
    let canonical = manifest.canonicalize()?;
    assert!(from_registry_in(
        &pkg(Some("registry+..."), &canonical.to_string_lossy())?,
        &link
    ));
    assert!(from_registry_in(
        &pkg(
            Some("registry+..."),
            &link
                .join("registry/src/index/x-1.0.0/Cargo.toml")
                .to_string_lossy()
        )?,
        &link
    ));
    Ok(())
}

#[test]
fn trusted_flags_reach_registry_crates_only() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("cargo-home");
    let src = home.join("registry/src/index/x-1.0.0");
    fs::create_dir_all(&src)?;
    let manifest = src.join("Cargo.toml");
    fs::write(&manifest, "[package]\nname = \"x\"\nversion = \"1.0.0\"\n")?;
    let args = vec!["-Z".to_string(), "trusted-crate".to_string()];
    let registry = pkg(Some("registry+..."), &manifest.to_string_lossy())?;
    let local = pkg(None, &tmp.path().join("ws/Cargo.toml").to_string_lossy())?;
    assert_eq!(registry_only(&registry, &home, &args), args.as_slice());
    assert!(registry_only(&local, &home, &args).is_empty());
    Ok(())
}
