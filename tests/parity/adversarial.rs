use super::*;

#[test]
fn importing_a_complete_cache_does_not_rerun_build_scripts() {
    let project = Project::new(&[
        ("Cargo.toml", &manifest("imported_script", "")),
        ("src/lib.rs", "pub fn value() {}"),
        (
            "build.rs",
            r#"fn main() {
            use std::io::Write;
            println!("cargo:rerun-if-changed=build.rs");
            let mut log = std::fs::OpenOptions::new().create(true).append(true).open("../script-runs").expect("open run log");
            writeln!(log, "run").expect("record script run");
        }"#,
        ),
    ]);
    assert!(project.artificer(&["check"], &[]).ok());
    let export = project
        .dir
        .parent()
        .expect("fixture path has a parent")
        .join("export");
    artificer::export(&project.home, &export, 0, 0).expect("export fixture cache");
    let imported = project
        .dir
        .parent()
        .expect("fixture path has a parent")
        .join("imported");
    artificer::import(&imported, &export).expect("import fixture cache");
    let checked = artificer::check(&project.dir, &imported).expect("compile fixture");
    assert_eq!(checked.script, artificer::ScriptOutcome::Restored);
    assert_eq!(
        fs::read_to_string(project.path("../script-runs")).expect("read fixture text"),
        "run\n"
    );
}

#[test]
fn a_new_nested_test_target_invalidates_cached_metadata() {
    let project = Project::new(&[
        ("Cargo.toml", &manifest("nested_target", "")),
        ("src/lib.rs", "pub fn value() {}"),
    ]);
    fs::create_dir_all(project.path("tests/late")).expect("create fixture directory");
    assert!(project.artificer(&["check", "--tests"], &[]).ok());
    fs::write(
        project.path("tests/late/main.rs"),
        "compile_error!(\"new target must compile\");",
    )
    .expect("write fixture file");
    project.parity(&["check", "--tests"], &[]);
}

#[test]
fn a_new_implicit_binary_invalidates_cached_metadata() {
    let project = Project::new(&[
        ("Cargo.toml", &manifest("implicit_binary", "")),
        ("src/lib.rs", "pub fn value() {}"),
    ]);
    assert!(project.artificer(&["check"], &[]).ok());
    fs::write(
        project.path("src/main.rs"),
        "compile_error!(\"new target must compile\");",
    )
    .expect("write fixture file");
    project.parity(&["check"], &[]);
}

#[test]
fn a_custom_target_directory_does_not_become_a_source_input() {
    let project = Project::new(&[
        ("Cargo.toml", &manifest("custom_output", "")),
        (".cargo/config.toml", "[build]\ntarget-dir = 'output'\n"),
        ("src/main.rs", "fn main() { println!(\"same program\"); }"),
    ]);
    assert!(project.artificer(&["build"], &[]).ok());
    let first = artificer::store_stat(&project.home)
        .expect("read fixture cache counters")
        .misses;
    assert!(project.artificer(&["build"], &[]).ok());
    assert_eq!(
        artificer::store_stat(&project.home)
            .expect("read fixture cache counters")
            .misses,
        first,
        "build output invalidated its own source key"
    );
}

#[cfg(not(windows))]
#[test]
fn a_daemon_refreshes_features_of_an_external_path_dependency() {
    let project = Project::new(&[
        (
            "Cargo.toml",
            &manifest(
                "external_graph",
                "[dependencies]\ndep = { path = '../dep' }\n",
            ),
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", dep::value()); }",
        ),
        (
            "../dep/Cargo.toml",
            &manifest("dep", "[features]\nchanged = []\ndefault = []\n"),
        ),
        (
            "../dep/src/lib.rs",
            "pub fn value() -> bool { cfg!(feature = \"changed\") }",
        ),
    ]);
    assert!(project.artificer(&["mods", "on", "serve"], &[]).ok());
    let first = project.artificer_served(&["build"], &[]);
    fs::write(
        project.path("../dep/Cargo.toml"),
        manifest("dep", "[features]\nchanged = []\ndefault = ['changed']\n"),
    )
    .expect("write fixture file");
    let changed = project.artificer_served(&["build"], &[]);
    let stopped = project.artificer(&["serve", "stop"], &[]);
    assert!(first.ok() && changed.ok() && stopped.ok());
    let output = Command::new(project.path(&format!(
        "target/debug/external_graph{}",
        std::env::consts::EXE_SUFFIX
    )))
    .output()
    .expect("run fixture command");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "true",
        "external dependency features remained cached"
    );
}

#[cfg(not(windows))]
#[test]
fn a_daemon_refreshes_target_paths_outside_the_workspace() {
    let project = Project::new(&[
        (
            "Cargo.toml",
            &manifest(
                "external_graph",
                "[dependencies]\ndep = { path = '../dep' }\n",
            ),
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", dep::value()); }",
        ),
        ("../dep/Cargo.toml", &manifest("dep", "")),
        ("../dep/src/lib.rs", "pub fn value() -> u8 { 1 }"),
        ("../dep/src/alternate.rs", "pub fn value() -> u8 { 2 }"),
    ]);
    assert!(project.artificer(&["mods", "on", "serve"], &[]).ok());
    let first = project.artificer_served(&["build"], &[]);
    fs::write(
        project.path("../dep/Cargo.toml"),
        manifest("dep", "[lib]\npath = 'src/alternate.rs'\n"),
    )
    .expect("write fixture file");
    let changed = project.artificer_served(&["build"], &[]);
    let stopped = project.artificer(&["serve", "stop"], &[]);
    assert!(
        first.ok() && changed.ok() && stopped.ok(),
        "daemon did not resolve the changed external target"
    );
    let output = Command::new(project.path(&format!(
        "target/debug/external_graph{}",
        std::env::consts::EXE_SUFFIX
    )))
    .output()
    .expect("run fixture command");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "2");
}

#[cfg(not(windows))]
#[test]
fn a_daemon_applies_configuration_edits_without_a_restart() {
    let project = Project::new(&[
        ("Cargo.toml", &manifest("config_refresh", "")),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", cfg!(proof_changed)); }",
        ),
    ]);
    assert!(project.artificer(&["mods", "on", "serve"], &[]).ok());
    let first = project.artificer_served(&["build"], &[]);
    let warm = project.artificer_served(&["build"], &[]);
    fs::write(
        project.path(".cargo/config.toml"),
        "[build]\ntarget-dir = 'target'\nrustflags = ['--cfg', 'proof_changed']\n",
    )
    .expect("write fixture file");
    let changed = project.artificer_served(&["build"], &[]);
    let stopped = project.artificer(&["serve", "stop"], &[]);
    assert!(first.ok() && warm.ok() && changed.ok() && stopped.ok());
    let output = Command::new(project.path(&format!(
        "target/debug/config_refresh{}",
        std::env::consts::EXE_SUFFIX
    )))
    .output()
    .expect("run fixture command");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "true",
        "daemon reused the previous compiler flags"
    );
}

#[test]
fn package_version_components_exclude_build_metadata() {
    let project = Project::new(&[
        (
            "Cargo.toml",
            &manifest("version_components", "").replace("0.1.0", "1.2.3-rc.4+build.7"),
        ),
        (
            "build.rs",
            r#"fn main() {
            println!("cargo:rustc-env=SCRIPT_VERSION={}|{}|{}", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_VERSION_PATCH"), env!("CARGO_PKG_VERSION_PRE"));
        }"#,
        ),
        (
            "src/main.rs",
            r#"fn main() {
            println!("{}|{}|{}", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_VERSION_PATCH"), env!("CARGO_PKG_VERSION_PRE"));
            println!("{}", env!("SCRIPT_VERSION"));
        }"#,
        ),
    ]);
    project.parity_run(&[]);
}
