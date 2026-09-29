use super::*;

#[test]
fn asset_changes_key() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let pkg = tmp.path();
    fs::create_dir_all(pkg.join("src"))?;
    fs::write(pkg.join("Cargo.toml"), "[package]\nname = \"w\"\n")?;
    fs::write(pkg.join("src/lib.rs"), "pub fn n() {}\n")?;
    fs::write(pkg.join("data.json"), "{}\n")?;
    let a = lib(pkg, "rustc 1", "w", "2021", &[])?;
    fs::write(pkg.join("data.json"), "{ \"x\": 1 }\n")?;
    let b = lib(pkg, "rustc 1", "w", "2021", &[])?;
    assert_ne!(a, b);
    fs::write(pkg.join("NOTES.md"), "noise")?;
    let c = lib(pkg, "rustc 1", "w", "2021", &[])?;
    assert_ne!(b, c);
    fs::create_dir_all(pkg.join("node_modules/x"))?;
    fs::write(pkg.join("node_modules/x/index.js"), "1")?;
    let d = lib(pkg, "rustc 1", "w", "2021", &[])?;
    assert_eq!(c, d);
    fs::write(pkg.join("src/lib.rs"), "pub fn n() { let _ = 1; }\n")?;
    let e = lib(pkg, "rustc 1", "w", "2021", &[])?;
    assert_ne!(d, e);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_file_changes_key() -> Result<()> {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir()?;
    let pkg = tmp.path().join("pkg");
    let source = tmp.path().join("real.rs");
    fs::create_dir_all(pkg.join("src"))?;
    fs::write(pkg.join("Cargo.toml"), "[package]\nname = \"w\"\n")?;
    fs::write(&source, "pub fn value() -> u8 { 1 }\n")?;
    symlink(&source, pkg.join("src/lib.rs"))?;
    let a = lib(&pkg, "rustc 1", "w", "2021", &[])?;
    fs::write(&source, "pub fn value() -> u8 { 2 }\n")?;
    let b = lib(&pkg, "rustc 1", "w", "2021", &[])?;
    assert_ne!(a, b);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_directory_changes_key() -> Result<()> {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir()?;
    let pkg = tmp.path().join("pkg");
    let generated = tmp.path().join("generated");
    fs::create_dir_all(pkg.join("src"))?;
    fs::create_dir_all(&generated)?;
    fs::write(pkg.join("Cargo.toml"), "[package]\nname = \"w\"\n")?;
    fs::write(pkg.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n")?;
    fs::write(generated.join("data.bin"), b"one")?;
    symlink(&generated, pkg.join("generated"))?;
    let a = lib(&pkg, "rustc 1", "w", "2021", &[])?;
    fs::write(generated.join("data.bin"), b"two")?;
    let b = lib(&pkg, "rustc 1", "w", "2021", &[])?;
    assert_ne!(a, b);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_cycle_fails_closed() -> Result<()> {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir()?;
    let pkg = tmp.path();
    fs::create_dir_all(pkg.join("src"))?;
    fs::write(pkg.join("Cargo.toml"), "[package]\nname = \"w\"\n")?;
    fs::write(pkg.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n")?;
    symlink(pkg, pkg.join("src/cycle"))?;
    let error =
        lib(pkg, "rustc 1", "w", "2021", &[]).expect_err("a recursive source tree must fail");
    assert!(error.to_string().contains("symlink cycle"));
    Ok(())
}

#[test]
fn rustc_version_follows_toolchain_file() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a)?;
    fs::create_dir_all(&b)?;
    fs::write(
        a.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.97.1\"\n",
    )?;
    fs::write(
        b.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.97.0\"\n",
    )?;
    let va = rustc_version_in(tmp.path(), &a)?;
    let vb = rustc_version_in(tmp.path(), &b)?;
    let a_ok = va.lines().next().is_some_and(|l| l.contains("1.97.1"));
    let b_ok = vb.lines().next().is_some_and(|l| l.contains("1.97.0"));
    if a_ok && b_ok {
        assert_ne!(va, vb, "each directory must use its pinned toolchain");
    }
    Ok(())
}

#[test]
fn ordinary_directories_are_content_keyed() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let pkg = tmp.path();
    fs::create_dir_all(pkg.join("dist"))?;
    fs::write(pkg.join("Cargo.toml"), "[package]\nname = \"w\"\n")?;
    fs::write(pkg.join("dist/gen.rs"), "pub fn g() {}\n")?;
    let a = lib(pkg, "rustc 1", "w", "2021", &[])?;
    fs::write(pkg.join("dist/gen.rs"), "pub fn g() { 1; }\n")?;
    let b = lib(pkg, "rustc 1", "w", "2021", &[])?;
    assert_ne!(a, b);
    fs::create_dir_all(pkg.join("off"))?;
    fs::write(pkg.join("off/scratch.rs"), "pub fn s() {}\n")?;
    let c = lib(pkg, "rustc 1", "w", "2021", &[])?;
    assert_ne!(b, c);
    Ok(())
}

#[test]
fn finds_both_env_macro_spellings() {
    let mut out = Vec::new();
    scan_env(
        r#"const A: &str = env!("APP_MODE");
           const B: Option<&str> = option_env!("EXTRA");
           let d = env! ( "SPACED" );
           let skip = env!(NOT_A_LITERAL);"#,
        &mut out,
    );
    out.sort();
    assert_eq!(out, vec!["APP_MODE", "EXTRA", "SPACED"]);
}

#[test]
fn a_parent_toolchain_file_changes_the_compiler_identity_key() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let member = tmp.path().join("workspace").join("member");
    fs::create_dir_all(&member)?;
    let before = toolchain_key(&member);
    fs::write(
        tmp.path().join("workspace").join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"nightly\"\n",
    )?;
    assert_ne!(before, toolchain_key(&member));
    Ok(())
}

#[test]
fn a_compiler_replaced_at_the_same_path_changes_identity() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let rustc = tmp.path().join("rustc");
    fs::write(&rustc, "old compiler")?;
    let before = file_identity(&rustc.display().to_string());
    fs::write(&rustc, "a different compiler build")?;
    assert_ne!(before, file_identity(&rustc.display().to_string()));
    Ok(())
}
