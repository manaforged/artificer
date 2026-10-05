use super::*;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn ok(home: &Path, root: &Path, args: &[&str]) -> String {
    let out = artificer(home, root).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn fallbacks(home: &Path, root: &Path) -> u64 {
    let out = artificer(home, root)
        .args(["stat", "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stat JSON");
    value["fallbacks"].as_u64().expect("fallbacks")
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn target_selection_flags_build_only_what_they_name() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("multi");
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"multi\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(&root.join("src/lib.rs"), "pub fn n() -> u8 { 1 }\n");
    write(
        &root.join("src/bin/one.rs"),
        "fn main() { println!(\"{}\", multi::n()); }\n",
    );
    write(&root.join("src/bin/two.rs"), "fn main() {}\n");
    let home = tmp.path().join("home");
    ok(&home, &root, &["check", "--lib"]);
    ok(&home, &root, &["build", "--bin", "one"]);
    let bin = |name: &str| {
        root.join(format!(
            "target/debug/{name}{}",
            std::env::consts::EXE_SUFFIX
        ))
    };
    assert!(bin("one").is_file());
    assert!(!bin("two").exists(), "--bin one builds only `one`");
    assert_eq!(fallbacks(&home, &root), 0);
}

#[test]
fn config_env_reaches_the_compile_and_a_change_rebuilds() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("greet");
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"greet\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        &root.join("src/main.rs"),
        "fn main() { println!(\"{}\", env!(\"GREETING\")); }\n",
    );
    let home = tmp.path().join("home");
    for word in ["hi", "yo"] {
        write(
            &root.join(".cargo/config.toml"),
            &format!("[env]\nGREETING = \"{word}\"\n"),
        );
        assert_eq!(ok(&home, &root, &["run"]), word);
    }
    assert_eq!(fallbacks(&home, &root), 0);
}

#[cfg(unix)]
#[test]
fn a_host_runner_wraps_run() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ran");
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"ran\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        &root.join("src/main.rs"),
        "fn main() { println!(\"inner\"); }\n",
    );
    let runner = tmp.path().join("runner.sh");
    write(&runner, "#!/bin/sh\necho outer\nexec \"$@\"\n");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755)).unwrap();
    write(
        &root.join(".cargo/config.toml"),
        &format!("[target.'cfg(unix)']\nrunner = \"{}\"\n", runner.display()),
    );
    let home = tmp.path().join("home");
    assert_eq!(ok(&home, &root, &["run"]), "outer\ninner");
    assert_eq!(fallbacks(&home, &root), 0);
}

#[test]
fn a_config_patch_builds_through_the_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let upstream = tmp.path().join("upstream");
    write(
        &upstream.join("Cargo.toml"),
        "[package]\nname = \"dep\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        &upstream.join("src/lib.rs"),
        "pub fn which() -> &'static str { \"upstream\" }\n",
    );
    git(&upstream, &["init", "--quiet"]);
    git(&upstream, &["add", "."]);
    git(&upstream, &["commit", "--quiet", "-m", "fixture"]);
    let local = tmp.path().join("local");
    write(
        &local.join("Cargo.toml"),
        "[package]\nname = \"dep\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        &local.join("src/lib.rs"),
        "pub fn which() -> &'static str { \"patched\" }\n",
    );
    let root = tmp.path().join("app");
    let url = file_url(&upstream);
    write(
        &root.join("Cargo.toml"),
        &format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ndep = {{ git = '{url}' }}\n"
        ),
    );
    write(
        &root.join("src/main.rs"),
        "fn main() { println!(\"{}\", dep::which()); }\n",
    );
    write(
        &root.join(".cargo/config.toml"),
        &format!(
            "[patch.'{url}']\ndep = {{ path = '{}' }}\n",
            local.display()
        ),
    );
    let home = tmp.path().join("home");
    assert_eq!(ok(&home, &root, &["run"]), "patched");
    assert_eq!(fallbacks(&home, &root), 0);
}

#[test]
fn dependencies_get_line_table_debug_info_and_workspace_crates_keep_theirs() {
    let tmp = tempfile::tempdir().unwrap();
    let upstream = tmp.path().join("upstream");
    write(
        &upstream.join("Cargo.toml"),
        "[package]\nname = \"dep\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(&upstream.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n");
    git(&upstream, &["init", "--quiet"]);
    git(&upstream, &["add", "."]);
    git(&upstream, &["commit", "--quiet", "-m", "fixture"]);
    let root = tmp.path().join("app");
    let url = file_url(&upstream);
    write(
        &root.join("Cargo.toml"),
        &format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ndep = {{ git = '{url}' }}\n"
        ),
    );
    write(
        &root.join("src/lib.rs"),
        "pub fn value() -> u8 { dep::value() }\n",
    );
    let home = tmp.path().join("home");
    let debuginfo = || {
        let out = artificer(&home, &root)
            .env("ARTIFICER_TRACE", "1")
            .arg("build")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(out.status.success(), "{stderr}");
        let last = |name: &str| {
            stderr
                .lines()
                .find(|line| line.contains(&format!("\"--crate-name\" \"{name}\"")))
                .and_then(|line| line.rsplit("\"debuginfo=").next())
                .and_then(|rest| rest.split('"').next())
                .map(str::to_string)
        };
        (last("dep"), last("app"))
    };
    let lines = Some("line-tables-only".to_string());
    let full = Some("2".to_string());
    assert_eq!(debuginfo(), (lines, full.clone()));
    let off = artificer(&home, &root)
        .args(["mods", "off", "slim-deps"])
        .output()
        .unwrap();
    assert!(off.status.success());
    assert_eq!(debuginfo(), (full.clone(), full));
}
