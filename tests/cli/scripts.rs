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

fn build_time_workspace(ws: &Path) {
    write(
        &ws.join("Cargo.toml"),
        "[workspace]\nmembers = [\"app\", \"gen\"]\nresolver = \"2\"\n",
    );
    write(
        &ws.join("gen/Cargo.toml"),
        "[package]\nname = \"gen\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        &ws.join("gen/build.rs"),
        "fn main() {\n    let opt = std::env::var(\"OPT_LEVEL\").unwrap();\n    let debug = std::env::var(\"DEBUG\").unwrap();\n    println!(\"cargo:rustc-env=GEN_PROFILE={opt}-{debug}\");\n}\n",
    );
    write(
        &ws.join("gen/src/lib.rs"),
        "pub const PROFILE: &str = env!(\"GEN_PROFILE\");\n",
    );
    write(
        &ws.join("app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[build-dependencies]\ngen = { path = \"../gen\" }\n",
    );
    write(
        &ws.join("app/build.rs"),
        "fn main() {\n    println!(\"cargo:rustc-env=APP_GEN={}\", gen::PROFILE);\n}\n",
    );
    write(
        &ws.join("app/src/main.rs"),
        "fn main() {\n    println!(\"{}\", env!(\"APP_GEN\"));\n}\n",
    );
}

#[test]
fn a_build_time_only_package_uses_the_build_override_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    build_time_workspace(&ws);
    let home = tmp.path().join("home");
    for args in [
        &["run", "-q", "-p", "app"][..],
        &["run", "-q", "-p", "app", "--release"][..],
    ] {
        let cargo = stock(&ws)
            .args(args)
            .env("CARGO_TARGET_DIR", tmp.path().join("stock"))
            .output()
            .unwrap();
        let ours = artificer(&home, &ws).args(args).output().unwrap();
        assert!(
            ours.status.success(),
            "{}",
            String::from_utf8_lossy(&ours.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&ours.stdout),
            String::from_utf8_lossy(&cargo.stdout),
            "{args:?}"
        );
    }
    let traced = artificer(&tmp.path().join("traced"), &ws)
        .env("ARTIFICER_TRACE", "1")
        .args(["build", "-p", "app"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&traced.stderr);
    let command = |name: &str| {
        stderr
            .lines()
            .find(|line| line.contains(&format!("\"--crate-name\" \"{name}\"")))
            .unwrap_or_else(|| panic!("no rustc command for {name}: {stderr}"))
            .to_string()
    };
    assert!(command("gen").contains("debuginfo=0"), "{}", command("gen"));
    assert!(command("app").contains("debuginfo=2"), "{}", command("app"));
    let script = command("build_script_build");
    assert!(script.contains("embed-bitcode=no"), "{script}");
}

#[test]
fn a_build_time_only_package_skips_bitcode_under_lto() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    build_time_workspace(&ws);
    let manifest = ws.join("Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        format!("{text}\n[profile.release]\nlto = true\n"),
    )
    .unwrap();
    let traced = artificer(&tmp.path().join("home"), &ws)
        .env("ARTIFICER_TRACE", "1")
        .args(["build", "-p", "app", "--release"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&traced.stderr);
    assert!(traced.status.success(), "{stderr}");
    let generator = stderr
        .lines()
        .find(|line| line.contains("\"--crate-name\" \"gen\""))
        .unwrap_or_else(|| panic!("no rustc command for gen: {stderr}"));
    assert!(generator.contains("embed-bitcode=no"), "{generator}");
    assert!(!generator.contains("lto="), "{generator}");
}
