use super::*;

#[cfg(unix)]
#[test]
fn a_rustc_replaced_at_the_same_path_recompiles() {
    use std::os::unix::fs::PermissionsExt;
    let p = Project::new(&[
        ("Cargo.toml", &manifest("same_path_rustc", "")),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", cfg!(changed)); }\n",
        ),
    ]);
    let real = rustup_tool("rustc");
    let wrapper = p.dir.parent().unwrap().join("rustc-wrapper");
    let write = |extra: &str| {
        fs::write(
            &wrapper,
            format!("#!/bin/sh\nexec '{}' {extra} \"$@\"\n", real.display()),
        )
        .unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    };
    let rustc = wrapper.display().to_string();
    write("");
    let first = p.artificer(&["run"], &[("RUSTC", &rustc)]);
    assert!(first.ok(), "{}", first.stdout);
    assert_eq!(first.stdout.trim(), "false");
    write("--cfg changed --check-cfg 'cfg(changed)'");
    let second = p.artificer(&["run"], &[("RUSTC", &rustc)]);
    assert!(second.ok(), "{}", second.stdout);
    assert_eq!(
        second.stdout.trim(),
        "true",
        "the old compiler's output was reused"
    );
}

#[test]
fn a_bin_can_use_its_own_lib() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("binlib", "")),
        ("src/lib.rs", "pub fn hello() -> u8 { 7 }\n"),
        (
            "src/bin/tool.rs",
            "fn main() { println!(\"{}\", binlib::hello()); }\n",
        ),
    ]);
    p.parity(&["check"], &[]);
    p.parity(&["check", "--all-targets"], &[]);
    p.parity(&["build"], &[]);
}

#[test]
fn a_build_script_sees_only_its_own_features() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest(
                "feature_subset",
                "build = \"build.rs\"\n[dependencies]\nsubsetdep = { path = \"subsetdep\", features = [\"x\"] }\n[build-dependencies]\nsubsetdep = { path = \"subsetdep\" }\n",
            ),
        ),
        (
            "subsetdep/Cargo.toml",
            &manifest("subsetdep", "[features]\nx = []\n"),
        ),
        (
            "subsetdep/src/lib.rs",
            "pub fn has_x() -> bool { cfg!(feature = \"x\") }\n",
        ),
        (
            "build.rs",
            "fn main() { println!(\"cargo:rustc-env=BUILD_X={}\", subsetdep::has_x()); }\n",
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"{} {}\", env!(\"BUILD_X\"), subsetdep::has_x()); }\n",
        ),
    ]);
    let stock = p.stock(&["run", "-q"], &[]);
    assert_eq!(stock.stdout.trim(), "false true");
    let run = p.artificer(&["run"], &[]);
    if run.ok() {
        assert_eq!(
            run.stdout.trim(),
            "false true",
            "the build script saw normal-code features"
        );
    } else {
        assert_eq!(run.code, Some(2), "artificer must decline, not fail");
    }
}

#[test]
fn a_build_dependency_feature_stays_out_of_normal_code() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest(
                "feature_split",
                "build = \"build.rs\"\n[dependencies]\nsplitdep = { path = \"splitdep\" }\n[build-dependencies]\nsplitdep = { path = \"splitdep\", features = [\"x\"] }\n",
            ),
        ),
        (
            "splitdep/Cargo.toml",
            &manifest("splitdep", "[features]\nx = []\n"),
        ),
        (
            "splitdep/src/lib.rs",
            "pub fn has_x() -> bool { cfg!(feature = \"x\") }\n",
        ),
        ("build.rs", "fn main() { let _ = splitdep::has_x(); }\n"),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", splitdep::has_x()); }\n",
        ),
    ]);
    let stock = p.stock(&["run", "-q"], &[]);
    assert_eq!(stock.stdout.trim(), "false");
    let run = p.artificer(&["run"], &[]);
    if run.ok() {
        assert_eq!(
            run.stdout.trim(),
            "false",
            "a build-only feature reached main"
        );
    } else {
        assert_eq!(run.code, Some(2), "artificer must decline, not fail");
    }
}

#[test]
fn rustflags_cfg_reaches_the_compiler() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("rf", "")),
        (
            "src/main.rs",
            "fn main() {\n    if cfg!(alpha) { println!(\"ALPHA\") } else { println!(\"PLAIN\") }\n}\n",
        ),
    ]);
    p.parity_run(&[]);
    p.parity_run(&[("RUSTFLAGS", "--cfg alpha")]);
}

#[test]
fn rustflags_deny_warnings_fails_both() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("den", "")),
        ("src/lib.rs", "pub fn f() -> u8 { let unused = 5; 1 }\n"),
    ]);
    p.parity(&["check"], &[]);
    p.parity(&["check"], &[("RUSTFLAGS", "-D warnings")]);
}

#[test]
fn release_binary_lands_in_target_release() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("rel", "")),
        ("src/main.rs", "fn main() { println!(\"hi\") }\n"),
    ]);
    assert!(p.artificer(&["build", "--release"], &[]).ok());
    let exe = if cfg!(windows) { "rel.exe" } else { "rel" };
    assert!(
        p.path("target/release").join(exe).is_file(),
        "release build must ship to target/release",
    );
    assert!(
        !p.path("target/debug").join(exe).is_file(),
        "a release build must not appear in target/debug",
    );
}

#[test]
fn no_stray_directories_in_the_source_tree() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("clean", "")),
        ("src/lib.rs", "pub fn f() {}\n"),
    ]);
    assert!(p.artificer(&["check"], &[]).ok());
    assert!(
        !p.path("off").exists(),
        "`off/` directory in the source tree"
    );
    for stray in ["incremental", "deps"] {
        assert!(
            !p.path(stray).exists(),
            "stray `{stray}/` in the source tree",
        );
    }
}

#[test]
fn release_flag_changes_debug_assertions() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("dbg", "")),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", cfg!(debug_assertions)) }\n",
        ),
    ]);
    let f = p.artificer(&["run", "--release"], &[]);
    let s = p.stock(&["run", "-q", "--release"], &[]);
    assert!(f.ok() && s.ok());
    assert_eq!(f.stdout.trim(), s.stdout.trim(), "release debug_assertions");
    assert_eq!(f.stdout.trim(), "false");
}

#[test]
fn feature_selection_matches_cargo() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("feat", "\n[features]\ndefault = [\"a\"]\na = []\nb = []\n"),
        ),
        (
            "src/main.rs",
            "fn main() {\n    let mut v = String::new();\n    if cfg!(feature = \"a\") { v.push('a') }\n    if cfg!(feature = \"b\") { v.push('b') }\n    println!(\"{v}\");\n}\n",
        ),
    ]);
    p.parity_run(&[]);
    for args in [
        vec!["run", "--no-default-features"],
        vec!["run", "--features", "b"],
        vec!["run", "--all-features"],
    ] {
        let f = p.artificer(&args, &[]);
        let mut sa = args.clone();
        sa.insert(1, "-q");
        let s = p.stock(&sa, &[]);
        assert!(f.ok() && s.ok(), "{args:?}");
        assert_eq!(f.stdout.trim(), s.stdout.trim(), "{args:?}");
    }
}

#[test]
fn a_member_feature_edit_invalidates_resolution() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nresolver = \"2\"\nmembers = [\"app\", \"dep\"]\n",
        ),
        (
            "app/Cargo.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             [dependencies]\ndep = { path = \"../dep\" }\n",
        ),
        (
            "app/src/main.rs",
            "fn main() { println!(\"{}\", dep::value()) }\n",
        ),
        (
            "dep/Cargo.toml",
            "[package]\nname = \"dep\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             [features]\nspecial = []\n",
        ),
        (
            "dep/src/lib.rs",
            "pub fn value() -> &'static str { if cfg!(feature = \"special\") { \"special\" } else { \"base\" } }\n",
        ),
    ]);
    let first = p.artificer(&["run", "-p", "app"], &[]);
    assert!(first.ok());
    assert_eq!(first.stdout.trim(), "base");

    fs::write(
        p.path("app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         [dependencies]\ndep = { path = \"../dep\", features = [\"special\"] }\n",
    )
    .expect("enable the member dependency feature");
    let artificer = p.artificer(&["run", "-p", "app"], &[]);
    let stock = p.stock(&["run", "-q", "-p", "app"], &[]);
    assert!(artificer.ok());
    assert!(stock.ok());
    assert_eq!(artificer.stdout.trim(), "special");
    assert_eq!(artificer.stdout.trim(), stock.stdout.trim());
}

#[test]
fn an_edit_is_never_masked_by_a_timestamp() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("edit", "")),
        ("src/main.rs", "fn main() { println!(\"{}\", 1) }\n"),
    ]);
    p.parity_run(&[]);
    let src = p.path("src/main.rs");
    let before = fs::metadata(&src).unwrap().modified().unwrap();
    fs::write(&src, "fn main() { println!(\"{}\", 9) }\n").unwrap();
    let f = fs::File::options().write(true).open(&src).unwrap();
    f.set_modified(before).unwrap();
    drop(f);
    let run = p.artificer(&["run"], &[]);
    assert!(run.ok());
    assert_eq!(run.stdout.trim(), "9", "artificer served a stale artifact");
}

#[test]
fn profile_overrides_apply() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("prof", "\n[profile.dev]\ndebug-assertions = false\n"),
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", cfg!(debug_assertions)) }\n",
        ),
    ]);
    p.parity_run(&[]);
}

#[test]
fn lints_table_is_enforced() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("lint", "\n[lints.rust]\nunused_variables = \"deny\"\n"),
        ),
        ("src/lib.rs", "pub fn f() -> u8 { let unused = 5; 1 }\n"),
    ]);
    p.parity(&["check"], &[]);
}

#[test]
fn config_rustflags_apply() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("cfgf", "")),
        (
            ".cargo/config.toml",
            "[build]\nrustflags = [\"--cfg\", \"viaconfig\"]\n",
        ),
        (
            "src/main.rs",
            "fn main() { if cfg!(viaconfig) { println!(\"ON\") } else { println!(\"OFF\") } }\n",
        ),
    ]);
    p.parity_run(&[]);
}
