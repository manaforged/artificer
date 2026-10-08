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
    fs::copy(env!("CARGO_BIN_EXE_artificer"), &shim_path).expect("copy the shim");
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
    fs::copy(env!("CARGO_BIN_EXE_artificer"), &running).expect("copy the binary");
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

#[cfg(unix)]
#[test]
fn a_reentered_shim_execs_real_cargo_without_the_store() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("temporary directory");
    let sentinel = tmp.path().join("sentinel");
    let real = tmp.path().join("real-cargo");
    fs::write(
        &real,
        format!(
            "#!/bin/sh\necho \"$ARTIFICER_SHIM_DEPTH $*\" >> {}\n",
            sentinel.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
    let store = tmp.path().join("store");
    let status = Command::new(shim_binary())
        .args(["build", "--release"])
        .env("ARTIFICER_SHIM_DEPTH", "1")
        .env("ARTIFICER_REAL_CARGO", &real)
        .env("ARTIFICER_HOME", &store)
        .env("HOME", tmp.path())
        .current_dir(tmp.path())
        .status()
        .expect("run shim");
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(&sentinel).unwrap(),
        "2 build --release\n"
    );
    assert!(!store.exists());
}

#[cfg(unix)]
#[test]
fn a_shim_that_resolves_to_itself_stops_instead_of_forking_forever() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("temporary directory");
    let sentinel = tmp.path().join("sentinel");
    let real = tmp.path().join("real-cargo");
    fs::write(
        &real,
        format!(
            "#!/bin/sh\necho pass >> {}\n\"{}\" \"$@\"\n",
            sentinel.display(),
            shim_binary().display()
        ),
    )
    .unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = Command::new(shim_binary())
        .args(["metadata", "--format-version", "1"])
        .env("ARTIFICER_SHIM_DEPTH", "1")
        .env("ARTIFICER_REAL_CARGO", &real)
        .env("ARTIFICER_HOME", tmp.path().join("store"))
        .env("HOME", tmp.path())
        .env_remove("ARTIFICER_NEST")
        .current_dir(tmp.path())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run shim");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll shim") {
            break status;
        }
        if std::time::Instant::now() > deadline {
            drop(child.kill());
            panic!("the shim kept re-entering itself");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(!status.success());
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    assert!(stderr.contains("refusing to start another"), "{stderr}");
    let passes = fs::read_to_string(&sentinel).unwrap().lines().count();
    assert!(passes <= 16, "{passes} nested passes");
}
