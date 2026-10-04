use super::*;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn manifest(name: &str, rest: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{rest}")
}

fn stat(home: &Path, root: &Path) -> serde_json::Value {
    let out = artificer(home, root)
        .args(["stat", "--json"])
        .output()
        .unwrap();
    serde_json::from_slice(&out.stdout).expect("stat JSON")
}

fn run_ok(home: &Path, root: &Path, target: &Path) -> String {
    let out = artificer(home, root)
        .arg("run")
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn build_scripts_and_normal_code_get_their_own_features() {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path();
    write(
        &t.join("dep/Cargo.toml"),
        &manifest("dep", "[features]\nextra = []\n"),
    );
    write(
        &t.join("dep/src/lib.rs"),
        "pub fn value() -> bool { cfg!(feature = \"extra\") }\n",
    );
    write(
        &t.join("mid/Cargo.toml"),
        &manifest("mid", "[dependencies]\ndep = { path = \"../dep\" }\n"),
    );
    write(
        &t.join("mid/src/lib.rs"),
        "pub fn value() -> bool { dep::value() }\n",
    );
    write(
        &t.join("helper/Cargo.toml"),
        &manifest(
            "helper",
            "[dependencies]\ndep = { path = \"../dep\", features = [\"extra\"] }\n",
        ),
    );
    write(
        &t.join("helper/src/lib.rs"),
        "pub fn saw() -> bool { dep::value() }\n",
    );
    let root = t.join("project");
    write(
        &root.join("Cargo.toml"),
        &manifest(
            "project",
            "[dependencies]\ndep = { path = \"../dep\" }\nmid = { path = \"../mid\" }\n[build-dependencies]\nhelper = { path = \"../helper\" }\nmid = { path = \"../mid\" }\n",
        ),
    );
    write(
        &root.join("build.rs"),
        "fn main() { println!(\"cargo:rustc-env=BUILD_SAW={} {}\", helper::saw(), mid::value()); }\n",
    );
    write(
        &root.join("src/main.rs"),
        "fn main() { println!(\"{} {} {}\", env!(\"BUILD_SAW\"), dep::value(), mid::value()); }\n",
    );
    let home = t.join("home");
    assert_eq!(
        run_ok(&home, &root, &root.join("target")),
        "true true false false"
    );
    let value = stat(&home, &root);
    assert_eq!(value["fallbacks"], 0, "{value}");
}

#[cfg(unix)]
#[test]
fn a_dependency_link_argument_stays_with_its_own_package() {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path();
    write(&t.join("linky/Cargo.toml"), &manifest("linky", ""));
    write(
        &t.join("linky/build.rs"),
        "fn main() { println!(\"cargo:rustc-link-arg=-Wl,--artificer-no-such-flag\"); }\n",
    );
    write(&t.join("linky/src/lib.rs"), "pub fn value() -> u8 { 7 }\n");
    let root = t.join("project");
    write(
        &root.join("Cargo.toml"),
        &manifest(
            "project",
            "[dependencies]\nlinky = { path = \"../linky\" }\n",
        ),
    );
    write(
        &root.join("src/main.rs"),
        "fn main() { println!(\"{}\", linky::value()); }\n",
    );
    let home = t.join("home");
    assert_eq!(run_ok(&home, &root, &root.join("target")), "7");
}

#[test]
fn units_moved_to_a_store_at_another_path_still_hit() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    write(&root.join("Cargo.toml"), &manifest("project", ""));
    write(
        &root.join("build.rs"),
        "fn main() {\n    let out = std::env::var(\"OUT_DIR\").unwrap();\n    std::fs::write(format!(\"{out}/gen.rs\"), \"pub fn n() -> u8 { 5 }\").unwrap();\n    println!(\"cargo:rustc-link-search=native={out}\");\n}\n",
    );
    write(
        &root.join("src/main.rs"),
        "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\nfn main() { println!(\"{}\", n()); }\n",
    );
    let first = tmp.path().join("first");
    assert_eq!(run_ok(&first, &root, &tmp.path().join("t1")), "5");
    let moved = tmp.path().join("moved");
    let export = artificer(&first, &root)
        .args(["export"])
        .arg(&moved)
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "{}",
        String::from_utf8_lossy(&export.stderr)
    );
    let second = tmp.path().join("elsewhere/nested/second");
    let import = artificer(&second, &root)
        .args(["import"])
        .arg(&moved)
        .output()
        .unwrap();
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    assert_eq!(run_ok(&second, &root, &tmp.path().join("t2")), "5");
    let value = stat(&second, &root);
    assert_eq!(value["misses"], 0, "{value}");
    assert!(value["hits"].as_u64().unwrap_or(0) >= 1, "{value}");
}
