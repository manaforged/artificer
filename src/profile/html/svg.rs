use super::super::report::time;
use super::super::{Outcome, Profile, Stage, UnitRecord, UnitTime, ms, steps, unit_times};
use super::escape;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const WIDTH: f64 = 1160.0;
const LABEL: f64 = 80.0;
const PLOT: f64 = WIDTH - LABEL - 10.0;
const ROW: f64 = 20.0;
const BAR: f64 = 16.0;
const AXIS: f64 = 20.0;
const CHART: f64 = 140.0;
const MAX_TICKS: u64 = 10;
const HIT: &str = "#7fb8e6";
const MISS: &str = "#f0a35e";
const FAILED: &str = "#d9534f";
const UNKNOWN: &str = "#bbbbbb";
const SETUP: &str = "#9e9e9e";
const CRITICAL: &str = "#6f2dbd";
const LINE: &str = "#2b6cb0";
const LIMIT: &str = "#d9534f";
const GRID: &str = "#d0d7de";

struct Scale {
    wall_us: u64,
}

impl Scale {
    fn x(&self, us: u64) -> f64 {
        LABEL + us.min(self.wall_us) as f64 * PLOT / self.wall_us.max(1) as f64
    }

    fn width(&self, start: u64, end: u64) -> f64 {
        (self.x(end) - self.x(start)).max(1.0)
    }
}

fn color(outcome: Option<Outcome>) -> &'static str {
    match outcome {
        Some(Outcome::Hit) => HIT,
        Some(Outcome::Miss) => MISS,
        Some(Outcome::Failed) => FAILED,
        None => UNKNOWN,
    }
}

fn ticks(out: &mut String, scale: &Scale, bottom: f64) {
    let wall_s = scale.wall_us.div_ceil(1_000_000).max(1);
    let step = wall_s.div_ceil(MAX_TICKS).max(1);
    let mut second = 0;
    while second <= wall_s {
        let x = scale.x(second * 1_000_000);
        out.push_str(&format!("<line x1=\"{x:.1}\" y1=\"0\" x2=\"{x:.1}\" y2=\"{bottom:.1}\" stroke=\"{GRID}\"/><text x=\"{x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{second}s</text>",
            bottom + 14.0
        ));
        second += step;
    }
}

fn rows(profile: &Profile) -> BTreeMap<Option<u16>, usize> {
    let workers: BTreeSet<u16> = profile
        .units
        .iter()
        .filter_map(|unit| unit.worker)
        .collect();
    let mut rows = BTreeMap::from([(None, 0)]);
    for (index, worker) in workers.into_iter().enumerate() {
        rows.insert(Some(worker), index + 1);
    }
    rows
}

fn row_labels(out: &mut String, rows: &BTreeMap<Option<u16>, usize>) {
    for (worker, row) in rows {
        let label = worker.map_or_else(|| "main".to_string(), |worker| format!("worker {worker}"));
        out.push_str(&format!(
            "<text x=\"4\" y=\"{:.1}\">{label}</text>",
            *row as f64 * ROW + BAR - 3.0
        ));
    }
}

fn tooltip(unit: &UnitRecord, stats: Option<&UnitTime>, start: u64, end: u64) -> String {
    let mut text = format!("{}\nduration {}", unit.label(), time(ms(end - start)));
    if let Some(stats) = stats {
        text.push_str(&format!(
            "\nwait {}\ncpu {}",
            time(stats.wait_ms),
            time(stats.cpu_ms)
        ));
    }
    text.push_str(&format!("\n{}", unit.outcome.map_or("-", Outcome::name)));
    escape(&text)
}

struct Lookup {
    rows: BTreeMap<Option<u16>, usize>,
    critical: HashSet<usize>,
    times: HashMap<u32, UnitTime>,
}

fn unit_rect(out: &mut String, scale: &Scale, lookup: &Lookup, index: usize, unit: &UnitRecord) {
    let (Some(start), Some(end)) = (unit.start_us, unit.end_us) else {
        return;
    };
    let row = lookup.rows.get(&unit.worker).copied().unwrap_or(0);
    let stroke = if lookup.critical.contains(&index) {
        format!(" stroke=\"{CRITICAL}\" stroke-width=\"2\"")
    } else {
        String::new()
    };
    let time = u32::try_from(index)
        .ok()
        .and_then(|index| lookup.times.get(&index));
    out.push_str(&format!("<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{BAR}\" fill=\"{}\"{stroke}><title>{}</title></rect>",
        scale.x(start),
        row as f64 * ROW,
        scale.width(start, end.max(start)),
        color(unit.outcome),
        tooltip(unit, time, start, end.max(start)),
    ));
}

fn setup_rects(out: &mut String, profile: &Profile, scale: &Scale) {
    for span in profile
        .spans
        .iter()
        .filter(|span| span.phase.stage() == Stage::Setup)
    {
        let end = span.end_us.max(span.start_us);
        out.push_str(&format!("<rect x=\"{:.1}\" y=\"0\" width=\"{:.1}\" height=\"{BAR}\" fill=\"{SETUP}\"><title>{} {}</title></rect>",
            scale.x(span.start_us),
            scale.width(span.start_us, end),
            span.phase.name(),
            time(ms(end - span.start_us)),
        ));
    }
}

pub(super) fn timeline(profile: &Profile, path: &[u32]) -> String {
    let scale = Scale {
        wall_us: profile.wall_us,
    };
    let lookup = Lookup {
        rows: rows(profile),
        critical: path
            .iter()
            .filter_map(|unit| usize::try_from(*unit).ok())
            .collect(),
        times: unit_times(profile)
            .into_iter()
            .map(|time| (time.unit, time))
            .collect(),
    };
    let bottom = lookup.rows.len() as f64 * ROW;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{:.0}\">",
        bottom + AXIS
    );
    ticks(&mut out, &scale, bottom);
    row_labels(&mut out, &lookup.rows);
    setup_rects(&mut out, profile, &scale);
    for (index, unit) in profile.units.iter().enumerate() {
        unit_rect(&mut out, &scale, &lookup, index, unit);
    }
    out.push_str("</svg>");
    out
}

fn levels(profile: &Profile) -> Vec<(u64, u32)> {
    let steps = steps(profile);
    let mut out = vec![(0, 0)];
    out.extend(steps.iter().map(|step| (step.start, step.running)));
    if let Some(last) = steps.last() {
        out.push((last.end, 0));
    }
    out
}

fn step_points(scale: &Scale, steps: &[(u64, u32)], y: impl Fn(u32) -> f64) -> String {
    let mut points = String::new();
    let mut level = 0;
    for (at, next) in steps {
        let x = scale.x(*at);
        points.push_str(&format!("{x:.1},{:.1} {x:.1},{:.1} ", y(level), y(*next)));
        level = *next;
    }
    points.push_str(&format!("{:.1},{:.1}", scale.x(scale.wall_us), y(level)));
    points
}

pub(super) fn concurrency(profile: &Profile) -> String {
    let scale = Scale {
        wall_us: profile.wall_us,
    };
    let steps = levels(profile);
    let peak = steps.iter().map(|(_, level)| *level).max().unwrap_or(0);
    let top = peak.max(profile.jobs).max(1);
    let y = |level: u32| CHART - f64::from(level) * CHART / f64::from(top);
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{:.0}\">",
        CHART + AXIS
    );
    ticks(&mut out, &scale, CHART);
    let limit = y(profile.jobs);
    out.push_str(&format!("<line x1=\"{LABEL}\" y1=\"{limit:.1}\" x2=\"{:.1}\" y2=\"{limit:.1}\" stroke=\"{LIMIT}\" stroke-dasharray=\"4 3\"/><text x=\"4\" y=\"{:.1}\">jobs {}</text>",
        LABEL + PLOT,
        limit + 4.0,
        profile.jobs
    ));
    out.push_str(&format!(
        "<polyline fill=\"none\" stroke=\"{LINE}\" stroke-width=\"1.5\" points=\"{}\"/></svg>",
        step_points(&scale, &steps, y)
    ));
    out
}
