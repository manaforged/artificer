use super::*;
use anyhow::Result;

#[test]
fn cargo_commands_still_work_when_the_store_is_unavailable() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    let unavailable = temp.path().join("not-a-directory");
    fs::write(&unavailable, "occupied")?;
    let output = shim(&temp.path().join("shim"), &root)
        .arg("--version")
        .env("ARTIFICER_HOME", &unavailable)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8(output.stdout)?.starts_with("cargo "));
    Ok(())
}

#[test]
fn activation_quotes_the_install_path_and_is_idempotent() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("a 'quoted' $HOME directory");
    let output = artificer(&temp.path().join("store"), temp.path())
        .arg("env")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let script = String::from_utf8(output.stdout)?;
    #[cfg(unix)]
    let (original, separator, mut shell, print) = (
        "/usr/bin:/bin",
        ":",
        Command::new("sh"),
        "printf '%s' \"$PATH\"",
    );
    #[cfg(windows)]
    let (original, separator, mut shell, print) = (
        r"C:\Windows\System32",
        ";",
        Command::new("powershell"),
        "[Console]::Write($env:Path)",
    );
    #[cfg(unix)]
    shell.arg("-c");
    #[cfg(windows)]
    shell.args(["-NoProfile", "-Command"]);
    let actual = shell
        .arg(format!("{script}\n{script}\n{print}"))
        .env("PATH", original)
        .output()?;
    assert!(actual.status.success(), "{actual:?}");
    assert_eq!(
        String::from_utf8(actual.stdout)?,
        format!(
            "{}{separator}{original}",
            home.join(".artificer").join("bin").display()
        )
    );
    Ok(())
}

#[test]
fn temporary_bypass_works_with_a_broken_cache_configuration() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    let home = temp.path().join("shim");
    fs::create_dir_all(home.join("store"))?;
    fs::write(home.join("store/mods.toml"), "not valid TOML")?;
    let output = shim(&home, &root)
        .arg("check")
        .env("ARTIFICER_DISABLED", "1")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()?;
    assert!(output.status.success(), "{output:?}");
    assert!(!home.join("store/units").exists());
    Ok(())
}

#[test]
fn install_disable_enable_and_uninstall_preserve_cargo_and_the_cache() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    let home = temp.path().join("user");
    let cargo_home = home.join(".cargo");
    let store = temp.path().join("store");
    let suffix = std::env::consts::EXE_SUFFIX;
    let proxy = artificer::cargo_home()
        .join("bin")
        .join(format!("cargo{suffix}"));
    let rustup = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            artificer::cargo_home()
                .parent()
                .expect("Cargo home has a parent")
                .join(".rustup")
        });
    let cargo = cargo_home.join("bin").join(format!("cargo{suffix}"));
    fs::create_dir_all(cargo_home.join("bin"))?;
    if fs::hard_link(proxy.canonicalize()?, &cargo).is_err() {
        fs::copy(&proxy, &cargo)?;
    }
    let rustc_proxy = proxy.with_file_name(format!("rustc{suffix}"));
    let rustc = cargo.with_file_name(format!("rustc{suffix}"));
    if fs::hard_link(rustc_proxy.canonicalize()?, &rustc).is_err() {
        fs::copy(&rustc_proxy, &rustc)?;
    }
    let command = |program: &Path| {
        let mut command = Command::new(program);
        command
            .current_dir(&root)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("CARGO_HOME", &cargo_home)
            .env("RUSTUP_HOME", &rustup)
            .env("ARTIFICER_HOME", &store)
            .env_remove("ARTIFICER_REAL_CARGO")
            .env("ARTIFICER_NOSERVE", "1")
            .env("CARGO_TARGET_DIR", root.join("target"))
            .env("SHELL", "/bin/zsh")
            .env_remove("ZDOTDIR")
            .env_remove("ARTIFICER_DISABLED");
        command
    };
    fs::create_dir_all(&home)?;
    fs::write(home.join(".profile"), "export KEEP=1\n")?;
    let installed = command(Path::new(env!("CARGO_BIN_EXE_artificer")))
        .arg("install")
        .args(cfg!(windows).then_some("--no-modify-path"))
        .output()?;
    assert!(installed.status.success(), "{installed:?}");
    let binary = cargo_home.join("bin").join(format!("artificer{suffix}"));
    let shim = home.join(".artificer/bin").join(format!("cargo{suffix}"));
    #[cfg(unix)]
    {
        let line = r#"[ ! -f "$HOME/.artificer/env" ] || . "$HOME/.artificer/env""#;
        assert_eq!(
            fs::read_to_string(home.join(".profile"))?,
            format!("export KEEP=1\n{line}\n")
        );
        assert_eq!(
            fs::read_to_string(home.join(".zshenv"))?,
            format!("{line}\n")
        );
        for (shell, flag) in [("bash", "-lc"), ("zsh", "-c")] {
            let Ok(found) = Command::new(shell)
                .args([flag, "command -v cargo"])
                .env_clear()
                .env("HOME", &home)
                .env("PATH", "/usr/bin:/bin")
                .output()
            else {
                continue;
            };
            assert_eq!(
                String::from_utf8(found.stdout)?.trim(),
                shim.display().to_string(),
                "{shell} {flag}"
            );
        }
        let login = Command::new("bash")
            .args(["-lc", "cargo check"])
            .current_dir(&root)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .env("RUSTUP_HOME", &rustup)
            .env("CARGO_HOME", &cargo_home)
            .env("ARTIFICER_HOME", &store)
            .env("ARTIFICER_NOSERVE", "1")
            .env("CARGO_TARGET_DIR", root.join("target"))
            .output()?;
        assert!(login.status.success(), "{login:?}");
        assert!(artificer::store_stat(&store)?.misses > 0);
    }
    let first = command(&shim).arg("check").output()?;
    assert!(first.status.success(), "{first:?}");
    let before = artificer::store_stat(&store)?.misses;
    assert!(before > 0);
    let disabled = command(&binary).arg("disable").output()?;
    assert!(disabled.status.success(), "{disabled:?}");
    fs::write(root.join("src/lib.rs"), "pub const VALUE: u8 = 2;")?;
    let bypassed = command(&shim).arg("check").output()?;
    assert!(bypassed.status.success(), "{bypassed:?}");
    assert_eq!(artificer::store_stat(&store)?.misses, before);
    let status = command(&binary).args(["stat", "--json"]).output()?;
    let status: serde_json::Value = serde_json::from_slice(&status.stdout)?;
    assert_eq!(status["enabled"], false);
    let enabled = command(&binary).arg("enable").output()?;
    assert!(enabled.status.success(), "{enabled:?}");
    let resumed = command(&shim).arg("check").output()?;
    assert!(resumed.status.success(), "{resumed:?}");
    assert!(artificer::store_stat(&store)?.misses > before);
    #[cfg(unix)]
    let removed = command(&binary).arg("uninstall").output()?;
    #[cfg(windows)]
    let removed = command(Path::new("powershell"))
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/install.ps1"))
        .arg("-Uninstall")
        .output()?;
    assert!(removed.status.success(), "{removed:?}");
    assert!(!binary.exists());
    assert!(!shim.exists());
    assert!(store.join("units").is_dir());
    #[cfg(unix)]
    {
        assert_eq!(
            fs::read_to_string(home.join(".profile"))?,
            "export KEEP=1\n"
        );
        assert!(!home.join(".zshenv").exists());
        assert!(!home.join(".artificer").exists());
    }
    let restored = command(&cargo).arg("--version").output()?;
    assert!(restored.status.success(), "{restored:?}");
    assert!(String::from_utf8(restored.stdout)?.starts_with("cargo "));
    Ok(())
}

#[test]
fn an_install_without_real_cargo_leaves_nothing_behind() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("user");
    let store = temp.path().join("store");
    fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_artificer"))
        .arg("install")
        .current_dir(&home)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("CARGO_HOME", home.join(".cargo"))
        .env("ARTIFICER_HOME", &store)
        .env("ARTIFICER_REAL_CARGO", temp.path().join("missing-cargo"))
        .env("ARTIFICER_NOSERVE", "1")
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    assert!(!store.exists());
    assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
}
