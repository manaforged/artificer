use super::*;
use std::time::{Duration, Instant};

fn wait_for(mut ready: impl FnMut() -> bool) {
    let started = Instant::now();
    while !ready() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "fixture did not become ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn starting_a_second_process_does_not_replenish_a_borrowed_job_token() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let first_root = temp.path().join("first");
    let second_root = temp.path().join("second");
    write_clean_pkg(&first_root);
    write_clean_pkg(&second_root);
    fs::write(first_root.join("src/lib.rs"), "pub const VALUE: u8 = 1;")
        .expect("write fixture file");
    fs::write(second_root.join("src/lib.rs"), "pub const VALUE: u8 = 2;")
        .expect("write fixture file");
    let home = temp.path().join("store");
    let active = temp.path().join("active");
    let release = temp.path().join("release");
    let overlap = temp.path().join("overlap");
    let wrapper = temp.path().join("compiler-wrapper");
    fs::write(
        &wrapper,
        r#"#!/bin/sh
case " $* " in
  *" --crate-name clean "*)
    mkdir "$PROOF_ACTIVE" 2>/dev/null || printf overlap > "$PROOF_OVERLAP"
    while [ ! -f "$PROOF_RELEASE" ]; do sleep 0.01; done
    rmdir "$PROOF_ACTIVE" 2>/dev/null || :
    ;;
esac
exec "$@"
"#,
    )
    .expect("write fixture file");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))
        .expect("make fixture wrapper executable");
    let status = fs::read_to_string("/proc/self/status").expect("read fixture text");
    let cpu = status
        .lines()
        .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
        .expect("Linux exposes the allowed CPU list")
        .trim()
        .split([',', '-'])
        .next()
        .expect("CPU affinity contains a CPU");
    let spawn = |root: &Path| {
        Command::new("taskset")
            .args(["-c", cpu])
            .arg(env!("CARGO_BIN_EXE_artificer"))
            .arg("check")
            .current_dir(root)
            .env("ARTIFICER_HOME", &home)
            .env("ARTIFICER_NOSERVE", "1")
            .env_remove("CARGO_TARGET_DIR")
            .env("RUSTC_WRAPPER", &wrapper)
            .env("PROOF_ACTIVE", &active)
            .env("PROOF_RELEASE", &release)
            .env("PROOF_OVERLAP", &overlap)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn fixture process")
    };
    let mut first = spawn(&first_root);
    wait_for(|| active.is_dir());
    let pool_env = home.join("jobserver.env");
    let before = fs::metadata(&pool_env)
        .expect("read fixture metadata")
        .modified()
        .expect("read fixture modification time");
    let mut second = spawn(&second_root);
    wait_for(|| {
        fs::metadata(&pool_env)
            .expect("read fixture metadata")
            .modified()
            .expect("read fixture modification time")
            != before
    });
    let started = Instant::now();
    while !overlap.exists() && started.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(&release, "").expect("write fixture file");
    assert!(first.wait().expect("wait for fixture process").success());
    assert!(second.wait().expect("wait for fixture process").success());
    assert!(
        !overlap.exists(),
        "two compiler processes held the machine's single job token"
    );
}
