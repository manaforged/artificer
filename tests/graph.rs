use std::fs;
use std::path::Path;
use std::process::Command;

fn write(root: &Path, toml: &str, lib: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"target\"\n",
    )
    .unwrap();
    fs::write(root.join("Cargo.toml"), toml).unwrap();
    fs::write(root.join("src/lib.rs"), lib).unwrap();
    let st = Command::new("cargo")
        .args(["generate-lockfile", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .status()
        .unwrap();
    assert!(st.success());
}

fn target_dir(root: &Path) -> std::path::PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), Into::into)
}

fn find_file(root: &Path, name: &str) -> Option<std::path::PathBuf> {
    for entry in fs::read_dir(root).ok()? {
        let path = entry.ok()?.path();
        if path.is_dir() {
            if let Some(p) = find_file(&path, name) {
                return Some(p);
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
    }
    None
}

#[path = "graph/dependencies.rs"]
mod dependencies;

#[path = "graph/targets.rs"]
mod targets;

#[path = "graph/features.rs"]
mod features;
