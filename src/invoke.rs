use crate::cargo::{self, Package};
use crate::platform::env_path;
use crate::script::{self, Script};
use crate::session::{Artifact, Session};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn style(sess: &Session, cmd: &mut Command, link: bool, pkg: &Package, lto_ok: bool) {
    let settings = &sess.settings;
    cmd.env_remove("RUSTFLAGS");
    cmd.env_remove("CARGO_ENCODED_RUSTFLAGS");
    if !settings.lto || sess.build_only.contains(&pkg.id) {
        cmd.arg("-C").arg("embed-bitcode=no");
    }
    for a in sess.profile_args(pkg, lto_ok) {
        cmd.arg(a);
    }
    for a in settings
        .overrides
        .for_package(&pkg.name, pkg.source.is_some())
    {
        cmd.arg(a);
    }
    for a in settings.lints(pkg).iter() {
        cmd.arg(a);
    }
    for a in &settings.threads {
        cmd.arg(a);
    }
    for a in crate::unit_key::trusted_args(sess, pkg) {
        cmd.arg(a);
    }
    for a in crate::unit_key::early_args(sess) {
        cmd.arg(a);
    }
    for a in check_cfg_args(pkg) {
        cmd.arg("--check-cfg").arg(a);
    }
    for a in &settings.rustflags {
        cmd.arg(a);
    }
    if link {
        for a in &settings.host_linker {
            cmd.arg(a);
        }
    }
}

pub(crate) fn check_cfg_args(pkg: &Package) -> Vec<String> {
    let mut args = vec!["cfg(docsrs,test)".to_string()];
    let names: Vec<String> = pkg.declared.keys().map(|f| format!("\"{f}\"")).collect();
    args.push(if names.is_empty() {
        "cfg(feature, values())".to_string()
    } else {
        format!("cfg(feature, values({}))", names.join(", "))
    });
    args
}

#[expect(
    clippy::too_many_arguments,
    reason = "the rustc command must receive every independent Cargo unit input"
)]
pub(crate) fn rustc_base(
    mut cmd: Command,
    sess: &Session,
    pkg: &Package,
    target: &cargo::Target,
    link: bool,
    features: &[String],
    out: &Path,
    script: Option<&Script>,
    lto_ok: bool,
) -> Command {
    let crate_name = target.name.replace('-', "_");
    cmd.args(["--crate-name", &crate_name, "--edition", &target.edition]);
    style(sess, &mut cmd, link, pkg, lto_ok);
    let mut remaps = [
        (pkg.root().to_path_buf(), "."),
        (env_path(&sess.settings.home), crate::inputs::STORE_TOKEN),
    ];
    remaps.sort_by_key(|(from, _)| from.as_os_str().len());
    for (from, to) in remaps {
        cmd.arg("--remap-path-prefix")
            .arg(format!("{}={to}", from.display()));
    }
    cmd.arg("--out-dir").arg(out);
    if let Some(dir) = sess.settings.incremental_dir(pkg) {
        cmd.arg("-C").arg(format!("incremental={}", dir.display()));
    }
    if pkg.source.is_some() {
        cmd.arg("--cap-lints").arg("allow");
    }
    for feat in features {
        cmd.arg("--cfg").arg(format!("feature=\"{feat}\""));
    }
    cmd.env("CARGO_CRATE_NAME", crate_name);
    cargo::set_package_env(&mut cmd, pkg);
    if let Some(s) = script {
        apply_script(&mut cmd, s);
    }
    cmd
}

pub(crate) fn apply_script(cmd: &mut Command, s: &Script) {
    cmd.env("OUT_DIR", env_path(&s.out_dir));
    for cfg in script::rustc_cfgs(&s.output) {
        cmd.arg("--cfg").arg(cfg);
    }
    for (k, v) in script::rustc_envs(&s.output) {
        cmd.env(k, v);
    }
    for lib in script::link_libs(&s.output) {
        cmd.arg("-l").arg(lib);
    }
    for search in script::link_search(&s.output) {
        cmd.arg("-L").arg(search);
    }
    for arg in script::link_args(&s.output) {
        cmd.arg("-C").arg(format!("link-arg={arg}"));
    }
    for cfg in script::check_cfgs(&s.output) {
        cmd.arg("--check-cfg").arg(cfg);
    }
    let mut flags = script::rustc_flags(&s.output).into_iter();
    while let (Some(flag), Some(value)) = (flags.next(), flags.next()) {
        cmd.arg(flag).arg(value);
    }
}

pub(crate) fn add_natives(cmd: &mut Command, sess: &Session, root: &str, dev: bool) {
    let linked = sess.link_set(root, dev);
    let natives = sess.natives.lock().expect("natives");
    let mut owners: Vec<&String> = natives.keys().filter(|id| linked.contains(*id)).collect();
    owners.sort();
    let mut seen = std::collections::HashSet::new();
    for out in owners.into_iter().filter_map(|id| natives.get(id)) {
        for search in script::link_search(out) {
            if seen.insert(search.clone()) {
                cmd.arg("-L").arg(search);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ExternSet {
    Lib,
    Test,
}

pub(crate) const EARLY_DIR: &str = "early";

pub(crate) fn early_rmeta(art: &Artifact) -> Option<PathBuf> {
    let name = art.path.with_extension("rmeta");
    let file = art.path.parent()?.join(EARLY_DIR).join(name.file_name()?);
    file.is_file().then_some(file)
}

pub(crate) fn meta_file(art: &Artifact, early: bool) -> PathBuf {
    early
        .then(|| early_rmeta(art))
        .flatten()
        .unwrap_or_else(|| art.rmeta.clone().unwrap_or_else(|| art.path.clone()))
}

pub(crate) fn add_externs(
    cmd: &mut Command,
    sess: &Session,
    node: &cargo::Node,
    set: ExternSet,
    prefer_meta: bool,
) -> Result<()> {
    let arts = sess
        .artifacts
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut dirs: Vec<&Path> = arts.values().filter_map(|a| a.path.parent()).collect();
    dirs.sort_unstable();
    dirs.dedup();
    let early = crate::unit_key::early_consumer(sess, &node.id);
    for dir in dirs {
        cmd.arg("-L").arg(format!("dependency={}", dir.display()));
        let early_dir = dir.join(EARLY_DIR);
        if early && early_dir.is_dir() {
            cmd.arg("-L")
                .arg(format!("dependency={}", early_dir.display()));
        }
    }
    for d in &node.deps {
        let ok = match set {
            ExternSet::Lib => d.usable_for_lib(),
            ExternSet::Test => d.usable_for_lib() || d.usable_for_dev(),
        };
        if !ok {
            continue;
        }
        let Some(art) = arts.get(&d.pkg) else {
            bail!("missing artifact for {}", d.pkg);
        };
        let path = if prefer_meta {
            meta_file(art, early)
        } else {
            art.path.clone()
        };
        cmd.arg("--extern")
            .arg(format!("{}={}", d.name, path.display()));
    }
    Ok(())
}

pub(crate) fn search_dirs(sess: &Session) -> Vec<PathBuf> {
    let arts = sess
        .artifacts
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut dirs: Vec<_> = arts
        .values()
        .filter_map(|art| art.path.parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

pub(crate) fn script_externs(sess: &Session, node: &cargo::Node) -> Result<Vec<(String, PathBuf)>> {
    let arts = sess
        .artifacts
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut v = Vec::new();
    for d in &node.deps {
        if !d.usable_for_script() {
            continue;
        }
        let Some(art) = arts.get(&d.pkg) else {
            bail!("missing build-dep artifact for {}", d.pkg);
        };
        v.push((d.name.clone(), art.path.clone()));
    }
    Ok(v)
}

pub(crate) fn uses_target_tmpdir(target: &cargo::Target) -> bool {
    target
        .kind
        .iter()
        .any(|kind| matches!(kind.as_str(), "test" | "bench"))
}

pub(crate) fn set_target_tmpdir(cmd: &mut Command, sess: &Session, enabled: bool) {
    if let (true, Some(path)) = (enabled, &sess.target_tmpdir) {
        cmd.env("CARGO_TARGET_TMPDIR", env_path(path));
    } else {
        cmd.env_remove("CARGO_TARGET_TMPDIR");
    }
}

mod diagnostics;
mod staging;
pub(crate) use diagnostics::{Early, note_rustc, primary_env, replay, run_rustc};
pub(crate) use staging::try_claim;

#[cfg(test)]
#[path = "invoke_tests.rs"]
mod tests;
