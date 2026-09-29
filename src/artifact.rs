use crate::cargo;
use crate::compile::Compiled;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(crate) fn link_types(target: &cargo::Target, proc_macro: bool) -> Vec<String> {
    if proc_macro {
        return vec!["proc-macro".to_string()];
    }
    if target.kind.iter().any(|k| k == "bin") {
        return vec!["bin".to_string()];
    }
    let mut v: Vec<String> = target
        .crate_types
        .iter()
        .map(|t| {
            if t == "lib" {
                "rlib".to_string()
            } else {
                t.clone()
            }
        })
        .collect();
    if v.is_empty() {
        v.push("rlib".to_string());
    }
    v
}

pub fn bin_name(target: &str) -> String {
    if cfg!(windows) {
        format!("{target}.exe")
    } else {
        target.to_string()
    }
}

pub(crate) fn shared_name(crate_name: &str) -> String {
    if cfg!(windows) {
        format!("{crate_name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{crate_name}.dylib")
    } else {
        format!("lib{crate_name}.so")
    }
}

pub(crate) fn static_name(crate_name: &str) -> String {
    if cfg!(windows) {
        format!("{crate_name}.lib")
    } else {
        format!("lib{crate_name}.a")
    }
}

pub fn deliver(
    roots: &[String],
    compiled: &HashMap<String, Compiled>,
    target: &Path,
) -> Result<()> {
    let mut pending: Vec<(PathBuf, PathBuf)> = Vec::new();
    for root in roots {
        let Some(c) = compiled.get(root) else {
            continue;
        };
        for (name, src) in &c.shipped {
            pending.push((src.clone(), target.join(name)));
        }
    }
    if pending.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(target)?;
    for (src, dst) in pending {
        crate::platform::replace_atomic(&dst, |tmp| std::fs::copy(&src, tmp).map(drop))
            .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    }
    Ok(())
}

pub(crate) fn deps_name(src: &Path, crate_name: &str) -> String {
    let hash = blake3::hash(src.as_os_str().as_encoded_bytes()).to_hex();
    format!(
        "{crate_name}-{}{}",
        &hash[..16],
        std::env::consts::EXE_SUFFIX
    )
}

pub(crate) fn place_exe(src: &Path, dir: &Path, file: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let dst = dir.join(file);
    crate::platform::replace_atomic(&dst, |tmp| {
        std::fs::hard_link(src, tmp).or_else(|_| std::fs::copy(src, tmp).map(drop))
    })
    .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    Ok(dst)
}

pub(crate) fn find_artifact(
    out: &Path,
    crate_name: &str,
    proc_macro: bool,
    extra: &str,
    prefer_meta: bool,
) -> Result<PathBuf> {
    if proc_macro {
        for ext in ["dylib", "so", "dll"] {
            let p = out.join(format!("lib{crate_name}-{extra}.{ext}"));
            if p.is_file() {
                return Ok(p);
            }
            let p = out.join(format!("{crate_name}-{extra}.{ext}"));
            if p.is_file() {
                return Ok(p);
            }
        }
        bail!(
            "no proc-macro artifact for {crate_name} in {}",
            out.display()
        );
    }
    let rmeta = out.join(format!("lib{crate_name}-{extra}.rmeta"));
    let rlib = out.join(format!("lib{crate_name}-{extra}.rlib"));
    let bin_meta = out.join(format!("{crate_name}-{extra}.rmeta"));
    if prefer_meta && rmeta.is_file() {
        return Ok(rmeta);
    }
    if prefer_meta && bin_meta.is_file() {
        return Ok(bin_meta);
    }
    if rlib.is_file() {
        return Ok(rlib);
    }
    if let Ok(p) = find_typed(out, crate_name, extra, "cdylib") {
        return Ok(p);
    }
    if let Ok(p) = find_typed(out, crate_name, extra, "staticlib") {
        return Ok(p);
    }
    let exe = out.join(if cfg!(windows) {
        format!("{crate_name}-{extra}.exe")
    } else {
        format!("{crate_name}-{extra}")
    });
    if exe.is_file() {
        return Ok(exe);
    }
    if rmeta.is_file() {
        return Ok(rmeta);
    }
    if bin_meta.is_file() {
        return Ok(bin_meta);
    }
    bail!("no rmeta/rlib for {crate_name} in {}", out.display());
}

pub(crate) fn find_typed(out: &Path, crate_name: &str, extra: &str, kind: &str) -> Result<PathBuf> {
    let names: Vec<String> = if kind == "staticlib" {
        vec![
            format!("lib{crate_name}-{extra}.a"),
            format!("{crate_name}-{extra}.lib"),
        ]
    } else {
        vec![
            format!("lib{crate_name}-{extra}.so"),
            format!("lib{crate_name}-{extra}.dylib"),
            format!("{crate_name}-{extra}.dll"),
            format!("lib{crate_name}-{extra}.dll"),
        ]
    };
    for name in names {
        let p = out.join(name);
        if p.is_file() {
            return Ok(p);
        }
    }
    bail!("no {kind} artifact for {crate_name} in {}", out.display());
}
