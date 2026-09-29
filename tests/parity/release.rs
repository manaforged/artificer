use super::*;

#[test]
fn changing_an_included_file_outside_the_package_rebuilds() {
    let project = Project::new(&[
        ("Cargo.toml", &manifest("external_input", "")),
        (
            "src/main.rs",
            "fn main() { print!(\"{}\", include_str!(\"../../asset.txt\")); }",
        ),
    ]);
    let asset = project
        .dir
        .parent()
        .expect("fixture path has a parent")
        .join("asset.txt");
    fs::write(&asset, "before").expect("write fixture file");
    project.parity_run(&[]);
    fs::write(&asset, "after").expect("write fixture file");
    project.parity_run(&[]);
}

#[test]
fn a_binary_relinks_when_its_library_includes_a_changed_file() {
    let project = Project::new(&[
        ("Cargo.toml", &manifest("library_input", "")),
        (
            "src/lib.rs",
            "pub fn asset() -> &'static str { include_str!(\"../../lib-asset.txt\") }",
        ),
        (
            "src/main.rs",
            "fn main() { print!(\"{}\", library_input::asset()); }",
        ),
    ]);
    let asset = project
        .dir
        .parent()
        .expect("fixture path has a parent")
        .join("lib-asset.txt");
    fs::write(&asset, "before").expect("write fixture file");
    project.parity_run(&[]);
    fs::write(&asset, "after").expect("write fixture file");
    project.parity_run(&[]);
}

#[test]
fn an_empty_environment_value_does_not_reuse_an_unset_value() {
    let project = Project::new(&[
        ("Cargo.toml", &manifest("optional_env", "")),
        (
            "src/main.rs",
            "fn main() { println!(\"{:?}\", option_env!(\"ARTIFICER_PROOF_OPTION\")); }",
        ),
    ]);
    let mut command = Command::new(env!("CARGO_BIN_EXE_artificer"));
    command
        .arg("run")
        .current_dir(&project.dir)
        .env("ARTIFICER_HOME", &project.home)
        .env("ARTIFICER_NOSERVE", "1")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("ARTIFICER_PROOF_OPTION");
    let unset = command.output().expect("run fixture command");
    assert!(
        unset.status.success(),
        "{}",
        String::from_utf8_lossy(&unset.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&unset.stdout).trim(), "None");
    let empty = command
        .env("ARTIFICER_PROOF_OPTION", "")
        .output()
        .expect("run fixture command");
    assert!(
        empty.status.success(),
        "{}",
        String::from_utf8_lossy(&empty.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&empty.stdout).trim(), "Some(\"\")");
}

#[test]
fn build_scripts_receive_the_resolved_profile() {
    let project = Project::new(&[
        (
            "Cargo.toml",
            &manifest(
                "script_profile",
                "[profile.dev]\nopt-level = 2\ndebug = 0\n",
            ),
        ),
        (
            "build.rs",
            r#"fn main() {
            println!("cargo:rustc-env=OBSERVED={}:{}", std::env::var("OPT_LEVEL").expect("Cargo sets OPT_LEVEL"), std::env::var("DEBUG").expect("Cargo sets DEBUG"));
        }"#,
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", env!(\"OBSERVED\")); }",
        ),
    ]);
    project.parity_run(&[]);
}

#[test]
fn build_scripts_receive_compiler_flags_and_the_encoded_environment() {
    let project = Project::new(&[
        ("Cargo.toml", &manifest("script_flags", "")),
        (
            "build.rs",
            r#"fn main() {
            println!("cargo:rustc-env=OBSERVED={}:{}", cfg!(proof_flag), std::env::var("CARGO_ENCODED_RUSTFLAGS").expect("Cargo sets encoded flags"));
        }"#,
        ),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", env!(\"OBSERVED\")); }",
        ),
    ]);
    project.parity_run(&[("CARGO_ENCODED_RUSTFLAGS", "--cfg\x1fproof_flag")]);
}

#[cfg(not(windows))]
#[test]
fn a_daemon_does_not_reuse_an_arbitrary_client_environment_value() {
    let project = Project::new(&[
        ("Cargo.toml", &manifest("daemon_env", "")),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", env!(\"ARTIFICER_PROOF_VALUE\")); }",
        ),
    ]);
    assert!(project.artificer(&["mods", "on", "serve"], &[]).ok());
    let first = project.artificer_served(&["build"], &[("ARTIFICER_PROOF_VALUE", "first")]);
    let second = project.artificer_served(&["build"], &[("ARTIFICER_PROOF_VALUE", "second")]);
    let stopped = project.artificer(&["serve", "stop"], &[]);
    assert!(first.ok() && second.ok() && stopped.ok());
    let output = Command::new(project.path(&format!(
        "target/debug/daemon_env{}",
        std::env::consts::EXE_SUFFIX
    )))
    .output()
    .expect("run fixture command");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "second");
}

#[cfg(unix)]
#[test]
fn changing_the_compiler_path_cannot_reuse_the_previous_compiler_output() {
    use std::os::unix::fs::PermissionsExt;
    let mut project = Project::new(&[
        ("Cargo.toml", &manifest("compiler_path", "")),
        (
            "src/main.rs",
            "fn main() { println!(\"{}\", cfg!(proof_flag)); }",
        ),
    ]);
    let dir = project.dir.parent().expect("fixture path has a parent");
    let compiler = rustup_tool("rustc");
    let first = dir.join("compiler-first");
    let second = dir.join("compiler-second");
    for (path, flag) in [(&first, ""), (&second, "--cfg proof_flag")] {
        fs::write(
            path,
            format!("#!/bin/sh\nexec '{}' \"$@\" {flag}\n", compiler.display()),
        )
        .expect("write fixture file");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("make fixture wrapper executable");
    }
    project.parity_run(&[("RUSTC", first.to_str().expect("fixture path is UTF-8"))]);
    project.stock_target = dir.join("second-stock-target");
    project.parity_run(&[("RUSTC", second.to_str().expect("fixture path is UTF-8"))]);
}

#[test]
fn modifying_an_export_after_import_does_not_change_the_imported_program() {
    let mut project = Project::new(&[
        ("Cargo.toml", &manifest("imported_program", "")),
        ("src/main.rs", "fn main() { println!(\"CACHE ORIGINAL\"); }"),
    ]);
    assert!(project.artificer(&["build"], &[]).ok());
    let export = project
        .dir
        .parent()
        .expect("fixture path has a parent")
        .join("export");
    assert!(
        project
            .artificer(
                &["export", export.to_str().expect("fixture path is UTF-8")],
                &[]
            )
            .ok()
    );
    project.home = project
        .dir
        .parent()
        .expect("fixture path has a parent")
        .join("imported-store");
    assert!(
        project
            .artificer(
                &["import", export.to_str().expect("fixture path is UTF-8")],
                &[]
            )
            .ok()
    );
    let mut pending = vec![export];
    let mut modified = false;
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).expect("read exported unit directory") {
            let path = entry.expect("read fixture directory entry").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let mut bytes = fs::read(&path).expect("read fixture bytes");
            if let Some(offset) = bytes.windows(14).position(|w| w == b"CACHE ORIGINAL") {
                bytes[offset..offset + 14].copy_from_slice(b"CACHE MUTATED!");
                fs::write(path, bytes).expect("write fixture file");
                modified = true;
            }
        }
    }
    assert!(modified, "fixture must modify an exported program");
    let result = project.artificer(&["run"], &[]);
    assert!(result.ok());
    assert_eq!(result.stdout.trim(), "CACHE ORIGINAL");
}
