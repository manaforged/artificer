use super::*;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;

const NONBLOCK: i32 = if cfg!(target_os = "macos") {
    0x4
} else {
    0x800
};

fn open(fifo: &Path) -> Result<fs::File> {
    Ok(fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(NONBLOCK)
        .open(fifo)?)
}

fn drain(file: &mut fs::File) -> Result<usize> {
    let mut count = 0;
    let mut buf = [0u8; 128];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => count += read,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(count)
}

#[test]
fn install_keeps_the_pool_alive() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();
    install(home)?;
    let mut file = open(&home.join("jobserver.fifo"))?;
    let count = drain(&mut file)?;
    assert!(count >= 1, "pool must outlive install writes");
    file.write_all(&vec![b'+'; count])?;
    Ok(())
}

#[test]
fn install_tops_up_after_a_leak() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();
    install(home)?;
    let fifo = home.join("jobserver.fifo");
    let count = drain(&mut open(&fifo)?)?;
    assert!(count >= 1);
    fill(&fifo, count)?;
    let mut file = open(&fifo)?;
    let restored = drain(&mut file)?;
    file.write_all(&vec![b'+'; restored])?;
    assert_eq!(restored, count);
    Ok(())
}

#[test]
fn filling_repeatedly_holds_one_writer() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();
    install(home)?;
    let fifo = home.join("jobserver.fifo");
    assert!(fifo.exists(), "FIFO must exist");
    for _ in 0..40 {
        install(home)?;
    }
    assert_eq!(writers(&fifo), 1, "40 fills must leave one writer");
    Ok(())
}

#[test]
fn concurrent_installs_do_not_inflate_the_pool() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().to_path_buf();
    install(&home)?;
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .max(1);
    std::thread::scope(|scope| -> Result<()> {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let home = home.clone();
                scope.spawn(move || install(&home))
            })
            .collect();
        for handle in handles {
            handle.join().expect("installer thread must not panic")?;
        }
        Ok(())
    })?;
    let depth = pool_depth(&home.join("jobserver.fifo"))?;
    assert!(
        depth <= cores,
        "pool holds {depth} tokens on a {cores}-core host",
    );
    Ok(())
}

#[test]
fn acquire_returns_the_token() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();
    install(home)?;
    let fifo = home.join("jobserver.fifo");
    let before = pool_depth(&fifo)?;
    {
        let _permit = acquire(home)?;
        let during = pool_depth(&fifo)?;
        assert_eq!(during, before.saturating_sub(1));
    }
    assert_eq!(pool_depth(&fifo)?, before);
    Ok(())
}

#[test]
fn isolate_drops_jobserver_vars() -> Result<()> {
    unsafe {
        std::env::set_var("MAKEFLAGS", "--jobserver-auth=fifo:/nope");
        std::env::set_var("CARGO_MAKEFLAGS", "--jobserver-auth=fifo:/nope");
    }
    let mut cmd = std::process::Command::new("/usr/bin/env");
    isolate(&mut cmd);
    let out = cmd.output()?;
    unsafe {
        std::env::remove_var("MAKEFLAGS");
        std::env::remove_var("CARGO_MAKEFLAGS");
    }
    anyhow::ensure!(out.status.success(), "env");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        !text.lines().any(|line| line.starts_with("MAKEFLAGS=")),
        "{text}"
    );
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with("CARGO_MAKEFLAGS=")),
        "{text}"
    );
    Ok(())
}

#[test]
fn fifo_avoids_whitespace_in_the_auth_path() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("cache home").join("artificer");
    fs::create_dir_all(&home)?;
    let path = fifo_at(&home, tmp.path());
    assert!(
        !path.to_string_lossy().contains(char::is_whitespace),
        "{}",
        path.display()
    );
    Ok(())
}

#[test]
fn fifo_uses_a_no_space_alias_for_the_same_home() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let physical = tmp.path().join("cache home").join("artificer");
    fs::create_dir_all(&physical)?;
    let alias = tmp.path().join("cargo");
    std::os::unix::fs::symlink(tmp.path().join("cache home"), &alias)?;

    assert_eq!(
        fifo_at(&physical, &alias),
        alias.join("artificer").join("jobserver.fifo")
    );
    Ok(())
}

fn held_files(fifo: &Path) -> Result<usize> {
    match fs::read_dir(held::dir(fifo)) {
        Ok(entries) => Ok(entries.count()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

#[test]
fn a_dead_holder_token_returns_to_a_blocked_acquire() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().to_path_buf();
    install(&home)?;
    let fifo = home.join("jobserver.fifo");
    drain(&mut open(&fifo)?)?;
    let dir = held::dir(&fifo);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("4194303-0-0"), b"")?;
    let (send, recv) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(send.send(acquire(&home).map(drop)));
    });
    let waited = recv.recv_timeout(held::RECLAIM_AFTER * 5)?;
    waited?;
    assert_eq!(pool_depth(&fifo)?, 1);
    assert_eq!(held_files(&fifo)?, 0);
    Ok(())
}

#[test]
fn a_live_holder_token_is_not_reclaimed() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();
    install(home)?;
    let fifo = home.join("jobserver.fifo");
    let before = pool_depth(&fifo)?;
    let dir = held::dir(&fifo);
    fs::create_dir_all(&dir)?;
    let live = dir.join("4194303-1-0");
    let lock = fs::File::create(&live)?;
    lock.lock()?;
    assert_eq!(held::reclaim(&fifo)?, 0);
    assert_eq!(pool_depth(&fifo)?, before);
    assert!(live.exists());
    Ok(())
}

#[test]
fn dropping_a_permit_removes_its_holder_and_returns_the_token() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();
    install(home)?;
    let fifo = home.join("jobserver.fifo");
    let before = pool_depth(&fifo)?;
    let permit = acquire(home)?;
    assert_eq!(held_files(&fifo)?, 1);
    assert_eq!(held::reclaim(&fifo)?, 0);
    drop(permit);
    assert_eq!(held_files(&fifo)?, 0);
    assert_eq!(pool_depth(&fifo)?, before);
    Ok(())
}
