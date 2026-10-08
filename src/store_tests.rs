use super::*;
use anyhow::{Result, anyhow};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

#[test]
fn hold_serializes() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().to_path_buf();
    let order = Arc::new(Mutex::new(Vec::new()));
    let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
    let a_home = home.clone();
    let a_ord = order.clone();
    let a = thread::spawn(move || -> Result<()> {
        let _guard = hold(&a_home, "d")?;
        acquired_tx
            .send(())
            .map_err(|_stopped| anyhow!("the test stopped waiting"))?;
        a_ord
            .lock()
            .map_err(|_poison| anyhow!("order mutex poisoned"))?
            .push(1);
        thread::sleep(Duration::from_millis(80));
        a_ord
            .lock()
            .map_err(|_poison| anyhow!("order mutex poisoned"))?
            .push(2);
        Ok(())
    });
    acquired_rx
        .recv()
        .map_err(|_dead| anyhow!("first holder died before acquiring"))?;
    let b_ord = order.clone();
    let b = thread::spawn(move || -> Result<()> {
        let _guard = hold(&home, "d")?;
        b_ord
            .lock()
            .map_err(|_poison| anyhow!("order mutex poisoned"))?
            .push(3);
        Ok(())
    });
    a.join()
        .map_err(|_thread| anyhow!("first holder thread panicked"))??;
    b.join()
        .map_err(|_thread| anyhow!("second holder thread panicked"))??;
    assert_eq!(
        *order
            .lock()
            .map_err(|_poison| anyhow!("order mutex poisoned"))?,
        vec![1, 2, 3]
    );
    Ok(())
}

fn backdate(path: &Path) -> Result<()> {
    let hour_ago = SystemTime::now() - Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(path)?
        .set_modified(hour_ago)?;
    Ok(())
}

fn unit(home: &Path, name: &str) -> Result<PathBuf> {
    let dir = home.join("units").join(LAYOUT).join(name);
    fs::create_dir_all(dir.join("out"))?;
    fs::write(dir.join("ok"), "")?;
    fs::write(dir.join("out/libx.rlib"), b"x")?;
    Ok(dir)
}

#[test]
fn parallel_publish_never_collides() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().to_path_buf();
    let mut joins = Vec::new();
    for thread_id in 0..8 {
        let home = home.clone();
        joins.push(thread::spawn(move || -> Result<()> {
            for unit_id in 0..50 {
                let name = format!("u-{thread_id}-{unit_id}");
                let slot = Slot::new(&home, &name);
                let src = home.join("stage").join(&name);
                fs::create_dir_all(src.join("out"))?;
                fs::write(src.join("out/libx.rlib"), b"x")?;
                slot.copy_from(&src)?;
                assert!(slot.hit());
            }
            Ok(())
        }));
    }
    for join in joins {
        join.join()
            .map_err(|_thread| anyhow!("publisher thread panicked"))??;
    }
    Ok(())
}

#[test]
fn gc_evicts_old_unit() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let dir = unit(tmp.path(), "u-dead")?;
    backdate(&dir.join("ok"))?;
    let (gone, bytes) = gc_units(tmp.path(), Duration::ZERO)?;
    assert_eq!(gone, 1);
    assert!(bytes >= 1);
    assert!(!dir.exists());
    Ok(())
}

#[test]
fn gc_skips_held_unit() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let dir = unit(tmp.path(), "u-held")?;
    backdate(&dir.join("ok"))?;
    let guard = hold(tmp.path(), "u-held")?;
    let (gone, _) = gc_units(tmp.path(), Duration::ZERO)?;
    assert_eq!(gone, 0);
    assert!(dir.exists());
    drop(guard);
    let released = std::time::Instant::now();
    while gc_units(tmp.path(), Duration::ZERO)?.0 == 0 {
        assert!(
            released.elapsed() < Duration::from_secs(5),
            "a released unit was never evicted"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!dir.exists());
    Ok(())
}

#[test]
fn gc_keeps_fresh_unit() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let dir = unit(tmp.path(), "u-fresh")?;
    let (gone, _) = gc_units(tmp.path(), AGE)?;
    assert_eq!(gone, 0);
    assert!(dir.join("ok").is_file());
    Ok(())
}

#[test]
fn gc_cap_evicts_oldest_first() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let old = unit(tmp.path(), "u-old")?;
    let new = unit(tmp.path(), "u-new")?;
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(24 * 3600);
    fs::File::options()
        .write(true)
        .open(old.join("ok"))?
        .set_modified(past)?;
    let one = size(&new)?;
    let (gone, _) = gc_cap(tmp.path(), one)?;
    assert_eq!(gone, 1);
    assert!(!old.exists(), "oldest unit evicted first");
    assert!(new.join("ok").is_file(), "hot unit survives the cap");
    Ok(())
}

#[test]
fn gc_evicts_old_meta() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let cache = tmp.path().join("cargo-meta/cache");
    fs::create_dir_all(&cache)?;
    fs::write(cache.join("k.json"), b"{}")?;
    fs::write(tmp.path().join("rustc-runs"), "ran\n")?;
    backdate(&cache.join("k.json"))?;
    backdate(&tmp.path().join("rustc-runs"))?;
    let (gone, bytes) = gc_units(tmp.path(), Duration::ZERO)?;
    assert_eq!(gone, 0, "no units, only cache");
    assert!(bytes >= 3);
    assert!(!cache.join("k.json").exists());
    assert!(!tmp.path().join("rustc-runs").exists());
    fs::write(cache.join("k.json"), b"{}")?;
    fs::write(tmp.path().join("rustc-runs"), "ran\n")?;
    gc_units(tmp.path(), AGE)?;
    assert!(cache.join("k.json").is_file(), "fresh cache stays");
    assert!(tmp.path().join("rustc-runs").is_file(), "fresh log stays");
    Ok(())
}

#[test]
fn stats_sum_over_builds() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home)?;
    assert_eq!(stats(&home)?, (0, 0));
    bump_stats(&home, 3, 1)?;
    bump_stats(&home, 2, 4)?;
    assert_eq!(stats(&home)?, (5, 5));
    Ok(())
}

#[test]
fn fallback_notes_sum() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home)?;
    assert_eq!(fallbacks(&home), (0, None));
    note_fallback(&home, "--target is set");
    note_fallback(&home, "profile `custom` belongs to cargo");
    assert_eq!(
        fallbacks(&home),
        (2, Some("profile `custom` belongs to cargo".to_string()))
    );
    Ok(())
}

#[test]
fn build_records_count_and_explain() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    assert!(crate::ready(&home));
    note_build(&home, "check", 3, 1, 120, None);
    note_build(&home, "build", 0, 5, 400, None);
    note_fallback(&home, "--target is set");
    note_fallback(&home, "--target is set");
    note_fallback(&home, "profile `custom` belongs to cargo");

    let records = build_records(&home);
    assert_eq!(records.len(), 5);
    assert_eq!(records[0]["hits"], 3);
    assert_eq!(records[1]["op"], "build");
    assert_eq!(
        fallback_reasons(&home),
        vec![
            ("--target is set".to_string(), 2),
            ("profile `custom` belongs to cargo".to_string(), 1),
        ]
    );
    Ok(())
}

#[test]
fn gc_removes_unused_units_of_older_layouts() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let layout = tmp.path().join("units").join("v1");
    let old = layout.join("u-old");
    let fresh = layout.join("u-fresh");
    for unit in [&old, &fresh] {
        fs::create_dir_all(unit.join("out"))?;
        fs::write(unit.join("ok"), "")?;
    }
    let long_ago = SystemTime::now() - AGE - Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(old.join("ok"))?
        .set_modified(long_ago)?;
    let current = unit(tmp.path(), "u-current")?;
    let (gone, _) = gc_units(tmp.path(), AGE)?;
    assert_eq!(gone, 1);
    assert!(!old.exists());
    assert!(fresh.join("ok").is_file());
    assert!(current.join("ok").is_file());
    Ok(())
}

fn aged_unit(home: &Path, name: &str, hours: u64) -> Result<PathBuf> {
    let dir = unit(home, name)?;
    fs::write(dir.join("out/libx.rlib"), [0_u8; 1000])?;
    let past = SystemTime::now() - Duration::from_secs(hours * 3600);
    fs::File::options()
        .write(true)
        .open(dir.join("ok"))?
        .set_modified(past)?;
    Ok(dir)
}

#[test]
fn gc_cap_evicts_below_the_cap_to_leave_headroom() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let units = (0..20)
        .map(|at| aged_unit(tmp.path(), &format!("u-{at:02}"), 100 - at))
        .collect::<Result<Vec<_>>>()?;
    let total = usage(tmp.path())?;
    let cap = total - 1;
    gc_cap(tmp.path(), cap)?;
    let left = usage(tmp.path())?;
    assert!(
        left <= cap - cap / 100 * gc::HEADROOM_SHARE,
        "{left} bytes left under a cap of {cap}"
    );
    assert!(
        units
            .last()
            .is_some_and(|newest| newest.join("ok").is_file())
    );
    Ok(())
}

#[test]
fn gc_collects_a_rename_left_by_a_dead_process() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().join("units").join(LAYOUT);
    let dead = root.join(".u-gone.artificer-4000000000-0");
    let live = root.join(format!(".u-busy.artificer-{}-0", std::process::id()));
    for dir in [&dead, &live] {
        fs::create_dir_all(dir.join("out"))?;
        fs::write(dir.join("out/libx.rlib"), b"x")?;
    }
    gc_cap(tmp.path(), u64::MAX)?;
    assert!(
        !dead.exists(),
        "a dead process's leftover stays in the store"
    );
    assert!(
        live.exists(),
        "an in-flight rename by a live process was removed"
    );
    Ok(())
}

#[test]
fn discarding_a_unit_leaves_nothing_behind() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let dir = unit(tmp.path(), "u-gone")?;
    discard(&dir)?;
    assert!(!dir.exists());
    let leftovers = fs::read_dir(tmp.path().join("units").join(LAYOUT))?.count();
    assert_eq!(leftovers, 0);
    Ok(())
}
