use super::*;

#[test]
fn daily_gc_writes_its_stamp_without_sweep() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    gc(tmp.path(), true)?;
    assert!(tmp.path().join("gc.stamp").is_file());
    Ok(())
}

#[test]
fn analyzer_config_is_read_as_toml() {
    let shim = dirs_home()
        .join(".artificer")
        .join("bin")
        .join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    let cfg = format!("[cargo]\nextraEnv = {{ CARGO = \"{}\" }}\n", shim.display());
    assert!(analyzer_wired(&cfg));
    assert!(!analyzer_wired(
        "[cargo]\nextraEnv = { CARGO = \"/usr/bin/cargo\" }\n"
    ));
}

#[test]
fn analyzer_config_survives_bad_toml() {
    assert!(analyzer_wired("this is not = = toml .artificer"));
    assert!(!analyzer_wired("this is not = = toml"));
}
