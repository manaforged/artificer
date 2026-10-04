use super::{Analysis, CategoryTotal, Outcome, PhaseTotal, Stage, UnitTime, Window, label};
use std::time::Duration;

const MIB: u64 = 1 << 20;
const PERCENT: u64 = 100;
const INDENT: &str = "  ";
const GAP: &str = "  ";

pub(crate) fn time(ms: u64) -> String {
    crate::out::elapsed(Duration::from_millis(ms))
}

pub(crate) fn signed(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "+" };
    format!("{sign}{}", time(ms.unsigned_abs()))
}

pub(crate) fn table(rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = Vec::new();
    for row in rows {
        for (column, cell) in row.iter().enumerate() {
            match widths.get_mut(column) {
                Some(width) => *width = (*width).max(cell.chars().count()),
                None => widths.push(cell.chars().count()),
            }
        }
    }
    let lines: Vec<String> = rows
        .iter()
        .map(|row| {
            let cells: Vec<String> = row
                .iter()
                .zip(&widths)
                .map(|(cell, width)| format!("{cell:width$}"))
                .collect();
            format!("{INDENT}{}", cells.join(GAP).trim_end())
        })
        .collect();
    lines.join("\n")
}

pub(crate) fn section(title: &str, rows: &[Vec<String>]) -> String {
    format!("{title}\n{}", table(rows))
}

fn header(analysis: &Analysis) -> String {
    let result = match &analysis.fallback {
        Some(reason) => format!("{}: {reason}", analysis.result.name()),
        None => analysis.result.name().to_string(),
    };
    section(
        &format!("profile {}", analysis.id),
        &[
            vec!["command".into(), analysis.command.join(" ")],
            vec!["started".into(), super::utc(analysis.started_at_ms)],
            vec!["result".into(), result],
        ],
    )
}

fn summary(analysis: &Analysis) -> String {
    let units = &analysis.units;
    let parallel = &analysis.parallelism;
    section(
        "summary",
        &[
            vec!["wall".into(), time(analysis.wall_ms)],
            vec!["cpu".into(), time(analysis.cpu_ms)],
            vec![
                "jobs".into(),
                format!("{} on {} cores", analysis.jobs, analysis.cores),
            ],
            vec![
                "units".into(),
                format!(
                    "{} total, {} hit, {} miss, {} failed",
                    units.total, units.hit, units.miss, units.failed
                ),
            ],
            vec!["setup".into(), time(analysis.setup_ms)],
            vec!["tail".into(), time(analysis.tail_ms)],
            vec![
                "parallelism".into(),
                format!(
                    "{:.1} average, {} peak, {} idle core time",
                    parallel.average,
                    parallel.peak,
                    time(parallel.idle_core_ms)
                ),
            ],
        ],
    )
}

fn unit_label(unit: &UnitTime) -> String {
    label(&unit.name, &unit.version, unit.role)
}

fn critical(analysis: &Analysis) -> Option<String> {
    if analysis.critical_path.is_empty() {
        return None;
    }
    let mut rows = vec![vec![
        "start".into(),
        "duration".into(),
        "wait".into(),
        "outcome".into(),
        "cpu".into(),
        "unit".into(),
    ]];
    rows.extend(analysis.critical_path.iter().map(|step| {
        vec![
            time(step.start_ms),
            time(step.duration_ms),
            time(step.wait_ms),
            step.outcome.map_or("-", Outcome::name).into(),
            time(step.cpu_ms),
            unit_label(step),
        ]
    }));
    Some(section(
        &format!("critical path {}", time(analysis.critical_ms)),
        &rows,
    ))
}

fn phase_rows(phases: &[&PhaseTotal]) -> Vec<Vec<String>> {
    phases
        .iter()
        .map(|phase| {
            vec![
                phase.phase.name().into(),
                format!("{}x", phase.count),
                time(phase.total_ms),
                format!("cpu {}", time(phase.cpu_ms)),
            ]
        })
        .collect()
}

fn phases(analysis: &Analysis) -> Option<String> {
    let shown: Vec<&PhaseTotal> = analysis
        .phases
        .iter()
        .filter(|phase| phase.stage != Stage::Wrapper)
        .collect();
    let (setup, work): (Vec<&PhaseTotal>, Vec<&PhaseTotal>) = shown
        .into_iter()
        .partition(|phase| phase.stage == Stage::Setup);
    let mut parts = Vec::new();
    if !work.is_empty() {
        parts.push(section("where the time went", &phase_rows(&work)));
    }
    if !setup.is_empty() {
        parts.push(section("artificer setup", &phase_rows(&setup)));
    }
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn top(analysis: &Analysis) -> Option<String> {
    if analysis.top_units.is_empty() {
        return None;
    }
    let mut rows = vec![vec![
        "duration".into(),
        "wait".into(),
        "cpu".into(),
        "rss".into(),
        "outcome".into(),
        "unit".into(),
    ]];
    rows.extend(analysis.top_units.iter().map(|unit| {
        vec![
            time(unit.duration_ms),
            time(unit.wait_ms),
            time(unit.cpu_ms),
            format!("{} MB", unit.peak_rss_bytes / MIB),
            unit.outcome.map_or("-", Outcome::name).into(),
            unit_label(unit),
        ]
    }));
    Some(section("top units", &rows))
}

fn window_row(window: &Window) -> Vec<String> {
    vec![
        time(window.start_ms),
        time(window.end_ms.saturating_sub(window.start_ms)),
        format!("{} running", window.running),
        window.units.join(", "),
    ]
}

fn idle(analysis: &Analysis) -> Option<String> {
    if analysis.low_parallelism.is_empty() {
        return None;
    }
    let mut rows = vec![vec![
        "start".into(),
        "length".into(),
        "most".into(),
        "units".into(),
    ]];
    rows.extend(analysis.low_parallelism.iter().map(window_row));
    Some(section("idle cores", &rows))
}

fn compiler(analysis: &Analysis) -> Option<String> {
    if analysis.compiler.is_empty() {
        return None;
    }
    let total: u64 = analysis.compiler.iter().map(|c| c.total_ms).sum();
    let rows: Vec<Vec<String>> = analysis
        .compiler
        .iter()
        .map(|category: &CategoryTotal| {
            vec![
                category.category.name().into(),
                time(category.total_ms),
                format!("{}%", category.total_ms * PERCENT / total.max(1)),
            ]
        })
        .collect();
    Some(section("compiler passes", &rows))
}

pub(crate) fn text(analysis: &Analysis) -> String {
    let parts: Vec<String> = [
        Some(header(analysis)),
        Some(summary(analysis)),
        critical(analysis),
        phases(analysis),
        top(analysis),
        idle(analysis),
        compiler(analysis),
    ]
    .into_iter()
    .flatten()
    .collect();
    format!("{}\n", parts.join("\n\n"))
}
