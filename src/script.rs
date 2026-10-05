use crate::action::{Action, Key, Kind};
use crate::cargo::Package;
use anyhow::{Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

const STAMP_FILE: &str = "stamp";
const TREE_FILE: &str = "script-tree";

#[derive(Clone)]
pub struct Script {
    pub(crate) lease: std::sync::Arc<std::fs::File>,
    pub out_dir: PathBuf,
    pub output: String,
    pub stamp: String,
    pub restored: bool,
}

fn identity(home: &Path, stamp: &str, out_dir: &Path) -> String {
    let mut key = Key::new();
    key.feed(stamp.as_bytes());
    key.feed_str(&crate::inputs::portable(home, &out_dir.to_string_lossy()));
    key.full_digest()
}

fn stamp_of(home: &Path, recorded: &str, out_dir: &Path) -> String {
    let mut key = Key::new();
    key.feed(recorded.as_bytes());
    watch(&mut key, home, out_dir);
    key.full_digest()
}

pub fn ensure(
    pkg: &Package,
    sess: &crate::session::Session,
    features: &[String],
    externs: &[(String, PathBuf)],
    search: &[PathBuf],
    dep_env: &[(String, String)],
) -> Result<Script> {
    let settings = &sess.settings;
    let Some(script) = pkg.script_target() else {
        bail!("ensure called without custom-build target");
    };
    let source_key = sess.source_key(pkg, None)?;
    let job = Job {
        pkg,
        settings,
        script,
        features,
        externs,
        search,
        dep_env,
        opt_level: sess.profile_value(pkg, "opt-level").unwrap_or("0"),
        debug: sess
            .profile_value(pkg, "debuginfo")
            .is_some_and(|v| v != "0" && v != "none"),
        source_key: &source_key,
    };
    let bin_key = job.bin_digest()?;
    let bin_unit = format!("{}{bin_key}", Kind::ScriptBin.prefix());
    let digest = job.digest(&bin_unit)?;
    debug_key(pkg, &digest, features, externs.len());
    let action = Action::begin(&settings.home, Kind::Script, &digest)?;
    let out_dir = action.out.clone();
    let bin_dir = action.slot.dir.join("bin");
    if let Some(recorded) = restorable(&action, &job, &bin_unit) {
        let stamp = fs::read_to_string(action.slot.dir.join(STAMP_FILE))
            .unwrap_or_else(|_| stamp_of(&settings.home, &recorded, &out_dir));
        return Ok(Script {
            lease: action.lease()?,
            stamp: identity(&settings.home, &stamp, &out_dir),
            out_dir,
            output: crate::inputs::concrete(&settings.home, &recorded),
            restored: true,
        });
    }

    action.invalidate()?;
    fs::create_dir_all(&out_dir)?;
    let (bin_lease, compiled) = compile(sess, &job, &bin_key)?;
    crate::artifact::place_exe(&compiled.join(job::bin_file()), &bin_dir, &job::bin_file())?;
    sess.retain(bin_lease);
    let output = job.run(&bin_dir, &out_dir)?;
    let recorded = crate::inputs::portable(&settings.home, &output);
    let stamp = stamp_of(&settings.home, &recorded, &out_dir);
    record(&action, &job, &recorded, &stamp)?;
    Ok(Script {
        lease: action.lease()?,
        stamp: identity(&settings.home, &stamp, &out_dir),
        out_dir,
        output,
        restored: false,
    })
}

fn record(action: &Action, job: &Job, recorded: &str, stamp: &str) -> Result<()> {
    let dir = &action.slot.dir;
    fs::write(dir.join("output"), recorded)?;
    fs::write(dir.join(STAMP_FILE), stamp)?;
    action.finish()?;
    let settings = job.settings;
    let stamp = input_stamp(job.pkg, settings, recorded, &action.out);
    fs::write(dir.join("script-inputs"), stamp)?;
    fs::write(dir.join(TREE_FILE), job.source_key)?;
    Ok(())
}

fn compile(
    sess: &crate::session::Session,
    job: &Job,
    bin_key: &str,
) -> Result<(std::sync::Arc<std::fs::File>, PathBuf)> {
    let (pkg, home) = (job.pkg, &job.settings.home);
    let action = Action::begin(home, Kind::ScriptBin, bin_key)?;
    let bin_dir = action.out.clone();
    let mut rustc_cmd = job.rustc_cmd(&bin_dir);
    if action.hit() && crate::inputs::matches(home, &bin_dir, pkg.root(), &rustc_cmd) {
        return Ok((action.lease()?, bin_dir));
    }
    action.invalidate()?;
    fs::create_dir_all(&bin_dir)?;
    rustc_cmd.arg(&job.script.src_path);
    crate::invoke::run_rustc(&mut rustc_cmd, sess, pkg, job.script, &bin_dir, None)?;
    action.finish()?;
    Ok((action.lease()?, bin_dir))
}

fn restorable(action: &Action, job: &Job, bin_unit: &str) -> Option<String> {
    let pkg = job.pkg;
    let dir = &action.slot.dir;
    if !action.hit() {
        return None;
    }
    let home = &job.settings.home;
    let compiled = crate::store::Slot::new(home, bin_unit).out_dir();
    if !crate::inputs::matches(home, &compiled, pkg.root(), &job.rustc_cmd(&compiled)) {
        return None;
    }
    let recorded = fs::read_to_string(dir.join("output")).ok()?;
    let stamp = fs::read_to_string(dir.join("script-inputs")).ok()?;
    let settings = job.settings;
    let current = input_stamp(pkg, settings, &recorded, &action.out);
    let tree = directives::watches(&recorded)
        || fs::read_to_string(dir.join(TREE_FILE)).is_ok_and(|tree| tree == job.source_key);
    (stamp == current && tree).then_some(recorded)
}

mod job;
use job::Job;

mod directives;
use directives::watch;
pub use directives::{
    check_cfgs, link_args, link_libs, link_search, metadata, rustc_cfgs, rustc_envs, rustc_flags,
};

use directives::input_stamp;

fn debug_key(pkg: &Package, digest: &str, features: &[String], externs: usize) {
    if std::env::var("ARTIFICER_DEBUG_KEY").is_ok_and(|w| w == pkg.name) {
        crate::out::diag(format!(
            "SCRIPT {} digest={digest} feats={features:?} externs={externs}",
            pkg.name,
        ));
    }
}
