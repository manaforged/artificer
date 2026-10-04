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
fn rustc_shares_the_job_token_pool() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    write_workspace(&root, &[("alpha", "pub fn value() -> u8 { 1 }\n")]);
    let out = artificer(&tmp.path().join("home"), &root)
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
    assert!(alpha.contains("--jobserver-auth=fifo:"), "{alpha}");
}
