use super::*;

#[test]
fn standalone_help_and_version_identify_artificer() {
    let help = Command::new(env!("CARGO_BIN_EXE_artificer"))
        .arg("--help")
        .output()
        .expect("run Artificer help");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage: artificer"));

    let version = Command::new(env!("CARGO_BIN_EXE_artificer"))
        .arg("--version")
        .output()
        .expect("run Artificer version");
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        concat!("artificer ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn shim_version_still_identifies_cargo() {
    let tmp = tempfile::tempdir().expect("temporary directory");
    let wrapped = shim(&tmp.path().join("shim"), tmp.path())
        .arg("--version")
        .output()
        .expect("run shim version");
    let direct = stock(tmp.path())
        .arg("--version")
        .output()
        .expect("run Cargo version");
    assert_eq!(wrapped.status.code(), direct.status.code());
    assert_eq!(wrapped.stdout, direct.stdout);
    assert_eq!(wrapped.stderr, direct.stderr);
}

#[test]
fn unknown_flag_exits_2_for_shim_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    let status = artificer(&home, &root)
        .args(["check", "--definitely-unknown"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}

#[test]
fn locked_stays_on_the_fast_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    let status = artificer(&home, &root)
        .args(["check", "--locked"])
        .status()
        .unwrap();
    let direct = stock(&root).args(["check", "--locked"]).output().unwrap();
    assert_eq!(
        status.code(),
        direct.status.code(),
        "--locked without a lock must fail exactly as cargo does"
    );
    let st = stock(&root)
        .args(["generate-lockfile", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .status()
        .unwrap();
    assert!(st.success());
    let status = artificer(&home, &root)
        .args(["check", "--locked"])
        .status()
        .unwrap();
    assert_eq!(
        status.code(),
        Some(0),
        "--locked with a fresh lock checks on artificer"
    );
}

#[test]
fn existing_file_is_a_test_filter_not_a_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    fs::write(root.join("green"), b"not a dir").unwrap();
    let home = tmp.path().join("home");
    let status = artificer(&home, &root)
        .args(["test", "green"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
}

#[test]
fn test_filter_reaches_the_harness() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");

    let status = artificer(&home, &root)
        .args(["test", "green"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));

    let status = artificer(&home, &root).arg("test").status().unwrap();
    assert_ne!(status.code(), Some(0));

    let status = artificer(&home, &root)
        .args(["test", "--", "--exact", "green"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
}

#[test]
fn run_executes_the_shipped_bin() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"runner\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"ran:{:?}\", std::env::args().nth(1)); }\n",
    )
    .unwrap();
    let home = tmp.path().join("home");
    let out = artificer(&home, &root)
        .args(["run", "--", "hello"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ran:Some(\"hello\")"), "{stdout}");
}

#[test]
fn run_forwards_the_child_exit_code() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"leaver\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() { std::process::exit(7); }\n",
    )
    .unwrap();
    let home = tmp.path().join("home");
    let status = artificer(&home, &root).arg("run").status().unwrap();
    assert_eq!(status.code(), Some(7));
}

#[test]
fn doctests_run_and_fail_like_cargo() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"documented\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "/// Adds one.\n///\n/// ```\n/// assert_eq!(documented::add_one(1), 2);\n/// ```\npub fn add_one(n: u8) -> u8 { n + 1 }\n",
    )
    .unwrap();
    let home = tmp.path().join("home");
    let status = artificer(&home, &root).arg("test").status().unwrap();
    assert_eq!(status.code(), Some(0));

    fs::write(
        root.join("src/lib.rs"),
        "/// Adds one.\n///\n/// ```\n/// assert_eq!(documented::add_one(1), 3);\n/// ```\npub fn add_one(n: u8) -> u8 { n + 1 }\n",
    )
    .unwrap();
    let status = artificer(&home, &root).arg("test").status().unwrap();
    assert_ne!(
        status.code(),
        Some(0),
        "a failing doctest must fail the run"
    );
}

#[test]
fn check_covers_every_bin_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    fs::create_dir_all(root.join("src/bin")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"bins\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn n() -> u8 { 1 }\n").unwrap();
    fs::write(
        root.join("src/bin/extra.rs"),
        "fn main() { let _: u8 = \"no\"; }\n",
    )
    .unwrap();
    let home = tmp.path().join("home");
    let status = artificer(&home, &root).arg("check").status().unwrap();
    assert_ne!(status.code(), Some(0));
}

#[test]
fn check_tests_flag_compiles_integration_tests() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"withtests\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn n() -> u8 { 1 }\n").unwrap();
    fs::write(
        root.join("tests/it.rs"),
        "#[test] fn t() { let _: u8 = \"no\"; }\n",
    )
    .unwrap();
    let home = tmp.path().join("home");

    let status = artificer(&home, &root).arg("check").status().unwrap();
    assert_eq!(
        status.code(),
        Some(0),
        "plain check skips tests/, like cargo"
    );

    let status = artificer(&home, &root)
        .args(["check", "--tests"])
        .status()
        .unwrap();
    assert_ne!(
        status.code(),
        Some(0),
        "--tests must surface the broken test target"
    );
}

#[test]
fn test_runs_bin_unit_tests() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"binunit\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() {}\n#[cfg(test)] mod t { #[test] fn boom() { panic!(\"bin unit test\"); } }\n",
    )
    .unwrap();
    let home = tmp.path().join("home");
    let status = artificer(&home, &root).arg("test").status().unwrap();
    assert_ne!(
        status.code(),
        Some(0),
        "cargo test runs bin unit tests; artificer must too"
    );
}

#[test]
fn doctor_reports_each_link() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    let out = artificer(&home, &root).arg("doctor").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("ok   jobserver") || stdout.contains("native Cargo scheduler"),
        "{stdout}"
    );
    assert!(stdout.contains("ok   store"), "{stdout}");
    let any_bad = stdout.lines().any(|l| l.starts_with("BAD "));
    assert_eq!(
        out.status.code(),
        Some(if any_bad { 1 } else { 0 }),
        "{stdout}"
    );
}

#[test]
fn explicit_color_is_modeled() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    let status = artificer(&home, &root)
        .args(["check", "--color", "always"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    let units = fs::read_dir(home.join("units").join(artificer::LAYOUT))
        .map(|d| d.count())
        .unwrap_or(0);
    assert!(units > 0, "the command must go through the store");
}

#[test]
fn color_equals_form_is_modeled() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    let home = tmp.path().join("home");
    let status = artificer(&home, &root)
        .args(["check", "--color=never"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    let units = fs::read_dir(home.join("units").join(artificer::LAYOUT))
        .map(|d| d.count())
        .unwrap_or(0);
    assert!(units > 0, "the command must go through the store");
}

#[test]
fn subcommand_help_prints_usage_and_is_not_a_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_clean_pkg(&root);
    let home = tmp.path().join("home");
    for args in [
        &["build", "--help"][..],
        &["check", "-h"],
        &["export", "--help"],
        &["help", "why-miss"],
    ] {
        let out = artificer(&home, &root).args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(text.contains("Usage: artificer"), "{args:?}: {text}");
    }
    assert_eq!(artificer::store_stat(&home).unwrap().fallbacks, 0);
}

#[test]
fn why_miss_without_a_record_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_clean_pkg(&root);
    let out = artificer(&tmp.path().join("home"), &root)
        .args(["why-miss", "nosuchcrate"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
}
