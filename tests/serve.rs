#![cfg(not(windows))]

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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

fn wait_ping(home: &Path) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if artificer::serve_ping(home) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("serve did not start");
}

fn enable_serve(home: &Path) {
    let mut mods = artificer::load_mods(home).expect("load compile modes");
    mods.set("serve", true).expect("enable serve mode");
    artificer::save_mods(home, &mods).expect("save compile modes");
}

#[test]
fn down_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let mut req = artificer::ServeRequest {
        token: String::new(),
        op: "check".into(),
        dir: tmp.path().to_path_buf(),
        packages: Vec::new(),
        json: false,
        workspace: false,
        all_features: false,
        features: Vec::new(),
        no_default: false,
        meta_flags: Vec::new(),
        release: false,
        link: false,
        no_run: false,
        lib: false,
        doc: false,
        only: Vec::new(),
        tests: false,
        all_targets: false,
        args: Vec::new(),
        target_dir: None,
    };
    assert!(artificer::serve_try(&home, &mut req).is_none());
}

#[test]
fn daemon_check_hits_second() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(format!("home-{n}"));
    fs::create_dir_all(&home).unwrap();
    enable_serve(&home);
    let a = tmp.path().join("a");
    write_lib(&a, "widget");

    let h = home.clone();
    let worker = std::thread::spawn(move || {
        drop(artificer::serve_listen(&h));
    });
    wait_ping(&home);

    let mut req = artificer::ServeRequest {
        token: String::new(),
        op: "check".into(),
        dir: a.clone(),
        packages: Vec::new(),
        json: false,
        workspace: false,
        all_features: false,
        features: Vec::new(),
        no_default: false,
        meta_flags: Vec::new(),
        release: false,
        link: false,
        no_run: false,
        lib: false,
        doc: false,
        only: Vec::new(),
        tests: false,
        all_targets: false,
        args: Vec::new(),
        target_dir: None,
    };
    let code = artificer::serve_try(&home, &mut req)
        .expect("daemon up")
        .unwrap();
    assert_eq!(code, 0);
    let code = artificer::serve_try(&home, &mut req)
        .expect("daemon up")
        .unwrap();
    assert_eq!(code, 0);

    artificer::serve_stop(&home).unwrap();
    drop(worker.join());
}

#[test]
fn a_daemon_from_an_older_install_is_retired() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(format!("home-{n}"));
    fs::create_dir_all(&home).unwrap();
    enable_serve(&home);

    let h = home.clone();
    let worker = std::thread::spawn(move || {
        drop(artificer::serve_listen(&h));
    });
    wait_ping(&home);

    let stamp = home.join("serve.build");
    assert!(stamp.is_file(), "a live daemon records its build");
    fs::write(&stamp, "some other artificer").unwrap();

    let a = tmp.path().join("a");
    write_lib(&a, "widget");
    let mut req = artificer::ServeRequest {
        token: String::new(),
        op: "check".into(),
        dir: a,
        packages: Vec::new(),
        json: false,
        workspace: false,
        all_features: false,
        features: Vec::new(),
        no_default: false,
        meta_flags: Vec::new(),
        release: false,
        link: false,
        no_run: false,
        lib: false,
        doc: false,
        only: Vec::new(),
        tests: false,
        all_targets: false,
        args: Vec::new(),
        target_dir: None,
    };
    drop(artificer::serve_try(&home, &mut req));
    assert_ne!(
        fs::read_to_string(&stamp).unwrap_or_default(),
        "some other artificer",
        "a daemon whose build no longer matches must not keep serving",
    );
    artificer::serve_stop(&home).unwrap();
    drop(worker.join());
}

#[test]
fn changed_compiler_environment_is_refused() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(format!("home-{n}"));
    fs::create_dir_all(&home).unwrap();
    enable_serve(&home);
    let a = tmp.path().join("a");
    write_lib(&a, "envcheck");

    let h = home.clone();
    let worker = std::thread::spawn(move || {
        drop(artificer::serve_listen(&h));
    });
    wait_ping(&home);

    let mut req = artificer::ServeRequest {
        token: String::new(),
        op: "check".into(),
        dir: a,
        packages: Vec::new(),
        json: false,
        workspace: false,
        all_features: false,
        features: Vec::new(),
        no_default: false,
        meta_flags: Vec::new(),
        release: false,
        link: false,
        no_run: false,
        lib: false,
        doc: false,
        only: Vec::new(),
        tests: false,
        all_targets: false,
        args: Vec::new(),
        target_dir: None,
    };
    let code = artificer::serve_try(&home, &mut req)
        .expect("daemon up")
        .unwrap();
    assert_eq!(code, 0);

    fs::write(home.join("serve.env"), "RUSTC_WRAPPER=/busted\n").unwrap();
    assert!(
        artificer::serve_try(&home, &mut req).is_none(),
        "a daemon with a stale environment snapshot must not serve"
    );

    artificer::serve_stop(&home).unwrap();
    drop(worker.join());
}
