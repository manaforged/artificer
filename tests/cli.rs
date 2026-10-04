use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

#[path = "cli/release.rs"]
mod release;

#[path = "cli/adversarial.rs"]
mod adversarial;

#[path = "cli/ergonomics.rs"]
mod ergonomics;

#[cfg(unix)]
#[path = "cli/lifetime.rs"]
mod lifetime;

#[cfg(target_os = "linux")]
#[path = "cli/concurrency.rs"]
mod concurrency;

fn artificer(home: &Path, dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_artificer"));
    cmd.env("ARTIFICER_HOME", home)
        .env("ARTIFICER_NOSERVE", "1")
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(dir);
    cmd
}

fn stock(dir: &Path) -> Command {
    static CARGO: OnceLock<PathBuf> = OnceLock::new();
    let cargo = CARGO.get_or_init(|| {
        let output = Command::new("rustup")
            .args(["which", "cargo"])
            .current_dir(dir)
            .output()
            .expect("run rustup which cargo");
        assert!(
            output.status.success(),
            "rustup could not resolve Cargo: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        PathBuf::from(
            String::from_utf8(output.stdout)
                .expect("Cargo path is UTF-8")
                .trim(),
        )
    });
    let mut cmd = Command::new(cargo);
    cmd.current_dir(dir);
    cmd
}

fn shim_binary() -> &'static Path {
    static SHIM: OnceLock<PathBuf> = OnceLock::new();
    SHIM.get_or_init(|| {
        let dir =
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("shim-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create shim directory");
        let binary = dir.join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
        if !binary.is_file() && fs::hard_link(env!("CARGO_BIN_EXE_artificer"), &binary).is_err() {
            fs::copy(env!("CARGO_BIN_EXE_artificer"), &binary).expect("copy cargo shim");
        }
        binary
    })
}

fn shim(home: &Path, dir: &Path) -> Command {
    fs::create_dir_all(home).expect("create shim home");
    let mut cmd = Command::new(shim_binary());
    cmd.env("ARTIFICER_REAL_CARGO", stock(dir).get_program())
        .env("ARTIFICER_HOME", home.join("store"))
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(dir);
    cmd
}

fn write_pkg(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"filters\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "#[test] fn green() {}\n#[test] fn red() { panic!(\"boom\"); }\n",
    )
    .unwrap();
}

fn write_clean_pkg(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"clean\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "#[test]\nfn green() {}\n").unwrap();
}

fn write_workspace(root: &Path, members: &[(&str, &str)]) {
    let names: Vec<String> = members
        .iter()
        .map(|(name, _)| format!("\"crates/{name}\""))
        .collect();
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\nresolver = \"2\"\nmembers = [{}]\n",
            names.join(", ")
        ),
    )
    .unwrap();
    for (name, source) in members {
        let dir = root.join("crates").join(name);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        fs::write(dir.join("src/lib.rs"), source).unwrap();
    }
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

#[path = "cli/dispatch.rs"]
mod dispatch;

#[path = "cli/profile.rs"]
mod profile;

#[path = "cli/harness.rs"]
mod harness;

#[cfg(unix)]
#[path = "cli/limits.rs"]
mod limits;

#[path = "cli/shim.rs"]
mod shim_dispatch;

#[path = "cli/cache.rs"]
mod cache;

#[path = "cli/selection.rs"]
mod selection;

#[path = "cli/fallback.rs"]
mod fallback;

#[path = "cli/output.rs"]
mod output;

#[path = "cli/targets.rs"]
mod targets;

#[path = "cli/store_growth.rs"]
mod store_growth;

#[path = "cli/edit_loop.rs"]
mod edit_loop;

#[path = "cli/sharing.rs"]
mod sharing;

#[path = "cli/remote.rs"]
mod remote;

#[path = "cli/coverage.rs"]
mod coverage;

#[path = "cli/scripts.rs"]
mod scripts;
