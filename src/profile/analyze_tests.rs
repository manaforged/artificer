use super::*;
use crate::profile::model::{Part, SCHEMA, Span, Usage};
use crate::profile::phase::ProcessPhase;

fn unit(name: &str, deps: &[u32], start_ms: u64, end_ms: u64) -> UnitRecord {
    UnitRecord {
        package: format!("path+file:///ws/{name}#{name}@0.1.0"),
        name: name.to_string(),
        version: "0.1.0".to_string(),
        role: Role::Package,
        deps: deps.to_vec(),
        start_us: Some(start_ms * 1000),
        end_us: Some(end_ms * 1000),
        worker: Some(0),
        outcome: Some(Outcome::Miss),
    }
}

fn rustc(unit: u32, start_ms: u64, end_ms: u64, cpu_ms: u64) -> Span {
    Span {
        phase: Phase::Process(ProcessPhase::Rustc),
        unit: Some(unit),
        worker: Some(0),
        start_us: start_ms * 1000,
        end_us: end_ms * 1000,
        usage: Some(Usage {
            user_us: cpu_ms * 1000,
            system_us: 0,
            peak_rss_bytes: 1 << 20,
        }),
        passes: Vec::new(),
    }
}

fn profile(part: Part, wall_ms: u64) -> Profile {
    Profile {
        schema: SCHEMA,
        id: "test".to_string(),
        started_at_ms: 0,
        wall_us: wall_ms * 1000,
        command: vec!["build".to_string()],
        dir: "/ws".to_string(),
        target_dir: None,
        jobs: 4,
        cores: 4,
        passes: false,
        failed: false,
        fallback: None,
        units: part.units,
        spans: part.spans,
    }
}

#[test]
fn the_critical_path_follows_the_dependency_that_ended_last_and_reports_its_wait() {
    let part = Part {
        units: vec![
            unit("a", &[], 10, 100),
            unit("side", &[], 10, 40),
            unit("b", &[0, 1], 150, 300),
            unit("c", &[2], 300, 500),
        ],
        spans: vec![
            rustc(0, 10, 100, 80),
            rustc(1, 10, 40, 30),
            rustc(2, 150, 300, 140),
            rustc(3, 300, 500, 190),
        ],
        target_dir: None,
    };
    let analysis = analyze(&profile(part, 520));
    let path: Vec<(&str, u64, u64)> = analysis
        .critical_path
        .iter()
        .map(|step| (step.name.as_str(), step.wait_ms, step.cpu_ms))
        .collect();
    assert_eq!(path, [("a", 0, 80), ("b", 50, 140), ("c", 0, 190)]);
    assert_eq!(analysis.critical_ms, 90 + 50 + 150 + 200);
    assert_eq!(analysis.setup_ms, 10);
    assert_eq!(analysis.tail_ms, 20);
    assert_eq!(analysis.cpu_ms, 440);
    assert_eq!(analysis.parallelism.peak, 2);
    assert_eq!(analysis.parallelism.busy_ms, 90 + 30 + 150 + 200);
}

#[test]
fn a_gap_with_one_process_running_is_reported_as_a_low_parallelism_window() {
    let part = Part {
        units: vec![unit("wide", &[], 0, 100), unit("narrow", &[0], 100, 1600)],
        spans: vec![
            rustc(0, 0, 100, 10),
            rustc(0, 0, 100, 10),
            rustc(1, 100, 1600, 10),
        ],
        target_dir: None,
    };
    let analysis = analyze(&profile(part, 1600));
    let windows: Vec<(u64, u64, u32, Vec<String>)> = analysis
        .low_parallelism
        .iter()
        .map(|w| (w.start_ms, w.end_ms, w.running, w.units.clone()))
        .collect();
    assert_eq!(windows, [(100, 1600, 1, vec!["narrow".to_string()])]);
}

#[test]
fn utc_formats_unix_milliseconds_across_leap_days_and_centuries() {
    assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
    assert_eq!(utc(951_782_400_123), "2000-02-29 00:00:00 UTC");
    assert_eq!(utc(1_759_600_000_999), "2025-10-04 17:46:40 UTC");
    assert_eq!(utc(4_102_444_800_000), "2100-01-01 00:00:00 UTC");
}
