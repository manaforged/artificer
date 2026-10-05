use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn write_lib(root: &Path, name: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn n() -> u8 { 1 }\n").unwrap();
}

#[test]
fn incremental_and_deps_go() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(format!("home-{n}"));
    fs::create_dir_all(&home).unwrap();
    let root = tmp.path().join("ws");
    write_lib(&root, "widget");
    let inc = root.join("target/debug/incremental/foo");
    let compiled = root.join("target/debug/artificer/0123abcd");
    fs::create_dir_all(&compiled).unwrap();
    fs::write(compiled.join("libwidget-0123abcd.rlib"), b"old").unwrap();
    let deps = root.join("target/debug/deps");
    fs::create_dir_all(&inc).unwrap();
    fs::create_dir_all(&deps).unwrap();
    fs::write(deps.join("libwidget.rlib"), b"keep").unwrap();
    fs::create_dir_all(root.join("off/debug/incremental/bar")).unwrap();
    let cross = root.join("target/x86_64-apple-darwin/debug/incremental/baz");
    fs::create_dir_all(&cross).unwrap();

    let result = std::process::Command::new(env!("CARGO_BIN_EXE_artificer"))
        .arg("clean")
        .current_dir(&root)
        .env("ARTIFICER_HOME", &home)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!root.join("target/debug/incremental").exists());
    assert!(
        !root.join("target/debug/artificer").exists(),
        "clean drops the compile directory"
    );
    assert!(root.join("off/debug/incremental").exists());
    assert!(!cross.exists(), "cross-target incremental goes");
    assert!(
        !deps.join("libwidget.rlib").is_file(),
        "clean drops target/debug/deps"
    );
}

#[test]
fn published_scratch_is_dropped() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(format!("home-{n}"));
    assert!(artificer::ready(&home));
    let unit = home.join("units").join(artificer::LAYOUT).join("u-dead");
    let scratch = home.join("scratch/u-dead");
    fs::create_dir_all(unit.join("out")).unwrap();
    fs::write(unit.join("ok"), "").unwrap();
    fs::create_dir_all(&scratch).unwrap();
    fs::write(scratch.join("libx.rlib"), b"copy").unwrap();
    let root = tmp.path().join("ws");
    write_lib(&root, "widget");

    let result = std::process::Command::new(env!("CARGO_BIN_EXE_artificer"))
        .arg("clean")
        .current_dir(&root)
        .env("ARTIFICER_HOME", &home)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!scratch.exists());
    assert!(unit.join("ok").is_file(), "units stay");
}

#[cfg(unix)]
#[test]
fn orphaned_scratch_is_dropped_once_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    assert!(artificer::ready(&home));
    fs::create_dir_all(home.join("units").join(artificer::LAYOUT)).unwrap();
    let stale = home.join("scratch/u-stale");
    let fresh = home.join("scratch/u-fresh");
    for dir in [&stale, &fresh] {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("libx.rlib"), b"partial").unwrap();
    }
    fs::File::open(&stale)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600))
        .unwrap();
    let root = tmp.path().join("ws");
    write_lib(&root, "widget");

    let result = std::process::Command::new(env!("CARGO_BIN_EXE_artificer"))
        .arg("clean")
        .current_dir(&root)
        .env("ARTIFICER_HOME", &home)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!stale.exists(), "a stale scratch dir with no unit goes");
    assert!(fresh.is_dir(), "a recent scratch dir stays");
}

#[test]
fn store_stat_counts_units() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let unit = home.join("units").join(artificer::LAYOUT).join("u-stat");
    fs::create_dir_all(unit.join("out")).unwrap();
    fs::write(unit.join("ok"), "").unwrap();
    fs::write(unit.join("out/libx.rmeta"), b"x").unwrap();
    let stat = artificer::store_stat(&home).unwrap();
    assert_eq!(stat.units, 1);
    assert!(stat.bytes >= 1);
}
