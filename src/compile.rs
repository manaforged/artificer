use crate::action;
use crate::artifact;
use crate::cargo::{self, Package};
use crate::invoke;
use crate::script::{self, Script};
use crate::session::{Artifact, Session};
use crate::settings;
use crate::unit_key;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptOutcome {
    None,
    Ran,
    Restored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustcOutcome {
    Ran,
    Restored,
}

pub struct Compiled {
    _leases: crate::session::Leases,
    pub artifact: Artifact,
    pub rustc: RustcOutcome,
    pub script: ScriptOutcome,
    pub shipped: Vec<(String, PathBuf)>,
    pub default_run: Option<String>,
}

pub(crate) fn waits_for_link(sess: &Session, pkg: &Package) -> bool {
    package::PackageUnit::new(sess, pkg).is_none_or(|unit| !unit.pipelines())
}

pub fn run_script(sess: &Session, meta: &cargo::Metadata, id: &str) -> Result<()> {
    let pkg = cargo::package(meta, id)?;
    let node = cargo::node(meta, id)?;
    ensure_script(sess, pkg, node).map(drop)
}

fn script_outcome(script: Option<&Script>) -> ScriptOutcome {
    match script {
        None => ScriptOutcome::None,
        Some(s) if s.restored => ScriptOutcome::Restored,
        Some(_) => ScriptOutcome::Ran,
    }
}

pub fn compile_pkg(sess: &Session, meta: &cargo::Metadata, id: &str) -> Result<Option<Compiled>> {
    let pkg = cargo::package(meta, id)?;
    let node = cargo::node(meta, id)?;
    let Some(unit) = package::PackageUnit::new(sess, pkg) else {
        return Ok(None);
    };
    let script = ensure_script(sess, pkg, node)?;
    let stamp = script.as_ref().map(|s| s.stamp.clone());
    let keyed = unit_key::unit_digest(
        sess,
        pkg,
        node,
        unit.shape.name(),
        &node.features,
        &unit.types,
        unit.lto_ok,
        stamp.as_deref(),
        false,
    )?;
    let action = action::Action::begin(&sess.settings.home, action::Kind::Unit, &keyed.digest)?
        .lineage(keyed.lineage.clone());
    let out = action.out.clone();
    let mut cmd = unit.command(node, &out, script.as_ref(), &keyed)?;
    let manifest = unit_key::dep_manifest(sess, node, false, None)?;
    let early = unit.early(&out, &keyed.digest);
    let rustc = unit.rustc(&action, &mut cmd, &manifest, early.as_ref())?;
    let art = unit.artifact(&out, &keyed.digest)?;
    sess.put(id.to_string(), art.clone());
    sess.retain(action.lease()?);
    let shipped = if sess.ship.contains(&pkg.id) {
        ship_outputs(sess, pkg, node, unit.lib, &out, &keyed.digest, &art)?
    } else {
        Vec::new()
    };
    Ok(Some(Compiled {
        _leases: std::sync::Arc::clone(&sess.leases),
        artifact: art,
        rustc,
        script: script_outcome(script.as_ref()),
        shipped,
        default_run: pkg.default_run.clone(),
    }))
}

fn dep_env(sess: &Session, node: &cargo::Node) -> Vec<(String, String)> {
    let published = sess.published.lock().expect("published");
    let mut out: Vec<(String, String)> = node
        .deps
        .iter()
        .filter(|d| d.usable_for_lib() || d.usable_for_script())
        .filter_map(|d| published.get(&d.pkg))
        .flat_map(|vars| vars.iter().cloned())
        .collect();
    drop(published);
    out.sort();
    out.dedup();
    out
}

fn ensure_script(sess: &Session, pkg: &Package, node: &cargo::Node) -> Result<Option<Script>> {
    if pkg.script_target().is_none() {
        return Ok(None);
    }
    let cached = sess
        .scripts
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&pkg.id)
        .cloned();
    if let Some(script) = cached {
        return Ok(Some(script));
    }
    let script = run_script_now(sess, pkg, node)?;
    sess.scripts
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(pkg.id.clone(), script.clone());
    Ok(Some(script))
}

fn run_script_now(sess: &Session, pkg: &Package, node: &cargo::Node) -> Result<Script> {
    crate::profile::span(crate::profile::WrapperPhase::Script, || {
        let script = script::ensure(
            pkg,
            sess,
            &node.features,
            &invoke::script_externs(sess, node)?,
            &invoke::search_dirs(sess),
            &dep_env(sess, node),
        )?;
        sess.retain(std::sync::Arc::clone(&script.lease));
        if let Some(links) = &pkg.links {
            let prefix = links.to_uppercase().replace('-', "_");
            let vars = script::metadata(&script.output)
                .into_iter()
                .map(|(k, v)| {
                    (
                        format!("DEP_{prefix}_{}", k.to_uppercase().replace('-', "_")),
                        v,
                    )
                })
                .collect();
            sess.published
                .lock()
                .expect("published")
                .insert(pkg.id.clone(), vars);
        }
        sess.natives
            .lock()
            .expect("natives")
            .insert(pkg.id.clone(), script.output.clone());
        Ok(script)
    })
}

mod extras;
mod package;
pub use extras::{check_extras, doctest_cmd};

mod executables;
pub use executables::compile_example;
use executables::{compile_bin, ship_outputs};

mod testing;
pub use testing::{TestBin, TestSel, compile_tests};
