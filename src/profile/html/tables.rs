use super::super::report::time;
use super::super::{Analysis, Outcome, Stage, UnitTime};
use super::escape;

fn row(out: &mut String, cells: &[String]) {
    out.push_str("<tr>");
    for cell in cells {
        out.push_str(&format!("<td>{cell}</td>"));
    }
    out.push_str("</tr>");
}

fn table(out: &mut String, title: &str, head: &[&str], rows: &[Vec<String>]) {
    out.push_str(&format!("<h2>{}</h2><table><tr>", escape(title)));
    for cell in head {
        out.push_str(&format!("<th>{}</th>", escape(cell)));
    }
    out.push_str("</tr>");
    for cells in rows {
        row(out, cells);
    }
    out.push_str("</table>");
}

pub(super) fn summary(out: &mut String, analysis: &Analysis, result: &str) {
    let units = &analysis.units;
    let par = &analysis.parallelism;
    let rows = vec![
        vec!["Wall".into(), time(analysis.wall_ms)],
        vec!["CPU".into(), time(analysis.cpu_ms)],
        vec!["Result".into(), escape(result)],
        vec![
            "Units".into(),
            format!(
                "{} total, {} hit, {} miss, {} failed",
                units.total, units.hit, units.miss, units.failed
            ),
        ],
        vec![
            "Jobs".into(),
            format!("{} on {} cores", analysis.jobs, analysis.cores),
        ],
        vec![
            "Parallelism".into(),
            format!("{:.2} average, {} peak", par.average, par.peak),
        ],
        vec!["Idle core time".into(), time(par.idle_core_ms)],
        vec!["Setup".into(), time(analysis.setup_ms)],
        vec!["Tail".into(), time(analysis.tail_ms)],
        vec!["Critical path".into(), time(analysis.critical_ms)],
    ];
    table(out, "Summary", &["Measure", "Value"], &rows);
}

fn unit_label(unit: &UnitTime) -> String {
    escape(&format!("{} {}", unit.name, unit.version))
}

fn critical(out: &mut String, analysis: &Analysis) {
    let rows: Vec<Vec<String>> = analysis
        .critical_path
        .iter()
        .map(|unit| {
            vec![
                time(unit.start_ms),
                time(unit.duration_ms),
                time(unit.wait_ms),
                unit.outcome.map_or("-", Outcome::name).into(),
                unit_label(unit),
            ]
        })
        .collect();
    table(
        out,
        "Critical path",
        &["Start", "Duration", "Wait", "Outcome", "Unit"],
        &rows,
    );
}

fn top(out: &mut String, analysis: &Analysis) {
    let rows: Vec<Vec<String>> = analysis
        .top_units
        .iter()
        .map(|unit| {
            vec![
                unit_label(unit),
                time(unit.duration_ms),
                time(unit.cpu_ms),
                time(unit.wait_ms),
                unit.outcome.map_or("-", Outcome::name).into(),
            ]
        })
        .collect();
    table(
        out,
        "Top units",
        &["Unit", "Duration", "CPU", "Wait", "Outcome"],
        &rows,
    );
}

fn phases(out: &mut String, analysis: &Analysis) {
    let rows: Vec<Vec<String>> = analysis
        .phases
        .iter()
        .filter(|phase| phase.stage != Stage::Wrapper)
        .map(|phase| {
            vec![
                phase.phase.name().into(),
                phase.count.to_string(),
                time(phase.total_ms),
                time(phase.cpu_ms),
            ]
        })
        .collect();
    table(out, "Phases", &["Phase", "Count", "Total", "CPU"], &rows);
}

fn windows(out: &mut String, analysis: &Analysis) {
    let rows: Vec<Vec<String>> = analysis
        .low_parallelism
        .iter()
        .map(|window| {
            vec![
                time(window.start_ms),
                time(window.end_ms.saturating_sub(window.start_ms)),
                window.running.to_string(),
                escape(&window.units.join(", ")),
            ]
        })
        .collect();
    table(
        out,
        "Low parallelism",
        &["Start", "Length", "Running", "Units"],
        &rows,
    );
}

fn compiler(out: &mut String, analysis: &Analysis) {
    if analysis.compiler.is_empty() {
        return;
    }
    let rows: Vec<Vec<String>> = analysis
        .compiler
        .iter()
        .map(|total| vec![total.category.name().into(), time(total.total_ms)])
        .collect();
    table(out, "Compiler passes", &["Category", "Total"], &rows);
}

pub(super) fn details(out: &mut String, analysis: &Analysis) {
    critical(out, analysis);
    top(out, analysis);
    phases(out, analysis);
    windows(out, analysis);
    compiler(out, analysis);
}
