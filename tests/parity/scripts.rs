use super::*;

#[test]
fn workspace_lints_are_inherited() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"m\"]\nresolver = \"2\"\n\n[workspace.lints.rust]\nunused_variables = \"deny\"\n",
        ),
        (
            "m/Cargo.toml",
            "[package]\nname = \"m\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lints]\nworkspace = true\n",
        ),
        ("m/src/lib.rs", "pub fn f() -> u8 { let unused = 5; 1 }\n"),
    ]);
    p.parity(&["check", "--workspace"], &[]);
}

#[test]
fn tuned_release_profile_is_applied() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest(
                "tuned",
                "\n[profile.release]\ndebug-assertions = true\ncodegen-units = 1\n",
            ),
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", cfg!(debug_assertions)) }\n",
        ),
    ]);
    let f = p.artificer(&["run", "--release"], &[]);
    let s = p.stock(&["run", "-q", "--release"], &[]);
    assert!(f.ok() && s.ok());
    assert_eq!(f.stdout.trim(), s.stdout.trim());
    assert_eq!(f.stdout.trim(), "true", "manifest profile must win");
}

#[test]
fn config_target_dir_is_honoured() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("td", "")),
        (
            ".cargo/config.toml",
            "[build]\ntarget-dir = \"custom-out\"\n",
        ),
        ("src/main.rs", "fn main() { println!(\"hi\") }\n"),
    ]);
    assert!(p.artificer(&["build"], &[]).ok());
    let exe = if cfg!(windows) { "td.exe" } else { "td" };
    assert!(
        p.path("custom-out/debug").join(exe).is_file(),
        "build.target-dir must decide where the binary lands",
    );
}

#[test]
fn a_gated_target_is_skipped() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest(
                "gate",
                "\n[features]\nextra = []\n\n[[bin]]\nname = \"needs-extra\"\npath = \"src/bin/needs.rs\"\nrequired-features = [\"extra\"]\n",
            ),
        ),
        ("src/lib.rs", "pub fn f() {}\n"),
        (
            "src/bin/needs.rs",
            "fn main() { nonexistent_crate::boom() }\n",
        ),
    ]);
    p.parity(&["check"], &[]);
    p.parity(&["check", "--all-targets"], &[]);
}

#[test]
fn a_profile_asking_for_lto_builds() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("ltop", "\n[profile.release]\nlto = \"thin\"\n"),
        ),
        ("src/main.rs", "fn main() { println!(\"ok\") }\n"),
    ]);
    p.parity(&["build", "--release"], &[]);
}

#[test]
fn panic_abort_still_allows_tests() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("pa", "\n[profile.release]\npanic = \"abort\"\n"),
        ),
        (
            "src/lib.rs",
            "pub fn f() -> u8 { 1 }\n#[cfg(test)] mod t { #[test] fn ok() { assert_eq!(super::f(), 1) } }\n",
        ),
    ]);
    p.parity(&["test", "--release"], &[]);
}

#[test]
fn check_all_targets_defines_bin_exe() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("bx2", "")),
        ("src/main.rs", "fn main() { println!(\"ran\") }\n"),
        (
            "tests/it.rs",
            "#[test]\nfn has_path() { assert!(!env!(\"CARGO_BIN_EXE_bx2\").is_empty()) }\n",
        ),
    ]);
    p.parity(&["check", "--all-targets"], &[]);
}

#[test]
fn a_build_does_not_poison_a_later_lint_run() {
    let clippy = rustup_tool("clippy-driver");
    let p = Project::new(&[
        (
            "Cargo.toml",
            &manifest("poison", "\n[lints.clippy]\nneedless_return = \"deny\"\n"),
        ),
        ("src/lib.rs", "pub fn n(x: u8) -> u8 { return x + 1; }\n"),
    ]);
    assert!(p.artificer_served(&["build"], &[]).ok());
    let wrapper = clippy.display().to_string();
    let lint = p.artificer_served(&["check"], &[("RUSTC_WORKSPACE_WRAPPER", &wrapper)]);
    assert!(
        !lint.ok(),
        "clippy must still reject the code after a plain build warmed the store",
    );
}

#[test]
fn the_discovered_target_set_is_part_of_the_key() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("ts", "")),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
        (
            "tests/a.rs",
            "#[test]\nfn a_works() { assert_eq!(ts::f(), 1) }\n",
        ),
    ]);
    p.parity(&["test"], &[]);

    std::fs::write(
        p.path("tests/c.rs"),
        "#[test]\nfn c_must_fail() { panic!(\"must fail\") }\n",
    )
    .unwrap();
    let f = p.artificer(&["test"], &[]);
    assert!(!f.ok(), "a newly added failing test must fail the run");
    p.parity(&["test"], &[]);

    std::fs::remove_file(p.path("tests/c.rs")).unwrap();
    let f = p.artificer(&["test"], &[]);
    assert!(f.ok(), "removing a test must not wedge the project");
    p.parity(&["test"], &[]);
}

#[test]
fn a_build_script_error_fails_the_build() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("se", "")),
        ("src/main.rs", "fn main() { println!(\"hi\") }\n"),
        (
            "build.rs",
            "fn main() { println!(\"cargo::error=unsupported configuration\"); }\n",
        ),
    ]);
    p.parity(&["check"], &[]);
}

#[test]
fn every_named_package_is_checked() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n",
        ),
        (
            "a/Cargo.toml",
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "a/src/lib.rs",
            "pub fn f() -> u8 { let _x: i32 = \"broken\"; 1 }\n",
        ),
        (
            "b/Cargo.toml",
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        ("b/src/lib.rs", "pub fn g() -> u8 { 2 }\n"),
    ]);
    p.parity(&["check", "-p", "a", "-p", "b"], &[]);
    p.parity(&["check", "-p", "b", "-p", "a"], &[]);
}

#[test]
fn compile_time_env_is_part_of_the_key() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("ek", "")),
        (
            "src/main.rs",
            "fn main() { println!(\"MODE={}\", env!(\"APP_MODE\")) }\n",
        ),
    ]);
    p.parity_run(&[("APP_MODE", "dev")]);
    p.parity_run(&[("APP_MODE", "prod")]);
    p.parity_run(&[("APP_MODE", "dev")]);
}

#[test]
fn a_second_checkout_gets_its_own_manifest_dir() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("md", "")),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", env!(\"CARGO_MANIFEST_DIR\")) }\n",
        ),
    ]);
    let canon = |path: &Path| {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string()
    };
    let first = p.artificer(&["run"], &[]);
    assert!(first.ok());
    assert_eq!(canon(Path::new(first.stdout.trim())), canon(&p.dir));

    let twin = p.dir.parent().unwrap().join("twin");
    copy_tree(&p.dir, &twin);
    let out = Command::new(env!("CARGO_BIN_EXE_artificer"))
        .env("ARTIFICER_HOME", &p.home)
        .env("ARTIFICER_NOSERVE", "1")
        .current_dir(&twin)
        .arg("run")
        .output()
        .unwrap();
    let seen = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(
        canon(Path::new(&seen)),
        canon(&twin),
        "the second checkout must report its own manifest dir",
    );
}

#[test]
fn an_env_free_crate_still_shares_across_checkouts() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("sh", "")),
        ("src/main.rs", "fn main() { println!(\"plain\") }\n"),
    ]);
    assert!(p.artificer(&["check"], &[]).ok());
    let twin = p.dir.parent().unwrap().join("twin2");
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
        "second checkout must be all hits, got: {log}",
    );
}

#[test]
fn a_declared_script_input_is_watched() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("gen", "")),
        ("schema.txt", "7\n"),
        (
            "build.rs",
            "fn main() {\n    println!(\"cargo::rerun-if-changed=schema.txt\");\n    let v = std::fs::read_to_string(\"schema.txt\").unwrap();\n    let out = std::env::var(\"OUT_DIR\").unwrap();\n    std::fs::write(format!(\"{out}/gen.rs\"), format!(\"pub fn v() -> u8 {{ {} }}\", v.trim())).unwrap();\n}\n",
        ),
        (
            "src/main.rs",
            "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\nfn main() { println!(\"v={}\", v()) }\n",
        ),
    ]);
    p.parity_run(&[]);
    std::fs::write(p.path("schema.txt"), "9\n").unwrap();
    let f = p.artificer(&["run"], &[]);
    assert_eq!(f.stdout.trim(), "v=9", "the regenerated code must be used");
    p.parity_run(&[]);
}

#[test]
fn build_script_double_colon_directives() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("dc", "")),
        (
            "build.rs",
            "fn main() {\n    println!(\"cargo::rustc-check-cfg=cfg(modern)\");\n    println!(\"cargo::rustc-cfg=modern\");\n    println!(\"cargo::rustc-env=GEN=42\");\n}\n",
        ),
        (
            "src/main.rs",
            "fn main() {\n    if cfg!(modern) { println!(\"MODERN {}\", env!(\"GEN\")) } else { println!(\"MISSING\") }\n}\n",
        ),
    ]);
    p.parity_run(&[]);
}

#[test]
fn build_script_single_colon_directives() {
    let p = Project::new(&[
        ("Cargo.toml", &manifest("sc", "")),
        (
            "build.rs",
            "fn main() {\n    println!(\"cargo:rustc-check-cfg=cfg(legacy)\");\n    println!(\"cargo:rustc-cfg=legacy\");\n}\n",
        ),
        (
            "src/main.rs",
            "fn main() {\n    if cfg!(legacy) { println!(\"LEGACY\") } else { println!(\"MISSING\") }\n}\n",
        ),
    ]);
    p.parity_run(&[]);
}
