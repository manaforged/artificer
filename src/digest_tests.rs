use super::*;
use std::time::{Duration, SystemTime};

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

#[test]
fn a_memoized_file_keeps_its_env_names_across_processes() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    let path = tmp.path().join("source.rs");
    fs::write(&path, "const A: &str = env!(\"APP_MODE\");\n")?;
    fs::File::options()
        .write(true)
        .open(&path)?
        .set_modified(SystemTime::now() - Duration::from_secs(60))?;
    let canonical = fs::canonicalize(&path)?;
    let meta = fs::metadata(&canonical)?;
    let first = Memo::new(Some(&home)).known(&canonical, &meta)?;
    assert_eq!(first.env, vec!["APP_MODE"]);
    loaded()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    let shard = fs::read_to_string(shard_path(&home, &canonical))?;
    assert!(
        shard.contains("APP_MODE"),
        "the memo records the names: {shard}"
    );
    let again = Memo::new(Some(&home)).known(&canonical, &meta)?;
    assert_eq!(again.env, first.env);
    assert_eq!(again.digest, first.digest);
    Ok(())
}
