use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn write_lib(root: &Path, name: &str, edition: &str, body: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"{edition}\"\n"),
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), body).unwrap();
}

fn rustc_runs(home: &Path) -> usize {
    fs::read_to_string(home.join("rustc-runs"))
        .unwrap_or_default()
        .lines()
        .filter(|l| *l == "ran")
        .count()
}

fn home(tmp: &Path, n: u64) -> std::path::PathBuf {
    let home = tmp.join(format!("home-{n}"));
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("COUNT_RUSTC"), "").unwrap();
    home.canonicalize().expect("artificer home")
}

#[test]
fn second_checkout_skips_rustc() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path(), n);
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    write_lib(&a, "widget", "2021", "pub fn n() -> u8 { 1 }\n");
    write_lib(&b, "widget", "2021", "pub fn n() -> u8 { 1 }\n");

    let first = artificer::check(&a, &home).unwrap();
    assert_eq!(first.rustc, artificer::RustcOutcome::Ran);
    assert_eq!(first.script, artificer::ScriptOutcome::None);
    let name = first.rlib.file_name().unwrap().to_string_lossy();
    assert!(
        name.starts_with("libwidget-") && (name.ends_with(".rmeta") || name.ends_with(".rlib"))
    );
    assert_eq!(rustc_runs(&home), 1);

    let second = artificer::check(&b, &home).unwrap();
    assert_eq!(second.rustc, artificer::RustcOutcome::Restored);
    assert_eq!(rustc_runs(&home), 1, "second checkout must not run rustc");
    assert!(second.rlib.is_file());
}

#[test]
fn same_dir_second_check_restores() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path(), n);
    let a = tmp.path().join("a");
    write_lib(&a, "widget", "2021", "pub fn n() -> u8 { 1 }\n");
    assert_eq!(
        artificer::check(&a, &home).unwrap().rustc,
        artificer::RustcOutcome::Ran
    );
    assert_eq!(
        artificer::check(&a, &home).unwrap().rustc,
        artificer::RustcOutcome::Restored
    );
    assert_eq!(rustc_runs(&home), 1);
}

#[test]
fn edit_misses() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path(), n);
    let a = tmp.path().join("a");
    write_lib(&a, "widget", "2021", "pub fn n() -> u8 { 1 }\n");
    assert_eq!(
        artificer::check(&a, &home).unwrap().rustc,
        artificer::RustcOutcome::Ran
    );
    fs::write(a.join("src/lib.rs"), "pub fn n() -> u8 { 2 }\n").unwrap();
    assert_eq!(
        artificer::check(&a, &home).unwrap().rustc,
        artificer::RustcOutcome::Ran
    );
    assert_eq!(rustc_runs(&home), 2);
}

#[test]
fn rustc_failure_does_not_publish() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path(), n);
    let a = tmp.path().join("a");
    write_lib(&a, "widget", "2021", "fn broken (\n");
    artificer::check(&a, &home).expect_err("broken rustc");
    let units = home.join("units").join(artificer::LAYOUT);
    if units.is_dir() {
        for entry in fs::read_dir(&units).unwrap() {
            let entry = entry.unwrap();
            assert!(
                !entry.path().join("ok").is_file(),
                "failed rustc must not leave an ok marker"
            );
        }
    }
}

#[test]
fn hit_uses_unit_path() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path(), n);
    let a = tmp.path().join("a");
    write_lib(&a, "widget", "2021", "pub fn n() -> u8 { 1 }\n");
    let first = artificer::check(&a, &home).unwrap();
    assert!(
        first
            .rlib
            .canonicalize()
            .unwrap()
            .starts_with(home.join("units")),
        "hit path {}",
        first.rlib.display()
    );
    let second = artificer::check(&a, &home).unwrap();
    assert_eq!(second.rustc, artificer::RustcOutcome::Restored);
    assert_eq!(first.rlib, second.rlib);
    let scratch = home.join("scratch");
    if scratch.is_dir() {
        let kids: Vec<_> = fs::read_dir(&scratch).unwrap().collect();
        assert!(kids.is_empty(), "units hit must not restore to scratch");
    }
}

#[test]
fn asset_file_misses() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path(), n);
    let a = tmp.path().join("a");
    write_lib(&a, "widget", "2021", "pub fn n() -> u8 { 1 }\n");
    assert_eq!(
        artificer::check(&a, &home).unwrap().rustc,
        artificer::RustcOutcome::Ran
    );
    fs::write(a.join("NOTES.md"), "noise").unwrap();
    assert_eq!(
        artificer::check(&a, &home).unwrap().rustc,
        artificer::RustcOutcome::Ran,
        "an asset may be an include_str! input"
    );
    fs::create_dir_all(a.join("node_modules/x")).unwrap();
    fs::write(a.join("node_modules/x/index.js"), "1").unwrap();
    assert_eq!(
        artificer::check(&a, &home).unwrap().rustc,
        artificer::RustcOutcome::Restored,
        "node_modules junk is not a compile input"
    );
    assert_eq!(rustc_runs(&home), 2);
}

#[test]
fn check_emits_rmeta() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path(), n);
    let a = tmp.path().join("a");
    write_lib(&a, "widget", "2021", "pub fn n() -> u8 { 1 }\n");
    let report = artificer::check(&a, &home).unwrap();
    let name = report.rlib.file_name().unwrap().to_string_lossy();
    assert!(
        name.ends_with(".rmeta"),
        "check path should skip LLVM link: {name}"
    );
}

#[test]
fn bin_only_check() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path(), n);
    let a = tmp.path().join("a");
    fs::create_dir_all(a.join("src")).unwrap();
    fs::write(
        a.join("Cargo.toml"),
        "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(a.join("src/main.rs"), "fn main() {}\n").unwrap();
    let report = artificer::check(&a, &home).unwrap();
    assert_eq!(report.rustc, artificer::RustcOutcome::Ran);
    assert!(report.rlib.is_file());
    let second = artificer::check(&a, &home).unwrap();
    assert_eq!(second.rustc, artificer::RustcOutcome::Restored);
}

#[test]
fn edition_2015_compiles() {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path(), n);
    let a = tmp.path().join("a");
    write_lib(
        &a,
        "old",
        "2015",
        "pub fn take(_b: Box<std::fmt::Display>) {}\n",
    );
    let report = artificer::check(&a, &home).unwrap();
    assert_eq!(report.rustc, artificer::RustcOutcome::Ran);
    let name = report.rlib.file_name().unwrap().to_string_lossy();
    assert!(name.starts_with("libold-") && (name.ends_with(".rmeta") || name.ends_with(".rlib")));
}
