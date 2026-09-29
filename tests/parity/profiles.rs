use super::*;

#[test]
fn a_test_only_env_use_does_not_key_the_library() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("tenv", "")),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
        (
            "tests/it.rs",
            "#[test]\nfn uses_manifest_dir() {\n    assert!(!env!(\"CARGO_MANIFEST_DIR\").is_empty());\n}\n",
        ),
    ]);
    assert!(p.artificer(&["check"], &[]).ok());
    let twin = p.dir.parent().unwrap().join("twin-tenv");
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
        "a test-only env use must not pin the library to one checkout, got: {log}",
    );
}

#[test]
fn a_member_cwd_still_inherits_workspace_lints() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/m\"]\nresolver = \"2\"\n[workspace.lints.rust]\nunused_variables = \"deny\"\n",
        ),
        (
            "crates/m/Cargo.toml",
            "[package]\nname = \"m\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[lints]\nworkspace = true\n",
        ),
        ("crates/m/src/lib.rs", "pub fn f() { let unused = 1; }\n"),
    ]);
    let member = p.dir.join("crates/m");
    let artificer = Command::new(env!("CARGO_BIN_EXE_artificer"))
        .env("ARTIFICER_HOME", &p.home)
        .env("ARTIFICER_NOSERVE", "1")
        .current_dir(&member)
        .arg("check")
        .output()
        .unwrap();
    let stock = Command::new(stock_cargo())
        .current_dir(&member)
        .arg("check")
        .output()
        .unwrap();
    assert_eq!(
        artificer.status.code(),
        stock.status.code(),
        "member-cwd check must agree with cargo on workspace lints",
    );
    assert_ne!(
        artificer.status.code(),
        Some(0),
        "the deny must actually fire"
    );
}

#[test]
fn a_watched_directory_sees_a_change_below_it() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[package]\nname = \"dw\"\nversion = \"0.1.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n",
        ),
        (
            "build.rs",
            "use std::{env, fs, path::PathBuf};\nfn main() {\n    println!(\"cargo::rerun-if-changed=proto\");\n    let v = fs::read_to_string(\"proto/api.txt\").unwrap();\n    let out = PathBuf::from(env::var(\"OUT_DIR\").unwrap()).join(\"gen.rs\");\n    fs::write(out, format!(\"pub const V: &str = \\\"{}\\\";\", v.trim())).unwrap();\n}\n",
        ),
        (
            "src/lib.rs",
            "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\npub fn v() -> &'static str { V }\n",
        ),
        (
            "src/bin/show.rs",
            "fn main() { println!(\"{}\", dw::v()); }\n",
        ),
        ("proto/api.txt", "one\n"),
    ]);
    assert!(p.artificer(&["run"], &[]).ok());
    fs::write(p.dir.join("proto/api.txt"), "two\n").unwrap();
    let out = p.artificer(&["run"], &[]);
    assert!(
        out.stdout.contains("two"),
        "a change under a watched directory must regenerate, got: {}",
        out.stdout,
    );
}

#[test]
fn a_lint_table_check_cfg_reaches_rustc() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest(
                "lcfg",
                "\n[lints.rust]\nunexpected_cfgs = { level = \"deny\", check-cfg = ['cfg(custom_flag)'] }\n",
            ),
        ),
        (
            "src/lib.rs",
            "#[cfg(custom_flag)]\npub fn g() {}\npub fn f() -> u8 { 1 }\n",
        ),
    ]);
    p.parity(&["check"], &[]);
}

#[test]
fn a_test_profile_table_applies_to_cargo_test() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("ptest", "\n[profile.test]\ndebug-assertions = false\n"),
        ),
        (
            "src/lib.rs",
            "#[test]\nfn da() { assert!(!cfg!(debug_assertions)); }\n",
        ),
    ]);
    p.parity(&["test"], &[]);
}

#[test]
fn an_unknown_bin_name_is_not_a_success() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("bsel", "")),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
        ("src/bin/real.rs", "fn main() {}\n"),
    ]);
    let out = p.artificer(&["build", "--bin", "nope"], &[]);
    assert_ne!(
        out.code,
        Some(0),
        "an unknown --bin must not report success, got: {}",
        out.stdout,
    );
}

#[test]
fn clean_preserves_other_store_versions() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("layout", "")),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
    ]);
    assert!(p.artificer(&["check"], &[]).ok());
    let units = p.home.join("units");
    let current = units.join(artificer::LAYOUT);
    let stale = units.join("v-old");
    copy_tree(&current, &stale);
    let out = p.artificer(&["clean"], &[]);
    assert!(out.ok(), "clean failed: {}", out.stdout);
    assert!(
        stale.exists(),
        "another Artificer version may still be reading its own layout",
    );
}

#[test]
fn a_test_binary_runs_from_its_package_root() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("cwdt", "")),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
        ("fixture.txt", "hello\n"),
        (
            "tests/it.rs",
            "#[test]\nfn reads_a_relative_fixture() {\n    let s = std::fs::read_to_string(\"fixture.txt\").unwrap();\n    assert_eq!(s.trim(), \"hello\");\n}\n",
        ),
    ]);
    p.parity(&["test"], &[]);
}

#[test]
fn a_named_package_override_reaches_the_dependency() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\", \"dep\"]\nresolver = \"2\"\n\n[profile.dev.package.dep]\ndebug-assertions = false\n",
        ),
        (
            "app/Cargo.toml",
            &manifest("app", "\n[dependencies]\ndep = { path = \"../dep\" }\n"),
        ),
        (
            "app/src/main.rs",
            "fn main() { println!(\"{} {}\", cfg!(debug_assertions), dep::f()) }\n",
        ),
        ("dep/Cargo.toml", &manifest("dep", "")),
        (
            "dep/src/lib.rs",
            "pub fn f() -> bool { cfg!(debug_assertions) }\n",
        ),
    ]);
    p.parity_run(&[]);
}

#[test]
fn the_wildcard_override_skips_workspace_members() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\", \"dep\"]\nresolver = \"2\"\n\n[profile.dev.package.\"*\"]\ndebug-assertions = false\n",
        ),
        (
            "app/Cargo.toml",
            &manifest("app", "\n[dependencies]\ndep = { path = \"../dep\" }\n"),
        ),
        (
            "app/src/main.rs",
            "fn main() { println!(\"{} {}\", cfg!(debug_assertions), dep::f()) }\n",
        ),
        ("dep/Cargo.toml", &manifest("dep", "")),
        (
            "dep/src/lib.rs",
            "pub fn f() -> bool { cfg!(debug_assertions) }\n",
        ),
    ]);
    p.parity_run(&[]);
}

#[test]
fn an_override_edit_recompiles_the_package() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\", \"dep\"]\nresolver = \"2\"\n\n[profile.dev.package.dep]\ndebug-assertions = false\n",
        ),
        (
            "app/Cargo.toml",
            &manifest("app", "\n[dependencies]\ndep = { path = \"../dep\" }\n"),
        ),
        (
            "app/src/main.rs",
            "fn main() { println!(\"{}\", dep::f()) }\n",
        ),
        ("dep/Cargo.toml", &manifest("dep", "")),
        (
            "dep/src/lib.rs",
            "pub fn f() -> bool { cfg!(debug_assertions) }\n",
        ),
    ]);
    let toml = p.path("Cargo.toml");
    fs::write(&toml, "[workspace]\nmembers = [\"app\", \"dep\"]\nresolver = \"2\"\n\n[profile.dev.package.dep]\ndebug-assertions = false\n").unwrap();
    p.parity_run(&[]);
    fs::write(&toml, "[workspace]\nmembers = [\"app\", \"dep\"]\nresolver = \"2\"\n\n[profile.dev.package.dep]\ndebug-assertions = true\n").unwrap();
    p.parity_run(&[]);
}

#[test]
fn target_cfg_rustflags_match_the_host() {
    let os = std::env::consts::OS;
    let p = Project::new(&[
        ("Cargo.toml", &manifest("tcfg", "")),
        (
            ".cargo/config.toml",
            &format!(
                "[target.'cfg(target_os = \"{os}\")']\nrustflags = [\"--cfg\", \"viahostcfg\"]\n"
            ),
        ),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
        (
            "src/main.rs",
            "fn main() { if cfg!(viahostcfg) { println!(\"ON\") } else { println!(\"OFF\") } }\n",
        ),
    ]);
    p.parity_run(&[]);
}

#[test]
fn a_non_matching_target_table_contributes_nothing() {
    let other = if std::env::consts::OS == "windows" {
        "macos"
    } else {
        "windows"
    };
    let p = Project::new(&[
        ("Cargo.toml", &manifest("tneg", "")),
        (
            ".cargo/config.toml",
            &format!(
                "[target.'cfg(target_os = \"{other}\")']\nrustflags = [\"--cfg\", \"elsewhere\"]\n"
            ),
        ),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
        (
            "src/main.rs",
            "fn main() { if cfg!(elsewhere) { println!(\"LEAKED\") } else { println!(\"CLEAN\") } }\n",
        ),
    ]);
    p.parity_run(&[]);
}
