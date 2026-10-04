use crate::action::{Action, Key, Kind};
use crate::cargo::Package;
use anyhow::{Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

const STAMP_FILE: &str = "stamp";

pub struct Script {
    pub(crate) lease: std::sync::Arc<std::fs::File>,
    pub out_dir: PathBuf,
    pub output: String,
    pub stamp: String,
    pub restored: bool,
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
    let source_key = sess.source_key(pkg)?;
    let job = Job {
        pkg,
        settings,
        script,
        features,
        externs,
        search,
        dep_env,
        opt_level: settings.profile_value(pkg, "opt-level").unwrap_or("0"),
        debug: settings
            .profile_value(pkg, "debuginfo")
            .is_some_and(|v| v != "0" && v != "none"),
        source_key: &source_key,
    };
    let digest = job.digest()?;
    if std::env::var("ARTIFICER_DEBUG_KEY").is_ok_and(|w| w == pkg.name) {
        eprintln!(
            "SCRIPT {} digest={digest} feats={features:?} externs={}",
            pkg.name,
            externs.len(),
        );
    }
    let action = Action::begin(&settings.home, Kind::Script, &digest)?;
    let out_dir = action.out.clone();
    let bin_dir = action.slot.dir.join("bin");
    let mut rustc_cmd = job.rustc_cmd(&bin_dir);
    if let Some(recorded) = restorable(&action, &job, &rustc_cmd) {
        let stamp = fs::read_to_string(action.slot.dir.join(STAMP_FILE))
            .unwrap_or_else(|_| stamp_of(&settings.home, &recorded, &out_dir));
        return Ok(Script {
            lease: action.lease()?,
            out_dir,
            output: crate::inputs::concrete(&settings.home, &recorded),
            stamp,
            restored: true,
        });
    }

    action.invalidate()?;
    fs::create_dir_all(&out_dir)?;
    fs::create_dir_all(&bin_dir)?;
    rustc_cmd.arg(&script.src_path);
    crate::invoke::run_rustc(&mut rustc_cmd, sess, pkg, script, &bin_dir)?;
    let output = job.run(&bin_dir, &out_dir)?;
    let recorded = crate::inputs::portable(&settings.home, &output);
    let stamp = stamp_of(&settings.home, &recorded, &out_dir);
    record(&action, &job, &recorded, &stamp)?;
    Ok(Script {
        lease: action.lease()?,
        out_dir,
        output,
        stamp,
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
    Ok(())
}

fn restorable(action: &Action, job: &Job, rustc_cmd: &std::process::Command) -> Option<String> {
    let pkg = job.pkg;
    let dir = &action.slot.dir;
    if !action.hit()
        || !crate::inputs::matches(&job.settings.home, &dir.join("bin"), pkg.root(), rustc_cmd)
    {
        return None;
    }
    let recorded = fs::read_to_string(dir.join("output")).ok()?;
    let stamp = fs::read_to_string(dir.join("script-inputs")).ok()?;
    let settings = job.settings;
    let current = input_stamp(pkg, settings, &recorded, &action.out);
    (stamp == current).then_some(recorded)
}

mod job;
use job::Job;

mod directives;
use directives::watch;
pub use directives::{
    check_cfgs, link_args, link_libs, link_search, metadata, rustc_cfgs, rustc_envs, rustc_flags,
};

use directives::input_stamp;
