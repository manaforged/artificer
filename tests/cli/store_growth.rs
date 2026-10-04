use super::*;

fn write_chain(root: &Path, base_body: &str) {
    fs::create_dir_all(root.join("base/src")).unwrap();
    fs::create_dir_all(root.join("app/src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"base\", \"app\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("base/Cargo.toml"),
        "[package]\nname = \"base\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("base/src/lib.rs"), base_body).unwrap();
    fs::write(
        root.join("app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nbase = { path = \"../base\" }\n",
    )
    .unwrap();
    fs::write(
        root.join("app/src/lib.rs"),
        "pub fn twice() -> u8 { base::value() * 2 }\n",
    )
    .unwrap();
}

fn units_of(home: &Path, crate_name: &str) -> usize {
    let prefix = format!("lib{crate_name}-");
    let Ok(entries) = fs::read_dir(home.join("units").join(artificer::LAYOUT)) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|unit| {
            fs::read_dir(unit.path().join("out")).is_ok_and(|files| {
                files
                    .filter_map(Result::ok)
                    .any(|f| f.file_name().to_string_lossy().starts_with(&prefix))
            })
        })
        .count()
}

fn build(home: &Path, root: &Path) {
    let out = artificer(home, root).arg("build").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn editing_a_workspace_crate_replaces_its_units_and_its_dependents_units() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let home = tmp.path().join("home");
    write_chain(&root, "pub fn value() -> u8 { 1 }\n");
    build(&home, &root);
    let base = units_of(&home, "base");
    let app = units_of(&home, "app");
    assert!(base > 0 && app > 0, "base {base} app {app}");

    for n in 2..5 {
        fs::write(
            root.join("base/src/lib.rs"),
            format!("pub fn value() -> u8 {{ {n} }}\n"),
        )
        .unwrap();
        build(&home, &root);
    }

    assert_eq!(units_of(&home, "base"), base, "edited crate");
    assert_eq!(units_of(&home, "app"), app, "dependent of the edited crate");
}
