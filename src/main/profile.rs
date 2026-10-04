use super::help;
use artificer::ProfileCommand;
use std::path::PathBuf;
use std::process::ExitCode;

const DEFAULT_LIMIT: usize = 20;

fn show(args: &[String]) -> Option<ProfileCommand> {
    let mut id = None;
    let mut json = false;
    let mut trace = None;
    let mut html = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--trace" => trace = Some(PathBuf::from(iter.next()?)),
            "--html" => html = Some(PathBuf::from(iter.next()?)),
            flag if flag.starts_with('-') => return None,
            name if id.is_none() => id = Some(name.to_string()),
            _ => return None,
        }
    }
    Some(ProfileCommand::Show {
        id,
        json,
        trace,
        html,
    })
}

fn list(args: &[String]) -> Option<ProfileCommand> {
    let mut limit = DEFAULT_LIMIT;
    let mut json = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--limit" => limit = iter.next()?.parse().ok()?,
            _ => return None,
        }
    }
    Some(ProfileCommand::List { limit, json })
}

fn diff(args: &[String]) -> Option<ProfileCommand> {
    let mut ids = Vec::new();
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            flag if flag.starts_with('-') => return None,
            id => ids.push(id.to_string()),
        }
    }
    let [base, head] = <[String; 2]>::try_from(ids).ok()?;
    Some(ProfileCommand::Diff { base, head, json })
}

fn parse(rest: &[String]) -> Option<ProfileCommand> {
    match rest.split_first() {
        Some((first, tail)) if first == "list" => list(tail),
        Some((first, tail)) if first == "diff" => diff(tail),
        _ => show(rest),
    }
}

pub(super) fn profile_cmd(rest: &[String]) -> anyhow::Result<ExitCode> {
    let Some(command) = parse(rest) else {
        return Ok(help::usage(help::Sub::Profile));
    };
    let report = artificer::profile_command(&artificer::default_home(), &command)?;
    print!("{report}");
    Ok(ExitCode::SUCCESS)
}
