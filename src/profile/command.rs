use super::report::{table, time};
use super::{Analysis, BuildResult, Profile};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

const NONE_YET: &str = "no build profiles yet; run a cargo build through Artificer first";

pub enum ProfileCommand {
    Show {
        id: Option<String>,
        json: bool,
        trace: Option<PathBuf>,
        html: Option<PathBuf>,
    },
    List {
        limit: usize,
        json: bool,
    },
    Diff {
        base: String,
        head: String,
        json: bool,
    },
}

#[derive(Serialize)]
struct ListRow<'a> {
    id: &'a str,
    started_at_ms: u64,
    wall_ms: u64,
    result: BuildResult,
    hit: usize,
    miss: usize,
    failed: usize,
    command: &'a [String],
}

pub fn profile_command(home: &Path, command: &ProfileCommand) -> Result<String> {
    match command {
        ProfileCommand::Show {
            id,
            json,
            trace,
            html,
        } => show(
            home,
            id.as_deref(),
            *json,
            trace.as_deref(),
            html.as_deref(),
        ),
        ProfileCommand::List { limit, json } => list(home, *limit, *json),
        ProfileCommand::Diff { base, head, json } => diff(home, base, head, *json),
    }
}

fn resolve(home: &Path, prefix: &str) -> Result<String> {
    let ids = super::list(home)?;
    if ids.is_empty() {
        bail!(NONE_YET);
    }
    if ids.iter().any(|id| id == prefix) {
        return Ok(prefix.to_string());
    }
    let matches: Vec<&str> = ids
        .iter()
        .map(String::as_str)
        .filter(|id| id.starts_with(prefix))
        .collect();
    match matches.as_slice() {
        [] => bail!("no profile matches {prefix}; run artificer profile list"),
        [one] => Ok((*one).to_string()),
        many => bail!("{prefix} matches several profiles: {}", many.join(", ")),
    }
}

fn pick(home: &Path, id: Option<&str>) -> Result<Profile> {
    match id {
        Some(prefix) => super::load(home, &resolve(home, prefix)?),
        None => super::latest(home)?.context(NONE_YET),
    }
}

fn write(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, body).with_context(|| format!("write {}", path.display()))
}

fn show(
    home: &Path,
    id: Option<&str>,
    json: bool,
    trace: Option<&Path>,
    html: Option<&Path>,
) -> Result<String> {
    let profile = pick(home, id)?;
    let analysis = super::analyze(&profile);
    let mut out = if json {
        format!("{}\n", serde_json::to_string(&analysis)?)
    } else {
        super::report::text(&analysis)
    };
    if let Some(path) = trace {
        write(path, &super::trace::render(&profile)?)?;
        out.push_str(&format!("wrote trace to {}\n", path.display()));
    }
    if let Some(path) = html {
        write(path, &super::html::render(&profile))?;
        out.push_str(&format!("wrote report to {}\n", path.display()));
    }
    Ok(out)
}

fn row<'a>(analysis: &'a Analysis) -> ListRow<'a> {
    ListRow {
        id: &analysis.id,
        started_at_ms: analysis.started_at_ms,
        wall_ms: analysis.wall_ms,
        result: analysis.result,
        hit: analysis.units.hit,
        miss: analysis.units.miss,
        failed: analysis.units.failed,
        command: &analysis.command,
    }
}

fn text_row(analysis: &Analysis) -> Vec<String> {
    vec![
        analysis.id.clone(),
        super::utc(analysis.started_at_ms),
        time(analysis.wall_ms),
        analysis.result.name().into(),
        format!("{} hit", analysis.units.hit),
        format!("{} miss", analysis.units.miss),
        analysis.command.join(" "),
    ]
}

fn list(home: &Path, limit: usize, json: bool) -> Result<String> {
    let ids = super::list(home)?;
    let loaded: Vec<(String, Result<Analysis>)> = ids
        .into_iter()
        .take(limit)
        .map(|id| {
            let analysis = super::load(home, &id).map(|profile| super::analyze(&profile));
            (id, analysis)
        })
        .collect();
    if json {
        let rows: Vec<ListRow<'_>> = loaded
            .iter()
            .filter_map(|(_, analysis)| analysis.as_ref().ok())
            .map(row)
            .collect();
        return Ok(format!("{}\n", serde_json::to_string(&rows)?));
    }
    if loaded.is_empty() {
        return Ok(format!("{NONE_YET}\n"));
    }
    let rows: Vec<Vec<String>> = loaded
        .iter()
        .map(|(id, analysis)| match analysis {
            Ok(analysis) => text_row(analysis),
            Err(error) => vec![id.clone(), format!("error: {error:#}")],
        })
        .collect();
    Ok(format!("{}\n", table(&rows)))
}

fn diff(home: &Path, base: &str, head: &str, json: bool) -> Result<String> {
    let base = super::load(home, &resolve(home, base)?)?;
    let head = super::load(home, &resolve(home, head)?)?;
    let diff = super::diff::diff(&base, &head);
    if json {
        return Ok(format!("{}\n", serde_json::to_string(&diff)?));
    }
    Ok(super::diff::text(&diff))
}
