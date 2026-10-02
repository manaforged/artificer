use super::*;

#[test]
fn shim_hands_cargo_commands_named_like_artificer_tools_to_cargo() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let home = tmp.path().join("shim");
    for args in [
        &["install", "--list"][..],
        &["uninstall", "--help"][..],
        &["doctor"][..],
        &["env"][..],
    ] {
        let wrapped = shim(&home, tmp.path())
            .args(args)
            .output()
            .expect("run shim");
        let direct = stock(tmp.path()).args(args).output().expect("run Cargo");
        assert_eq!(wrapped.status.code(), direct.status.code(), "{args:?}");
        assert_eq!(wrapped.stdout, direct.stdout, "{args:?}");
        assert!(
            !String::from_utf8_lossy(&wrapped.stdout).contains("artificer:"),
            "{args:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_stale_shim_replaces_itself_with_the_installed_artificer() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("temporary directory");
    let shim_dir = tmp.path().join(".artificer/bin");
    let cargo_home = tmp.path().join("cargo-home");
    fs::create_dir_all(&shim_dir).unwrap();
    fs::create_dir_all(cargo_home.join("bin")).unwrap();
    let shim_path = shim_dir.join("cargo");
    fs::hard_link(env!("CARGO_BIN_EXE_artificer"), &shim_path).expect("link the shim");
    let installed = cargo_home.join("bin").join("artificer");
    fs::write(&installed, "#!/bin/sh\necho newer-artificer \"$@\"\n").unwrap();
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o755)).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&installed)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let output = Command::new(&shim_path)
        .env("HOME", tmp.path())
        .env("CARGO_HOME", &cargo_home)
        .env("ARTIFICER_HOME", tmp.path().join("store"))
        .current_dir(tmp.path())
        .args(["check", "--offline"])
        .output()
        .expect("run stale shim");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "newer-artificer check --offline"
    );
    assert_eq!(fs::read(&shim_path).unwrap(), fs::read(&installed).unwrap());
}

#[test]
fn a_cargo_named_binary_outside_the_control_home_is_never_overwritten() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("temporary directory");
    let elsewhere = tmp.path().join("bin");
    let cargo_home = tmp.path().join("cargo-home");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::create_dir_all(cargo_home.join("bin")).unwrap();
    let running = elsewhere.join("cargo");
    fs::hard_link(env!("CARGO_BIN_EXE_artificer"), &running).expect("link the binary");
    let installed = cargo_home.join("bin").join("artificer");
    fs::write(&installed, "newer").unwrap();
    let refreshed =
        artificer::refresh_shim(&running, &tmp.path().join(".artificer"), &cargo_home).unwrap();
    assert!(!refreshed);
    assert_eq!(
        fs::read(&running).unwrap(),
        fs::read(env!("CARGO_BIN_EXE_artificer")).unwrap()
    );
}
