use super::*;

#[test]
fn long_compiler_arguments_survive_nested_wrappers() -> anyhow::Result<()> {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(scratch)?;
    let tmp = tempfile::tempdir_in(scratch)?;
    let wrapper = tmp.path().join("compiler wrapper");
    fs::create_dir_all(wrapper.join("src"))?;
    fs::write(
        wrapper.join("Cargo.toml"),
        "[package]\nname = \"forward\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(
        wrapper.join("src/main.rs"),
        r#"fn main() -> std::process::ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(program) = args.next() else { return std::process::ExitCode::FAILURE; };
    match std::process::Command::new(program).args(args).status() {
        Ok(status) if status.success() => std::process::ExitCode::SUCCESS,
        _ => std::process::ExitCode::FAILURE,
    }
}
"#,
    )?;
    let home = tmp.path().join("store");
    let built = artificer(&home, &wrapper).arg("build").output()?;
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let wrapper = wrapper.join("target/debug/forward.exe");
    let root = tmp.path().join("arguments with café");
    fs::create_dir_all(root.join("src"))?;
    let mut manifest = String::from(
        "[package]\nname = \"long_args\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[features]\n",
    );
    for index in 0..1500 {
        manifest.push_str(&format!(
            "long_feature_name_for_windows_argument_limits_{index:04} = []\n"
        ));
    }
    fs::write(root.join("Cargo.toml"), manifest)?;
    fs::write(
        root.join("src/main.rs"),
        r#"fn main() {
    assert!(cfg!(quoted = "hello café with spaces"));
    assert!(cfg!(from_file));
    println!("argument values preserved");
}
"#,
    )?;
    let flags = root.join("extra flags.txt");
    fs::write(&flags, "--cfg\nfrom_file\n")?;
    let flags = [
        "--cfg".to_string(),
        r#"quoted="hello café with spaces""#.to_string(),
        format!("@{}", flags.display()),
    ]
    .join("\x1f");
    for _ in 0..2 {
        let output = artificer(&home, &root)
            .arg("run")
            .env("RUSTC_WRAPPER", &wrapper)
            .env("RUSTC_WORKSPACE_WRAPPER", &wrapper)
            .env("CARGO_ENCODED_RUSTFLAGS", &flags)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout)?.trim(),
            "argument values preserved"
        );
    }
    Ok(())
}

#[test]
fn long_doctest_arguments_run_the_documented_example() -> anyhow::Result<()> {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(scratch)?;
    let tmp = tempfile::tempdir_in(scratch)?;
    let root = tmp.path().join("long docs with café");
    fs::create_dir_all(root.join("src"))?;
    let mut manifest = String::from(
        "[package]\nname = \"long_docs\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[features]\n",
    );
    for index in 0..1500 {
        manifest.push_str(&format!(
            "long_feature_name_for_windows_argument_limits_{index:04} = []\n"
        ));
    }
    fs::write(root.join("Cargo.toml"), manifest)?;
    fs::write(
        root.join("src/lib.rs"),
        r##"#![doc = r#"```
assert_eq!(long_docs::answer(), 7);
```"#]
pub fn answer() -> u8 { 7 }
"##,
    )?;
    let output = artificer(&tmp.path().join("store"), &root)
        .args(["test", "--doc"])
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)?.contains("1 passed; 0 failed"));
    Ok(())
}

#[test]
fn doctests_link_native_libraries_from_transitive_dependencies() -> anyhow::Result<()> {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(scratch)?;
    let tmp = tempfile::tempdir_in(scratch)?;
    let root = tmp.path().join("native docs");
    let dependency = root.join("ffi");
    fs::create_dir_all(root.join("src"))?;
    fs::create_dir_all(dependency.join("src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"native_docs\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nffi = { path = \"ffi\" }\n",
    )?;
    fs::write(
        dependency.join("Cargo.toml"),
        "[package]\nname = \"ffi\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nwindows-sys = { version = \"=0.60.2\", features = [\"Win32_System_Threading\"] }\n",
    )?;
    fs::write(
        dependency.join("src/lib.rs"),
        r#"pub fn process_id() -> u32 {
    // SAFETY: GetCurrentProcessId has no preconditions.
    unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() }
}
"#,
    )?;
    fs::write(
        root.join("src/lib.rs"),
        r##"#![doc = r#"```
assert_ne!(native_docs::process_id(), 0);
```"#]
pub fn process_id() -> u32 { ffi::process_id() }
"##,
    )?;
    let output = artificer(&tmp.path().join("store"), &root)
        .args(["test", "--doc"])
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)?.contains("1 passed; 0 failed"));
    Ok(())
}
