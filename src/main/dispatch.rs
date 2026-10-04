use super::*;

pub(super) fn request(a: &cli::BuildArgs, sub: help::Sub, dir: PathBuf) -> artificer::ServeRequest {
    let warm = sub == help::Sub::Warm;
    let targets = a.select.targets().unwrap_or_default();
    artificer::ServeRequest {
        token: String::new(),
        op: a.cmd.clone(),
        dir,
        packages: a.packages.clone(),
        json: a.json,
        workspace: a.workspace || warm,
        all_features: a.all_features,
        features: a.features.clone(),
        no_default: a.no_default,
        meta_flags: a.meta_flags.clone(),
        target_dir: a.target_dir.clone(),
        release: a.release,
        link: sub == help::Sub::Build,
        no_run: a.no_run,
        lib: a.select.lib,
        doc: a.doc_only,
        only: a.select.tests.names().to_vec(),
        tests: targets.tests,
        all_targets: targets.all,
        args: a.pass.clone(),
        profile: None,
    }
}

pub(super) fn dev(a: &cli::BuildArgs, sub: help::Sub) -> bool {
    sub == help::Sub::Test || a.select.wants_dev()
}

pub(super) fn run_target(
    a: &cli::BuildArgs,
    req: &artificer::ServeRequest,
    home: &Path,
) -> Result<ExitCode> {
    let code = artificer::run_cmd(
        &req.dir,
        &req.packages,
        a.select.bins.names().first().map(String::as_str),
        a.select.examples.names().first().map(String::as_str),
        home,
        artificer::CheckOpts {
            all_features: req.all_features,
            features: req.features.clone(),
            no_default: req.no_default,
            meta_flags: req.meta_flags.clone(),
            target_dir: req.target_dir.clone(),
            release: req.release,
            ..Default::default()
        },
        &req.args,
    )?;
    Ok(child_exit(code))
}

pub(super) fn test(req: &mut artificer::ServeRequest, home: &Path) -> Result<ExitCode> {
    if let Some(code) = artificer::serve_try(home, req) {
        return Ok(child_exit(code?));
    }
    let code = artificer::test_package(
        &req.dir,
        &req.packages,
        home,
        &artificer::TestOpts {
            no_run: req.no_run,
            json: req.json,
            workspace: req.workspace,
            all_features: req.all_features,
            features: req.features.clone(),
            no_default: req.no_default,
            meta_flags: req.meta_flags.clone(),
            target_dir: req.target_dir.clone(),
            release: req.release,
            lib: req.lib,
            doc: req.doc,
            only: req.only.clone(),
            args: req.args.clone(),
        },
    )?;
    Ok(child_exit(code))
}

pub(super) fn check(
    req: &mut artificer::ServeRequest,
    select: &artificer::TargetSel,
    home: &Path,
) -> Result<ExitCode> {
    let served = select.targets().is_some();
    if served && let Some(code) = artificer::serve_try(home, req) {
        return Ok(child_exit(code?));
    }
    let code = artificer::check_selected(
        &req.dir,
        &req.packages,
        home,
        artificer::CheckOpts {
            json: req.json,
            workspace: req.workspace,
            all_features: req.all_features,
            features: req.features.clone(),
            no_default: req.no_default,
            meta_flags: req.meta_flags.clone(),
            target_dir: req.target_dir.clone(),
            release: req.release,
            link: req.link,
            targets: artificer::Targets::default(),
        },
        select,
    )?;
    Ok(child_exit(code))
}
