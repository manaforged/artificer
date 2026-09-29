use std::{fs, path::Path};

fn unit(home: &Path) -> std::path::PathBuf {
    let path = home.join("units").join(artificer::LAYOUT).join("u-fixture");
    fs::create_dir_all(path.join("out")).expect("create fixture directory");
    fs::write(path.join("out/artifact"), "complete").expect("write fixture file");
    fs::write(path.join("script-inputs"), "recorded inputs").expect("write fixture file");
    fs::write(path.join("ok"), "").expect("write fixture file");
    path
}

#[test]
fn reexport_repairs_an_interrupted_copy_and_import_keeps_validation_inputs() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let home = temp.path().join("store");
    let dest = temp.path().join("export");
    unit(&home);
    let partial = unit(&dest);
    fs::remove_file(partial.join("ok")).expect("remove fixture marker");
    fs::write(partial.join("out/artifact"), "partial").expect("write fixture file");
    let report = artificer::export(&home, &dest, 0, 0).expect("export fixture cache");
    assert_eq!(report.units, 1, "an incomplete export must be repaired");
    let restored = temp.path().join("restored");
    artificer::import(&restored, &dest).expect("import fixture cache");
    let restored = restored
        .join("units")
        .join(artificer::LAYOUT)
        .join("u-fixture");
    assert_eq!(
        fs::read_to_string(restored.join("out/artifact")).expect("read fixture text"),
        "complete"
    );
    assert_eq!(
        fs::read_to_string(restored.join("script-inputs")).expect("read fixture text"),
        "recorded inputs"
    );
}

#[test]
fn export_accepts_the_full_days_range_without_overflow() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let home = temp.path().join("store");
    unit(&home);
    let report = artificer::export(&home, &temp.path().join("export"), u64::MAX, 0)
        .expect("export fixture cache");
    assert_eq!(report.units, 1);
}

#[cfg(unix)]
#[test]
fn export_rejects_a_symlink_without_publishing_a_partial_unit() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let home = temp.path().join("store");
    let path = unit(&home);
    std::os::unix::fs::symlink("artifact", path.join("out/link")).expect("create fixture symlink");
    let dest = temp.path().join("export");
    let result = artificer::export(&home, &dest, 0, 0);
    assert!(
        result.is_err(),
        "a symlink was silently omitted from the exported unit"
    );
    let restored = temp.path().join("restored");
    assert_eq!(
        artificer::import(&restored, &dest)
            .expect("import fixture cache")
            .units,
        0
    );
}
