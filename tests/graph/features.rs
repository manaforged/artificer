use super::*;

#[test]
fn example_dev_dependency_change_recompiles() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let dep = tmp.path().join("dd");
    fs::create_dir_all(dep.join("src")).unwrap();
    fs::write(
        dep.join("Cargo.toml"),
        "[package]\nname = \"dd\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(dep.join("src/lib.rs"), "pub fn n() -> u8 { 1 }\n").unwrap();

    let a = tmp.path().join("a");
    fs::create_dir_all(a.join("src")).unwrap();
    fs::create_dir_all(a.join("examples")).unwrap();
    fs::write(
        a.join("Cargo.toml"),
        "[package]\nname = \"exd\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dev-dependencies]\ndd = { path = \"../dd\" }\n",
    )
    .unwrap();
    fs::write(a.join("src/lib.rs"), "pub fn n() -> u8 { 0 }\n").unwrap();
    fs::write(
        a.join("examples/show.rs"),
        "fn main() { std::process::exit(dd::n() as i32) }\n",
    )
    .unwrap();

    let first = artificer::run_cmd(
        &a,
        &[],
        None,
        Some("show"),
        &home,
        artificer::CheckOpts::default(),
        &[],
    )
    .unwrap();
    assert_eq!(first, 1, "the example sees its dev-dependency");

    fs::write(dep.join("src/lib.rs"), "pub fn n() -> u8 { 2 }\n").unwrap();
    let second = artificer::run_cmd(
        &a,
        &[],
        None,
        Some("show"),
        &home,
        artificer::CheckOpts::default(),
        &[],
    )
    .unwrap();
    assert_eq!(
        second, 2,
        "a changed dev-dependency must rebuild the example, not restore it"
    );
}

#[test]
fn why_miss_explains_a_source_change() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("wm");
    write(
        &a,
        "[package]\nname = \"wm\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "pub fn n() -> u8 { 1 }\n",
    );
    artificer::check(&a, &home).unwrap();
    std::fs::write(a.join("src/lib.rs"), "pub fn n() -> u8 { 2 }\n").unwrap();
    artificer::check(&a, &home).unwrap();
    let report = artificer::why_miss(&home, "wm").expect("two records");
    assert!(report.contains("was: source:"), "{report}");
    assert!(report.contains("now: source:"), "{report}");
}

#[test]
fn a_warm_build_runs_no_rustc() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    std::fs::create_dir_all(a.join("src")).unwrap();
    std::fs::write(
        a.join("Cargo.toml"),
        "[package]\nname = \"warm\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(a.join("src/main.rs"), "fn main() { println!(\"ok\"); }\n").unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("COUNT_RUSTC"), "").unwrap();
    let opts = artificer::CheckOpts {
        link: true,
        ..Default::default()
    };
    artificer::check_cmd(&a, &[], &home, opts.clone()).unwrap();
    let first = std::fs::read_to_string(home.join("rustc-runs"))
        .unwrap()
        .lines()
        .count();
    assert!(first > 0, "the first build compiles");
    artificer::check_cmd(&a, &[], &home, opts).unwrap();
    let second = std::fs::read_to_string(home.join("rustc-runs"))
        .unwrap()
        .lines()
        .count();
    assert_eq!(second, first, "a warm build must restore, not run rustc");
}

#[test]
fn concurrent_checks_share_one_store() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"racer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "pub fn n() -> u8 { 1 }\n",
    );
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let a = a.clone();
                let home = home.clone();
                scope.spawn(move || {
                    artificer::check_cmd(&a, &[], &home, artificer::CheckOpts::default())
                })
            })
            .collect();
        for handle in handles {
            let code = handle.join().expect("worker").expect("check");
            assert_eq!(code, 0);
        }
    });
}
