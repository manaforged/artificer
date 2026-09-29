use super::*;

#[test]
fn delivered_bin_does_not_alias_the_store() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"solo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "pub fn n() -> u8 { 1 }\n",
    );
    fs::write(a.join("src/main.rs"), "fn main() {}\n").unwrap();

    let code = artificer::check_cmd(
        &a,
        &[],
        &home,
        artificer::CheckOpts {
            link: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(code, 0);

    let name = if cfg!(windows) { "solo.exe" } else { "solo" };
    let delivered = target_dir(&a).join("debug").join(name);
    let stored = find_file(&home, name).expect("store unit");
    let before = fs::read(&stored).unwrap();
    fs::write(&delivered, b"tampered").unwrap();
    assert_eq!(fs::read(&stored).unwrap(), before);
}

#[test]
fn build_ships_cdylib() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"shared\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n",
        "#[no_mangle] pub extern \"C\" fn n() -> u8 { 1 }\n",
    );

    let code = artificer::check_cmd(
        &a,
        &[],
        &home,
        artificer::CheckOpts {
            link: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(code, 0);
    let file = if cfg!(windows) {
        "shared.dll"
    } else if cfg!(target_os = "macos") {
        "libshared.dylib"
    } else {
        "libshared.so"
    };
    let shipped = target_dir(&a).join("debug").join(file);
    assert!(shipped.is_file(), "missing {}", shipped.display());
}

#[test]
fn build_ships_every_bin() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"toolbox\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "pub fn n() -> u8 { 1 }\n",
    );
    fs::write(
        a.join("src/main.rs"),
        "fn main() { assert_eq!(toolbox::n(), 1); }\n",
    )
    .unwrap();
    fs::create_dir_all(a.join("src/bin")).unwrap();
    fs::write(
        a.join("src/bin/helper.rs"),
        "fn main() { assert_eq!(toolbox::n(), 1); }\n",
    )
    .unwrap();

    let code = artificer::check_cmd(
        &a,
        &[],
        &home,
        artificer::CheckOpts {
            link: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(code, 0);
    let debug = target_dir(&a).join("debug");
    for name in ["toolbox", "helper"] {
        let exe = debug.join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        });
        assert!(exe.is_file(), "missing {}", exe.display());
        let status = Command::new(&exe).status().unwrap();
        assert!(status.success(), "{} failed to run", exe.display());
    }
}

#[test]
fn feature_flags_reach_the_resolver() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"feat\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[features]\ndefault = [\"bad\"]\nbad = []\non = []\n",
        "#[cfg(feature = \"bad\")] compile_error!(\"default feature must be off\");\n#[cfg(not(feature = \"on\"))] compile_error!(\"the on feature must be set\");\npub fn n() -> u8 { 1 }\n",
    );
    artificer::check_cmd(&a, &[], &home, artificer::CheckOpts::default())
        .expect_err("locked must fail");
    let code = artificer::check_cmd(
        &a,
        &[],
        &home,
        artificer::CheckOpts {
            no_default: true,
            features: vec!["on".to_string()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(code, 0);
}

#[test]
fn test_target_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"sel\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "pub fn n() -> u8 { 1 }\n#[cfg(test)] mod t { #[test] fn unit() { panic!(\"lib tests must not run\") } }\n",
    );
    std::fs::create_dir_all(a.join("tests")).unwrap();
    std::fs::write(
        a.join("tests/host.rs"),
        "#[test]\nfn int() { assert_eq!(sel::n(), 1); }\n",
    )
    .unwrap();
    let code = artificer::test_package(
        &a,
        &[],
        &home,
        &artificer::TestOpts {
            only: vec!["host".to_string()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        code, 0,
        "only tests/host.rs runs; the panicking lib test must not"
    );
    let missing = artificer::test_package(
        &a,
        &[],
        &home,
        &artificer::TestOpts {
            only: vec!["nope".to_string()],
            ..Default::default()
        },
    );
    missing.expect_err("a missing --test name must be an error");
    let code = artificer::test_package(
        &a,
        &[],
        &home,
        &artificer::TestOpts {
            lib: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(code, 0, "--lib must run the lib's failing unit test");
}

#[test]
fn run_example_sees_dev_deps() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"exr\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dev-dependencies]\ncfg-if = \"1\"\n",
        "pub fn n() -> u8 { 7 }\n",
    );
    std::fs::create_dir_all(a.join("examples")).unwrap();
    std::fs::write(
        a.join("examples/show.rs"),
        "fn main() { cfg_if::cfg_if! { if #[cfg(unix)] { std::process::exit(exr::n() as i32) } else { std::process::exit(exr::n() as i32) } } }\n",
    )
    .unwrap();
    let code = artificer::run_cmd(
        &a,
        &[],
        None,
        Some("show"),
        &home,
        artificer::CheckOpts::default(),
        &[],
    )
    .unwrap();
    assert_eq!(code, 7, "the example must run and see lib + dev-deps");
    let missing = artificer::run_cmd(
        &a,
        &[],
        None,
        Some("nope"),
        &home,
        artificer::CheckOpts::default(),
        &[],
    );
    missing.expect_err("a missing example name must be an error");
}

#[test]
fn locked_guards_the_lockfile() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"lk\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "pub fn n() -> u8 { 1 }\n",
    );
    std::fs::write(
        a.join("Cargo.toml"),
        "[package]\nname = \"lk\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ncfg-if = \"1\"\n",
    )
    .unwrap();
    let locked = artificer::check_cmd(
        &a,
        &[],
        &home,
        artificer::CheckOpts {
            meta_flags: vec!["--locked".to_string()],
            ..Default::default()
        },
    );
    locked.expect_err("--locked must refuse a stale lockfile");
    let unlocked = artificer::check_cmd(&a, &[], &home, artificer::CheckOpts::default()).unwrap();
    assert_eq!(
        unlocked, 0,
        "without --locked the lock updates and the check passes"
    );
}

#[test]
fn release_is_a_profile_dimension() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    fs::create_dir_all(a.join("src")).unwrap();
    fs::write(
        a.join("Cargo.toml"),
        "[package]\nname = \"prof\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        a.join("src/main.rs"),
        "fn main() { std::process::exit(i32::from(cfg!(debug_assertions))) }\n",
    )
    .unwrap();
    let st = Command::new("cargo")
        .args(["generate-lockfile", "--manifest-path"])
        .arg(a.join("Cargo.toml"))
        .status()
        .unwrap();
    assert!(st.success());
    let dev = artificer::run_cmd(
        &a,
        &[],
        None,
        None,
        &home,
        artificer::CheckOpts::default(),
        &[],
    )
    .unwrap();
    assert_eq!(dev, 1, "dev keeps debug assertions");
    let rel = artificer::run_cmd(
        &a,
        &[],
        None,
        None,
        &home,
        artificer::CheckOpts {
            release: true,
            ..Default::default()
        },
        &[],
    )
    .unwrap();
    assert_eq!(rel, 0, "release turns debug assertions off");
    let dev2 = artificer::run_cmd(
        &a,
        &[],
        None,
        None,
        &home,
        artificer::CheckOpts::default(),
        &[],
    )
    .unwrap();
    assert_eq!(dev2, 1, "the dev unit survives beside the release unit");
}
