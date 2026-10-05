use super::*;

const TEST_A: &str = "#[test]\nfn a() { assert_eq!(app::value(), 1); }\n";

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn package(root: &Path, script: Option<&str>) {
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        &root.join("src/lib.rs"),
        "pub const DATA: &str = include_str!(\"../tests/data.txt\");\npub fn value() -> u8 { 1 }\n",
    );
    write(
        &root.join("src/main.rs"),
        "fn main() { println!(\"{}\", app::DATA.trim()); }\n",
    );
    write(&root.join("tests/data.txt"), "first\n");
    write(&root.join("tests/a.rs"), TEST_A);
    write(
        &root.join("tests/b.rs"),
        "#[test]\nfn b() { assert_eq!(app::value(), 1); }\n",
    );
    if let Some(script) = script {
        write(&root.join("build.rs"), script);
    }
}

fn edit_test_a(root: &Path) {
    write(&root.join("tests/a.rs"), &format!("{TEST_A}// edited\n"));
}

fn compiled(home: &Path, root: &Path, args: &[&str]) -> (Vec<String>, String) {
    let out = artificer(home, root)
        .env("ARTIFICER_TRACE", "1")
        .args(args)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{stderr}");
    let crates = stderr
        .lines()
        .filter(|line| line.starts_with("ARTIFICER_CMD"))
        .filter_map(|line| {
            let name = line
                .split("\"--crate-name\" \"")
                .nth(1)?
                .split('"')
                .next()?;
            Some(name.to_string())
        })
        .collect();
    (
        crates,
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    )
}

fn runs(tmp: &Path) -> String {
    fs::read_to_string(tmp.join("runs")).unwrap_or_default()
}

const COUNTING_SCRIPT: &str = "fn main() {\n    let log = std::path::PathBuf::from(std::env::var(\"CARGO_MANIFEST_DIR\").unwrap()).join(\"../runs\");\n    let before = std::fs::read_to_string(&log).unwrap_or_default();\n    std::fs::write(&log, format!(\"{before}x\")).unwrap();\n    WATCH\n}\n";

#[test]
fn editing_a_test_file_leaves_the_library_and_binary_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("app"), tmp.path().join("home"));
    package(&root, None);
    compiled(&home, &root, &["build"]);
    edit_test_a(&root);
    let (rebuilt, _) = compiled(&home, &root, &["build"]);
    assert!(rebuilt.is_empty(), "a test edit recompiled {rebuilt:?}");
}

#[test]
fn editing_one_test_file_recompiles_only_that_test() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("app"), tmp.path().join("home"));
    package(&root, None);
    compiled(&home, &root, &["test", "--no-run"]);
    edit_test_a(&root);
    let (rebuilt, _) = compiled(&home, &root, &["test", "--no-run"]);
    assert_eq!(rebuilt, vec!["a".to_string()]);
}

#[test]
fn a_library_sees_an_edit_to_a_file_it_reads_from_another_targets_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("app"), tmp.path().join("home"));
    package(&root, None);
    assert_eq!(compiled(&home, &root, &["run", "-q"]).1, "first");
    write(&root.join("tests/data.txt"), "second\n");
    assert_eq!(compiled(&home, &root, &["run", "-q"]).1, "second");
}

#[test]
fn a_build_script_that_names_its_inputs_ignores_a_test_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("app"), tmp.path().join("home"));
    package(
        &root,
        Some(&COUNTING_SCRIPT.replace("WATCH", "println!(\"cargo::rerun-if-changed=build.rs\");")),
    );
    compiled(&home, &root, &["build"]);
    edit_test_a(&root);
    let (rebuilt, _) = compiled(&home, &root, &["build"]);
    assert_eq!(runs(tmp.path()), "x", "the script ran again");
    assert!(rebuilt.is_empty(), "a test edit recompiled {rebuilt:?}");
}

#[test]
fn a_build_script_without_rerun_directives_reruns_on_any_package_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("app"), tmp.path().join("home"));
    package(&root, Some(&COUNTING_SCRIPT.replace("WATCH", "")));
    compiled(&home, &root, &["build"]);
    edit_test_a(&root);
    compiled(&home, &root, &["build"]);
    assert_eq!(runs(tmp.path()), "xx");
}
