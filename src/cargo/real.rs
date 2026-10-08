use super::*;

pub const SHIM_DEPTH: &str = "ARTIFICER_SHIM_DEPTH";

pub(crate) const REAL_CARGO: &str = "ARTIFICER_REAL_CARGO";

#[must_use]
pub fn shim_depth() -> u32 {
    std::env::var(SHIM_DEPTH)
        .ok()
        .and_then(|depth| depth.trim().parse().ok())
        .unwrap_or(0)
}

pub(crate) fn is_self(candidate: &Path, shims: &[PathBuf]) -> bool {
    let resolved = crate::resolve_path(candidate);
    shims
        .iter()
        .any(|shim| crate::resolve_path(shim) == resolved || same_file(candidate, shim))
}

#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(_: &Path, _: &Path) -> bool {
    false
}

fn shims() -> Vec<PathBuf> {
    let mut shims = vec![
        crate::home::control_home()
            .join("bin")
            .join(format!("cargo{}", std::env::consts::EXE_SUFFIX)),
    ];
    if let Ok(exe) = std::env::current_exe() {
        shims.push(exe);
    }
    shims
}

fn candidates() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(p) = std::env::var(REAL_CARGO) {
        found.push(PathBuf::from(p));
    }
    let stamp = dirs_home().join(".artificer").join("real-cargo");
    if let Ok(p) = std::fs::read_to_string(&stamp) {
        found.push(PathBuf::from(p.trim()));
    }
    found.push(rustup_cargo());
    found
}

pub(crate) fn resolve_real(candidates: &[PathBuf], shims: &[PathBuf]) -> Result<PathBuf> {
    candidates
        .iter()
        .find(|p| p.is_file() && !pinned_toolchain_cargo(p) && !is_self(p, shims))
        .cloned()
        .with_context(|| {
            format!(
                "no real Cargo found; every candidate is missing, pinned, or the Artificer shim; set {REAL_CARGO}"
            )
        })
}

pub(crate) fn cargo_bin() -> Result<PathBuf> {
    resolve_real(&candidates(), &shims())
}

pub fn real_cargo_command() -> Result<Command> {
    let real = cargo_bin()?;
    let mut cmd = Command::new(&real);
    cmd.env(REAL_CARGO, &real);
    cmd.env(SHIM_DEPTH, (shim_depth() + 1).to_string());
    Ok(cmd)
}

#[cfg(test)]
#[path = "real_tests.rs"]
mod tests;
