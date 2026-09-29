use super::*;

#[test]
fn a_package_config_cargo_ignores_does_not_fail_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = tmp.path().join("outer");
    fs::create_dir_all(&outer).unwrap();
    let root = tmp.path().join("pkg");
    write_clean_pkg(&root);
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::write(root.join(".cargo/config.toml"), "not valid toml [").unwrap();
    let out = artificer(&tmp.path().join("home"), &outer)
        .args(["check", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn config_from_the_invocation_directory_is_not_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = tmp.path().join("outer");
    fs::create_dir_all(outer.join(".cargo")).unwrap();
    fs::write(
        outer.join(".cargo/config.toml"),
        "[build]\nrustflags = [\"--cfg\", \"from_outer\"]\n",
    )
    .unwrap();
    let root = tmp.path().join("pkg");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"cfgdir\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"{}\", cfg!(from_outer)); }\n",
    )
    .unwrap();
    let manifest = root.join("Cargo.toml");
    let stock_run = stock(&outer)
        .args(["run", "-q", "--manifest-path"])
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", tmp.path().join("stock-target"))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&stock_run.stdout).trim(), "true");
    let out = artificer(&tmp.path().join("home"), &outer)
        .args(["run", "--manifest-path"])
        .arg(&manifest)
        .output()
        .unwrap();
    if out.status.success() {
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "true");
    } else {
        assert_eq!(out.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("Cargo config"), "{stderr}");
    }
}

#[test]
fn a_refused_store_directory_is_left_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_clean_pkg(&root);
    let foreign = tmp.path().join("foreign");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("notes.txt"), "mine").unwrap();
    let out = artificer(&foreign, &root).arg("check").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let names: Vec<_> = fs::read_dir(&foreign)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("notes.txt")]);
}

#[test]
fn a_relative_rustc_path_is_declined() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_clean_pkg(&root);
    let out = artificer(&tmp.path().join("home"), &root)
        .arg("check")
        .env("RUSTC", "tools/rustc")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("RUSTC"), "{stderr}");
}

#[test]
fn config_profile_table_falls_back() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::write(
        root.join(".cargo/config.toml"),
        "[profile.dev]\nopt-level = 1\n",
    )
    .unwrap();
    let out = artificer(&tmp.path().join("home"), &root)
        .arg("check")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("profile"), "{stderr}");
}

#[test]
fn missing_feature_probe_falls_back_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let out = artificer(&tmp.path().join("home"), &root)
        .arg("check")
        .env("ARTIFICER_NO_TREE", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("feature resolution"), "{stderr}");
}

#[test]
fn unknown_config_table_falls_back() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::write(root.join(".cargo/config.toml"), "[future-table]\nkey = 1\n").unwrap();
    let out = artificer(&tmp.path().join("home"), &root)
        .arg("check")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("future-table"), "{stderr}");
}

#[test]
fn unknown_cargo_env_falls_back() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let out = artificer(&tmp.path().join("home"), &root)
        .arg("check")
        .env("CARGO_FUTURE_KNOB", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("CARGO_FUTURE_KNOB"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn unmodeled_metadata_falls_back() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let fake = tmp.path().join("fake-cargo");
    fs::write(
        &fake,
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'cargo 1.98.0 (fake)';;\n  metadata) echo 'not json';;\n  *) exit 1;;\nesac\n",
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let out = artificer(&tmp.path().join("home"), &root)
        .arg("check")
        .env("ARTIFICER_REAL_CARGO", &fake)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("metadata format is not modeled"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn a_bare_rustc_with_a_relative_path_entry_is_declined() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_clean_pkg(&root);
    let path = format!("tools:{}", std::env::var("PATH").unwrap());
    let out = artificer(&tmp.path().join("home"), &root)
        .arg("check")
        .env("RUSTC", "rustc")
        .env("PATH", path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("RUSTC"), "{stderr}");
}

#[test]
fn a_package_glob_runs_cargo() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    write_workspace(
        &root,
        &[
            ("alpha", "pub fn value() -> u8 { 1 }\n"),
            ("beta", "pub fn value() -> u8 { 2 }\n"),
        ],
    );
    let out = shim(&tmp.path().join("shim"), &root)
        .args(["check", "-p", "al*"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let stat = artificer::store_stat(&tmp.path().join("shim/store")).unwrap();
    assert_eq!(stat.fallbacks, 1, "{:?}", stat.fallback_last);
}
