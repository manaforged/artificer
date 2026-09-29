use super::{export, import};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

fn unit(home: &Path, name: &str, body: &[u8], used: SystemTime) {
    let dir = home.join("units").join(crate::store::LAYOUT).join(name);
    fs::create_dir_all(dir.join("out")).unwrap();
    fs::write(dir.join("out").join("libx.rmeta"), body).unwrap();
    fs::write(dir.join("ok"), b"").unwrap();
    let marker = fs::OpenOptions::new()
        .write(true)
        .open(dir.join("ok"))
        .unwrap();
    marker.set_modified(used).unwrap();
}

fn stored(home: &Path, name: &str) -> bool {
    home.join("units")
        .join(crate::store::LAYOUT)
        .join(name)
        .join("ok")
        .is_file()
}

#[test]
fn export_selects_by_last_use_and_import_adds_missing_units() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    let dest = tmp.path().join("dest");
    let fresh = SystemTime::now();
    let old = fresh - Duration::from_secs(40 * 24 * 3600);
    unit(&home, "u-fresh", b"fresh", fresh);
    unit(&home, "u-old", b"old", old);

    let report = export(&home, &dest, 7, 0)?;
    assert_eq!(report.units, 1, "only the fresh unit is exported");
    let layout = crate::store::LAYOUT;
    assert!(
        dest.join("units")
            .join(layout)
            .join("u-fresh/out/libx.rmeta")
            .is_file()
    );
    assert!(!dest.join("units").join(layout).join("u-old").exists());

    let other = tmp.path().join("other");
    let added = import(&other, &dest)?;
    assert_eq!(added.units, 1);
    assert_eq!(
        fs::read(
            other
                .join("units")
                .join(crate::store::LAYOUT)
                .join("u-fresh/out/libx.rmeta"),
        )?,
        b"fresh"
    );
    assert!(stored(&other, "u-fresh"));

    let again = import(&other, &dest)?;
    assert_eq!(again.units, 0, "a second import adds nothing");
    Ok(())
}

#[test]
fn export_honors_the_byte_budget_and_zero_days() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    unit(&home, "u-one", &[0u8; 64], SystemTime::now());
    unit(&home, "u-two", &[0u8; 64], SystemTime::now());

    let capped = export(&home, &tmp.path().join("capped"), 0, 100)?;
    assert_eq!(
        capped.units, 1,
        "one unit fits the budget, the other does not"
    );

    let all = export(&home, &tmp.path().join("all"), 0, 0)?;
    assert_eq!(all.units, 2, "zero days and zero bytes means everything");
    Ok(())
}

#[test]
fn export_keeps_existing_entries() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    let dest = tmp.path().join("dest");
    unit(&home, "u-keep", b"keep", SystemTime::now());
    export(&home, &dest, 0, 0)?;
    fs::write(dest.join("sentinel"), b"x")?;
    export(&home, &dest, 0, 0)?;
    assert!(dest.join("sentinel").is_file());
    assert!(
        dest.join("units")
            .join(crate::store::LAYOUT)
            .join("u-keep/ok")
            .is_file()
    );
    Ok(())
}

#[test]
fn incomplete_units_are_skipped() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    let half = home.join("units").join(crate::store::LAYOUT).join("u-half");
    fs::create_dir_all(half.join("out"))?;
    let dest = tmp.path().join("dest");
    assert_eq!(export(&home, &dest, 0, 0)?.units, 0);
    Ok(())
}

#[test]
fn importing_a_missing_directory_is_a_cache_miss() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let report = import(&tmp.path().join("home"), &tmp.path().join("nothing"))?;
    assert_eq!(report.units, 0);
    Ok(())
}
