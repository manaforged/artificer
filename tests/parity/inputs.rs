use super::*;

#[test]
fn a_declared_script_env_is_watched() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("envs", "")),
        (
            "build.rs",
            "fn main() {\n    println!(\"cargo::rerun-if-env-changed=PICK\");\n    println!(\"cargo::rustc-env=PICKED={}\", std::env::var(\"PICK\").unwrap_or_else(|_| \"none\".into()));\n}\n",
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"PICKED={}\", env!(\"PICKED\")) }\n",
        ),
    ]);
    p.parity_run(&[("PICK", "a")]);
    p.parity_run(&[("PICK", "b")]);
}

#[test]
fn an_unknown_feature_gate_is_rejected() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest(
                "ct",
                "\n[features]\nreal = []\n\n[lints.rust]\nunexpected_cfgs = \"deny\"\n",
            ),
        ),
        (
            "src/lib.rs",
            "pub fn f() -> u8 { if cfg!(feature = \"typo\") { 1 } else { 2 } }\n",
        ),
    ]);
    p.parity(&["check"], &[]);
}

#[test]
fn a_declared_feature_gate_is_accepted() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest(
                "cok",
                "\n[features]\nreal = []\n\n[lints.rust]\nunexpected_cfgs = \"deny\"\n",
            ),
        ),
        (
            "src/lib.rs",
            "pub fn f() -> u8 { if cfg!(feature = \"real\") { 1 } else { 2 } }\n",
        ),
    ]);
    p.parity(&["check"], &[]);
    p.parity(&["check", "--features", "real"], &[]);
}

#[test]
fn a_script_declared_cfg_is_checked() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("sc2", "\n[lints.rust]\nunexpected_cfgs = \"deny\"\n"),
        ),
        (
            "build.rs",
            "fn main() {\n    println!(\"cargo::rustc-check-cfg=cfg(known)\");\n    println!(\"cargo::rustc-cfg=known\");\n}\n",
        ),
        (
            "src/lib.rs",
            "pub fn f() -> u8 { if cfg!(known) { 1 } else { 2 } }\n",
        ),
    ]);
    p.parity(&["check"], &[]);
}

#[test]
fn a_script_rustc_flags_line_is_applied() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("rfl", "")),
        (
            "build.rs",
            "fn main() { println!(\"cargo::rustc-flags=-L /tmp/artificer-nonexistent-search\"); }\n",
        ),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
    ]);
    p.parity(&["check"], &[]);
}

#[test]
fn an_included_asset_is_content_addressed() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("ak", "")),
        ("data.txt", "AAAA\n"),
        (
            "src/main.rs",
            "fn main() { print!(\"{}\", include_str!(\"../data.txt\")) }\n",
        ),
    ]);
    p.parity_run(&[]);

    let asset = p.path("data.txt");
    let stamp = fs::metadata(&asset).unwrap().modified().unwrap();
    fs::write(&asset, "BBBB\n").unwrap();
    let f = fs::File::options().write(true).open(&asset).unwrap();
    f.set_modified(stamp).unwrap();
    drop(f);

    let run = p.artificer(&["run"], &[]);
    assert!(run.ok());
    assert_eq!(
        run.stdout.trim(),
        "BBBB",
        "a same-length asset rewrite must not be masked by a timestamp",
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_source_is_content_addressed() {
    use std::os::unix::fs::symlink;

    let p = Project::new(&[
        ("Cargo.toml", &manifest("symlink_src", "")),
        ("src/main.rs", "fn main() { println!(\"placeholder\") }\n"),
    ]);
    let source = p._tmp.path().join("main.rs");
    fs::write(&source, "fn main() { println!(\"one\") }\n").expect("write external source");
    fs::remove_file(p.path("src/main.rs")).expect("remove placeholder source");
    symlink(&source, p.path("src/main.rs")).expect("link external source");
    p.parity_run(&[]);

    fs::write(&source, "fn main() { println!(\"two\") }\n").expect("edit external source");
    p.parity_run(&[]);
}

#[cfg(unix)]
#[test]
fn a_symlinked_asset_is_content_addressed() {
    use std::os::unix::fs::symlink;

    let p = Project::new(&[
        ("Cargo.toml", &manifest("symlink_asset", "")),
        ("data.txt", "placeholder\n"),
        (
            "src/main.rs",
            "fn main() { print!(\"{}\", include_str!(\"../data.txt\")) }\n",
        ),
    ]);
    let asset = p._tmp.path().join("data.txt");
    fs::write(&asset, "one\n").expect("write external asset");
    fs::remove_file(p.path("data.txt")).expect("remove placeholder asset");
    symlink(&asset, p.path("data.txt")).expect("link external asset");
    p.parity_run(&[]);

    fs::write(&asset, "two\n").expect("edit external asset");
    p.parity_run(&[]);
}

#[test]
fn a_doctest_answers_to_rustflags() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("dt", "")),
        (
            "src/lib.rs",
            "/// ```\n/// assert!(dt::f() > 0);\n/// let unused = 5;\n/// ```\npub fn f() -> u8 { 1 }\n",
        ),
    ]);
    p.parity(&["test"], &[]);
    p.parity(&["test"], &[("RUSTFLAGS", "-D warnings")]);
}

#[test]
fn a_second_checkout_of_a_scripted_crate_is_all_hits() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("shared", "")),
        (
            "build.rs",
            "fn main() {\n    println!(\"cargo::rustc-check-cfg=cfg(built)\");\n    println!(\"cargo::rustc-cfg=built\");\n}\n",
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", cfg!(built)) }\n",
        ),
    ]);
    assert!(
        p.artificer(&["check"], &[]).ok(),
        "first build must succeed"
    );

    let twin = p.dir.parent().unwrap().join("twin-scripted");
    copy_tree(&p.dir, &twin);
    let out = Command::new(env!("CARGO_BIN_EXE_artificer"))
        .env("ARTIFICER_HOME", &p.home)
        .env("ARTIFICER_NOSERVE", "1")
        .current_dir(&twin)
        .arg("check")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(
        nothing_compiled(&log),
        "a second checkout must reuse every unit, got: {log}",
    );
}

#[test]
fn a_member_script_that_prints_the_workspace_root_reruns_in_a_second_checkout() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
        ),
        (
            "member/Cargo.toml",
            &manifest("member", "build = \"build.rs\"\n"),
        ),
        (
            "member/build.rs",
            "fn main() { let dir = std::path::PathBuf::from(std::env::var(\"CARGO_MANIFEST_DIR\").unwrap()); println!(\"cargo:rustc-env=ROOT_DIR={}\", dir.parent().unwrap().display()); }\n",
        ),
        (
            "member/src/main.rs",
            "fn main() { println!(\"{}\", env!(\"ROOT_DIR\")) }\n",
        ),
    ]);
    let first = p.artificer(&["run", "-p", "member"], &[]);
    assert!(first.ok());
    assert!(first.stdout.trim().ends_with("proj"), "{}", first.stdout);

    let twin = p.dir.parent().unwrap().join("twin-root");
    copy_tree(&p.dir, &twin);
    let out = Command::new(env!("CARGO_BIN_EXE_artificer"))
        .env("ARTIFICER_HOME", &p.home)
        .env("ARTIFICER_NOSERVE", "1")
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(&twin)
        .args(["run", "-p", "member"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = String::from_utf8_lossy(&out.stdout);
    assert!(
        printed.trim().ends_with("twin-root"),
        "the second checkout printed the first checkout's root: {printed}"
    );
}

#[test]
fn a_build_script_that_prints_its_path_reruns_in_a_second_checkout() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("script_path", "build = \"build.rs\"\n"),
        ),
        (
            "build.rs",
            "fn main() { println!(\"cargo:rustc-env=SCRIPT_DIR={}\", std::env::var(\"CARGO_MANIFEST_DIR\").unwrap()); }\n",
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", env!(\"SCRIPT_DIR\")) }\n",
        ),
    ]);
    let first = p.artificer(&["run"], &[]);
    assert!(first.ok());
    assert!(first.stdout.trim().ends_with("proj"), "{}", first.stdout);

    let twin = p.dir.parent().unwrap().join("twin-path");
    copy_tree(&p.dir, &twin);
    let out = Command::new(env!("CARGO_BIN_EXE_artificer"))
        .env("ARTIFICER_HOME", &p.home)
        .env("ARTIFICER_NOSERVE", "1")
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(&twin)
        .arg("run")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = String::from_utf8_lossy(&out.stdout);
    assert!(
        printed.trim().ends_with("twin-path"),
        "the second checkout printed the first checkout's path: {printed}"
    );
}

#[test]
fn a_second_checkout_of_a_workspace_is_all_hits() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"base\", \"top\"]\nresolver = \"2\"\n",
        ),
        (
            "base/Cargo.toml",
            "[package]\nname = \"base\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "base/build.rs",
            "fn main() {\n    println!(\"cargo::rustc-check-cfg=cfg(built)\");\n    println!(\"cargo::rustc-cfg=built\");\n}\n",
        ),
        (
            "base/src/lib.rs",
            "pub fn f() -> u8 { if cfg!(built) { 1 } else { 0 } }\n",
        ),
        (
            "top/Cargo.toml",
            "[package]\nname = \"top\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nbase = { path = \"../base\" }\n",
        ),
        (
            "top/src/main.rs",
            "fn main() { println!(\"{}\", base::f()) }\n",
        ),
    ]);
    assert!(p.artificer(&["check", "--workspace"], &[]).ok());

    let twin = p.dir.parent().unwrap().join("twin-ws");
    copy_tree(&p.dir, &twin);
    let out = Command::new(env!("CARGO_BIN_EXE_artificer"))
        .env("ARTIFICER_HOME", &p.home)
        .env("ARTIFICER_NOSERVE", "1")
        .current_dir(&twin)
        .args(["check", "--workspace"])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(
        nothing_compiled(&log),
        "a second checkout of a workspace must reuse every unit, got: {log}",
    );
}

#[test]
fn a_toolchain_file_does_not_pin_units_to_one_checkout() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("tc", "")),
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\n"),
        ("src/main.rs", "fn main() { println!(\"hi\") }\n"),
    ]);
    assert!(p.artificer(&["check"], &[]).ok());

    let twin = p.dir.parent().unwrap().join("twin-toolchain");
    copy_tree(&p.dir, &twin);
    let out = Command::new(env!("CARGO_BIN_EXE_artificer"))
        .env("ARTIFICER_HOME", &p.home)
        .env("ARTIFICER_NOSERVE", "1")
        .current_dir(&twin)
        .arg("check")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(
        nothing_compiled(&log),
        "a toolchain file must not make units checkout-specific, got: {log}",
    );
}

#[test]
fn a_test_cfg_block_is_not_an_unknown_cfg() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("tcfg", "\n[lints.rust]\nunexpected_cfgs = \"deny\"\n"),
        ),
        (
            "src/lib.rs",
            "pub fn f() -> u8 { 1 }\n#[cfg(test)]\nmod t {\n    #[test]\n    fn ok() { assert_eq!(super::f(), 1) }\n}\n",
        ),
    ]);
    p.parity(&["check"], &[]);
    p.parity(&["test"], &[]);
}
