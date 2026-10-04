use anyhow::{Context, Result};

mod cli;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

enum Dispatch {
    Completed(ExitCode),
    Fallback,
}

impl From<ExitCode> for Dispatch {
    fn from(code: ExitCode) -> Self {
        Self::Completed(code)
    }
}

fn main() -> ExitCode {
    if let Some(path) = artificer::toolchain_path() {
        // SAFETY: main has not started another thread yet.
        unsafe { env::set_var("PATH", path) };
    }
    let args_os = env::args_os().skip(1).collect::<Vec<_>>();
    if shim()
        && env::var_os(SHIM_REFRESHED).is_none()
        && let Some(code) = refreshed(&args_os)
    {
        return code;
    }
    let args = args_os
        .iter()
        .cloned()
        .map(OsString::into_string)
        .collect::<std::result::Result<Vec<_>, _>>();
    let args = match args {
        Ok(args) => args,
        Err(_) if shim() => return stock(&args_os).unwrap_or_else(failed),
        Err(_) => {
            eprintln!("error: Artificer arguments must be valid UTF-8");
            return ExitCode::from(2);
        }
    };
    let result = match run(args) {
        Err(error) if error.is::<artificer::Unmodeled>() => Ok(fallback(error)),
        result => result,
    };
    match result {
        Ok(Dispatch::Fallback) if shim() => stock(&args_os).unwrap_or_else(failed),
        Ok(Dispatch::Fallback) => ExitCode::from(2),
        Ok(Dispatch::Completed(code)) => code,
        Err(error) => failed(error),
    }
}

const SHIM_REFRESHED: &str = "ARTIFICER_SHIM_REFRESHED";

fn refreshed(args: &[OsString]) -> Option<ExitCode> {
    let exe = env::current_exe().ok()?;
    let refreshed =
        artificer::refresh_shim(&exe, &artificer::control_home(), &artificer::cargo_home());
    if !matches!(refreshed, Ok(true)) {
        return None;
    }
    let status = Command::new(&exe)
        .args(args)
        .env(SHIM_REFRESHED, "1")
        .status()
        .ok()?;
    Some(child_exit(status.code().unwrap_or(1)))
}

fn failed(error: anyhow::Error) -> ExitCode {
    if !error.is::<artificer::Reported>() {
        artificer::report_error(format!("{error:#}"));
    }
    ExitCode::from(101)
}

fn shim() -> bool {
    env::args_os()
        .next()
        .and_then(|path| {
            PathBuf::from(path)
                .file_stem()
                .map(|name| name.to_os_string())
        })
        .is_some_and(|name| name.eq_ignore_ascii_case("cargo"))
}

fn stock(args: &[OsString]) -> Result<ExitCode> {
    let external = args.first().and_then(|arg| arg.to_str());
    let accelerate = matches!(external, Some("clippy" | "nextest"))
        && matches!(artificer::enabled(&artificer::default_home()), Ok(true));
    let mut command = match (accelerate, external) {
        (true, Some("clippy")) => Command::new("cargo-clippy"),
        (true, Some("nextest")) => Command::new("cargo-nextest"),
        _ => Command::new(artificer::stock_cargo()),
    };
    command.args(args);
    if accelerate {
        command.env("CARGO", env::current_exe()?);
        artificer::isolate(&mut command);
    }
    let status = command.status()?;
    Ok(child_exit(status.code().unwrap_or(1)))
}

fn fallback(message: impl std::fmt::Display) -> Dispatch {
    let message = message.to_string();
    artificer::note_fallback(&artificer::default_home(), &message);
    if !shim() && env::var_os("ARTIFICER_SHIM").is_none() {
        eprintln!("artificer: {message}");
    }
    Dispatch::Fallback
}

fn run(args: Vec<String>) -> Result<Dispatch> {
    if shim() {
        return build(&args);
    }
    direct(args)
}

fn direct(args: Vec<String>) -> Result<Dispatch> {
    match args.as_slice() {
        [] => {
            eprintln!("{}", help::overview());
            return Ok(ExitCode::from(2).into());
        }
        [arg] if arg == "--help" || arg == "-h" => {
            println!("{}", help::overview());
            return Ok(ExitCode::SUCCESS.into());
        }
        [arg] if arg == "--version" || arg == "-V" => {
            println!("artificer {}", env!("CARGO_PKG_VERSION"));
            return Ok(ExitCode::SUCCESS.into());
        }
        [first, rest @ ..] if first == "help" => return Ok(help::help_cmd(rest).into()),
        _ => {}
    }
    let Some((sub, at)) = help::subcommand(&args) else {
        eprintln!("{}", help::overview());
        return Ok(ExitCode::from(2).into());
    };
    if help::wants_help(&args[at + 1..]) {
        println!("{}", help::page(sub));
        return Ok(ExitCode::SUCCESS.into());
    }
    if at == 0
        && let Some(code) = tool(sub, &args[1..])?
    {
        return Ok(code.into());
    }
    build(&args)
}

fn tool(sub: help::Sub, rest: &[String]) -> Result<Option<ExitCode>> {
    use help::Sub;
    let simple = matches!(sub, Sub::Env | Sub::Doctor | Sub::Enable | Sub::Disable);
    if simple && !rest.is_empty() {
        return Ok(Some(help::usage(sub)));
    }
    match sub {
        Sub::Check | Sub::Build | Sub::Run | Sub::Test | Sub::Warm | Sub::Clean => Ok(None),
        Sub::Env | Sub::Enable | Sub::Disable | Sub::Doctor | Sub::Install | Sub::Uninstall => {
            setup_tool(sub, rest).map(Some)
        }
        _ => store_tool(sub, rest).map(Some),
    }
}

fn setup_tool(sub: help::Sub, rest: &[String]) -> Result<ExitCode> {
    use help::Sub;
    match sub {
        Sub::Env => {
            print!("{}", artificer::env_script());
            Ok(ExitCode::SUCCESS)
        }
        Sub::Enable | Sub::Disable => enable_cmd(sub == Sub::Enable),
        Sub::Doctor => artificer::doctor(&artificer::default_home()).map(child_exit),
        Sub::Install => install_cmd(rest),
        _ => uninstall_cmd(rest),
    }
}

fn store_tool(sub: help::Sub, rest: &[String]) -> Result<ExitCode> {
    use help::Sub;
    match sub {
        Sub::Mods => mods_cmd(rest),
        Sub::Stat => stat_cmd(rest),
        Sub::Serve => serve_cmd(rest),
        Sub::Export => export_cmd(rest),
        Sub::Import => import_cmd(rest),
        Sub::WhyMiss => why_miss_cmd(rest),
        Sub::Remote => remote::remote_cmd(rest),
        Sub::Pull => remote::pull_cmd(rest),
        _ => why_fallback_cmd(rest),
    }
}

fn parsed(args: &[String]) -> std::result::Result<(cli::BuildArgs, help::Sub), Dispatch> {
    let a = match cli::parse(args, shim()) {
        cli::Parsed::Build(a) => *a,
        cli::Parsed::Fallback(reason) => return Err(fallback(reason)),
        cli::Parsed::Unknown => {
            if !shim() {
                eprintln!("{}", help::overview());
            }
            return Err(Dispatch::Fallback);
        }
    };
    let sub = help::lookup(&a.cmd).ok_or(Dispatch::Fallback)?;
    artificer::set_quiet(a.quiet);
    artificer::set_trace(a.verbose);
    if let Some(choice) = a.color {
        artificer::set_color(choice);
    }
    if let Some(jobs) = a.jobs {
        artificer::set_jobs(jobs);
    }
    Ok((a, sub))
}

fn build(args: &[String]) -> Result<Dispatch> {
    let (a, sub) = match parsed(args) {
        Ok(parsed) => parsed,
        Err(dispatch) => return Ok(dispatch),
    };
    let home = artificer::default_home();
    if let Some(declined) = declined(&home)? {
        return Ok(declined);
    }
    let dir = a.dir.clone().map_or_else(env::current_dir, Ok)?;
    if sub == help::Sub::Clean {
        return clean(&dir, a.target_dir.as_deref(), &home);
    }
    let mut req = request(&a, sub, dir);
    if let Some(reason) = artificer::passthrough_reason(&req, dev(&a, sub, &req), &home)? {
        return Ok(fallback(reason));
    }
    let code = match sub {
        help::Sub::Run => run_target(&a, &req, &home),
        help::Sub::Test => test(&mut req, &home),
        _ => check(&mut req, &home),
    }?;
    Ok(code.into())
}

fn declined(home: &Path) -> Result<Option<Dispatch>> {
    if !artificer::ready(home) {
        return Ok(Some(fallback(format!(
            "store {} is not writable, or is a non-empty directory Artificer did not create; remove it or set ARTIFICER_HOME to an empty directory",
            home.display()
        ))));
    }
    if !artificer::enabled(home)? {
        if !shim() {
            eprintln!(
                "artificer: disabled; run `artificer enable` and unset ARTIFICER_DISABLED to enable"
            );
        }
        return Ok(Some(Dispatch::Fallback));
    }
    Ok(None)
}

fn request(a: &cli::BuildArgs, sub: help::Sub, dir: PathBuf) -> artificer::ServeRequest {
    let warm = sub == help::Sub::Warm;
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
        lib: a.lib_only,
        doc: a.doc_only,
        only: a.only.clone(),
        tests: a.tests,
        all_targets: a.all_targets,
        args: a.pass.clone(),
    }
}

fn dev(a: &cli::BuildArgs, sub: help::Sub, req: &artificer::ServeRequest) -> bool {
    match sub {
        help::Sub::Test => true,
        help::Sub::Run => a.example.is_some(),
        _ => req.tests || req.all_targets,
    }
}

fn run_target(a: &cli::BuildArgs, req: &artificer::ServeRequest, home: &Path) -> Result<ExitCode> {
    let code = artificer::run_cmd(
        &req.dir,
        &req.packages,
        a.bin.as_deref(),
        a.example.as_deref(),
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

fn test(req: &mut artificer::ServeRequest, home: &Path) -> Result<ExitCode> {
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

fn check(req: &mut artificer::ServeRequest, home: &Path) -> Result<ExitCode> {
    if let Some(code) = artificer::serve_try(home, req) {
        return Ok(child_exit(code?));
    }
    let code = artificer::check_cmd(
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
            targets: artificer::Targets {
                tests: req.tests,
                all: req.all_targets,
            },
        },
    )?;
    Ok(child_exit(code))
}

#[path = "main/clean.rs"]
mod clean;
#[path = "main/commands.rs"]
mod commands;
use clean::clean;
#[path = "main/help.rs"]
mod help;
#[path = "main/remote.rs"]
mod remote;
use commands::{
    enable_cmd, export_cmd, import_cmd, install_cmd, mods_cmd, serve_cmd, stat_cmd, uninstall_cmd,
    why_fallback_cmd, why_miss_cmd,
};

fn child_exit(code: i32) -> ExitCode {
    u8::try_from(code).map_or(ExitCode::from(1), ExitCode::from)
}
