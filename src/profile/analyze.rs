use super::concurrency::{low_windows, parallelism, steps};
use super::model::{Outcome, Profile, Role, UnitRecord, ms};
use super::passes::{Category, categorize};
use super::phase::{Phase, Stage};
use serde::Serialize;
use std::collections::HashMap;

const TOP: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildResult {
    Built,
    Failed,
    Fallback,
}

impl BuildResult {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Built => "built",
            Self::Failed => "failed",
            Self::Fallback => "fallback",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct UnitCounts {
    pub total: usize,
    pub hit: usize,
    pub miss: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Parallelism {
    pub average: f64,
    pub peak: u32,
    pub busy_ms: u64,
    pub idle_core_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PhaseTotal {
    pub phase: Phase,
    pub stage: Stage,
    pub count: u32,
    pub total_ms: u64,
    pub cpu_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct UnitTime {
    pub unit: u32,
    pub name: String,
    pub version: String,
    pub role: Role,
    pub outcome: Option<Outcome>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub duration_ms: u64,
    pub wait_ms: u64,
    pub cpu_ms: u64,
    pub peak_rss_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Window {
    pub start_ms: u64,
    pub end_ms: u64,
    pub running: u32,
    pub units: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CategoryTotal {
    pub category: Category,
    pub total_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Analysis {
    pub id: String,
    pub command: Vec<String>,
    pub started_at_ms: u64,
    pub wall_ms: u64,
    pub cpu_ms: u64,
    pub cores: u32,
    pub jobs: u32,
    pub result: BuildResult,
    pub fallback: Option<String>,
    pub units: UnitCounts,
    pub setup_ms: u64,
    pub tail_ms: u64,
    pub parallelism: Parallelism,
    pub critical_ms: u64,
    pub critical_path: Vec<UnitTime>,
    pub top_units: Vec<UnitTime>,
    pub phases: Vec<PhaseTotal>,
    pub low_parallelism: Vec<Window>,
    pub compiler: Vec<CategoryTotal>,
}

pub(crate) fn utc(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rest = secs % 86_400;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

struct Costs {
    cpu: Vec<u64>,
    rss: Vec<u64>,
}

fn costs(profile: &Profile) -> Costs {
    let mut cpu = vec![0u64; profile.units.len()];
    let mut rss = vec![0u64; profile.units.len()];
    for span in &profile.spans {
        let (Some(unit), Some(usage)) = (span.unit, span.usage) else {
            continue;
        };
        let Ok(index) = usize::try_from(unit) else {
            continue;
        };
        if let Some(total) = cpu.get_mut(index) {
            *total += usage.user_us + usage.system_us;
        }
        if let Some(peak) = rss.get_mut(index) {
            *peak = (*peak).max(usage.peak_rss_bytes);
        }
    }
    Costs { cpu, rss }
}

fn ready_us(profile: &Profile, unit: &UnitRecord, origin: u64) -> u64 {
    unit.deps
        .iter()
        .filter_map(|dep| usize::try_from(*dep).ok())
        .filter_map(|dep| profile.units.get(dep))
        .filter_map(|dep| dep.end_us)
        .max()
        .unwrap_or(origin)
}

fn unit_time(profile: &Profile, costs: &Costs, index: usize, origin: u64) -> Option<UnitTime> {
    let unit = profile.units.get(index)?;
    let (start, end) = (unit.start_us?, unit.end_us?);
    let ready = ready_us(profile, unit, origin).min(start);
    Some(UnitTime {
        unit: u32::try_from(index).ok()?,
        name: unit.name.clone(),
        version: unit.version.clone(),
        role: unit.role,
        outcome: unit.outcome,
        start_ms: ms(start),
        end_ms: ms(end),
        duration_ms: ms(end.saturating_sub(start)),
        wait_ms: ms(start - ready),
        cpu_ms: ms(costs.cpu.get(index).copied().unwrap_or(0)),
        peak_rss_bytes: costs.rss.get(index).copied().unwrap_or(0),
    })
}

fn origin_us(profile: &Profile) -> u64 {
    profile
        .units
        .iter()
        .filter_map(|unit| unit.start_us)
        .min()
        .unwrap_or(profile.wall_us)
}

pub(crate) fn critical_path(profile: &Profile) -> Vec<u32> {
    let mut path = Vec::new();
    let last = profile
        .units
        .iter()
        .enumerate()
        .filter_map(|(index, unit)| unit.end_us.map(|end| (end, index)))
        .max();
    let mut cursor = last.map(|(_, index)| index);
    while let Some(index) = cursor {
        let Some(unit) = profile.units.get(index) else {
            break;
        };
        path.push(u32::try_from(index).unwrap_or(u32::MAX));
        cursor = unit
            .deps
            .iter()
            .filter_map(|dep| usize::try_from(*dep).ok())
            .filter_map(|dep| {
                profile
                    .units
                    .get(dep)
                    .and_then(|d| d.end_us)
                    .map(|end| (end, dep))
            })
            .max()
            .map(|(_, dep)| dep);
    }
    path.reverse();
    path
}

fn counts(profile: &Profile) -> UnitCounts {
    let mut counts = UnitCounts {
        total: profile.units.len(),
        ..UnitCounts::default()
    };
    for unit in &profile.units {
        match unit.outcome {
            Some(Outcome::Hit) => counts.hit += 1,
            Some(Outcome::Miss) => counts.miss += 1,
            Some(Outcome::Failed) => counts.failed += 1,
            None => {}
        }
    }
    counts
}

fn phases(profile: &Profile) -> Vec<PhaseTotal> {
    let mut totals: HashMap<Phase, PhaseTotal> = HashMap::new();
    for span in &profile.spans {
        let total = totals.entry(span.phase).or_insert(PhaseTotal {
            phase: span.phase,
            stage: span.phase.stage(),
            count: 0,
            total_ms: 0,
            cpu_ms: 0,
        });
        total.count += 1;
        total.total_ms += ms(span.end_us.saturating_sub(span.start_us));
        total.cpu_ms += span
            .usage
            .map_or(0, |usage| ms(usage.user_us + usage.system_us));
    }
    let mut out: Vec<PhaseTotal> = totals.into_values().collect();
    out.sort_by(|a, b| b.total_ms.cmp(&a.total_ms).then(a.phase.cmp(&b.phase)));
    out
}

fn compiler(profile: &Profile) -> Vec<CategoryTotal> {
    let mut totals: HashMap<Category, u64> = HashMap::new();
    for span in profile.spans.iter().filter(|span| !span.passes.is_empty()) {
        for (category, us) in categorize(&span.passes) {
            *totals.entry(category).or_default() += us;
        }
    }
    let mut out: Vec<CategoryTotal> = totals
        .into_iter()
        .map(|(category, us)| CategoryTotal {
            category,
            total_ms: ms(us),
        })
        .collect();
    out.sort_by(|a, b| {
        b.total_ms
            .cmp(&a.total_ms)
            .then(a.category.cmp(&b.category))
    });
    out
}

fn result(profile: &Profile) -> BuildResult {
    if profile.fallback.is_some() {
        BuildResult::Fallback
    } else if profile.failed {
        BuildResult::Failed
    } else {
        BuildResult::Built
    }
}

fn all_times(profile: &Profile, costs: &Costs, origin: u64) -> Vec<UnitTime> {
    (0..profile.units.len())
        .filter_map(|unit| unit_time(profile, costs, unit, origin))
        .collect()
}

pub(crate) fn unit_times(profile: &Profile) -> Vec<UnitTime> {
    all_times(profile, &costs(profile), origin_us(profile))
}

pub(crate) fn analyze(profile: &Profile) -> Analysis {
    let costs = costs(profile);
    let origin = origin_us(profile);
    let path: Vec<UnitTime> = critical_path(profile)
        .into_iter()
        .filter_map(|unit| usize::try_from(unit).ok())
        .filter_map(|unit| unit_time(profile, &costs, unit, origin))
        .collect();
    let mut top = all_times(profile, &costs, origin);
    top.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms).then(a.unit.cmp(&b.unit)));
    top.truncate(TOP);
    let last_end = profile.units.iter().filter_map(|u| u.end_us).max();
    let segments = steps(profile);
    Analysis {
        id: profile.id.clone(),
        command: profile.command.clone(),
        started_at_ms: profile.started_at_ms,
        wall_ms: ms(profile.wall_us),
        cpu_ms: ms(profile
            .spans
            .iter()
            .filter_map(|span| span.usage)
            .map(|usage| usage.user_us + usage.system_us)
            .sum()),
        cores: profile.cores,
        jobs: profile.jobs,
        result: result(profile),
        fallback: profile.fallback.clone(),
        units: counts(profile),
        setup_ms: ms(origin.min(profile.wall_us)),
        tail_ms: ms(last_end.map_or(0, |end| profile.wall_us.saturating_sub(end))),
        critical_ms: path
            .iter()
            .map(|step| step.duration_ms + step.wait_ms)
            .sum(),
        critical_path: path,
        top_units: top,
        parallelism: parallelism(profile, &segments),
        phases: phases(profile),
        low_parallelism: low_windows(profile, &segments),
        compiler: compiler(profile),
    }
}

#[cfg(test)]
#[path = "analyze_tests.rs"]
mod tests;
