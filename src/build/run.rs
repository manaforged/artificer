use super::*;

pub fn run_cmd(
    dir: &Path,
    packages: &[String],
    bin: Option<&str>,
    example: Option<&str>,
    home: &Path,
    opts: CheckOpts,
    args: &[String],
) -> Result<i32> {
    if let Some(name) = example {
        return run_example(dir, packages, name, home, &opts, args);
    }
    let (root, compiled, settings) =
        check_graph(dir, packages, home, false, &opts, &TargetSel::default())?;
    let c = compiled.get(&root).with_context(|| "no package to run")?;
    let name = match bin {
        Some(b) => artifact::bin_name(b),
        None => {
            let names: Vec<&str> = c.shipped.iter().map(|(n, _)| n.as_str()).collect();
            match names.as_slice() {
                [] => bail!("no bin target to run"),
                [one] => (*one).to_string(),
                many => bail!("several bin targets ({}); pass --bin", many.join(", ")),
            }
        }
    };
    if !c.shipped.iter().any(|(n, _)| *n == name) {
        bail!("no bin target `{name}`");
    }
    execute(&settings, &settings.profile_dir().join(name), args)
}

fn run_example(
    dir: &Path,
    packages: &[String],
    name: &str,
    home: &Path,
    opts: &CheckOpts,
    args: &[String],
) -> Result<i32> {
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
        false,
        true,
        opts.target_dir.as_deref(),
    )?;
    let (meta, roots) = (&plan.meta, &plan.roots);
    let root = roots.first().context("no package to run")?.clone();
    let order = cargo::test_closure_many(meta, roots)?;
    let ws = cargo::root(meta, &plan.pkg_dir);
    let profile = profile_name(opts.release, "dev");
    let mut sess = Session::with_profile(
        home,
        &plan.pkg_dir,
        ws,
        profile,
        &meta.workspace_members,
        &meta.packages,
        opts.target_dir.as_deref(),
    )?;
    sess.primary = roots.iter().cloned().collect();
    let compiled = schedule::compile_ids(&sess, meta, &order)?;
    record_stats(home, &compiled, started, "run");
    let exe = compile::compile_example(&sess, meta, &root, name)?;
    let exe = artifact::place_exe(
        &exe,
        &sess.settings.profile_dir().join("examples"),
        &artifact::bin_name(name),
    )?;
    finished(ws, profile, started);
    execute(&sess.settings, &exe, args)
}

fn execute(settings: &Settings, exe: &Path, args: &[String]) -> Result<i32> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let shown = exe.strip_prefix(&cwd).unwrap_or(exe);
    let mut line = shown.display().to_string();
    for arg in args {
        line.push(' ');
        line.push_str(arg);
    }
    crate::out::status(crate::out::Status::Running, format!("`{line}`"));
    let status = settings
        .exec_cmd(exe)
        .args(args)
        .status()
        .with_context(|| format!("run {}", exe.display()))?;
    Ok(status.code().unwrap_or(1))
}
