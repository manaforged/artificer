use super::*;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn write_harness_free(root: &Path, test_body: &str) {
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"h\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[test]]\nname = \"custom\"\nharness = false\n",
    );
    write(
        &root.join("src/lib.rs"),
        "pub fn answer() -> u8 {\n    42\n}\n",
    );
    write(&root.join("tests/custom.rs"), test_body);
}

#[test]
fn a_harness_free_test_runs_its_own_main_with_cfg_test() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("h");
    write_harness_free(
        &root,
        "fn main() {\n    assert_eq!(h::answer(), 42);\n    if cfg!(test) {\n        println!(\"custom main ran\");\n    }\n}\n",
    );
    let out = artificer(&tmp.path().join("home"), &root)
        .args(["test", "--test", "custom"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("custom main ran"), "{stdout}");
    assert!(!stdout.contains("running 0 tests"), "{stdout}");
}

#[test]
fn check_tests_leaves_out_test_functions_of_a_harness_free_target() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("h");
    write_harness_free(
        &root,
        "fn main() {}\n\n#[test]\nfn left_out() {\n    let _n: u8 = \"not a number\";\n}\n",
    );
    let out = artificer(&tmp.path().join("home"), &root)
        .args(["check", "--tests"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
