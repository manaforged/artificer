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
}

pub fn compile_pkg(sess: &Session, meta: &cargo::Metadata, id: &str) -> Result<Option<Compiled>> {
    let pkg = cargo::package(meta, id)?;
    let node = cargo::node(meta, id)?;
    let Some(lib) = pkg.lib_target().or_else(|| pkg.bin_target()) else {
        return Ok(None);
    };
    let features = &node.features;
    let crate_name = lib.name.replace('-', "_");
    let proc_macro = pkg.is_proc_macro();
    let is_bin = lib.kind.iter().any(|k| k == "bin");
    let kind = match (sess.needs_link(pkg), is_bin) {
        (false, false) => "meta",
        (false, true) => "meta-bin",
        (true, true) => "bin",
        (true, false) => "lib",
    };
    let types = if kind == "lib" {
        artifact::link_types(lib, proc_macro)
    } else {
        Vec::new()
    };
    let lto_ok = kind == "bin"
        || (!types.is_empty()
            && types
                .iter()
                .all(|t| matches!(t.as_str(), "cdylib" | "staticlib" | "dylib")));
    let script = ensure_script(sess, pkg, node)?;
    let stamp = script.as_ref().map(Script::stamp);
    let digest = unit_key::unit_digest(
        sess,
        pkg,
        node,
        kind,
        features,
        &types,
        lto_ok,
        stamp.as_deref(),
        false,
    )?;
    let action = action::Action::begin(&sess.settings.home, action::Kind::Unit, &digest)?;
    let slot = &action.slot;
    let out = action.out.clone();

    let script_outcome = match &script {
        None => ScriptOutcome::None,
        Some(s) if s.restored => ScriptOutcome::Restored,
        Some(_) => ScriptOutcome::Ran,
    };
    let mut cmd = sess.settings.rustc_cmd(pkg);
    if sess.needs_link(pkg) {
        cmd.arg("--emit=dep-info,metadata,link");
        for kind in artifact::link_types(lib, proc_macro) {
            cmd.arg("--crate-type").arg(kind);
        }
        if proc_macro {
            cmd.arg("-C").arg("prefer-dynamic");
        }
    } else {
        cmd.arg("--emit=dep-info,metadata");
        cmd.arg("--crate-type")
            .arg(if lib.kind.iter().any(|kind| kind == "bin") {
                "bin"
            } else {
                "lib"
            });
    }
    let mut cmd = invoke::rustc_base(
        cmd,
        sess,
        pkg,
        lib,
        sess.needs_link(pkg),
        features,
        &out,
        script.as_ref(),
        lto_ok,
    );
    cmd.arg("-C").arg(format!("metadata={digest}"));
    cmd.arg("-C").arg(format!("extra-filename=-{digest}"));
    invoke::add_externs(
        &mut cmd,
        sess,
        node,
        invoke::ExternSet::Lib,
        kind.starts_with("meta"),
    )?;
    if kind == "bin" || proc_macro {
        invoke::add_natives(&mut cmd, sess);
    }
    if proc_macro {
        cmd.arg("--extern").arg("proc_macro");
    }
    cmd.arg(&lib.src_path);
    invoke::primary_env(&mut cmd, sess, pkg);
    let manifest = unit_key::dep_manifest(sess, node, kind.starts_with("test-"), None)?;
    let mut hit = slot.hit();
    if hit
        && (!unit_key::deps_match(&out, &manifest)
            || !crate::inputs::matches(&out, pkg.root(), &cmd))
    {
        action.invalidate()?;
        hit = false;
    }
    let rustc = if hit {
        invoke::replay(sess, pkg, lib, &out);
        RustcOutcome::Restored
    } else {
        action.prepare()?;
        std::fs::create_dir_all(&out)?;
        invoke::note_rustc(&sess.settings.home);
        invoke::run_rustc(&mut cmd, sess, pkg, lib, &out)?;
        std::fs::write(out.join(unit_key::DEPS_FILE), &manifest)?;
        crate::out::timed(&format!("publish {}", pkg.name), || action.finish())?;
        RustcOutcome::Ran
    };

    let path = artifact::find_artifact(&out, &crate_name, proc_macro, &digest, kind == "meta")?;
    let rmeta = match path.extension().and_then(|e| e.to_str()) {
        Some("rlib") => {
            let sibling = path.with_extension("rmeta");
            sibling.is_file().then_some(sibling)
        }
        _ => None,
    };
    let art = Artifact {
        crate_name,
        path,
        rmeta,
        proc_macro,
    };
    sess.put(id.to_string(), art.clone());
    sess.retain(action.lease()?);
    let shipped = if sess.ship.contains(&pkg.id) {
        ship_outputs(sess, pkg, node, lib, &out, &digest, &art)?
    } else {
        Vec::new()
    };
    Ok(Some(Compiled {
        _leases: std::sync::Arc::clone(&sess.leases),
        artifact: art,
        rustc,
        script: script_outcome,
        shipped,
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
    crate::out::timed(&format!("script {}", pkg.name), || {
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
        Ok(Some(script))
    })
}

mod extras;
pub use extras::{check_extras, doctest_cmd};

mod executables;
pub use executables::compile_example;
use executables::{compile_bin, ship_outputs};

mod testing;
pub use testing::{TestBin, TestSel, compile_tests};
