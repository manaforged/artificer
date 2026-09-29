use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);
static ENV: Mutex<()> = Mutex::new(());

fn write_pkg(root: &Path, _counter: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"ex\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("build.rs"),
        r#"
fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    std::fs::write(format!("{out}/hello.rs"), "pub fn token() -> u8 { 7 }\n").unwrap();
    println!("cargo:rustc-cfg=forged");
    if let Ok(path) = std::env::var("ARTIFICER_TEST_RUNS") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(f, "ran").unwrap();
    }
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
include!(concat!(env!("OUT_DIR"), "/hello.rs"));
pub fn ok() -> u8 { token() }
"#,
    )
    .unwrap();
}

fn runs(counter: &Path) -> usize {
    fs::read_to_string(counter)
        .unwrap_or_default()
        .lines()
        .filter(|l| *l == "ran")
        .count()
}

#[test]
fn second_checkout_restores_script_out_dir() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(format!("home-{n}"));
    let counter = tmp.path().join("runs");
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    write_pkg(&a, &counter);
    write_pkg(&b, &counter);
    let _env = ENV.lock().unwrap();
    unsafe {
        std::env::set_var("ARTIFICER_TEST_RUNS", &counter);
    }

    let first = artificer::check(&a, &home).unwrap();
    assert_eq!(first.script, artificer::ScriptOutcome::Ran);
    assert_eq!(first.rustc, artificer::RustcOutcome::Ran);
    assert_eq!(runs(&counter), 1);

    let second = artificer::check(&b, &home).unwrap();
    assert_eq!(second.rustc, artificer::RustcOutcome::Restored);
    assert_eq!(second.script, artificer::ScriptOutcome::Restored);
    assert_eq!(runs(&counter), 1, "build.rs must not run on a lib hit");
    assert!(second.rlib.is_file());
}

#[test]
fn source_change_misses_and_reruns() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(format!("home-{n}"));
    let counter = tmp.path().join("runs");
    let a = tmp.path().join("a");
    write_pkg(&a, &counter);
    let _env = ENV.lock().unwrap();
    unsafe {
        std::env::set_var("ARTIFICER_TEST_RUNS", &counter);
    }
    assert_eq!(
        artificer::check(&a, &home).unwrap().script,
        artificer::ScriptOutcome::Ran
    );
    fs::write(
        a.join("build.rs"),
        r#"
fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    std::fs::write(format!("{out}/hello.rs"), "pub fn token() -> u8 { 8 }\n").unwrap();
    if let Ok(path) = std::env::var("ARTIFICER_TEST_RUNS") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        writeln!(f, "ran").unwrap();
    }
}
"#,
    )
    .unwrap();
    assert_eq!(
        artificer::check(&a, &home).unwrap().script,
        artificer::ScriptOutcome::Ran
    );
    assert_eq!(runs(&counter), 2);
}

#[test]
fn lib_edit_reruns_an_undeclared_script() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(format!("home-{n}"));
    let counter = tmp.path().join("runs");
    let a = tmp.path().join("a");
    write_pkg(&a, &counter);
    let _env = ENV.lock().unwrap();
    unsafe {
        std::env::set_var("ARTIFICER_TEST_RUNS", &counter);
    }
    assert_eq!(
        artificer::check(&a, &home).unwrap().script,
        artificer::ScriptOutcome::Ran
    );
    fs::write(
        a.join("src/lib.rs"),
        r#"
include!(concat!(env!("OUT_DIR"), "/hello.rs"));
pub fn ok() -> u8 { token() + 1 }
"#,
    )
    .unwrap();
    let again = artificer::check(&a, &home).unwrap();
    assert_eq!(again.script, artificer::ScriptOutcome::Ran);
    assert_eq!(again.rustc, artificer::RustcOutcome::Ran);
    assert_eq!(runs(&counter), 2);
}

#[test]
fn build_script_receives_feature_cfg() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("root");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"featured\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[features]\ndefault = [\"bundled\"]\nbundled = []\n",
    )
    .unwrap();
    fs::write(
        root.join("build.rs"),
        r#"
#[cfg(not(feature = "bundled"))]
compile_error!("build script feature cfg missing");

fn main() {}
"#,
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();

    let result = artificer::check(&root, &home).unwrap();

    assert_eq!(result.rustc, artificer::RustcOutcome::Ran);
}

#[test]
fn build_script_receives_rustdoc() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("root");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"rustdoc-env\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("build.rs"),
        r#"
fn main() {
    assert_eq!(std::env::var("RUSTDOC").as_deref(), Ok("rustdoc"));
}
"#,
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();

    let result = artificer::check(&root, &home).unwrap();

    assert_eq!(result.rustc, artificer::RustcOutcome::Ran);
}
