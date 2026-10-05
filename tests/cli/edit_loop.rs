use super::*;

fn incremental_sessions(root: &Path) -> Vec<String> {
    fs::read_dir(root.join("target/debug/incremental"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn workspace_crates_compile_incrementally_and_keep_their_state() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let home = tmp.path().join("home");
    write_workspace(&root, &[("alpha", "pub fn value() -> u8 { 1 }\n")]);
    let mods = artificer(&home, &root)
        .args(["mods", "on", "sweep"])
        .output()
        .unwrap();
    assert!(
        mods.status.success(),
        "{}",
        String::from_utf8_lossy(&mods.stderr)
    );
    for body in [
        "pub fn value() -> u8 { 1 }\n",
        "pub fn value() -> u8 { 2 }\n",
    ] {
        fs::write(root.join("crates/alpha/src/lib.rs"), body).unwrap();
        let out = artificer(&home, &root).arg("build").output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let sessions = incremental_sessions(&root);
        assert!(
            sessions.iter().any(|s| s.starts_with("alpha-")),
            "incremental state after each build: {sessions:?}"
        );
    }
    let sessions = incremental_sessions(&root);
    assert_eq!(
        sessions.iter().filter(|s| s.starts_with("alpha-")).count(),
        1,
        "an edit reuses the crate's incremental session: {sessions:?}"
    );

    let off = tmp.path().join("off");
    write_workspace(&off, &[("beta", "pub fn value() -> u8 { 1 }\n")]);
    let out = artificer(&home, &off)
        .arg("build")
        .env("CARGO_INCREMENTAL", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        incremental_sessions(&off).is_empty(),
        "CARGO_INCREMENTAL=0 wins"
    );
}

#[test]
fn the_threads_mode_reaches_rustc() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let home = tmp.path().join("home");
    write_workspace(&root, &[("alpha", "pub fn value() -> u8 { 1 }\n")]);
    let mods = artificer(&home, &root)
        .args(["mods", "on", "threads"])
        .output()
        .unwrap();
    assert!(
        mods.status.success(),
        "{}",
        String::from_utf8_lossy(&mods.stderr)
    );
    let out = artificer(&home, &root)
        .env("RUSTC_BOOTSTRAP", "1")
        .env("ARTIFICER_TRACE", "1")
        .arg("build")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let alpha = stderr
        .lines()
        .find(|line| line.contains("\"--crate-name\" \"alpha\""))
        .unwrap_or_else(|| panic!("no rustc command for alpha: {stderr}"));
    assert!(alpha.contains("\"threads="), "{alpha}");
}

#[cfg(unix)]
#[test]
fn rustc_draws_threads_from_a_pool_that_ends_with_the_build() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let home = tmp.path().join("home");
    write_workspace(&root, &[("alpha", "pub fn value() -> u8 { 1 }\n")]);
    let out = artificer(&home, &root)
        .env("ARTIFICER_TRACE", "1")
        .arg("build")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let alpha = stderr
        .lines()
        .find(|line| line.contains("\"--crate-name\" \"alpha\""))
        .unwrap_or_else(|| panic!("no rustc command for alpha: {stderr}"));
    let pools = home.canonicalize().unwrap().join("jobservers");
    assert!(
        alpha.contains(&format!("--jobserver-auth=fifo:{}", pools.display())),
        "{alpha}"
    );
    let left = fs::read_dir(&pools)
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0);
    assert_eq!(left, 0, "a build pool outlived its build");
}

#[test]
fn an_edited_crate_recompiles_in_place_and_its_dependents_see_the_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let home = tmp.path().join("home");
    write_workspace(
        &root,
        &[
            ("alpha", "pub fn value() -> u8 { 1 }\n"),
            ("beta", "pub fn value() -> u8 { 1 }\n"),
        ],
    );
    let app = root.join("crates/app");
    fs::create_dir_all(app.join("src")).unwrap();
    fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nalpha = { path = \"../alpha\" }\nbeta = { path = \"../beta\" }\n",
    )
    .unwrap();
    fs::write(
        app.join("src/main.rs"),
        "fn main() { println!(\"{}\", alpha::value() + beta::value()); }\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"crates/alpha\", \"crates/beta\", \"crates/app\"]\n",
    )
    .unwrap();
    let run = || {
        let out = artificer(&home, &root)
            .env("ARTIFICER_TRACE", "1")
            .args(["run", "-q", "-p", "app"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(out.status.success(), "{stderr}");
        let place = |name: &str| {
            let line = stderr
                .lines()
                .find(|line| line.contains(&format!("\"--crate-name\" \"{name}\"")))?;
            let dir = line.split("\"--out-dir\" \"").nth(1)?.split('"').next()?;
            let stem = line.split("\"extra-filename=").nth(1)?.split('"').next()?;
            Some((dir.to_string(), stem.to_string()))
        };
        let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (printed, place("alpha"), place("beta"))
    };
    let (first, alpha, beta) = run();
    assert_eq!(first, "2");
    let alpha = alpha.expect("alpha compiled");
    let beta = beta.expect("beta compiled");
    assert_ne!(alpha.0, beta.0, "each crate compiles in its own place");
    fs::write(
        root.join("crates/alpha/src/lib.rs"),
        "pub fn value() -> u8 { 5 }\n",
    )
    .unwrap();
    let (second, again, _) = run();
    assert_eq!(second, "6", "the dependent sees the edit");
    assert_eq!(
        again,
        Some(alpha),
        "the edited crate recompiles with the same output directory and name"
    );
}
