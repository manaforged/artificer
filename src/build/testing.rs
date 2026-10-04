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
    if !opts.doc {
        sess.set_target_tmpdir(target_tmpdir(&sess.settings)?);
        sess.ship = roots.iter().cloned().collect();
    }
    let sel = compile::TestSel {
        lib: opts.lib,
        only: opts.only.clone(),
    };
    let (compiled, mut harnesses) = crate::out::timed("schedule", || {
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
        let deps = profile_dir.join("deps");
        for (_, bin) in &mut bins {
            let file = artifact::deps_name(&bin.exe, &bin.target.name.replace('-', "_"));
            bin.exe = artifact::place_exe(&bin.exe, &deps, &file)?;
        }
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
    let workers = crate::schedule::job_cap().min(jobs.len().max(1));
    let code = Mutex::new(0i32);
    let failed = Mutex::new(None::<anyhow::Error>);
    let next = Mutex::new(0usize);
    let jobs: Mutex<Vec<Option<Job>>> = Mutex::new(jobs.into_iter().map(Some).collect());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sink = crate::out::current();
            scope.spawn(|| {
                crate::out::attach(sink);
                loop {
                    let job = {
                        let mut n = next.lock().expect("test index");
                        let mut jobs = jobs.lock().expect("test jobs");
                        if *n >= jobs.len() {
                            return;
                        }
                        let i = *n;
                        *n += 1;
                        jobs[i].take()
                    };
                    let Some(job) = job else {
                        continue;
                    };
                    let mut run = match job {
                        Job::Bin { pkg, exe, label } => {
                            crate::out::status(crate::out::Status::Running, label);
                            let mut run = std::process::Command::new(&exe);
                            run.current_dir(pkg.root());
                            cargo::set_package_env(&mut run, pkg);
                            run.args(&opts.args);
                            run
                        }
                        Job::Doc { name, cmd } => {
                            crate::out::err(format!("artificer: doctests {name}"));
                            cmd
                        }
                    };
                    crate::jobs::isolate(&mut run);
                    let status = if capture {
                        match run.output() {
                            Ok(out) => {
                                crate::out::replay(&out.stdout, &out.stderr);
                                out.status
                            }
                            Err(e) => {
                                let mut failed = failed.lock().expect("test spawn");
                                if failed.is_none() {
                                    *failed = Some(e.into());
                                }
                                return;
                            }
                        }
                    } else {
                        match run.status() {
                            Ok(status) => status,
                            Err(e) => {
                                let mut failed = failed.lock().expect("test spawn");
                                if failed.is_none() {
                                    *failed = Some(e.into());
                                }
                                return;
                            }
                        }
                    };
                    if !status.success() {
                        let mut code = code.lock().expect("test code");
                        if *code == 0 {
                            *code = status.code().unwrap_or(1);
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = failed
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        return Err(e);
    }
    Ok(code
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner))
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
    let args = manifest::profile(root, name);
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
