use super::*;

const BROKEN: &str = "pub fn value() -> u8 { \"no\" }\n";

#[test]
fn diagnostics_name_each_member_by_its_workspace_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    write_workspace(&root, &[("alpha", BROKEN), ("beta", BROKEN)]);
    let out = shim(&tmp.path().join("shim"), &root)
        .args(["check", "--workspace"])
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(101), "{stderr}");
    for name in ["alpha", "beta"] {
        let sep = std::path::MAIN_SEPARATOR;
        assert!(
            stderr.contains(&format!("--> crates{sep}{name}{sep}src{sep}lib.rs:1:")),
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!(
                "error: could not compile `{name}` (lib) due to 1 previous error"
            )),
            "{stderr}"
        );
    }
    assert!(
        !stderr.contains("--> ./src") && !stderr.contains("--> .\\src"),
        "{stderr}"
    );
    let stat = artificer::store_stat(&tmp.path().join("shim/store")).unwrap();
    assert_eq!(stat.fallbacks, 0, "{:?}", stat.fallback_last);
}

#[test]
fn status_lines_follow_cargo_and_the_color_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    write_workspace(&root, &[("alpha", "pub fn value() -> u8 { 1 }\n")]);
    let colored = shim(&tmp.path().join("shim"), &root)
        .args(["build", "--workspace"])
        .env("CARGO_TERM_COLOR", "always")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&colored.stderr);
    assert!(colored.status.success(), "{stderr}");
    assert!(
        stderr.contains("\u{1b}[1m\u{1b}[92m   Compiling\u{1b}[0m alpha v0.1.0"),
        "{stderr}"
    );
    assert!(
        stderr.contains("    Finished\u{1b}[0m `dev` profile"),
        "{stderr}"
    );
    let stat = artificer::store_stat(&tmp.path().join("shim/store")).unwrap();
    assert!(
        stat.misses > 0 && stat.fallbacks == 0,
        "{:?}",
        stat.fallback_last
    );
    fs::write(
        root.join("crates/alpha/src/lib.rs"),
        "pub fn value() -> u8 { 2 }\n",
    )
    .unwrap();
    let plain = shim(&tmp.path().join("shim"), &root)
        .args(["build", "--workspace", "--color", "never"])
        .env("CARGO_TERM_COLOR", "always")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&plain.stderr);
    assert!(plain.status.success(), "{stderr}");
    assert!(stderr.contains("   Compiling alpha v0.1.0"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr}");
    let stat = artificer::store_stat(&tmp.path().join("shim/store")).unwrap();
    assert_eq!(stat.fallbacks, 0, "{:?}", stat.fallback_last);
}

#[test]
fn a_closed_stderr_does_not_hang_the_build() {
    use std::io::Read;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    write_workspace(
        &root,
        &[
            ("a", "pub fn a() {}\n"),
            ("b", "pub fn b() {}\n"),
            ("c", "pub fn c() {}\n"),
        ],
    );
    let manifest = root.join("crates/c/Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        format!("{text}\n[dependencies]\na = {{ path = \"../a\" }}\nb = {{ path = \"../b\" }}\n"),
    )
    .unwrap();
    let mut child = artificer(&tmp.path().join("home"), &root)
        .env("ARTIFICER_TRACE", "1")
        .arg("build")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let mut first = [0u8; 1];
    stderr.read_exact(&mut first).unwrap();
    drop(stderr);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            drop(child.kill());
            panic!("the build hung after its stderr closed");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert!(status.success(), "{status}");
}
