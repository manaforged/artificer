use super::*;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn run(home: &Path, root: &Path) -> String {
    let out = artificer(home, root).arg("run").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn compiled_scripts(home: &Path) -> usize {
    fs::read_dir(home.join("units").join(artificer::LAYOUT))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().starts_with("scriptbin-"))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn a_source_edit_reruns_the_build_script_without_recompiling_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        &root.join("build.rs"),
        "fn main() {\n    let log = std::path::PathBuf::from(std::env::var(\"CARGO_MANIFEST_DIR\").unwrap()).join(\"../runs\");\n    let before = std::fs::read_to_string(&log).unwrap_or_default();\n    std::fs::write(&log, format!(\"{before}x\")).unwrap();\n}\n",
    );
    let home = tmp.path().join("home");
    for n in 1..3 {
        write(
            &root.join("src/main.rs"),
            &format!("fn main() {{ println!(\"{n}\"); }}\n"),
        );
        assert_eq!(run(&home, &root), n.to_string());
    }
    assert_eq!(
        fs::read_to_string(tmp.path().join("runs")).unwrap(),
        "xx",
        "reran"
    );
    assert_eq!(compiled_scripts(&home), 1, "compiled once");
}

#[test]
fn a_file_included_from_outside_the_package_still_reruns_the_script() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pkg");
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        &root.join("build.rs"),
        "include!(\"../shared.rs\");\nfn main() {\n    println!(\"cargo:rerun-if-changed=build.rs\");\n    println!(\"cargo:rustc-env=SHARED={}\", value());\n}\n",
    );
    write(
        &root.join("src/main.rs"),
        "fn main() { println!(\"{}\", env!(\"SHARED\")); }\n",
    );
    let home = tmp.path().join("home");
    for word in ["one", "two"] {
        write(
            &tmp.path().join("shared.rs"),
            &format!("fn value() -> &'static str {{ \"{word}\" }}\n"),
        );
        assert_eq!(run(&home, &root), word);
    }
}
