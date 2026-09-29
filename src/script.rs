use crate::action::{Action, Key, Kind};
use crate::cargo::Package;
use anyhow::{Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Script {
    pub(crate) lease: std::sync::Arc<std::fs::File>,
    pub out_dir: PathBuf,
    pub output: String,
    pub restored: bool,
}

impl Script {
    pub fn stamp(&self) -> String {
        let mut key = Key::new();
        key.feed(self.output.as_bytes());
        watch(&mut key, &self.out_dir);
        key.full_digest()
    }
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
    if let Some(output) = restorable(&action, &job, &rustc_cmd) {
        return Ok(Script {
            lease: action.lease()?,
            out_dir,
            output,
            restored: true,
        });
    }

    action.invalidate()?;
    fs::create_dir_all(&out_dir)?;
    fs::create_dir_all(&bin_dir)?;
    rustc_cmd.arg(&script.src_path);
    crate::invoke::run_rustc(&mut rustc_cmd, sess, pkg, script, &bin_dir)?;
    let output = job.run(&bin_dir, &out_dir)?;
    record(&action, &job, &output)?;
    Ok(Script {
        lease: action.lease()?,
        out_dir,
        output,
        restored: false,
    })
}

fn record(action: &Action, job: &Job, output: &str) -> Result<()> {
    let dir = &action.slot.dir;
    fs::write(dir.join("output"), output)?;
    action.finish()?;
    let stamp = input_stamp(job.pkg, output, &action.out, &job.settings.workspace_root);
    fs::write(dir.join("script-inputs"), stamp)?;
    Ok(())
}

fn restorable(action: &Action, job: &Job, rustc_cmd: &std::process::Command) -> Option<String> {
    let pkg = job.pkg;
    let dir = &action.slot.dir;
    if !action.hit() || !crate::inputs::matches(&dir.join("bin"), pkg.root(), rustc_cmd) {
        return None;
    }
    let output = fs::read_to_string(dir.join("output")).ok()?;
    let stamp = fs::read_to_string(dir.join("script-inputs")).ok()?;
    let current = input_stamp(pkg, &output, &action.out, &job.settings.workspace_root);
    (stamp == current).then_some(output)
}

mod job;
use job::Job;

mod directives;
use directives::watch;
pub use directives::{
    check_cfgs, link_args, link_libs, link_search, metadata, rustc_cfgs, rustc_envs, rustc_flags,
};

use directives::input_stamp;
