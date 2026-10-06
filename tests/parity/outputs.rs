use super::*;

#[test]
fn a_build_override_falls_through_to_cargo() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("bo", "\n[profile.dev.build-override]\nopt-level = 0\n"),
        ),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
    ]);
    let out = p.artificer(&["check"], &[]);
    assert_eq!(
        out.code,
        Some(2),
        "a build-override workspace must fall through, got: {:?} {}",
        out.code,
        out.stdout,
    );
}

#[test]
fn an_unmodelable_override_key_falls_through() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("uk", "\n[profile.dev.package.\"*\"]\nfrobnicate = true\n"),
        ),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
    ]);
    let out = p.artificer(&["check"], &[]);
    assert_ne!(out.code, Some(0), "must not compile through silently");
}

#[test]
fn debug_false_is_rustc_debuginfo_zero() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\", \"dep\"]\nresolver = \"2\"\n\n[profile.dev.package.\"*\"]\ndebug = false\n",
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
        ("dep/src/lib.rs", "pub fn f() -> u8 { 7 }\n"),
    ]);
    let f = p.artificer(&["check"], &[]);
    assert!(f.ok(), "check with debug=false failed: {}", f.stdout);
}

#[test]
fn cargo_bin_exe_is_defined_for_tests() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("bx", "")),
        ("src/main.rs", "fn main() { println!(\"ran\") }\n"),
        (
            "tests/it.rs",
            "#[test]\nfn runs() {\n    let out = std::process::Command::new(env!(\"CARGO_BIN_EXE_bx\")).output().unwrap();\n    assert!(out.status.success());\n}\n",
        ),
    ]);
    p.parity(&["test"], &[]);
}

#[test]
fn cargo_target_tmpdir_is_defined_for_integration_tests() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("tmpdir", "")),
        ("src/lib.rs", "pub fn value() -> u8 { 1 }\n"),
        (
            "tests/it.rs",
            "const TMP: &str = env!(\"CARGO_TARGET_TMPDIR\");\n#[test]\nfn exists() { assert!(std::path::Path::new(TMP).is_dir()) }\n",
        ),
    ]);
    p.parity(&["check", "--all-targets"], &[]);
    p.parity(&["test"], &[]);
}

#[test]
fn links_metadata_reaches_dependents() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"sys\", \"user\"]\nresolver = \"2\"\n",
        ),
        (
            "sys/Cargo.toml",
            "[package]\nname = \"sys\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlinks = \"foo\"\n",
        ),
        ("sys/src/lib.rs", "pub fn f() {}\n"),
        (
            "sys/build.rs",
            "fn main() { println!(\"cargo:version_number=42\"); }\n",
        ),
        (
            "user/Cargo.toml",
            "[package]\nname = \"user\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsys = { path = \"../sys\" }\n",
        ),
        (
            "user/build.rs",
            "fn main() { std::env::var(\"DEP_FOO_VERSION_NUMBER\").expect(\"DEP_FOO_VERSION_NUMBER\"); }\n",
        ),
        ("user/src/main.rs", "fn main() { println!(\"ok\") }\n"),
    ]);
    p.parity(&["check", "--workspace"], &[]);
}

#[test]
fn build_all_targets_and_test_tests_stay_in_artificer() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("every_target", "")),
        (
            "src/lib.rs",
            "pub fn seven() -> u8 { 7 }\n#[test]\nfn lib_unit() { assert_eq!(seven(), 7); }\n",
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", every_target::seven()); }\n#[test]\nfn bin_unit() {}\n",
        ),
        ("examples/demo.rs", "fn main() { println!(\"demo\"); }\n"),
        (
            "tests/outer.rs",
            "#[test]\nfn outer() { assert_eq!(every_target::seven(), 7); }\n",
        ),
        (
            "benches/speed.rs",
            "#[test]\nfn speed() { assert_eq!(every_target::seven(), 7); }\n",
        ),
    ]);
    let built = p.artificer(&["build", "--all-targets"], &[]);
    assert!(built.ok(), "{}", built.stderr);
    let demo = format!("demo{}", std::env::consts::EXE_SUFFIX);
    assert!(p.path("target/debug/examples").join(demo).is_file());
    p.parity(&["build", "--all-targets"], &[]);
    let tested = p.artificer(&["test", "--tests"], &[]);
    assert!(tested.ok(), "{}\n{}", tested.stdout, tested.stderr);
    for name in ["lib_unit", "bin_unit", "outer"] {
        assert!(
            tested.stdout.contains(name),
            "{name} did not run: {}",
            tested.stdout
        );
    }
    assert!(!tested.stdout.contains("speed"), "{}", tested.stdout);
    p.parity(&["test", "--tests"], &[]);
    assert_eq!(
        artificer::store_stat(&p.home)
            .expect("read fixture cache counters")
            .fallbacks,
        0,
        "a target-set flag fell back to cargo"
    );
}
