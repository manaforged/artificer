use super::*;

fn transfer_args(args: &[String]) -> std::result::Result<(PathBuf, u64, u64), String> {
    let mut dir = None;
    let mut days = 7u64;
    let mut max_gb = 2u64;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--days" => {
                i += 1;
                days = args
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .ok_or("--days needs a number")?;
            }
            "--max-gb" => {
                i += 1;
                max_gb = args
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .ok_or("--max-gb needs a number")?;
            }
            other if other.starts_with('-') => return Err(format!("unknown option `{other}`")),
            other => {
                if dir.is_some() {
                    return Err("one directory is enough".into());
                }
                dir = Some(PathBuf::from(other));
            }
        }
        i += 1;
    }
    let dir = dir.ok_or("a directory is required")?;
    Ok((dir, days, max_gb))
}

pub(super) fn export_cmd(args: &[String]) -> Result<ExitCode> {
    let (dir, days, max_gb) = match transfer_args(args) {
        Ok(parsed) => parsed,
        Err(reason) => {
            eprintln!("usage: artificer export DIR [--days N] [--max-gb N]: {reason}");
            return Ok(ExitCode::from(2));
        }
    };
    let home = artificer::default_home();
    let Some(max_bytes) = max_gb.checked_mul(1 << 30) else {
        eprintln!("artificer: --max-gb is too large");
        return Ok(ExitCode::from(2));
    };
    let report = artificer::export(&home, &dir, days, max_bytes)?;
    println!(
        "artificer: exported {} unit(s), {:.1} MB to {}",
        report.units,
        report.bytes as f64 / 1_048_576.0,
        dir.display()
    );
    Ok(ExitCode::from(0))
}

pub(super) fn import_cmd(args: &[String]) -> Result<ExitCode> {
    let [dir] = args else {
        return Ok(help::usage(help::Sub::Import));
    };
    if dir.starts_with('-') {
        return Ok(help::usage(help::Sub::Import));
    }
    let dir = PathBuf::from(dir);
    let home = artificer::default_home();
    let report = artificer::import(&home, &dir)?;
    println!(
        "artificer: imported {} unit(s), {:.1} MB from {}",
        report.units,
        report.bytes as f64 / 1_048_576.0,
        dir.display()
    );
    Ok(ExitCode::from(0))
}

pub(super) fn enable_cmd(enabled: bool) -> Result<ExitCode> {
    let home = artificer::default_home();
    let mut mods = artificer::load_mods(&home)?;
    mods.enabled = enabled;
    artificer::save_mods(&home, &mods)?;
    if !enabled {
        artificer::serve_stop(&home)?;
    }
    println!(
        "artificer: {} for {}",
        if enabled { "enabled" } else { "disabled" },
        home.display()
    );
    if enabled && env::var_os("ARTIFICER_DISABLED").is_some() {
        println!("artificer: unset ARTIFICER_DISABLED to enable this shell");
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn install_cmd(args: &[String]) -> Result<ExitCode> {
    let Some((modify_path, remote)) = super::remote::install_args(args) else {
        return Ok(help::usage(help::Sub::Install));
    };
    let binary = env::current_exe().context("resolve the running binary")?;
    let control = artificer::control_home();
    let real = artificer::stock_cargo();
    artificer::check_real_cargo(&real, &control)?;
    artificer::serve_stop(&artificer::default_home())?;
    let report = artificer::install(&binary, &real, &artificer::cargo_home(), &control)?;
    println!("artificer: installed {}", report.binary.display());
    println!("artificer: shim {}", report.shim.display());
    println!("artificer: store {}", artificer::default_home().display());
    path_setup(&control, modify_path)?;
    super::remote::install_remote(remote)?;
    Ok(ExitCode::from(0))
}

#[cfg(unix)]
fn user_home(control: &Path) -> &Path {
    control.parent().unwrap_or(control)
}

#[cfg(unix)]
fn shell_profiles(control: &Path, shell: Option<&Path>) -> Vec<PathBuf> {
    let zdotdir = env::var_os("ZDOTDIR").map(PathBuf::from);
    artificer::profiles(user_home(control), shell, zdotdir.as_deref())
}

#[cfg(unix)]
fn path_setup(control: &Path, modify: bool) -> Result<()> {
    let line = artificer::profile_line(user_home(control), control);
    if !modify {
        println!("Add this line to your shell profile:\n{line}");
        return Ok(());
    }
    let shell = env::var_os("SHELL").map(PathBuf::from);
    let changed =
        artificer::add_to_profiles(&shell_profiles(control, shell.as_deref()), &line, control)?;
    for file in &changed {
        println!("artificer: added the shim to PATH in {}", file.display());
    }
    if changed.is_empty() {
        println!("artificer: shell profiles already put the shim on PATH");
    }
    println!("New shells use the shim. For this shell, run:\n{line}");
    Ok(())
}

#[cfg(windows)]
fn path_setup(control: &Path, modify: bool) -> Result<()> {
    let bin = control.join("bin");
    if !modify {
        println!(
            "Put {} first on your user PATH to use the shim.",
            bin.display()
        );
    } else if artificer::add_to_user_path(&bin)? {
        println!("artificer: added {} to your user PATH", bin.display());
    } else {
        println!("artificer: your user PATH already starts with the shim");
    }
    print!(
        "New terminals use the shim. For this shell, run:\n{}",
        artificer::env_script()
    );
    Ok(())
}

pub(super) fn uninstall_cmd(args: &[String]) -> Result<ExitCode> {
    let purge = match args {
        [] => false,
        [flag] if flag == "--purge" => true,
        _ => {
            return Ok(help::usage(help::Sub::Uninstall));
        }
    };
    let home = artificer::default_home();
    artificer::serve_stop(&home)?;
    let cargo = artificer::stock_cargo();
    let cargo_home = artificer::cargo_home();
    let control = artificer::control_home();
    let package = artificer::cargo_package(&cargo_home);
    #[cfg(unix)]
    for file in artificer::remove_from_profiles(
        &artificer::installed_profiles(
            user_home(&control),
            env::var_os("ZDOTDIR").map(PathBuf::from).as_deref(),
        ),
        &artificer::profile_line(user_home(&control), &control),
        &control,
    )? {
        println!("artificer: removed the PATH line from {}", file.display());
    }
    let remaining = artificer::uninstall(&cargo_home, &control)?;
    println!("artificer: removed the cargo shim");
    #[cfg(windows)]
    if artificer::remove_from_user_path(&control.join("bin"))? {
        println!("artificer: removed the shim from your user PATH");
    }
    report_purge(&home, purge)?;
    for path in remaining {
        println!(
            "artificer: remove this file after this command exits: {}",
            path.display()
        );
    }
    if let Some(package) = package {
        remove_package(&cargo, &package)?;
    }
    println!("Remove Artificer settings from your editor, if you added any.");
    #[cfg(unix)]
    println!("Start a new shell or run `hash -r` to refresh command lookup.");
    Ok(ExitCode::from(0))
}

fn report_purge(home: &Path, purge: bool) -> Result<()> {
    if !purge {
        println!("artificer: cache kept at {}", home.display());
    } else if artificer::purge(home)? {
        println!("artificer: deleted the cache at {}", home.display());
    } else {
        println!(
            "artificer: {} has no Artificer cache tag; left in place",
            home.display()
        );
    }
    Ok(())
}

#[cfg(unix)]
fn remove_package(cargo: &Path, package: &str) -> Result<()> {
    let status = Command::new(cargo)
        .args(["uninstall", package])
        .status()
        .context("run cargo uninstall")?;
    if !status.success() {
        println!("artificer: run `cargo uninstall {package}` to remove the binary");
    }
    Ok(())
}

#[cfg(windows)]
fn remove_package(_cargo: &Path, package: &str) -> Result<()> {
    println!("artificer: run `cargo uninstall {package}` to remove the binary");
    Ok(())
}

pub(super) fn why_fallback_cmd(args: &[String]) -> Result<ExitCode> {
    let limit = match args {
        [] => Some(5),
        [flag, value] if flag == "--limit" => value.parse::<usize>().ok(),
        _ => None,
    };
    let Some(limit) = limit else {
        return Ok(help::usage(help::Sub::WhyFallback));
    };
    let home = artificer::default_home();
    print!("{}", artificer::fallback_report(&home, limit));
    Ok(ExitCode::from(0))
}

pub(super) fn why_miss_cmd(args: &[String]) -> Result<ExitCode> {
    let home = artificer::default_home();
    let [name] = args else {
        return Ok(help::usage(help::Sub::WhyMiss));
    };
    let Some(report) = artificer::why_miss(&home, name) else {
        eprintln!("artificer: no key record for `{name}`; run a build first");
        return Ok(ExitCode::from(1));
    };
    print!("{report}");
    Ok(ExitCode::from(0))
}

pub(super) fn serve_cmd(args: &[String]) -> Result<ExitCode> {
    let home = artificer::default_home();
    if !args.is_empty() && !matches!(args, [arg] if arg == "stop" || arg == "--stop") {
        return Ok(help::usage(help::Sub::Serve));
    }
    if !args.is_empty() {
        artificer::serve_stop(&home)?;
        eprintln!("artificer: serve stopped");
        return Ok(ExitCode::from(0));
    }
    artificer::serve_listen(&home)?;
    Ok(ExitCode::from(0))
}

fn stat_json(home: &Path, stat: &artificer::StoreStat) -> Result<()> {
    let total = stat.hits + stat.misses;
    let hit_rate = if total == 0 {
        0.0
    } else {
        stat.hits as f64 / total as f64
    };
    println!(
        "{}",
        serde_json::json!({
            "home": home,
            "enabled": artificer::enabled(home)?,
            "units": stat.units,
            "bytes": stat.bytes,
            "hits": stat.hits,
            "misses": stat.misses,
            "hit_rate": hit_rate,
            "fallbacks": stat.fallbacks,
            "fallback_last": stat.fallback_last,
            "builds": stat.builds,
            "last_build": stat.last_build,
            "meta": stat.meta,
            "scratch": stat.scratch,
        })
    );
    Ok(())
}

pub(super) fn stat_cmd(args: &[String]) -> Result<ExitCode> {
    if !args.is_empty() && !matches!(args, [arg] if arg == "--json") {
        return Ok(help::usage(help::Sub::Stat));
    }
    let home = artificer::default_home();
    let stat = artificer::store_stat(&home)?;
    if args.first().is_some_and(|a| a == "--json") {
        stat_json(&home, &stat)?;
        return Ok(ExitCode::from(0));
    }
    let mb = stat.bytes as f64 / 1_048_576.0;
    println!("home={}", home.display());
    println!("units={}", stat.units);
    println!("store={mb:.1}M");
    let meta_mb = stat.meta_bytes as f64 / 1_048_576.0;
    println!("meta={} ({meta_mb:.1}M)", stat.meta);
    let scratch_mb = stat.scratch_bytes as f64 / 1_048_576.0;
    println!("scratch={} ({scratch_mb:.1}M)", stat.scratch);
    println!(
        "hits={} misses={} builds={}",
        stat.hits, stat.misses, stat.builds
    );
    if let Some(last) = &stat.last_build {
        println!("last={last}");
    }
    match &stat.fallback_last {
        Some(last) => println!("fallbacks={} last={last}", stat.fallbacks),
        None => println!("fallbacks={}", stat.fallbacks),
    }
    #[cfg(unix)]
    {
        let fifo = home.join("jobserver.fifo");
        println!(
            "jobserver={}",
            if fifo.exists() { "fifo" } else { "missing" }
        );
    }
    #[cfg(not(unix))]
    println!("jobserver=native");
    let mods = artificer::load_mods(&home)?;
    for (name, on) in mods.table() {
        println!("{name}={}", if on { "on" } else { "off" });
    }
    Ok(ExitCode::from(0))
}

pub(super) fn mods_cmd(args: &[String]) -> Result<ExitCode> {
    let home = artificer::default_home();
    let mut mods = artificer::load_mods(&home)?;
    match args {
        [] => {
            for (name, on) in mods.table() {
                println!("{name}={}", if on { "on" } else { "off" });
            }
        }
        [op, name] if op == "on" => {
            mods.set(name, true)?;
            artificer::save_mods(&home, &mods)?;
            println!("{name}=on");
        }
        [op, name] if op == "off" => {
            mods.set(name, false)?;
            artificer::save_mods(&home, &mods)?;
            println!("{name}=off");
        }
        _ => {
            return Ok(help::usage(help::Sub::Mods));
        }
    }
    Ok(ExitCode::from(0))
}
