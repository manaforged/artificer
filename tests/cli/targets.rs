use super::*;

fn write_app(root: &Path, main: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("src/main.rs"), main).unwrap();
    fs::write(
        root.join("tests/beside.rs"),
        concat!(
            "#[test]\nfn binary_sits_beside_the_test() {\n",
            "    let exe = std::env::current_exe().unwrap();\n",
            "    let deps = exe.parent().unwrap();\n",
            "    assert!(deps.ends_with(\"deps\"), \"{}\", exe.display());\n",
            "    let bin = deps.parent().unwrap().join(format!(\"app{}\", std::env::consts::EXE_SUFFIX));\n",
            "    assert!(bin.is_file(), \"{}\", bin.display());\n",
            "}\n",
        ),
    )
    .unwrap();
}

#[test]
fn tests_run_from_the_target_deps_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("app");
    write_app(&root, "fn main() {}\n");
    let out = shim(&tmp.path().join("shim"), &root)
        .args(["test", "--test", "beside"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let stat = artificer::store_stat(&tmp.path().join("shim/store")).unwrap();
    assert!(
        stat.misses > 0 && stat.fallbacks == 0,
        "{:?}",
        stat.fallback_last
    );
}

#[test]
fn run_starts_the_program_from_the_target_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("app");
    write_app(
        &root,
        "fn main() { println!(\"{}\", std::env::current_exe().unwrap().display()); }\n",
    );
    let out = shim(&tmp.path().join("shim"), &root)
        .args(["run", "--bin", "app"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let exe = PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
    let metadata = stock(&root)
        .env_remove("CARGO_TARGET_DIR")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(&root)
        .output()
        .unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let expected = PathBuf::from(metadata["target_directory"].as_str().unwrap())
        .join("debug")
        .join(format!("app{}", std::env::consts::EXE_SUFFIX));
    assert_eq!(
        exe.canonicalize().unwrap(),
        expected.canonicalize().unwrap()
    );
    let stat = artificer::store_stat(&tmp.path().join("shim/store")).unwrap();
    assert_eq!(stat.fallbacks, 0, "{:?}", stat.fallback_last);
}

#[cfg(windows)]
#[test]
fn an_exit_code_above_255_is_never_success() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("app");
    write_app(&root, "fn main() { std::process::exit(256); }\n");
    let out = shim(&tmp.path().join("shim"), &root)
        .args(["run", "--bin", "app"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0), "{out:?}");
    let stat = artificer::store_stat(&tmp.path().join("shim/store")).unwrap();
    assert_eq!(stat.fallbacks, 0, "{:?}", stat.fallback_last);
}

#[test]
fn a_linker_for_another_target_keeps_the_build_modeled() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write_pkg(&root);
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::write(
        root.join(".cargo/config.toml"),
        "[target.riscv64gc-unknown-none-elf]\nlinker = \"riscv-none-elf-gcc\"\nrunner = \"qemu-riscv64\"\n",
    )
    .unwrap();
    let home = tmp.path().join("home");
    let status = artificer(&home, &root).arg("check").status().unwrap();
    assert_eq!(status.code(), Some(0));
    let units = fs::read_dir(home.join("units").join(artificer::LAYOUT))
        .map(|d| d.count())
        .unwrap_or(0);
    assert!(units > 0, "the build must go through the store");
}
