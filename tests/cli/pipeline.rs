use super::*;
use serde_json::Value;

const FUNCTIONS: usize = 600;
const STATEMENTS: usize = 30;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn heavy_source() -> String {
    let mut source = String::from("pub const BASE: u64 = 7;\n");
    for f in 0..FUNCTIONS {
        source.push_str(&format!(
            "pub fn step{f}(seed: u64) -> u64 {{\n    let mut v = seed;\n"
        ));
        for s in 0..STATEMENTS {
            source.push_str(&format!(
                "    v = v.rotate_left({}).wrapping_mul({}).wrapping_add({});\n",
                (f + s) % 63 + 1,
                2 * (f * STATEMENTS + s) + 3,
                f + s
            ));
        }
        source.push_str("    v\n}\n");
    }
    source
}

fn member(root: &Path, name: &str, deps: &str, file: &str, source: &str) {
    write(
        &root.join(name).join("Cargo.toml"),
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}"
        ),
    );
    write(&root.join(name).join("src").join(file), source);
}

fn unit<'v>(profile: &'v Value, name: &str) -> &'v Value {
    profile["top_units"]
        .as_array()
        .unwrap()
        .iter()
        .find(|unit| unit["name"] == name)
        .unwrap_or_else(|| panic!("no unit {name} in {profile}"))
}

fn ms(unit: &Value, field: &str) -> u64 {
    unit[field].as_u64().unwrap()
}

#[test]
fn a_library_compiles_while_its_dependency_is_still_in_codegen() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"heavy\", \"light\", \"app\"]\n\n[profile.dev]\nopt-level = 3\n",
    );
    member(&root, "heavy", "", "lib.rs", &heavy_source());
    member(
        &root,
        "light",
        "heavy = { path = \"../heavy\" }\n",
        "lib.rs",
        "pub fn value() -> u64 {\n    heavy::BASE * 6\n}\n",
    );
    member(
        &root,
        "app",
        "light = { path = \"../light\" }\n",
        "main.rs",
        "fn main() {\n    println!(\"{}\", light::value());\n}\n",
    );
    let home = tmp.path().join("home");
    let out = artificer(&home, &root).arg("build").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let exe = root
        .join("target/debug")
        .join(format!("app{}", std::env::consts::EXE_SUFFIX));
    let ran = Command::new(exe).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "42");

    let profile = artificer(&home, &root)
        .args(["profile", "--json"])
        .output()
        .unwrap();
    let profile: Value = serde_json::from_slice(&profile.stdout).unwrap();
    let (heavy, light, app) = (
        unit(&profile, "heavy"),
        unit(&profile, "light"),
        unit(&profile, "app"),
    );
    assert!(
        ms(light, "start_ms") < ms(heavy, "end_ms"),
        "light waited for heavy's codegen: {profile}"
    );
    assert!(
        ms(app, "start_ms") >= ms(heavy, "end_ms") && ms(app, "start_ms") >= ms(light, "end_ms"),
        "app linked before its dependencies finished: {profile}"
    );
}

fn unit_role<'v>(profile: &'v Value, name: &str, role: &str) -> &'v Value {
    profile["top_units"]
        .as_array()
        .unwrap()
        .iter()
        .find(|unit| unit["name"] == name && unit["role"] == role)
        .unwrap_or_else(|| panic!("no {role} unit {name} in {profile}"))
}

#[test]
fn a_build_script_runs_while_the_package_dependencies_still_compile() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"heavy\", \"scripted\"]\n\n[profile.dev]\nopt-level = 3\n",
    );
    member(&root, "heavy", "", "lib.rs", &heavy_source());
    member(
        &root,
        "scripted",
        "heavy = { path = \"../heavy\" }\n",
        "lib.rs",
        "pub fn value() -> u64 {\n    env!(\"SCRIPTED\").len() as u64 + heavy::BASE\n}\n",
    );
    write(
        &root.join("scripted/build.rs"),
        "fn main() {\n    println!(\"cargo:rustc-env=SCRIPTED=yes\");\n}\n",
    );
    let home = tmp.path().join("home");
    let out = artificer(&home, &root).arg("build").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let profile = artificer(&home, &root)
        .args(["profile", "--json"])
        .output()
        .unwrap();
    let profile: Value = serde_json::from_slice(&profile.stdout).unwrap();
    let heavy = unit_role(&profile, "heavy", "package");
    let script = unit_role(&profile, "scripted", "script");
    assert!(
        ms(script, "start_ms") < ms(heavy, "end_ms"),
        "the build script waited for heavy: {profile}"
    );
}

#[test]
fn a_package_with_a_library_and_a_binary_links_after_its_dependencies_finish() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"heavy\", \"both\"]\n\n[profile.dev]\nopt-level = 3\n",
    );
    member(&root, "heavy", "", "lib.rs", &heavy_source());
    member(
        &root,
        "both",
        "heavy = { path = \"../heavy\" }\n",
        "lib.rs",
        "pub fn value() -> u64 {\n    heavy::BASE * 6\n}\n",
    );
    write(
        &root.join("both/src/main.rs"),
        "fn main() {\n    println!(\"{}\", both::value() + heavy::step0(0) * 0);\n}\n",
    );
    let home = tmp.path().join("home");
    let out = artificer(&home, &root).arg("build").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let exe = root
        .join("target/debug")
        .join(format!("both{}", std::env::consts::EXE_SUFFIX));
    let ran = Command::new(exe).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "42");
}
