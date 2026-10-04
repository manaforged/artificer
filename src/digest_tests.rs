use super::*;
use std::time::{Duration, SystemTime};

#[cfg(unix)]
#[test]
fn a_same_length_edit_with_its_old_mtime_is_not_served_from_the_memo() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    let path = tmp.path().join("source.rs");
    fs::write(&path, "fn a() {}\n")?;
    let old = SystemTime::now() - Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&path)?
        .set_modified(old)?;
    let first = file(Some(&home), &path)?;
    assert_eq!(file(Some(&home), &path)?, first);
    assert!(home.join(MEMO_DIR).is_dir(), "a settled file is memoized");

    std::thread::sleep(Duration::from_millis(20));
    fs::write(&path, "fn b() {}\n")?;
    fs::File::options()
        .write(true)
        .open(&path)?
        .set_modified(old)?;
    assert_ne!(file(Some(&home), &path)?, first);
    Ok(())
}
