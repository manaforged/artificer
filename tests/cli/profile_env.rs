use super::*;

#[test]
fn profile_environment_reaches_rustc() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    let out = artificer(&home, &root)
        .arg("build")
        .env("ARTIFICER_TRACE", "1")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_PROFILE_DEV_OPT_LEVEL", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let rustc = stderr
        .lines()
        .find(|line| line.contains("\"--crate-name\" \"filters\""))
        .unwrap_or_else(|| panic!("no rustc command: {stderr}"));
    assert!(rustc.contains("\"debuginfo=0\""), "{rustc}");
    assert!(rustc.contains("\"opt-level=1\""), "{rustc}");
    assert!(!home.join("fallback.last").exists(), "{stderr}");

    let rejected = artificer(&home, &root)
        .arg("build")
        .env("CARGO_PROFILE_DEV_OPT_LEVEL", "9")
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("CARGO_PROFILE_DEV_OPT_LEVEL=9"), "{stderr}");
}
