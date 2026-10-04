use super::*;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn script_project(root: &Path, build_rs: &str) {
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"scripted\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(&root.join("build.rs"), build_rs);
    write(
        &root.join("src/main.rs"),
        "fn main() { println!(\"ok\"); }\n",
    );
}

fn build(home: &Path, root: &Path, target: &Path, extra: &[&str]) {
    let out = artificer(home, root)
        .arg("build")
        .args(extra)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn misses(home: &Path, root: &Path) -> u64 {
    let out = artificer(home, root)
        .args(["stat", "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stat JSON");
    value["misses"].as_u64().expect("misses")
}

const COUNTING: &str = "fn main() {\n    let count = std::path::PathBuf::from(std::env::var(\"CARGO_MANIFEST_DIR\").unwrap()).join(\"../runs\");\n    let before = std::fs::read_to_string(&count).unwrap_or_default();\n    std::fs::write(&count, format!(\"{before}x\")).unwrap();\n    let out = std::env::var(\"OUT_DIR\").unwrap();\n    std::fs::write(format!(\"{out}/thing.o\"), std::env::var(\"CARGO_MANIFEST_DIR\").unwrap()).unwrap();\n    println!(\"cargo:rerun-if-changed=build.rs\");\n}\n";

#[test]
fn a_pull_from_a_directory_remote_brings_units_that_hit() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    script_project(&root, "fn main() {}\n");
    let builder = tmp.path().join("builder");
    build(&builder, &root, &tmp.path().join("t1"), &[]);
    let fresh = tmp.path().join("fresh");
    for args in [
        vec![
            "remote".to_string(),
            "set".to_string(),
            builder.display().to_string(),
        ],
        vec!["pull".to_string()],
    ] {
        let out = artificer(&fresh, &root).args(&args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    build(&fresh, &root, &tmp.path().join("t2"), &[]);
    assert_eq!(misses(&fresh, &root), 0);
}

#[test]
fn build_script_units_are_shared_across_job_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    script_project(&root, COUNTING);
    let home = tmp.path().join("home");
    build(&home, &root, &tmp.path().join("t1"), &["-j", "1"]);
    build(&home, &root, &tmp.path().join("t2"), &["-j", "2"]);
    assert_eq!(fs::read_to_string(tmp.path().join("runs")).unwrap(), "x");
}

#[test]
fn a_moved_checkout_reuses_a_build_script_whose_objects_name_the_old_path() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("first");
    script_project(&first, COUNTING);
    let home = tmp.path().join("home");
    build(&home, &first, &tmp.path().join("t1"), &[]);
    let moved = tmp.path().join("moved");
    script_project(&moved, COUNTING);
    build(&home, &moved, &tmp.path().join("t2"), &[]);
    assert_eq!(fs::read_to_string(tmp.path().join("runs")).unwrap(), "x");
}
