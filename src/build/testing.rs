use super::*;

#[derive(Debug, Default)]
pub struct TestOpts {
    pub no_run: bool,
    pub json: bool,
    pub workspace: bool,
    pub all_features: bool,
    pub features: Vec<String>,
    pub no_default: bool,
    pub meta_flags: Vec<String>,
    pub release: bool,
    pub lib: bool,
    pub doc: bool,
    pub only: Vec<String>,
    pub args: Vec<String>,
    pub target_dir: Option<PathBuf>,
}

pub fn test_package(dir: &Path, packages: &[String], home: &Path, opts: &TestOpts) -> Result<i32> {
    jobs::install(home)?;
    let started = std::time::Instant::now();
    let extra = feature_extra(
        opts.all_features,
        &opts.features,
        opts.no_default,
        &opts.meta_flags,
    );
    let plan = plan(
        dir,
        packages,
        home,
        &extra,
        opts.workspace,
        true,
        opts.target_dir.as_deref(),
    )?;
    let (meta, roots) = (&plan.meta, &plan.roots);
    let order = cargo::test_closure_many(meta, roots)?;
    let ws = cargo::root(meta, &plan.pkg_dir);
    let profile = profile_name(opts.release, "test");
    let mut sess = Session::with_profile(
        home,
        &plan.pkg_dir,
        ws,
        profile,
        &meta.workspace_members,
        &meta.packages,
        opts.target_dir.as_deref(),
    )?;
    sess.json = opts.json;
    sess.primary = roots.iter().cloned().collect();
    sess.build_only = cargo::build_only(meta, roots, &order, true);
    if !opts.doc {
        sess.set_target_tmpdir(target_tmpdir(&sess.settings)?);
        sess.ship = roots.iter().cloned().collect();
    }
    let sel = compile::TestSel {
        lib: opts.lib,
        only: opts.only.clone(),
    };
    let (compiled, mut harnesses) =
        crate::profile::span(crate::profile::WrapperPhase::Schedule, || {
            if opts.doc {
                let compiled = schedule::compile_ids(&sess, meta, &order)?;
                Ok((compiled, std::collections::HashMap::new()))
            } else {
                schedule::compile_ids_and_tests(&sess, meta, &order, roots, &sel)
            }
        })?;
    let mut bins: Vec<(String, compile::TestBin)> = Vec::new();
    for root in roots {
        if let Some(root_bins) = harnesses.remove(root) {
            for bin in root_bins {
                bins.push((root.clone(), bin));
            }
        }
    }
    for name in &opts.only {
        if !bins.iter().any(|(_, b)| &b.target.name == name) {
            bail!("no test target named `{name}`");
        }
    }
    if !opts.doc {
        let profile_dir = sess.settings.profile_dir();
        artifact::deliver(roots, &compiled, &profile_dir)?;
        place_tests(bins.iter_mut().map(|(_, bin)| bin), &profile_dir)?;
    }
    let graph_hits = compiled
        .values()
        .filter(|c| c.rustc == RustcOutcome::Restored)
        .count() as u64;
    let graph_misses = compiled
        .values()
        .filter(|c| c.rustc == RustcOutcome::Ran)
        .count() as u64;
    let harness_hits = bins
        .iter()
        .filter(|(_, bin)| bin.rustc == RustcOutcome::Restored)
        .count() as u64;
    let harness_misses = bins.len() as u64 - harness_hits;
    record_counts(
        home,
        graph_hits + harness_hits,
        graph_misses + harness_misses,
        started,
        "test",
    );
    finished(ws, profile, started);
    if opts.json {
        let profile = json_profile(ws, profile, true);
        for (root, bin) in &bins {
            let pkg = cargo::package(meta, root)?;
            let crate_types = if bin.target.kind.iter().any(|k| k == "test") {
                vec!["bin".to_string()]
            } else if bin.target.crate_types.is_empty() {
                vec!["lib".to_string()]
            } else {
                bin.target.crate_types.clone()
            };
            let art = serde_json::json!({
                "reason": "compiler-artifact",
                "package_id": pkg.id,
                "manifest_path": pkg.manifest_path,
                "target": {
                    "kind": bin.target.kind,
                    "crate_types": crate_types,
                    "name": bin.target.name,
                    "src_path": bin.target.src_path,
                    "edition": bin.target.edition,
                    "required-features": bin.target.required_features,
                    "doc": bin.target.doc,
                    "doctest": bin.target.doctest,
                    "test": true
                },
                "profile": profile.clone(),
                "features": cargo::node(meta, root)?.features,
                "filenames": [bin.exe],
                "executable": bin.exe,
                "linked": true,
                "fresh": bin.rustc == RustcOutcome::Restored
            });
            crate::out::out(art);
        }
        crate::out::out(serde_json::json!({"reason":"build-finished","success":true}));
    }
    if opts.no_run {
        for (id, bin) in &bins {
            let label = harness_label(cargo::package(meta, id)?, bin, ws);
            crate::out::status(crate::out::Status::Executable, label);
        }
        return Ok(0);
    }
    enum Job<'a> {
        Bin {
            pkg: &'a cargo::Package,
            exe: PathBuf,
            label: String,
        },
        Doc {
            name: String,
            cmd: std::process::Command,
        },
    }
    let mut jobs = Vec::new();
    for (id, bin) in &bins {
        let pkg = cargo::package(meta, id)?;
        jobs.push(Job::Bin {
            pkg,
            exe: bin.exe.clone(),
            label: harness_label(pkg, bin, ws),
        });
    }
    if !opts.lib && opts.only.is_empty() {
        for root in roots {
            let pkg = cargo::package(meta, root)?;
            let node = cargo::node(meta, root)?;
            let Some(lib) = pkg.lib_target() else {
                continue;
            };
            let Some(cmd) = compile::doctest_cmd(&sess, pkg, node, lib, &opts.args)? else {
                continue;
            };
            jobs.push(Job::Doc {
                name: pkg.name.clone(),
                cmd,
            });
        }
    }
    let capture = crate::out::current().is_some();
    let statuses = crate::schedule::fan_out(jobs, |job| {
        let (mut run, what) = match job {
            Job::Bin { pkg, exe, label } => {
                crate::out::status(crate::out::Status::Running, label);
                let mut run = sess.settings.exec_cmd(&exe);
                run.current_dir(pkg.root());
                cargo::set_package_env(&mut run, pkg);
                run.args(&opts.args);
                (run, format!("`{}`", exe.display()))
            }
            Job::Doc { name, cmd } => {
                crate::out::err(format!("artificer: doctests {name}"));
                (cmd, format!("doctests for {name}"))
            }
        };
        crate::jobs::isolate(&mut run);
        let status = crate::profile::span(crate::profile::RunPhase::TestRun, || {
            if capture {
                crate::profile::output(&mut run).map(|out| {
                    crate::out::replay(&out.stdout, &out.stderr);
                    out.status
                })
            } else {
                crate::profile::status(&mut run)
            }
        })?;
        if !status.success() {
            crate::out::err(format!(
                "error: process didn't exit successfully: {what} ({status})"
            ));
        }
        Ok(status)
    })?;
    Ok(statuses
        .iter()
        .find(|status| !status.success())
        .map_or(0, |status| status.code().unwrap_or(1)))
}

pub(super) fn place_tests<'b>(
    bins: impl Iterator<Item = &'b mut compile::TestBin>,
    profile_dir: &Path,
) -> Result<()> {
    let deps = profile_dir.join("deps");
    for bin in bins {
        let file = artifact::deps_name(&bin.exe, &bin.target.name.replace('-', "_"));
        bin.exe = artifact::place_exe(&bin.exe, &deps, &file)?;
    }
    Ok(())
}

fn harness_label(pkg: &cargo::Package, bin: &compile::TestBin, ws: &Path) -> String {
    let src = bin
        .target
        .src_path
        .strip_prefix(pkg.root())
        .unwrap_or(&bin.target.src_path);
    let exe = bin.exe.strip_prefix(ws).unwrap_or(&bin.exe);
    let unit = if bin.target.kind.iter().any(|k| k == "test") {
        ""
    } else {
        "unittests "
    };
    format!("{unit}{} ({})", src.display(), exe.display())
}

pub(super) fn json_profile(root: &Path, name: &str, test: bool) -> serde_json::Value {
    let mut opt = "0".to_string();
    let mut debuginfo = 0;
    let mut assertions = false;
    let mut overflow = false;
    let args = manifest::profile(root, name, manifest::UnitUse::Runtime);
    for pair in args.windows(2).filter(|pair| pair[0] == "-C") {
        let Some((key, value)) = pair[1].split_once('=') else {
            continue;
        };
        match key {
            "opt-level" => opt = value.to_string(),
            "debuginfo" => {
                debuginfo = match value {
                    "line-tables-only" => 1,
                    value => value.parse().unwrap_or(0),
                }
            }
            "debug-assertions" => assertions = value == "true",
            "overflow-checks" => overflow = value == "true",
            _ => {}
        }
    }
    serde_json::json!({
        "opt_level": opt,
        "debuginfo": debuginfo,
        "debug_assertions": assertions,
        "overflow_checks": overflow,
        "test": test,
    })
}
