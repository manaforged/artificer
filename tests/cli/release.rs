use super::*;

#[test]
fn a_program_exiting_two_is_run_once() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    fs::write(
        root.join("src/main.rs"),
        r#"fn main() {
        use std::io::Write;
        let mut log = std::fs::OpenOptions::new().create(true).append(true).open("runs").expect("open run log");
        writeln!(log, "run").expect("record script run");
        std::process::exit(2);
    }"#,
    )
    .expect("write fixture file");
    let output = shim(&temp.path().join("shim"), &root)
        .arg("run")
        .output()
        .expect("run fixture command");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(root.join("runs")).expect("read fixture text"),
        "run\n"
    );
}

#[test]
fn cargo_install_keeps_cargos_package_installation_behavior() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"installed package\"); }",
    )
    .expect("write fixture file");
    let destination = temp.path().join("installed");
    let proxy = artificer::cargo_home()
        .join("bin")
        .join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    let rustup = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            artificer::cargo_home()
                .parent()
                .expect("fixture path has a parent")
                .join(".rustup")
        });
    let output = shim(&temp.path().join("shim"), &root)
        .args(["install", "--path", ".", "--root"])
        .arg(&destination)
        .env("ARTIFICER_REAL_CARGO", &proxy)
        .env("RUSTUP_HOME", &rustup)
        .env("HOME", temp.path())
        .env("USERPROFILE", temp.path())
        .env("CARGO_HOME", temp.path().join("cargo-home"))
        .output()
        .expect("run fixture command");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = destination
        .join("bin")
        .join(format!("clean{}", std::env::consts::EXE_SUFFIX));
    let run = Command::new(&binary)
        .output()
        .expect("Cargo must install the package binary");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "installed package"
    );
}

#[test]
fn invalid_maintenance_arguments_are_rejected_before_installation() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let output = artificer(&temp.path().join("store"), temp.path())
        .args(["install", "unexpected"])
        .env("HOME", temp.path())
        .env("USERPROFILE", temp.path())
        .env("CARGO_HOME", temp.path().join("cargo-home"))
        .output()
        .expect("run fixture command");
    assert_eq!(output.status.code(), Some(2));
    assert!(!temp.path().join("cargo-home/bin/artificer").exists());
}

#[test]
fn cargo_uninstall_removes_the_named_package() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    fs::write(root.join("src/main.rs"), "fn main() {}").expect("write fixture file");
    let destination = temp.path().join("installed");
    let installed = stock(&root)
        .args(["install", "--path", ".", "--root"])
        .arg(&destination)
        .env("CARGO_TARGET_DIR", root.join("stock-target"))
        .output()
        .expect("run fixture command");
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    let proxy = artificer::cargo_home()
        .join("bin")
        .join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    let rustup = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            artificer::cargo_home()
                .parent()
                .expect("fixture path has a parent")
                .join(".rustup")
        });
    let output = shim(&temp.path().join("shim"), &root)
        .args(["uninstall", "clean", "--root"])
        .arg(&destination)
        .env("ARTIFICER_REAL_CARGO", proxy)
        .env("RUSTUP_HOME", rustup)
        .env("HOME", temp.path())
        .env("USERPROFILE", temp.path())
        .env("CARGO_HOME", temp.path().join("cargo-home"))
        .output()
        .expect("run fixture command");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !destination
            .join("bin")
            .join(format!("clean{}", std::env::consts::EXE_SUFFIX))
            .exists(),
        "Cargo must uninstall the named package"
    );
}
