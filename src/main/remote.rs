use super::*;
use artificer::{Location, RemoteSource};

const NO_MODIFY_PATH: &str = "--no-modify-path";
const REMOTE_FLAG: &str = "--remote";

pub(super) fn install_args(args: &[String]) -> Option<(bool, Option<Location>)> {
    let mut modify_path = true;
    let mut remote = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            NO_MODIFY_PATH if modify_path => modify_path = false,
            REMOTE_FLAG if remote.is_none() => {
                i += 1;
                let raw = args.get(i)?;
                match Location::parse(raw) {
                    Ok(location) => remote = Some(location),
                    Err(error) => {
                        eprintln!("artificer: {error:#}");
                        return None;
                    }
                }
            }
            _ => return None,
        }
        i += 1;
    }
    Some((modify_path, remote))
}

pub(super) fn install_remote(remote: Option<Location>) -> Result<()> {
    let Some(location) = remote else {
        return Ok(());
    };
    let home = artificer::default_home();
    artificer::set_remote(&home, Some(&location))?;
    println!("artificer: remote {location}");
    artificer::spawn_pull(&home)?;
    println!("artificer: first pull started in the background");
    Ok(())
}

pub(super) fn remote_cmd(args: &[String]) -> Result<ExitCode> {
    let home = artificer::default_home();
    match args {
        [] => {
            match artificer::remote(&home)? {
                None => println!("artificer: no remote"),
                Some((location, RemoteSource::Config)) => println!("{location}"),
                Some((location, RemoteSource::Env)) => {
                    println!("{location} (from {})", artificer::REMOTE_ENV)
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        [verb, raw] if verb == "set" => {
            let location = Location::parse(raw)?;
            artificer::set_remote(&home, Some(&location))?;
            println!("artificer: remote {location}");
            Ok(ExitCode::SUCCESS)
        }
        [verb] if verb == "off" => {
            artificer::set_remote(&home, None)?;
            println!("artificer: remote cleared");
            Ok(ExitCode::SUCCESS)
        }
        _ => Ok(help::usage(help::Sub::Remote)),
    }
}

pub(super) fn pull_cmd(args: &[String]) -> Result<ExitCode> {
    if !args.is_empty() {
        return Ok(help::usage(help::Sub::Pull));
    }
    let (location, report) = artificer::pull(&artificer::default_home())?;
    println!(
        "artificer: pulled {} unit(s), {:.1} MB from {location}",
        report.units,
        report.bytes as f64 / 1_048_576.0
    );
    Ok(ExitCode::SUCCESS)
}
