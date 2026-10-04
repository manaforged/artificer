use super::*;

#[test]
fn stat_json_reports_the_store() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    artificer(&home, &root).arg("check").output().unwrap();
    let out = artificer(&home, &root)
        .args(["stat", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stat JSON");
    assert_eq!(value["builds"], 1);
    assert!(value["misses"].as_u64().unwrap_or(0) >= 1, "{value}");
    assert!(value["hit_rate"].is_number(), "{value}");
    assert!(value["units"].as_u64().unwrap_or(0) >= 1, "{value}");
}

#[test]
fn package_id_spec_selects_the_package() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("filters");
    write_pkg(&root);
    let meta = Command::new(std::env::var("CARGO").unwrap())
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        meta.status.success(),
        "{}",
        String::from_utf8_lossy(&meta.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&meta.stdout).unwrap();
    let spec = meta["packages"][0]["id"].as_str().unwrap();
    assert!(!spec.contains('@'), "{spec}");
    let home = tmp.path().join("home");
    let out = artificer(&home, &root)
        .args(["check", "-p", spec])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stat = artificer(&home, &root)
        .args(["stat", "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&stat.stdout).expect("stat JSON");
    assert_eq!(value["builds"], 1, "{value}");
}

#[cfg(unix)]
#[test]
fn wrappers_apply_the_way_cargo_applies_them() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    let dep = tmp.path().join("dep");
    fs::create_dir_all(ws.join("member/src")).unwrap();
    fs::create_dir_all(dep.join("src")).unwrap();
    fs::write(
        dep.join("Cargo.toml"),
        "[package]\nname = \"dep\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(dep.join("src/lib.rs"), "pub fn g() -> i32 { 0 }\n").unwrap();
    fs::write(
        ws.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(
        ws.join("member/Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ndep = { path = \"../../dep\" }\n",
    )
    .unwrap();
    fs::write(
        ws.join("member/src/lib.rs"),
        "pub fn f() -> i32 { dep::g() }\n",
    )
    .unwrap();

    let markers = tmp.path().join("markers");
    fs::create_dir_all(&markers).unwrap();
    let script = tmp.path().join("wrap.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nname=\nprev=\nfor a in \"$@\"; do\n  if [ \"$prev\" = \"--crate-name\" ]; then name=\"$a\"; fi\n  prev=\"$a\"\ndone\nif [ -n \"$name\" ]; then : > \"{}/$name\"; fi\nexec \"$@\"\n",
            markers.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let home = tmp.path().join("home");

    let out = artificer(&home, &ws)
        .arg("check")
        .env("RUSTC_WRAPPER", &script)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(markers.join("member").exists(), "member must be wrapped");
    assert!(
        markers.join("dep").exists(),
        "RUSTC_WRAPPER covers dependencies"
    );

    let script_ws = tmp.path().join("wrap-ws.sh");
    fs::copy(&script, &script_ws).unwrap();
    fs::remove_dir_all(&markers).unwrap();
    fs::create_dir_all(&markers).unwrap();
    let out = artificer(&home, &ws)
        .arg("check")
        .env("RUSTC_WORKSPACE_WRAPPER", &script_ws)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        markers.join("member").exists(),
        "the workspace wrapper reaches members"
    );
    assert!(
        !markers.join("dep").exists(),
        "the workspace wrapper skips non-members"
    );
}

#[test]
fn test_release_gates_the_release_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"filters\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[profile.release.build-override]\nopt-level = 1\n",
    )
    .unwrap();
    let out = artificer(&tmp.path().join("home"), &root)
        .args(["test", "--release"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("build-override"), "{stderr}");
}

#[test]
fn nextest_builds_through_the_store() {
    if !tool_available("cargo-nextest") {
        eprintln!("cargo-nextest is not installed; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_clean_pkg(&root);
    let home = tmp.path().join("home");
    let out = shim(&home, &root)
        .args(["nextest", "run"])
        .env("ARTIFICER_NOSERVE", "1")
        .env("CARGO_TARGET_DIR", tmp.path().join("target"))
        .output()
        .expect("run nextest through the shim");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let units = fs::read_dir(home.join("store/units").join(artificer::LAYOUT))
        .map(|dir| dir.count())
        .unwrap_or(0);
    assert!(units > 0, "nextest's build must land in the store");
}

#[test]
fn clippy_builds_through_the_store() {
    if !tool_available("cargo-clippy") {
        eprintln!("cargo-clippy is not installed; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_clean_pkg(&root);
    let home = tmp.path().join("home");
    let out = shim(&home, &root)
        .arg("clippy")
        .env("ARTIFICER_NOSERVE", "1")
        .env("CARGO_TARGET_DIR", tmp.path().join("target"))
        .output()
        .expect("run clippy through the shim");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let units = fs::read_dir(home.join("store/units").join(artificer::LAYOUT))
        .map(|dir| dir.count())
        .unwrap_or(0);
    assert!(units > 0, "clippy's build must land in the store");
}

#[test]
fn colored_cargo_output_does_not_disable_caching() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("app");
    let package = |dir: &Path, name: &str, deps: &str, body: &str| {
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\n{deps}"),
        )
        .unwrap();
        fs::write(dir.join("src/lib.rs"), body).unwrap();
    };
    package(&root.join("d"), "d", "", "pub fn d() -> u8 { 1 }\n");
    package(
        &root.join("c"),
        "c",
        "d = { path = \"../d\" }\n",
        "pub fn c() -> u8 { d::d() }\n",
    );
    package(
        &root.join("a"),
        "a",
        "c = { path = \"../c\" }\n",
        "pub fn a() -> u8 { c::c() }\n",
    );
    package(
        &root.join("b"),
        "b",
        "c = { path = \"../c\" }\n",
        "pub fn b() -> u8 { c::c() }\n",
    );
    package(
        &root,
        "app",
        "a = { path = \"a\" }\nb = { path = \"b\" }\n",
        "pub fn app() -> u8 { a::a() + b::b() }\n",
    );
    let out = artificer(&tmp.path().join("home"), &root)
        .arg("check")
        .env("CARGO_TERM_COLOR", "always")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_workspace_build_selects_default_members() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    for (name, source) in [
        ("kept", "pub fn kept() {}\n"),
        (
            "skipped",
            "compile_error!(\"built a non-default member\");\n",
        ),
    ] {
        fs::create_dir_all(ws.join(name).join("src")).unwrap();
        fs::write(
            ws.join(name).join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        fs::write(ws.join(name).join("src/lib.rs"), source).unwrap();
    }
    fs::write(
        ws.join("Cargo.toml"),
        "[workspace]\nmembers = [\"kept\", \"skipped\"]\ndefault-members = [\"kept\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    let home = tmp.path().join("home");
    let out = artificer(&home, &ws).arg("build").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stat = artificer(&home, &ws)
        .args(["stat", "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&stat.stdout).expect("stat JSON");
    assert_eq!(value["builds"], 1, "{value}");
    let all = artificer(&home, &ws)
        .args(["build", "--workspace"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&all.stderr).contains("built a non-default member"),
        "{}",
        String::from_utf8_lossy(&all.stderr)
    );
}
