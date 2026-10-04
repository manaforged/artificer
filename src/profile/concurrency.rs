use super::analyze::{Parallelism, Window};
use super::model::{Profile, ms};
use super::phase::Stage;

const LOW_WINDOW_US: u64 = 1_000_000;
const LOW_SHARE: u32 = 4;
const WINDOWS: usize = 5;
const WINDOW_UNITS: usize = 6;

pub(crate) struct Step {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) running: u32,
}

pub(crate) fn steps(profile: &Profile) -> Vec<Step> {
    let mut events: Vec<(u64, i32)> = profile
        .spans
        .iter()
        .filter(|span| span.phase.stage() == Stage::Process)
        .flat_map(|span| [(span.start_us, 1), (span.end_us, -1)])
        .collect();
    events.sort_unstable();
    let mut out = Vec::new();
    let mut running: i32 = 0;
    for pair in events.windows(2) {
        let [(at, delta), (next, _)] = pair else {
            continue;
        };
        running += delta;
        if next > at {
            out.push(Step {
                start: *at,
                end: *next,
                running: u32::try_from(running.max(0)).unwrap_or(0),
            });
        }
    }
    out
}

pub(crate) fn parallelism(profile: &Profile, segments: &[Step]) -> Parallelism {
    let busy: u64 = segments
        .iter()
        .map(|s| (s.end - s.start) * u64::from(s.running))
        .sum();
    let span = match (segments.first(), segments.last()) {
        (Some(first), Some(last)) => last.end - first.start,
        _ => 0,
    };
    let capacity = span * u64::from(profile.jobs.max(1));
    Parallelism {
        average: if span == 0 {
            0.0
        } else {
            busy as f64 / span as f64
        },
        peak: segments.iter().map(|s| s.running).max().unwrap_or(0),
        busy_ms: ms(busy),
        idle_core_ms: ms(capacity.saturating_sub(busy)),
    }
}

pub(crate) fn low_windows(profile: &Profile, segments: &[Step]) -> Vec<Window> {
    let threshold = (profile.jobs / LOW_SHARE).max(1);
    let mut windows: Vec<(u64, u64, u32)> = Vec::new();
    for segment in segments.iter().filter(|s| s.running <= threshold) {
        match windows.last_mut() {
            Some(last) if last.1 == segment.start => {
                last.1 = segment.end;
                last.2 = last.2.max(segment.running);
            }
            _ => windows.push((segment.start, segment.end, segment.running)),
        }
    }
    windows.retain(|(start, end, _)| end - start >= LOW_WINDOW_US);
    windows.sort_by_key(|(start, end, _)| std::cmp::Reverse(end - start));
    windows.truncate(WINDOWS);
    windows.sort_by_key(|(start, _, _)| *start);
    let mut names: Vec<Vec<String>> = vec![Vec::new(); windows.len()];
    for span in profile
        .spans
        .iter()
        .filter(|span| span.phase.stage() == Stage::Process)
    {
        let first = windows.partition_point(|(_, end, _)| *end <= span.start_us);
        for (slot, (start, _, _)) in windows.iter().enumerate().skip(first) {
            if *start >= span.end_us {
                break;
            }
            let name = span
                .unit
                .and_then(|unit| usize::try_from(unit).ok())
                .and_then(|unit| profile.units.get(unit))
                .map_or_else(|| span.phase.name().to_string(), |unit| unit.name.clone());
            if let Some(list) = names.get_mut(slot) {
                list.push(name);
            }
        }
    }
    windows
        .into_iter()
        .zip(names)
        .map(|((start, end, running), mut units)| {
            units.sort();
            units.dedup();
            units.truncate(WINDOW_UNITS);
            Window {
                start_ms: ms(start),
                end_ms: ms(end),
                running,
                units,
            }
        })
        .collect()
}
