use std::path::Path;
use std::process::Command;

pub fn threads(home: &Path, rustc: &str, dir: &Path) -> Vec<String> {
    match std::env::var("ARTIFICER_THREADS") {
        Ok(s) if s == "off" => return Vec::new(),
        _ => {}
    }
    stamped(home, "threads", rustc, || probe_threads(dir))
}

fn stamp_id(kind: &str, rustc: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(kind.as_bytes());
    h.update(rustc.as_bytes());
    h.update(
        std::env::var_os(crate::profile::BOOTSTRAP.0)
            .unwrap_or_default()
            .as_encoded_bytes(),
    );
    format!("{kind}-{}", &h.finalize().to_hex()[..16])
}

fn stamped(
    home: &Path,
    kind: &str,
    rustc: &str,
    probe: impl FnOnce() -> Vec<String>,
) -> Vec<String> {
    let stamp = home.join(stamp_id(kind, rustc));
    if let Ok(s) = std::fs::read_to_string(&stamp) {
        let s = s.trim();
        if s == "off" || s == "llvm" || s.is_empty() {
            return Vec::new();
        }
        return s.split_whitespace().map(str::to_string).collect();
    }
    let args = probe();
    let line = if args.is_empty() {
        "off".into()
    } else {
        args.join(" ")
    };
    drop(std::fs::create_dir_all(home));
    drop(std::fs::write(&stamp, &line));
    args
}

fn probe_threads(dir: &Path) -> Vec<String> {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let args = vec!["-Z".into(), format!("threads={}", threads_n(cores))];
    if rustc_lib(&args, dir) {
        return args;
    }
    Vec::new()
}

#[derive(Clone, Debug, Default)]
pub struct ForkFlags {
    pub trusted: Vec<String>,
    pub early: Vec<String>,
}

pub fn fork_flags(home: &Path, rustc: &str, dir: &Path, mods: &crate::mods::Mods) -> ForkFlags {
    let probe = |on: bool, kind: &str, flag: &str| {
        if !on {
            return Vec::new();
        }
        stamped(home, kind, rustc, || {
            let args = vec!["-Z".into(), flag.into()];
            if rustc_lib(&args, dir) {
                return args;
            }
            Vec::new()
        })
    };
    ForkFlags {
        trusted: probe(mods.trust, "trusted", "trusted-crate"),
        early: probe(mods.early, "early", "early-metadata"),
    }
}

fn threads_n(cores: usize) -> usize {
    cores.clamp(1, 8)
}

fn rustc_lib(extra: &[String], toolchain_dir: &Path) -> bool {
    let Some(dir) = scratch("lib") else {
        return false;
    };
    let src = dir.join("lib.rs");
    if std::fs::write(&src, "pub fn _n() {}\n").is_err() {
        return false;
    }
    let mut cmd = Command::new(crate::key::rustc_bin());
    cmd.current_dir(toolchain_dir);
    crate::jobs::isolate(&mut cmd);
    cmd.args(extra);
    cmd.args(["--crate-type", "lib", "--edition", "2021", "--out-dir"]);
    cmd.arg(&dir);
    cmd.arg(&src);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
    drop(std::fs::remove_dir_all(&dir));
    ok
}

fn scratch(tag: &str) -> Option<std::path::PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("artificer-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[cfg(test)]
#[path = "flags_tests.rs"]
mod tests;
