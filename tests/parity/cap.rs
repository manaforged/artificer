use super::*;

fn cached_units(home: &Path) -> usize {
    let Ok(layouts) = fs::read_dir(home.join("units")) else {
        return 0;
    };
    layouts
        .flatten()
        .filter(|layout| layout.path().is_dir())
        .flat_map(|layout| fs::read_dir(layout.path()).into_iter().flatten().flatten())
        .filter(|unit| unit.path().is_dir() && !unit.file_name().to_string_lossy().starts_with('.'))
        .count()
}

#[test]
fn a_build_over_the_store_cap_evicts_older_units_before_the_process_exits() {
    let p = Project::new(&[
        (
            "Cargo.toml",
            "[package]\nname = \"capped\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        ("src/main.rs", "fn main() { println!(\"first\"); }\n"),
    ]);
    assert!(p.artificer(&["build"], &[]).ok());
    assert_eq!(cached_units(&p.home), 1);

    fs::write(
        p.dir.join("src/main.rs"),
        "fn main() { println!(\"second\"); }\n",
    )
    .unwrap();
    for stamp in ["gc.stamp", "cap.stamp"] {
        let stamp = p.home.join(stamp);
        if stamp.exists() {
            fs::remove_file(stamp).unwrap();
        }
    }
    let built = p.artificer(&["build"], &[("ARTIFICER_STORE_CAP_GB", "0")]);
    assert!(built.ok(), "{}", built.stdout);

    assert_eq!(
        cached_units(&p.home),
        1,
        "only the unit the build holds stays"
    );
}
