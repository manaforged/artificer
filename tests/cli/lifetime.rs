use super::*;
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Output, Stdio};
use std::time::{Duration, Instant};

struct Paused {
    child: Option<Child>,
    release: PathBuf,
}

impl Paused {
    fn start(mut command: Command, marker: &Path, release: &Path) -> Self {
        let child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn fixture process");
        let paused = Self {
            child: Some(child),
            release: release.to_path_buf(),
        };
        let started = Instant::now();
        while !marker.exists() {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "compiler did not reach the barrier"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        paused
    }

    fn finish(mut self) -> Output {
        fs::write(&self.release, "").expect("write fixture file");
        self.child
            .take()
            .expect("fixture child is still owned")
            .wait_with_output()
            .expect("collect fixture output")
    }
}

impl Drop for Paused {
    fn drop(&mut self) {
        drop(fs::write(&self.release, ""));
        if let Some(mut child) = self.child.take() {
            drop(child.wait());
        }
    }
}

fn wrapper(root: &Path) -> PathBuf {
    let wrapper = root.join("compiler-wrapper");
    fs::write(
        &wrapper,
        r#"#!/bin/sh
case " $* " in
  *" --crate-name clean "*)
    if [ "$PROOF_PAUSE" = 1 ]; then
      touch "$PROOF_MARKER"
      count=0
      while [ ! -f "$PROOF_RELEASE" ]; do
        count=$((count + 1))
        [ "$count" -lt 3000 ] || exit 99
        sleep 0.01
      done
    fi
    ;;
esac
exec "$@"
"#,
    )
    .expect("write fixture file");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))
        .expect("make fixture wrapper executable");
    wrapper
}

#[test]
fn clean_cannot_evict_a_dependency_while_a_compiler_uses_it() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    fs::create_dir_all(root.join("dep/src")).expect("create fixture directory");
    fs::write(
        root.join("dep/Cargo.toml"),
        "[package]\nname = 'dep'\nversion = '0.1.0'\nedition = '2021'\n",
    )
    .expect("write fixture file");
    fs::write(
        root.join("dep/src/lib.rs"),
        "pub const VALUE: &str = \"dependency survives\";",
    )
    .expect("write fixture file");
    let manifest = root.join("Cargo.toml");
    fs::write(
        &manifest,
        format!(
            "{}\n[dependencies]\ndep = {{ path = 'dep' }}\n",
            fs::read_to_string(&manifest).expect("read fixture text")
        ),
    )
    .expect("write fixture file");
    fs::write(root.join("src/lib.rs"), "pub use dep::VALUE;").expect("write fixture file");
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"{}\", clean::VALUE); }",
    )
    .expect("write fixture file");
    let home = temp.path().join("store");
    let marker = temp.path().join("paused");
    let release = temp.path().join("release");
    let mut command = artificer(&home, &root);
    command
        .arg("run")
        .env("RUSTC_WRAPPER", wrapper(temp.path()))
        .env("PROOF_PAUSE", "1")
        .env("PROOF_MARKER", &marker)
        .env("PROOF_RELEASE", &release);
    let paused = Paused::start(command, &marker, &release);
    let cleaned = artificer(&home, &root)
        .arg("clean")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("ARTIFICER_STORE_CAP_GB", "0")
        .output()
        .expect("run fixture command");
    let built = paused.finish();
    assert!(
        cleaned.status.success(),
        "{}",
        String::from_utf8_lossy(&cleaned.stderr)
    );
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&built.stdout).trim(),
        "dependency survives"
    );
}

#[test]
fn concurrent_build_scripts_keep_each_builds_environment() {
    let temp = tempfile::tempdir().expect("create isolated fixture");
    let root = temp.path().join("project");
    write_clean_pkg(&root);
    fs::write(root.join("build.rs"), r#"fn main() {
        println!("cargo:rerun-if-env-changed=PROOF_VALUE");
        let value = std::env::var("PROOF_VALUE").expect("fixture sets PROOF_VALUE");
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
        std::fs::write(out.join("generated.rs"), format!("pub const VALUE: &str = {value:?};")).expect("write generated fixture");
    }"#).expect("write fixture file");
    fs::write(
        root.join("src/lib.rs"),
        "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));",
    )
    .expect("write fixture file");
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"{}\", clean::VALUE); }",
    )
    .expect("write fixture file");
    let shim_home = temp.path().join("shim");
    let home = shim_home.join("store");
    let marker = temp.path().join("paused");
    let release = temp.path().join("release");
    let wrapper = wrapper(temp.path());
    let mut command = artificer(&home, &root);
    command
        .arg("run")
        .env("RUSTC_WRAPPER", &wrapper)
        .env("PROOF_VALUE", "first")
        .env("PROOF_PAUSE", "1")
        .env("PROOF_MARKER", &marker)
        .env("PROOF_RELEASE", &release);
    let first = Paused::start(command, &marker, &release);
    let second = shim(&shim_home, &root)
        .arg("run")
        .env("ARTIFICER_NOSERVE", "1")
        .env("RUSTC_WRAPPER", &wrapper)
        .env("PROOF_VALUE", "second")
        .env("PROOF_PAUSE", "0")
        .output()
        .expect("run fixture command");
    let first = first.finish();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&second.stdout).trim(), "second");
    assert_eq!(
        String::from_utf8_lossy(&first.stdout).trim(),
        "first",
        "another build replaced the generated source"
    );
}
