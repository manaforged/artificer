use super::*;

#[test]
fn nonexistent_directory_errors_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    let out = artificer(&home, &root)
        .args(["check", "definitely-not-a-dir"])
        .output()
        .unwrap();
    assert_ne!(
        out.status.code(),
        Some(0),
        "a bogus dir must not silently check the cwd package"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no such directory"), "{stderr}");
}

#[test]
fn flag_shaped_value_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    let out = artificer(&home, &root)
        .args(["check", "-j", "--release"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "the shim must hand malformed syntax back to cargo"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("needs a value"),
        "a modeled flag with a bad value must say so: {stderr}"
    );

    let out = artificer(&home, &root)
        .args(["check", "--features"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs a value"));
}

#[test]
fn shim_leaves_global_options_to_cargo() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let args = ["--color", "never", "check", "--quiet"];
    let wrapped = shim(&tmp.path().join("shim"), &root)
        .args(args)
        .env("CARGO_TARGET_DIR", tmp.path().join("wrapped-target"))
        .output()
        .expect("run shim");
    let direct = stock(&root)
        .args(args)
        .env("CARGO_TARGET_DIR", tmp.path().join("stock-target"))
        .output()
        .expect("run stock cargo");
    assert_eq!(wrapped.status.code(), direct.status.code());
    assert_eq!(wrapped.stdout, direct.stdout);
    assert_eq!(wrapped.stderr, direct.stderr);
}

#[test]
fn malformed_values_match_stock_cargo() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    for args in [
        &["check", "--message-format"][..],
        &["check", "--message-format="][..],
        &["check", "--package="][..],
        &["check", "--features="][..],
        &["check", "--color=rainbow"][..],
        &["check", "--jobs=nope"][..],
    ] {
        let wrapped = shim(&home, &root)
            .args(args)
            .env("CARGO_TARGET_DIR", tmp.path().join("wrapped-target"))
            .output()
            .expect("run shim");
        let direct = stock(&root)
            .args(args)
            .env("CARGO_TARGET_DIR", tmp.path().join("stock-target"))
            .output()
            .expect("run stock cargo");
        assert_eq!(
            wrapped.status.code(),
            direct.status.code(),
            "status differs for {args:?}: {}",
            String::from_utf8_lossy(&wrapped.stderr),
        );
    }
}

#[test]
fn quiet_wrapper_is_quiet() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let out = shim(&tmp.path().join("home"), &root)
        .args(["check", "--quiet"])
        .env("CARGO_TARGET_DIR", tmp.path().join("target"))
        .output()
        .expect("run quiet check");
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn lib_selection_matches_stock_cargo() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let root = tmp.path().join("pkg");
    fs::create_dir_all(root.join("src")).expect("create source directory");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"only_bin\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write manifest");
    fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("write binary");
    let args = ["check", "--lib"];
    let wrapped = shim(&tmp.path().join("home"), &root)
        .args(args)
        .env("CARGO_TARGET_DIR", tmp.path().join("wrapped-target"))
        .output()
        .expect("run shim");
    let direct = stock(&root)
        .args(args)
        .env("CARGO_TARGET_DIR", tmp.path().join("stock-target"))
        .output()
        .expect("run stock cargo");
    assert_eq!(wrapped.status.code(), direct.status.code());
    assert!(!wrapped.status.success());
}

#[test]
fn json_test_output_still_runs_the_tests() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let out = artificer(&tmp.path().join("home"), &root)
        .args(["test", "--message-format=json"])
        .output()
        .expect("run JSON test");
    assert!(
        !out.status.success(),
        "the failing test did not run: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let no_run = artificer(&tmp.path().join("home"), &root)
        .args(["test", "--no-run", "--message-format=json"])
        .output()
        .expect("run JSON test --no-run");
    assert!(no_run.status.success());
}

#[test]
fn release_json_reports_the_release_profile() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    for selector in [vec!["--release"], vec!["--profile", "release"]] {
        let out = artificer(&tmp.path().join("home"), &root)
            .arg("check")
            .args(selector)
            .arg("--message-format=json")
            .output()
            .expect("run release JSON check");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let artifact = String::from_utf8(out.stdout)
            .expect("JSON output is UTF-8")
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|value| value["reason"] == "compiler-artifact")
            .expect("compiler artifact message");
        assert_eq!(artifact["profile"]["opt_level"], "3");
        assert_eq!(artifact["profile"]["debuginfo"], 0);
        assert_eq!(artifact["profile"]["debug_assertions"], false);
        assert_eq!(artifact["profile"]["overflow_checks"], false);
    }
}

#[test]
fn manifest_and_profile_errors_match_cargo() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    for (index, args) in [
        &["check", "--manifest-path", "definitely-missing.toml"][..],
        &["check", "--release", "--profile", "dev"][..],
    ]
    .into_iter()
    .enumerate()
    {
        let wrapped = shim(&tmp.path().join(format!("shim-{index}")), &root)
            .args(args)
            .output()
            .expect("run shim error case");
        let direct = stock(&root)
            .args(args)
            .output()
            .expect("run Cargo error case");
        assert_eq!(wrapped.status.code(), direct.status.code(), "{args:?}");
        assert_eq!(wrapped.stdout, direct.stdout, "{args:?}");
        assert_eq!(wrapped.stderr, direct.stderr, "{args:?}");
    }
}

#[test]
fn shim_routes_positional_arguments_like_cargo() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let root = tmp.path().join("pkg");
    write_pkg(&root);

    let wrapped = shim(&tmp.path().join("check-shim"), &root)
        .args(["check", "."])
        .output()
        .expect("run shim positional check");
    let direct = stock(&root)
        .args(["check", "."])
        .output()
        .expect("run Cargo positional check");
    assert_eq!(wrapped.status.code(), direct.status.code());
    assert_eq!(wrapped.stdout, direct.stdout);
    assert_eq!(wrapped.stderr, direct.stderr);

    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"{}\", std::env::args().skip(1).collect::<Vec<_>>().join(\"|\")) }\n",
    )
    .expect("write argument binary");
    let wrapped = shim(&tmp.path().join("run-shim"), &root)
        .args(["run", "hello", "--release"])
        .output()
        .expect("run shim binary");
    let direct = stock(&root)
        .args(["run", "hello", "--release"])
        .env("CARGO_TARGET_DIR", tmp.path().join("stock-run-target"))
        .output()
        .expect("run Cargo binary");
    assert_eq!(wrapped.status.code(), direct.status.code());
    assert_eq!(wrapped.stdout, direct.stdout);
    assert_eq!(
        String::from_utf8_lossy(&wrapped.stdout).trim(),
        "hello|--release"
    );

    let wrapped = shim(&tmp.path().join("test-shim"), &root)
        .args(["test", "first", "second"])
        .output()
        .expect("run shim test filters");
    let direct = stock(&root)
        .args(["test", "first", "second"])
        .output()
        .expect("run Cargo test filters");
    assert_eq!(wrapped.status.code(), direct.status.code());
    assert_eq!(wrapped.stdout, direct.stdout);
    assert_eq!(wrapped.stderr, direct.stderr);
}

#[cfg(unix)]
#[test]
fn shim_preserves_non_utf8_arguments_for_cargo() {
    use std::os::unix::ffi::OsStringExt;

    let tmp = tempfile::tempdir().expect("temporary directory");
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let path = std::ffi::OsString::from_vec(b"missing-\xff/Cargo.toml".to_vec());
    let wrapped = shim(&tmp.path().join("shim"), &root)
        .args([
            std::ffi::OsStr::new("check"),
            std::ffi::OsStr::new("--manifest-path"),
        ])
        .arg(&path)
        .output()
        .expect("run shim with non-UTF-8 path");
    let direct = stock(&root)
        .args([
            std::ffi::OsStr::new("check"),
            std::ffi::OsStr::new("--manifest-path"),
        ])
        .arg(path)
        .output()
        .expect("run Cargo with non-UTF-8 path");
    assert_eq!(wrapped.status.code(), direct.status.code());
    assert_eq!(wrapped.stdout, direct.stdout);
    assert_eq!(wrapped.stderr, direct.stderr);
}

#[test]
fn shim_falls_back_on_exit_2() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let out = shim(&tmp.path().join("shim"), &root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .expect("run metadata through shim");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("metadata JSON");
    assert_eq!(value["packages"][0]["name"], "filters");
}

#[test]
fn unmodeled_environment_falls_back() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    for name in ["CARGO_PROFILE_DEV_OPT_LEVEL", "CARGO_BUILD_RUSTFLAGS"] {
        let out = artificer(&home, &root)
            .arg("check")
            .env(name, "1")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{name}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(name), "{name}: {stderr}");
    }
    let count = fs::read_to_string(home.join("stat.fallbacks")).unwrap_or_default();
    assert_eq!(count.trim(), "2", "each fallback is recorded");
    let last = fs::read_to_string(home.join("fallback.last")).unwrap_or_default();
    assert!(last.contains("CARGO_BUILD_RUSTFLAGS"), "{last}");

    let report = artificer(&home, &root)
        .arg("why-fallback")
        .output()
        .unwrap();
    assert!(report.status.success());
    let stdout = String::from_utf8_lossy(&report.stdout);
    assert!(stdout.contains("2 fallback(s)"), "{stdout}");
    assert!(stdout.contains("CARGO_BUILD_RUSTFLAGS"), "{stdout}");
}
