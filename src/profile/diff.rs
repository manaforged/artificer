use super::report::{section, signed, time};
use super::{Analysis, Outcome, Phase, Profile, Role, UnitRecord, label, ms};
use serde::Serialize;
use std::collections::HashMap;

const TOP: usize = 15;

#[derive(Clone, Debug, Serialize)]
pub struct Delta {
    pub base: u64,
    pub head: u64,
    pub delta: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PhaseDelta {
    pub phase: Phase,
    pub base_ms: u64,
    pub head_ms: u64,
    pub delta_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct UnitDelta {
    pub package: String,
    pub name: String,
    pub version: String,
    pub role: Role,
    pub base_ms: Option<u64>,
    pub head_ms: Option<u64>,
    pub delta_ms: i64,
    pub base_outcome: Option<Outcome>,
    pub head_outcome: Option<Outcome>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diff {
    pub base: String,
    pub head: String,
    pub wall_ms: Delta,
    pub cpu_ms: Delta,
    pub hit: Delta,
    pub miss: Delta,
    pub phases: Vec<PhaseDelta>,
    pub units: Vec<UnitDelta>,
    pub base_critical_path: Vec<String>,
    pub head_critical_path: Vec<String>,
}

fn signed_delta(base: u64, head: u64) -> i64 {
    let base = i64::try_from(base).unwrap_or(i64::MAX);
    let head = i64::try_from(head).unwrap_or(i64::MAX);
    head.saturating_sub(base)
}

fn delta(base: u64, head: u64) -> Delta {
    Delta {
        base,
        head,
        delta: signed_delta(base, head),
    }
}

fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn phases(base: &Analysis, head: &Analysis) -> Vec<PhaseDelta> {
    let mut totals: HashMap<Phase, (u64, u64)> = HashMap::new();
    for phase in &base.phases {
        totals.entry(phase.phase).or_default().0 = phase.total_ms;
    }
    for phase in &head.phases {
        totals.entry(phase.phase).or_default().1 = phase.total_ms;
    }
    let mut out: Vec<PhaseDelta> = totals
        .into_iter()
        .map(|(phase, (base_ms, head_ms))| PhaseDelta {
            phase,
            base_ms,
            head_ms,
            delta_ms: signed_delta(base_ms, head_ms),
        })
        .collect();
    out.sort_by(|a, b| {
        b.delta_ms
            .unsigned_abs()
            .cmp(&a.delta_ms.unsigned_abs())
            .then(a.phase.cmp(&b.phase))
    });
    out
}

fn unit_delta(base: Option<&UnitRecord>, head: Option<&UnitRecord>) -> Option<UnitDelta> {
    let shown = head.or(base)?;
    let base_ms = base.and_then(UnitRecord::duration_us).map(ms);
    let head_ms = head.and_then(UnitRecord::duration_us).map(ms);
    Some(UnitDelta {
        package: shown.package.clone(),
        name: shown.name.clone(),
        version: shown.version.clone(),
        role: shown.role,
        base_ms,
        head_ms,
        delta_ms: signed_delta(base_ms.unwrap_or(0), head_ms.unwrap_or(0)),
        base_outcome: base.and_then(|unit| unit.outcome),
        head_outcome: head.and_then(|unit| unit.outcome),
    })
}

fn units(base: &Profile, head: &Profile) -> Vec<UnitDelta> {
    let mut by_key: HashMap<(&str, Role), &UnitRecord> = base
        .units
        .iter()
        .map(|unit| ((unit.package.as_str(), unit.role), unit))
        .collect();
    let mut out: Vec<UnitDelta> = head
        .units
        .iter()
        .filter_map(|unit| {
            let before = by_key.remove(&(unit.package.as_str(), unit.role));
            unit_delta(before, Some(unit))
        })
        .collect();
    out.extend(
        by_key
            .into_values()
            .filter_map(|unit| unit_delta(Some(unit), None)),
    );
    out.sort_by(|a, b| {
        b.delta_ms
            .unsigned_abs()
            .cmp(&a.delta_ms.unsigned_abs())
            .then_with(|| a.package.cmp(&b.package))
    });
    out.truncate(TOP);
    out
}

fn path_names(analysis: &Analysis) -> Vec<String> {
    analysis
        .critical_path
        .iter()
        .map(|step| label(&step.name, &step.version, step.role))
        .collect()
}

pub(crate) fn diff(base: &Profile, head: &Profile) -> Diff {
    let before = super::analyze(base);
    let after = super::analyze(head);
    Diff {
        base: before.id.clone(),
        head: after.id.clone(),
        wall_ms: delta(before.wall_ms, after.wall_ms),
        cpu_ms: delta(before.cpu_ms, after.cpu_ms),
        hit: delta(count(before.units.hit), count(after.units.hit)),
        miss: delta(count(before.units.miss), count(after.units.miss)),
        phases: phases(&before, &after),
        units: units(base, head),
        base_critical_path: path_names(&before),
        head_critical_path: path_names(&after),
    }
}

fn time_row(name: &str, value: &Delta) -> Vec<String> {
    vec![
        name.into(),
        time(value.base),
        time(value.head),
        signed(value.delta),
    ]
}

fn count_row(name: &str, value: &Delta) -> Vec<String> {
    vec![
        name.into(),
        value.base.to_string(),
        value.head.to_string(),
        format!("{:+}", value.delta),
    ]
}

fn totals(diff: &Diff) -> String {
    section(
        &format!("diff {} -> {}", diff.base, diff.head),
        &[
            vec![String::new(), "base".into(), "head".into(), "delta".into()],
            time_row("wall", &diff.wall_ms),
            time_row("cpu", &diff.cpu_ms),
            count_row("hit", &diff.hit),
            count_row("miss", &diff.miss),
        ],
    )
}

fn phase_section(diff: &Diff) -> Option<String> {
    let rows: Vec<Vec<String>> = diff
        .phases
        .iter()
        .filter(|phase| phase.delta_ms != 0)
        .map(|phase| {
            vec![
                phase.phase.name().into(),
                time(phase.base_ms),
                time(phase.head_ms),
                signed(phase.delta_ms),
            ]
        })
        .collect();
    (!rows.is_empty()).then(|| section("phases", &rows))
}

fn side(ms: Option<u64>) -> String {
    ms.map_or_else(|| "absent".into(), time)
}

fn outcome_change(unit: &UnitDelta) -> String {
    if unit.base_outcome == unit.head_outcome {
        return unit.head_outcome.map_or("-", Outcome::name).into();
    }
    format!(
        "{} -> {}",
        unit.base_outcome.map_or("-", Outcome::name),
        unit.head_outcome.map_or("-", Outcome::name)
    )
}

fn unit_section(diff: &Diff) -> Option<String> {
    if diff.units.is_empty() {
        return None;
    }
    let rows: Vec<Vec<String>> = diff
        .units
        .iter()
        .map(|unit| {
            vec![
                side(unit.base_ms),
                side(unit.head_ms),
                signed(unit.delta_ms),
                outcome_change(unit),
                label(&unit.name, &unit.version, unit.role),
            ]
        })
        .collect();
    Some(section("unit changes", &rows))
}

fn path_section(title: &str, names: &[String]) -> Option<String> {
    (!names.is_empty()).then(|| format!("{title}\n  {}", names.join(" -> ")))
}

pub(crate) fn text(diff: &Diff) -> String {
    let parts: Vec<String> = [
        Some(totals(diff)),
        phase_section(diff),
        unit_section(diff),
        path_section("base critical path", &diff.base_critical_path),
        path_section("head critical path", &diff.head_critical_path),
    ]
    .into_iter()
    .flatten()
    .collect();
    format!("{}\n", parts.join("\n\n"))
}
