use super::{Action, Key, Kind};
use std::fs;

#[test]
fn key_scheme_is_stable() {
    let mut key = Key::new();
    key.feed(b"hello");
    key.feed_str("world");

    let mut reference = blake3::Hasher::new();
    for part in ["hello", "world"] {
        reference.update(&(part.len() as u64).to_le_bytes());
        reference.update(part.as_bytes());
    }
    assert_eq!(
        key.digest(),
        reference.finalize().to_hex()[..32].to_string()
    );
}

#[test]
fn key_separates_concatenations() {
    let mut split = Key::new();
    split.feed(b"ab");
    split.feed(b"c");
    let mut joined = Key::new();
    joined.feed(b"a");
    joined.feed(b"bc");
    assert_ne!(split.digest(), joined.digest());
}

#[test]
fn action_round_trips_through_the_store() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;

    let first = Action::begin(&home, Kind::Unit, "k1")?;
    assert!(!first.hit());
    let expected = format!("units/{}/u-k1/out", crate::store::LAYOUT);
    assert!(first.out.ends_with(&expected), "{:?}", first.out);
    fs::create_dir_all(&first.out)?;
    fs::write(first.out.join("lib.rmeta"), b"body")?;
    first.finish()?;
    drop(first);

    let second = Action::begin(&home, Kind::Unit, "k1")?;
    assert!(second.hit(), "the finished action is a hit");
    assert_eq!(fs::read(second.out.join("lib.rmeta"))?, b"body");
    Ok(())
}

#[test]
fn kind_names_the_entry() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    for (kind, prefix) in [
        (Kind::Unit, "u-"),
        (Kind::Test, "test-"),
        (Kind::Script, "script-"),
    ] {
        let action = Action::begin(&home, kind, "abc")?;
        let name = action.slot.dir.file_name().unwrap().to_string_lossy();
        assert_eq!(name, format!("{prefix}abc"));
    }
    Ok(())
}

#[test]
fn moving_an_item_between_lists_changes_the_key() {
    let empty: [&str; 0] = [];
    let mut first = Key::new();
    first.feed_list(["-Copt-level=1"]).feed_list(empty);
    let mut second = Key::new();
    second.feed_list(empty).feed_list(["-Copt-level=1"]);
    assert_ne!(first.digest(), second.digest());
}
