use super::*;

#[test]
fn fresh_defaults_keep_cargo_compiler_choices() -> Result<()> {
    let mods = Mods::default();
    for name in ["rmeta", "meta-cache"] {
        assert!(mods.get(name)?, "{name} should default on");
    }
    for name in ["sweep", "cranelift", "slim", "linker", "threads", "serve"] {
        assert!(!mods.get(name)?, "{name} should require opt-in");
    }
    Ok(())
}

#[test]
fn roundtrip_off() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let mut mods = Mods::default();
    mods.set("rmeta", false)?;
    save(tmp.path(), &mods)?;
    let loaded = load(tmp.path())?;
    assert!(!loaded.rmeta);
    assert!(!loaded.slim);
    Ok(())
}

#[test]
fn unknown_name_errors() {
    let mut mods = Mods::default();
    let error = mods
        .set("nope", true)
        .expect_err("an unknown compile mode must fail");
    assert_eq!(error.to_string(), "unknown mod: nope");
}

#[test]
fn old_file_keeps_new_defaults() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    fs::write(tmp.path().join("mods.toml"), "units = false\n")?;
    let loaded = load(tmp.path())?;
    assert!(loaded.rmeta);
    assert!(!loaded.serve);
    Ok(())
}

#[test]
fn retired_correctness_switches_have_no_effect() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    fs::write(
        tmp.path().join("mods.toml"),
        "source-hash = false\nisolate = true\nsrc-cache = true\nunits = false\n",
    )?;
    let loaded = load(tmp.path())?;
    assert!(loaded.rmeta);
    Ok(())
}

#[test]
fn malformed_file_errors() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    fs::write(tmp.path().join("mods.toml"), "units = maybe\n")?;
    let error = load(tmp.path()).expect_err("invalid configuration must fail");
    assert!(error.to_string().contains("parse"));
    Ok(())
}
