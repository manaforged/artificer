use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SKIP: &[&str] = &[
    "target",
    ".git",
    ".jj",
    "Cargo.lock",
    "node_modules",
    ".scratch",
    "__pycache__",
    ".venv",
];

#[derive(Debug)]
pub(crate) struct Source {
    pub(crate) key: String,
    env_src: Vec<String>,
    env_all: Vec<String>,
}

impl Source {
    pub(crate) fn env(&self, tests: bool) -> &[String] {
        if tests { &self.env_all } else { &self.env_src }
    }
}

pub(crate) fn lib(
    home: Option<&Path>,
    pkg: &Path,
    rustc: &str,
    name: &str,
    edition: &str,
    skip: &[&Path],
) -> Result<Source> {
    let files = collect(pkg, pkg, skip)?;
    let src_only = pkg.join("src").is_dir();
    let mut memo = crate::digest::Memo::new(home);
    let mut hasher = blake3::Hasher::new();
    let (mut env_src, mut env_all) = (Vec::new(), Vec::new());
    feed(&mut hasher, rustc.as_bytes());
    feed(&mut hasher, name.as_bytes());
    feed(&mut hasher, edition.as_bytes());
    for f in &files {
        hasher.update(&(f.rel.len() as u64).to_le_bytes());
        hasher.update(f.rel.as_bytes());
        match &f.data {
            Data::File { canonical, meta } => {
                let scan = memo.known(canonical, meta)?;
                hasher.update(b"file");
                feed(&mut hasher, scan.digest.as_bytes());
                if !src_only || Path::new(&f.rel).starts_with("src") {
                    env_src.extend(scan.env.iter().cloned());
                }
                env_all.extend(scan.env);
            }
            Data::Link(target) => {
                hasher.update(b"link");
                feed(&mut hasher, target.to_string_lossy().as_bytes());
            }
        }
    }
    for names in [&mut env_src, &mut env_all] {
        names.sort();
        names.dedup();
    }
    Ok(Source {
        key: hex(&hasher),
        env_src,
        env_all,
    })
}

fn feed(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn hex(hasher: &blake3::Hasher) -> String {
    hasher.finalize().to_hex()[..32].to_string()
}

struct File {
    rel: String,
    data: Data,
}

enum Data {
    File {
        canonical: PathBuf,
        meta: fs::Metadata,
    },
    Link(PathBuf),
}

fn collect(root: &Path, dir: &Path, skip: &[&Path]) -> Result<Vec<File>> {
    let mut out = Vec::new();
    let mut ancestors = Vec::new();
    collect_into(root, dir, skip, &mut ancestors, &mut out)?;
    Ok(out)
}

fn collect_into(
    root: &Path,
    dir: &Path,
    skip: &[&Path],
    ancestors: &mut Vec<PathBuf>,
    out: &mut Vec<File>,
) -> Result<()> {
    let canonical = fs::canonicalize(dir).with_context(|| format!("scan {}", dir.display()))?;
    anyhow::ensure!(
        !ancestors.contains(&canonical),
        "symlink cycle while scanning {}",
        dir.display()
    );
    ancestors.push(canonical.clone());
    let result = (|| {
        let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name();
            if SKIP.iter().any(|s| name == *s) {
                continue;
            }
            let path = entry.path();
            if skip.iter().any(|s| path == *s || path.starts_with(s)) {
                continue;
            }
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel = rel.to_string_lossy().into_owned();
            let ft = entry.file_type()?;
            if ft.is_symlink() {
                out.push(File {
                    rel: rel.clone(),
                    data: Data::Link(fs::read_link(&path)?),
                });
            }
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                collect_into(root, &path, skip, ancestors, out)?;
            } else if meta.is_file() {
                let canonical = if ft.is_symlink() {
                    fs::canonicalize(&path).unwrap_or(path)
                } else {
                    canonical.join(&name)
                };
                out.push(File {
                    rel,
                    data: Data::File { canonical, meta },
                });
            }
        }
        Ok(())
    })();
    ancestors.pop();
    result
}

#[cfg(test)]
#[path = "key_tests.rs"]
mod tests;

pub(crate) fn scan_env(text: &str, out: &mut Vec<String>) {
    let bytes = text.as_bytes();
    let mut at = 0;
    while let Some(found) = text[at..].find("env!") {
        let mut i = at + found + "env!".len();
        at = i;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'(') {
            continue;
        }
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'"') {
            continue;
        }
        i += 1;
        let start = i;
        while bytes.get(i).is_some_and(|b| *b != b'"') {
            i += 1;
        }
        if let Some(name) = text.get(start..i)
            && !name.is_empty()
        {
            out.push(name.to_string());
        }
    }
}

mod compiler;
#[cfg(test)]
pub(crate) use compiler::file_identity;
pub(crate) use compiler::{
    explicit_rustc_identity, probe_memo, rustc_exe, rustc_print_cfg_with_flags, toolchain_key,
};
pub use compiler::{rustc_bin, rustc_host, rustc_print_cfg, rustc_version, rustc_version_in};
