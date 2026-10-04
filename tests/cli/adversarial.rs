use super::*;

#[cfg(unix)]
#[test]
fn equal_package_names_from_different_sources_keep_separate_features() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    for name in ["first", "second"] {
        let repo = temp.path().join(name);
        fs::create_dir_all(repo.join("src")).expect("create fixture directory");
        fs::write(repo.join("Cargo.toml"), "[package]\nname = 'dep'\nversion = '0.1.0'\nedition = '2021'\n[features]\nenabled = []\n").expect("write fixture file");
        fs::write(
            repo.join("src/lib.rs"),
            "pub fn value() -> bool { cfg!(feature = \"enabled\") }",
        )
        .expect("write fixture file");
        for args in [
            vec!["init", "--quiet"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        ] {
            let result = Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .expect("run fixture command");
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    let manifest = root.join("Cargo.toml");
    fs::write(&manifest, format!("{}\n[dependencies]\na = {{ package = 'dep', git = 'file://{}/first', features = ['enabled'] }}\nb = {{ package = 'dep', git = 'file://{}/second' }}\n", fs::read_to_string(&manifest).expect("read fixture text"), temp.path().display(), temp.path().display())).expect("write fixture file");
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"{} {}\", a::value(), b::value()); }",
    )
    .expect("write fixture file");
    let result = shim(&temp.path().join("shim"), &root)
        .arg("run")
        .output()
        .expect("run fixture command");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "true false");
    let stat = artificer(&temp.path().join("shim/store"), &root)
        .args(["stat", "--json"])
        .output()
        .expect("read store statistics");
    let value: serde_json::Value = serde_json::from_slice(&stat.stdout).expect("stat JSON");
    assert_eq!(value["fallbacks"], 0, "{value}");
    assert!(value["misses"].as_u64().unwrap_or(0) >= 2, "{value}");
}

#[test]
fn cargo_merges_flags_from_parent_and_project_configuration() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"{} {}\", cfg!(parent_flag), cfg!(project_flag)); }",
    )
    .expect("write fixture file");
    fs::create_dir_all(temp.path().join(".cargo")).expect("create fixture directory");
    fs::create_dir_all(root.join(".cargo")).expect("create fixture directory");
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\nrustflags = ['--cfg', 'parent_flag']\n",
    )
    .expect("write fixture file");
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\nrustflags = ['--cfg', 'project_flag']\n",
    )
    .expect("write fixture file");
    let result = shim(&temp.path().join("shim"), &root)
        .arg("run")
        .output()
        .expect("run fixture command");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim(),
        "true true",
        "parent compiler flags were lost"
    );
}

#[test]
fn a_relative_target_directory_is_resolved_from_its_configuration_file() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"configured output\"); }",
    )
    .expect("write fixture file");
    fs::create_dir_all(temp.path().join(".cargo")).expect("create fixture directory");
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = 'output'\n",
    )
    .expect("write fixture file");
    let result = artificer(&temp.path().join("store"), &root)
        .arg("build")
        .output()
        .expect("run fixture command");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let binary = temp.path().join(format!(
        "output/debug/clean{}",
        std::env::consts::EXE_SUFFIX
    ));
    let output = Command::new(binary)
        .output()
        .expect("binary must be delivered relative to the configuration file");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "configured output"
    );
}

#[test]
fn clean_uses_the_configured_target_and_preserves_other_directories() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    fs::create_dir_all(root.join(".cargo")).expect("create fixture directory");
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\ntarget-dir = 'output'\n",
    )
    .expect("write fixture file");
    for dir in [
        "output/debug/deps",
        "target/debug/deps",
        "off/debug/incremental",
    ] {
        fs::create_dir_all(root.join(dir)).expect("create fixture directory");
        fs::write(root.join(dir).join("keep"), "data").expect("write fixture file");
    }
    let result = artificer(&temp.path().join("store"), &root)
        .arg("clean")
        .output()
        .expect("run fixture command");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        !root.join("output/debug/deps").exists(),
        "configured target was not cleaned"
    );
    assert!(root.join("target/debug/deps/keep").is_file());
    assert!(root.join("off/debug/incremental/keep").is_file());
}

#[cfg(unix)]
#[test]
fn clean_does_not_follow_a_profile_symlink_outside_the_target() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    let outside = temp.path().join("unrelated");
    fs::create_dir_all(outside.join("deps")).expect("create fixture directory");
    fs::write(outside.join("deps/keep"), "data").expect("write fixture file");
    fs::create_dir_all(root.join("target")).expect("create fixture directory");
    std::os::unix::fs::symlink(&outside, root.join("target/debug"))
        .expect("create fixture symlink");
    let result = artificer(&temp.path().join("store"), &root)
        .arg("clean")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .expect("run fixture command");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        outside.join("deps/keep").is_file(),
        "clean removed data outside the target"
    );
}

#[test]
fn why_miss_does_not_disclose_compile_time_environment_values() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    fs::write(
        root.join("src/lib.rs"),
        "pub const VALUE: &str = env!(\"PROOF_CREDENTIAL\");",
    )
    .expect("write fixture file");
    let home = temp.path().join("store");
    let built = artificer(&home, &root)
        .arg("check")
        .env("PROOF_CREDENTIAL", "fixture-sensitive-value")
        .output()
        .expect("run fixture command");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let report = artificer(&home, &root)
        .args(["why-miss", "clean"])
        .output()
        .expect("run fixture command");
    let report = String::from_utf8_lossy(&report.stdout);
    assert!(report.contains("PROOF_CREDENTIAL"));
    assert!(
        !report.contains("fixture-sensitive-value"),
        "diagnostic exposed an environment value"
    );
}
