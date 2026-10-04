use crate::compile::{self, RustcOutcome, ScriptOutcome};
use crate::maintenance::gc_daily;
use crate::session::Session;
use crate::{artifact, cargo, config, features, jobs, manifest, mods, schedule, sweep};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug)]
pub struct Report {
    pub script: ScriptOutcome,
    pub rustc: RustcOutcome,
    pub rlib: PathBuf,
}

pub fn check(pkg: &Path, home: &Path) -> Result<Report> {
    check_package(pkg, &[], home)
}

pub fn check_package(dir: &Path, packages: &[String], home: &Path) -> Result<Report> {
    let (root, compiled, _) = check_graph(dir, packages, home, true, &CheckOpts::default())?;
    let last = compiled
        .get(&root)
        .with_context(|| format!("no lib target for {root}"))?;
    Ok(Report {
        script: last.script,
        rustc: last.rustc,
        rlib: last.artifact.path.clone(),
    })
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Targets {
    pub tests: bool,
    pub all: bool,
}

#[derive(Debug, Default, Clone)]
pub struct CheckOpts {
    pub json: bool,
    pub workspace: bool,
    pub all_features: bool,
    pub features: Vec<String>,
    pub no_default: bool,
    pub meta_flags: Vec<String>,
    pub target_dir: Option<PathBuf>,
    pub release: bool,
    pub link: bool,
    pub targets: Targets,
}

pub fn check_cmd(dir: &Path, packages: &[String], home: &Path, opts: CheckOpts) -> Result<i32> {
    let (_, compiled, _) = match check_graph(dir, packages, home, !opts.link, &opts) {
        Ok(v) => v,
        Err(e) => {
            if opts.json {
                crate::out::out(serde_json::json!({"reason":"build-finished","success":false}));
            }
            return Err(e);
        }
    };
    if opts.json {
        let manifest = cargo::find_manifest(dir)?;
        let mut extra = features::feature_args(opts.all_features, &opts.features, opts.no_default);
        extra.extend(opts.meta_flags.iter().cloned());
        let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
        let meta = cargo::metadata_extra(&manifest, &extra, home)?;
        let profile = json_profile(
            cargo::root(&meta, manifest.parent().unwrap_or(dir)),
            if opts.release { "release" } else { "dev" },
            false,
        );
        for (id, c) in &compiled {
            let pkg = cargo::package(&meta, id)?;
            let Some(lib) = pkg.lib_target().or_else(|| pkg.bin_target()) else {
                continue;
            };
            let crate_types = if lib.crate_types.is_empty() {
                vec!["lib".to_string()]
            } else {
                lib.crate_types.clone()
            };
            crate::out::out(serde_json::json!({
                "reason": "compiler-artifact",
                "package_id": pkg.id,
                "manifest_path": pkg.manifest_path,
                "target": {
                    "kind": lib.kind,
                    "crate_types": crate_types,
                    "name": lib.name,
                    "src_path": lib.src_path,
                    "edition": lib.edition,
                    "required-features": lib.required_features,
                    "doc": lib.doc,
                    "doctest": lib.doctest,
                    "test": lib.test
                },
                "profile": profile.clone(),
                "features": cargo::node(&meta, id)?.features,
                "filenames": [c.artifact.path],
                "executable": null,
                "linked": c.artifact.path.extension().is_some_and(|e| e != "rmeta"),
                "fresh": c.rustc == RustcOutcome::Restored
            }));
        }
        crate::out::out(serde_json::json!({"reason":"build-finished","success":true}));
        return Ok(0);
    }
    let hit = compiled
        .values()
        .filter(|c| c.rustc == RustcOutcome::Restored)
        .count();
    let miss = compiled
        .values()
        .filter(|c| c.rustc == RustcOutcome::Ran)
        .count();
    crate::out::err(format!("artificer: {hit} hit, {miss} rustc"));
    Ok(0)
}

fn finished(dir: &Path, name: &str, started: std::time::Instant) {
    let profile = json_profile(dir, name, false);
    let optimized = profile["opt_level"]
        .as_str()
        .is_some_and(|level| level != "0");
    let debuginfo = profile["debuginfo"].as_u64().is_some_and(|level| level > 0);
    let shape = match (optimized, debuginfo) {
        (true, true) => "optimized + debuginfo",
        (true, false) => "optimized",
        (false, true) => "unoptimized + debuginfo",
        (false, false) => "unoptimized",
    };
    crate::out::status(
        crate::out::Status::Finished,
        format!(
            "`{name}` profile [{shape}] target(s) in {}",
            crate::out::elapsed(started.elapsed())
        ),
    );
}

pub(crate) fn check_roots(
    meta: &cargo::Metadata,
    dir: &Path,
    packages: &[String],
    workspace: bool,
) -> Result<Vec<String>> {
    if !packages.is_empty() {
        return packages
            .iter()
            .map(|name| cargo::id_by_name(meta, name))
            .collect();
    }
    if workspace {
        return Ok(meta.workspace_members.clone());
    }
    match cargo::root_id(meta, dir) {
        Ok(id) => Ok(vec![id]),
        Err(_) if !meta.workspace_members.is_empty() => Ok(meta.workspace_members.clone()),
        Err(e) => Err(e),
    }
}

type Graph = (String, HashMap<String, compile::Compiled>, PathBuf);

pub(super) struct Plan {
    meta: cargo::Metadata,
    pkg_dir: PathBuf,
    roots: Vec<String>,
}

pub(super) fn feature_extra(
    all_features: bool,
    features: &[String],
    no_default: bool,
    meta_flags: &[String],
) -> Vec<String> {
    let mut extra = features::feature_args(all_features, features, no_default);
    extra.extend(meta_flags.iter().cloned());
    extra
}

pub(super) fn plan(
    dir: &Path,
    packages: &[String],
    home: &Path,
    extra: &[String],
    workspace: bool,
    dev: bool,
    target: Option<&Path>,
) -> Result<Plan> {
    let manifest = cargo::find_manifest(dir)?;
    let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
    let mut meta = crate::out::timed("metadata", || {
        cargo::metadata_extra(&manifest, &extra, home)
    })?;
    let pkg_dir = manifest.parent().unwrap_or(dir).to_path_buf();
    sweep_meta(&meta, home, &pkg_dir, target)?;
    let roots = check_roots(&meta, &pkg_dir, packages, workspace)?;
    let found = crate::out::timed("probe", || {
        features::selected(&manifest, &roots, &meta, &extra, dev, home)
    });
    let Some(sel) = found else {
        bail!("per-invocation feature resolution is unavailable");
    };
    features::narrow(&mut meta, &sel);
    Ok(Plan {
        meta,
        pkg_dir,
        roots,
    })
}

fn check_graph(
    dir: &Path,
    packages: &[String],
    home: &Path,
    meta_only: bool,
    opts: &CheckOpts,
) -> Result<Graph> {
    jobs::install(home)?;
    let started = std::time::Instant::now();
    let op = if opts.link { "build" } else { "check" };
    let extra = feature_extra(
        opts.all_features,
        &opts.features,
        opts.no_default,
        &opts.meta_flags,
    );
    let dev = opts.targets.tests || opts.targets.all;
    let plan = plan(
        dir,
        packages,
        home,
        &extra,
        opts.workspace,
        dev,
        opts.target_dir.as_deref(),
    )?;
    let root = plan
        .roots
        .first()
        .cloned()
        .with_context(|| "no packages to check")?;
    let order = if dev {
        cargo::test_closure_many(&plan.meta, &plan.roots)?
    } else {
        cargo::closure_many(&plan.meta, &plan.roots)?
    };
    let mut sess = check_session(home, &plan, meta_only, opts)?;
    sess.must_link = cargo::must_link(&plan.meta, &order);
    let compiled = check_units(&sess, &plan, &order, meta_only, opts)?;
    let ws = cargo::root(&plan.meta, &plan.pkg_dir);
    finished(ws, profile_name(opts.release, "dev"), started);
    record_stats(home, &compiled, started, op);
    Ok((root, compiled, sess.settings.profile_dir()))
}

fn profile_name(release: bool, base: &'static str) -> &'static str {
    if release { "release" } else { base }
}

fn check_session(home: &Path, plan: &Plan, meta_only: bool, opts: &CheckOpts) -> Result<Session> {
    let meta = &plan.meta;
    let ws = cargo::root(meta, &plan.pkg_dir);
    let mut sess = crate::out::timed("session", || {
        Session::with_profile(
            home,
            &plan.pkg_dir,
            ws,
            profile_name(opts.release, "dev"),
            &meta.workspace_members,
            &meta.packages,
            opts.target_dir.as_deref(),
        )
    })?;
    sess.json = opts.json;
    sess.meta_only = meta_only;
    sess.primary = plan.roots.iter().cloned().collect();
    if !meta_only {
        sess.ship = plan.roots.iter().cloned().collect();
    }
    if opts.targets.tests || opts.targets.all {
        sess.set_target_tmpdir(target_tmpdir(&sess.settings)?);
    }
    Ok(sess)
}

fn check_units(
    sess: &Session,
    plan: &Plan,
    order: &[String],
    meta_only: bool,
    opts: &CheckOpts,
) -> Result<HashMap<String, compile::Compiled>> {
    let (meta, roots) = (&plan.meta, &plan.roots);
    if meta_only {
        return schedule::compile_ids_and_extras(
            sess,
            meta,
            order,
            roots,
            opts.targets.tests,
            opts.targets.all,
        );
    }
    let compiled = schedule::compile_ids(sess, meta, order)?;
    artifact::deliver(roots, &compiled, &sess.settings.profile_dir())?;
    Ok(compiled)
}

fn record_stats(
    home: &Path,
    compiled: &HashMap<String, compile::Compiled>,
    started: std::time::Instant,
    op: &str,
) {
    let hits = compiled
        .values()
        .filter(|c| c.rustc == RustcOutcome::Restored)
        .count() as u64;
    let misses = compiled
        .values()
        .filter(|c| c.rustc == RustcOutcome::Ran)
        .count() as u64;
    record_counts(home, hits, misses, started, op);
}

fn record_counts(home: &Path, hits: u64, misses: u64, started: std::time::Instant, op: &str) {
    if let Err(error) = crate::store::bump_stats(home, hits, misses) {
        crate::out::err(format!("artificer: stats not recorded: {error:#}"));
    }
    crate::store::note_build(
        home,
        op,
        hits,
        misses,
        started.elapsed().as_millis() as u64,
        None,
    );
}

fn target_tmpdir(settings: &crate::settings::Settings) -> Result<PathBuf> {
    let tmp = settings.profile_dir().join("tmp");
    std::fs::create_dir_all(&tmp)?;
    Ok(tmp)
}

fn sweep_meta(
    meta: &cargo::Metadata,
    home: &Path,
    fallback: &Path,
    target: Option<&Path>,
) -> Result<()> {
    if mods::load(home)?.sweep {
        let workspace = if meta.workspace_root.as_os_str().is_empty() {
            fallback
        } else {
            meta.workspace_root.as_path()
        };
        let target = config::target_dir(target, fallback, workspace);
        if let Err(e) = sweep::workspace(&target, home, false) {
            crate::out::err(format!("artificer: sweep skipped: {e}"));
        }
    }
    gc_daily(home);
    Ok(())
}

mod testing;
use testing::json_profile;
pub use testing::{TestOpts, test_package};

mod run;
pub use run::run_cmd;
