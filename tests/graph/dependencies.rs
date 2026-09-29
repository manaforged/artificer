use super::*;

#[test]
fn cfg_if_dep() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        r#"[package]
name = "widget"
version = "0.1.0"
edition = "2021"

[dependencies]
cfg-if = "1"
"#,
        "pub fn n() -> u8 { 1 }\n",
    );
    let first = artificer::check(&a, &home).unwrap();
    assert_eq!(first.rustc, artificer::RustcOutcome::Ran);
    let second = artificer::check(&a, &home).unwrap();
    assert_eq!(second.rustc, artificer::RustcOutcome::Restored);
}

#[test]
fn lib_tests_run() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"nt\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "pub fn n() -> u8 { 1 }\n#[cfg(test)] mod tests { #[test] fn t() { assert_eq!(super::n(), 1); } }\n",
    );
    let code = artificer::test_package(&a, &[], &home, &artificer::TestOpts::default()).unwrap();
    assert_eq!(code, 0);
    let code = artificer::test_package(
        &a,
        &[],
        &home,
        &artificer::TestOpts {
            no_run: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(code, 0);
}

#[test]
fn integration_test_dev_dep() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        r#"[package]
name = "widget"
version = "0.1.0"
edition = "2021"

[dev-dependencies]
cfg-if = "1"
"#,
        "pub fn n() -> u8 { 1 }\n",
    );
    fs::create_dir_all(a.join("tests")).unwrap();
    fs::write(
        a.join("tests/uses_cfg.rs"),
        r#"
cfg_if::cfg_if! {
    if #[cfg(test)] {
        #[test]
        fn t() { assert_eq!(widget::n(), 1); }
    }
}
"#,
    )
    .unwrap();
    let code = artificer::test_package(&a, &[], &home, &artificer::TestOpts::default()).unwrap();
    assert_eq!(code, 0);
    let code = artificer::test_package(
        &a,
        &[],
        &home,
        &artificer::TestOpts {
            no_run: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(code, 0);
}

#[test]
fn check_dash_p() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let ws = tmp.path().join("ws");
    fs::create_dir_all(ws.join("a/src")).unwrap();
    fs::write(
        ws.join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(
        ws.join("a/Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(ws.join("a/src/lib.rs"), "pub fn n() -> u8 { 1 }\n").unwrap();
    let r = artificer::check_package(&ws, &["alpha".to_string()], &home).unwrap();
    assert_eq!(r.rustc, artificer::RustcOutcome::Ran);
    let r = artificer::check_package(&ws, &["alpha".to_string()], &home).unwrap();
    assert_eq!(r.rustc, artificer::RustcOutcome::Restored);
}

#[test]
fn check_json_emits_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"widget\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "pub fn n() -> u8 { 1 }\n",
    );
    let code = artificer::check_cmd(
        &a,
        &[],
        &home,
        artificer::CheckOpts {
            json: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(code, 0);
}

#[test]
fn serde_derive() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        r#"[package]
name = "s"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
"#,
        r#"
use serde::Serialize;
#[derive(Serialize)]
pub struct P { pub x: u8 }
"#,
    );
    let first = artificer::check(&a, &home).unwrap();
    assert_eq!(first.rustc, artificer::RustcOutcome::Ran);
    assert!(first.rlib.is_file());
    let second = artificer::check(&a, &home).unwrap();
    assert_eq!(second.rustc, artificer::RustcOutcome::Restored);
}

#[test]
fn target_specific_dependencies_follow_the_host() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("root");
    let wrong = tmp.path().join("wrong");
    write(
        &wrong,
        "[package]\nname = \"wrong\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "compile_error!(\"wrong-platform dependency compiled\");\n",
    );
    let target = if cfg!(target_os = "linux") {
        "cfg(target_os = \"windows\")"
    } else {
        "cfg(target_os = \"linux\")"
    };
    write(
        &root,
        &format!(
            "[package]\nname = \"root\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[target.'{target}'.dependencies]\nwrong = {{ path = \"../wrong\" }}\n"
        ),
        "pub fn n() -> u8 { 1 }\n",
    );

    let result = artificer::check(&root, &home).unwrap();

    assert_eq!(result.rustc, artificer::RustcOutcome::Ran);
}

#[test]
fn package_description_matches_cargo() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("root");
    write(
        &root,
        "[package]\nname = \"described\"\nversion = \"0.1.0\"\nedition = \"2021\"\ndescription = \"fixture description\"\n",
        "pub const DESCRIPTION: &str = env!(\"CARGO_PKG_DESCRIPTION\");\n#[cfg(test)] mod tests { #[test] fn description() { assert_eq!(super::DESCRIPTION, \"fixture description\"); } }\n",
    );

    let code = artificer::test_package(&root, &[], &home, &artificer::TestOpts::default()).unwrap();

    assert_eq!(code, 0);
}

#[test]
fn dependency_edit_rebuilds_the_consumer() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let dependency = tmp.path().join("dependency");
    let consumer = tmp.path().join("consumer");
    write(
        &dependency,
        "[package]\nname = \"dependency\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "pub fn value() -> u8 { 1 }\n",
    );
    write(
        &consumer,
        "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ndependency = { path = \"../dependency\" }\n",
        "pub fn value() -> u8 { dependency::value() }\n",
    );
    artificer::check(&consumer, &home).unwrap();
    fs::write(
        dependency.join("src/lib.rs"),
        "pub fn value() -> u8 { 2 }\n",
    )
    .unwrap();

    let rebuilt = artificer::check(&consumer, &home).unwrap();

    assert_eq!(rebuilt.rustc, artificer::RustcOutcome::Ran);
}

#[test]
fn virtual_workspace_test_runs_members() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let ws = tmp.path().join("ws");
    fs::create_dir_all(ws.join("a/src")).unwrap();
    fs::create_dir_all(ws.join("b/src")).unwrap();
    fs::write(
        ws.join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    for (dir, name) in [("a", "alpha"), ("b", "beta")] {
        fs::write(
            ws.join(dir).join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        fs::write(ws.join(dir).join("src/lib.rs"), "#[test] fn t() {}\n").unwrap();
    }

    let code = artificer::test_package(&ws, &[], &home, &artificer::TestOpts::default()).unwrap();
    assert_eq!(code, 0);

    fs::write(
        ws.join("b/src/lib.rs"),
        "#[test] fn t() { panic!(\"boom\"); }\n",
    )
    .unwrap();
    let code = artificer::test_package(&ws, &[], &home, &artificer::TestOpts::default()).unwrap();
    assert_ne!(code, 0);
}

#[test]
fn virtual_workspace_test_compiles_member_libs() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let ws = tmp.path().join("ws");
    fs::create_dir_all(ws.join("app/src")).unwrap();
    fs::create_dir_all(ws.join("base/src")).unwrap();
    fs::write(
        ws.join("Cargo.toml"),
        "[workspace]\nmembers = [\"app\", \"base\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(
        ws.join("app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nbase = { path = \"../base\" }\n",
    )
    .unwrap();
    fs::write(
        ws.join("app/src/lib.rs"),
        "#[cfg(test)] mod tests { #[test] fn uses_base() { assert_eq!(base::n(), 1); } }\n",
    )
    .unwrap();
    fs::write(
        ws.join("base/Cargo.toml"),
        "[package]\nname = \"base\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(ws.join("base/src/lib.rs"), "pub fn n() -> u8 { 1 }\n").unwrap();

    let code = artificer::test_package(&ws, &[], &home, &artificer::TestOpts::default()).unwrap();

    assert_eq!(code, 0);
}

#[test]
fn build_bin_sees_build_script_outputs() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = tmp.path().join("a");
    write(
        &a,
        "[package]\nname = \"gen\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "pub fn n() -> u8 { 1 }\n",
    );
    fs::write(
        a.join("build.rs"),
        "fn main() {\n    let out = std::env::var(\"OUT_DIR\").unwrap();\n    std::fs::write(format!(\"{out}/gen.rs\"), \"pub const V: u8 = 7;\\n\").unwrap();\n    println!(\"cargo:rustc-cfg=generated\");\n}\n",
    )
    .unwrap();
    fs::create_dir_all(a.join("src/bin")).unwrap();
    fs::write(
        a.join("src/bin/tool.rs"),
        "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\nfn main() {\n    assert_eq!(V, 7);\n    #[cfg(not(generated))]\n    compile_error!(\"missing build-script cfg\");\n}\n",
    )
    .unwrap();

    let code = artificer::check_cmd(
        &a,
        &[],
        &home,
        artificer::CheckOpts {
            link: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(code, 0);
    let exe = target_dir(&a)
        .join("debug")
        .join(if cfg!(windows) { "tool.exe" } else { "tool" });
    assert!(exe.is_file(), "missing {}", exe.display());
    assert!(Command::new(&exe).status().unwrap().success());
}
