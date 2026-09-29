use super::*;
use anyhow::Result;
use std::fs;

#[test]
fn resolve_path_collapses_a_symlink() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let real = tmp.path().join("real");
    fs::create_dir_all(&real)?;
    let link = tmp.path().join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link)?;
    #[cfg(not(unix))]
    std::os::windows::fs::symlink_dir(&real, &link)?;
    assert_eq!(
        resolve_path(&link),
        crate::platform::env_path(&real.canonicalize()?)
    );
    Ok(())
}

#[test]
fn cache_dir_is_the_default() {
    let cache = Path::new("/cache");
    assert_eq!(home_from(None, cache), cache.join("artificer"));
}

#[test]
fn explicit_home_outranks_the_cache() {
    let out = home_from(Some("/scratch/store"), Path::new("/cache"));
    assert_eq!(out, PathBuf::from("/scratch/store"));
}

#[test]
fn purge_deletes_only_a_tagged_cache() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let plain = tmp.path().join("plain");
    fs::create_dir_all(&plain)?;
    fs::write(plain.join("keep"), "user data")?;
    assert!(!purge(&plain)?);
    assert!(plain.join("keep").is_file());

    let cache = tmp.path().join("cache");
    assert!(ready(&cache));
    fs::create_dir_all(cache.join("units"))?;
    assert!(purge(&cache)?);
    assert!(!cache.exists());
    Ok(())
}

#[test]
fn ready_never_claims_a_directory_with_other_files() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let checkout = tmp.path().join("checkout");
    fs::create_dir_all(checkout.join(".git"))?;
    fs::write(checkout.join("Cargo.toml"), "[package]\n")?;
    assert!(!ready(&checkout));
    assert!(!checkout.join("CACHEDIR.TAG").exists());
    assert!(!purge(&checkout)?);
    assert!(checkout.join("Cargo.toml").is_file());
    Ok(())
}

#[test]
fn a_store_name_alone_is_not_ownership() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let cache = tmp.path().join("cache");
    assert!(ready(&cache));
    fs::write(cache.join("codegen-0123456789abcdef"), "llvm")?;
    assert!(purge(&cache)?);
    assert!(!cache.exists());

    let foreign = tmp.path().join("foreign");
    fs::create_dir_all(foreign.join("keys"))?;
    fs::write(foreign.join("keys").join("private.pem"), "secret")?;
    assert!(!ready(&foreign));
    assert!(!purge(&foreign)?);
    assert!(foreign.join("keys").join("private.pem").is_file());
    Ok(())
}

#[test]
fn another_tools_cache_tag_is_not_ownership() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let other = tmp.path().join("other-cache");
    fs::create_dir_all(&other)?;
    fs::write(
        other.join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )?;
    fs::create_dir_all(other.join("units"))?;
    fs::write(other.join("data.bin"), "theirs")?;
    assert!(!ready(&other));
    assert!(!purge(&other)?);
    assert!(other.join("data.bin").is_file());
    Ok(())
}
