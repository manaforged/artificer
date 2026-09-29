use super::{env_file, install, uninstall};
use std::fs;

fn fixture(
    tmp: &std::path::Path,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let binary = tmp.join("built-artificer");
    fs::write(&binary, b"fake-binary").unwrap();
    let real = tmp.join("real-cargo");
    fs::write(&real, b"#!/bin/sh\n").unwrap();
    let cargo_home = tmp.join("cargo-home");
    let control = tmp.join("control");
    (binary, real, cargo_home, control)
}

#[test]
fn install_writes_every_launcher_piece() {
    let tmp = tempfile::tempdir().unwrap();
    let (binary, real, cargo_home, control) = fixture(tmp.path());
    let report = install(&binary, &real, &cargo_home, &control).expect("install");

    assert_eq!(fs::read(&report.binary).unwrap(), b"fake-binary");
    assert_eq!(fs::read(&report.shim).unwrap(), b"fake-binary");
    let stamp = fs::read_to_string(control.join("real-cargo")).unwrap();
    assert_eq!(stamp.trim(), real.display().to_string());
    let env = fs::read_to_string(&report.env).unwrap();
    assert!(env.contains("bin"), "{env}");
    assert!(env.to_ascii_uppercase().contains("PATH"), "{env}");
}

#[test]
fn uninstall_removes_the_launcher_and_keeps_the_store() {
    let tmp = tempfile::tempdir().unwrap();
    let (binary, real, cargo_home, control) = fixture(tmp.path());
    let report = install(&binary, &real, &cargo_home, &control).expect("install");
    let store = tmp.path().join("cache").join("units");
    fs::create_dir_all(&store).unwrap();

    let remaining = uninstall(&cargo_home, &control).expect("uninstall");
    assert!(remaining.is_empty(), "{remaining:?}");
    assert!(!report.binary.exists());
    assert!(!report.shim.exists());
    assert!(!report.env.exists());
    assert!(!control.join("real-cargo").exists());
    assert!(store.is_dir(), "the cache must survive an uninstall");
}

#[test]
fn env_file_name_is_platform_shaped() {
    let tmp = tempfile::tempdir().unwrap();
    let name = env_file(tmp.path());
    let expected = if cfg!(windows) { "env.ps1" } else { "env" };
    assert_eq!(name.file_name().unwrap(), expected);
}

#[test]
fn uninstall_leaves_a_cargo_installed_binary_to_cargo() {
    let tmp = tempfile::tempdir().unwrap();
    let (binary, real, cargo_home, control) = fixture(tmp.path());
    let report = install(&binary, &real, &cargo_home, &control).expect("install");
    let bin = report.binary.file_name().unwrap().to_string_lossy();
    fs::write(
        cargo_home.join(".crates2.json"),
        format!(
            r#"{{"installs":{{"artificer-build 0.1.0 (registry+https://github.com/rust-lang/crates.io-index)":{{"bins":["{bin}"]}}}}}}"#
        ),
    )
    .unwrap();

    assert_eq!(
        super::cargo_package(&cargo_home).as_deref(),
        Some("artificer-build")
    );
    let remaining = uninstall(&cargo_home, &control).expect("uninstall");
    assert!(remaining.is_empty(), "{remaining:?}");
    assert!(report.binary.exists(), "cargo uninstall owns this binary");
    assert!(!report.shim.exists());
}

#[cfg(unix)]
#[test]
fn profiles_cover_login_and_non_interactive_shells() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let bash = super::profiles(home, Some(std::path::Path::new("/bin/bash")), None);
    assert_eq!(bash, vec![home.join(".profile")]);
    fs::write(home.join(".bashrc"), "").unwrap();
    let zsh = super::profiles(home, Some(std::path::Path::new("/bin/zsh")), None);
    assert_eq!(
        zsh,
        vec![
            home.join(".profile"),
            home.join(".bashrc"),
            home.join(".zshenv")
        ]
    );
}

#[cfg(unix)]
#[test]
fn profile_edits_are_idempotent_and_reversible() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join(".profile");
    fs::write(&file, "export A=1").unwrap();
    let files = [file.clone()];
    let line = super::profile_line(tmp.path(), &tmp.path().join(".artificer"));
    assert_eq!(
        line,
        r#"[ ! -f "$HOME/.artificer/env" ] || . "$HOME/.artificer/env""#
    );

    assert_eq!(
        super::add_to_profiles(&files, &line, tmp.path()).unwrap(),
        files
    );
    assert!(
        super::add_to_profiles(&files, &line, tmp.path())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        format!("export A=1\n{line}\n")
    );
    assert_eq!(
        super::remove_from_profiles(&files, &line, tmp.path()).unwrap(),
        files
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "export A=1\n");
}

#[test]
fn user_path_edits_put_the_shim_first_once_and_remove_it() {
    let shim = r"C:\Users\me\.artificer\bin";
    let path = r"C:\Users\me\.cargo\bin;C:\Tools";
    let added = super::path_prepend(path, shim).unwrap();
    assert_eq!(added, format!("{shim};{path}"));
    assert_eq!(super::path_prepend(&added, shim), None);
    assert_eq!(
        super::path_prepend(&format!("{path};{shim}\\"), shim).unwrap(),
        added
    );
    assert_eq!(super::path_remove(&added, shim).unwrap(), path);
    assert_eq!(super::path_remove(path, shim), None);
}

#[cfg(unix)]
#[test]
fn uninstall_finds_the_line_after_zdotdir_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let zdotdir = home.join("zsh");
    let files = super::installed_profiles(home, Some(&zdotdir));
    assert!(files.contains(&home.join(".zshenv")), "{files:?}");
    assert!(files.contains(&zdotdir.join(".zshenv")), "{files:?}");
}

#[test]
fn an_unexpanded_path_entry_names_the_same_directory() {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var(var).unwrap();
    let dir = format!("{home}\\.artificer\\bin");
    let path = format!("C:\\tools;%{var}%\\.artificer\\bin");
    assert_eq!(
        super::path_prepend(&path, &dir).unwrap(),
        format!("{dir};C:\\tools")
    );
    assert_eq!(super::path_remove(&path, &dir).unwrap(), "C:\\tools");
}
