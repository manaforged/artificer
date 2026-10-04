use super::{Profile, Span, US_PER_MS, UnitRecord, critical_path, steps, utc};
use serde_json::{Map, Value, json};
use std::cmp::Reverse;

const PID: u64 = 1;
const MAIN_TID: u64 = 0;
const CRITICAL_TID: u64 = 100_000;
const BYTES_PER_MB: f64 = 1_048_576.0;

struct Event {
    tid: u64,
    ts: u64,
    dur: u64,
    value: Value,
}

fn tid(worker: Option<u16>) -> u64 {
    worker.map_or(MAIN_TID, |worker| u64::from(worker) + 1)
}

fn ms(us: u64) -> f64 {
    us as f64 / US_PER_MS as f64
}

fn complete(name: String, cat: Value, tid: u64, ts: u64, dur: u64, args: Value) -> Event {
    let value = json!({
        "name": name,
        "cat": cat,
        "ph": "X",
        "ts": ts,
        "dur": dur,
        "pid": PID,
        "tid": tid,
        "args": args,
    });
    Event {
        tid,
        ts,
        dur,
        value,
    }
}

fn unit_event(unit: &UnitRecord, tid: u64, cat: &str) -> Option<Event> {
    let (start, end) = (unit.start_us?, unit.end_us?);
    let args = json!({
        "outcome": unit.outcome,
        "role": unit.role,
        "package": unit.package,
    });
    Some(complete(
        unit.label(),
        json!(cat),
        tid,
        start,
        end.saturating_sub(start),
        args,
    ))
}

fn span_args(span: &Span) -> Value {
    let mut args = Map::new();
    if let Some(usage) = span.usage {
        args.insert("user_ms".into(), json!(ms(usage.user_us)));
        args.insert("system_ms".into(), json!(ms(usage.system_us)));
        args.insert(
            "peak_rss_mb".into(),
            json!(usage.peak_rss_bytes as f64 / BYTES_PER_MB),
        );
    }
    if !span.passes.is_empty() {
        let passes: Map<String, Value> = span
            .passes
            .iter()
            .map(|pass| (pass.name.clone(), json!(ms(pass.us))))
            .collect();
        args.insert("passes".into(), Value::Object(passes));
    }
    Value::Object(args)
}

fn span_event(profile: &Profile, span: &Span) -> Event {
    let unit = span
        .unit
        .and_then(|unit| usize::try_from(unit).ok())
        .and_then(|unit| profile.units.get(unit));
    let name = unit.map_or_else(
        || span.phase.name().to_string(),
        |unit| format!("{} {}", span.phase.name(), unit.name),
    );
    complete(
        name,
        json!(span.phase.stage()),
        tid(span.worker),
        span.start_us,
        span.end_us.saturating_sub(span.start_us),
        span_args(span),
    )
}

fn critical_events(profile: &Profile) -> Vec<Event> {
    critical_path(profile)
        .into_iter()
        .filter_map(|unit| usize::try_from(unit).ok())
        .filter_map(|unit| profile.units.get(unit))
        .filter_map(|unit| unit_event(unit, CRITICAL_TID, "critical"))
        .collect()
}

fn timed_events(profile: &Profile) -> Vec<Event> {
    let mut events: Vec<Event> = profile
        .units
        .iter()
        .filter_map(|unit| unit_event(unit, tid(unit.worker), "unit"))
        .collect();
    events.extend(profile.spans.iter().map(|span| span_event(profile, span)));
    events.extend(critical_events(profile));
    events.sort_by_key(|event| (event.tid, event.ts, Reverse(event.dur)));
    events
}

fn thread_name(tid: u64, name: &str) -> Value {
    json!({
        "name": "thread_name",
        "ph": "M",
        "pid": PID,
        "tid": tid,
        "args": { "name": name },
    })
}

fn metadata(profile: &Profile, events: &[Event]) -> Vec<Value> {
    let mut out = vec![json!({
        "name": "process_name",
        "ph": "M",
        "pid": PID,
        "tid": MAIN_TID,
        "args": { "name": format!("artificer {}", profile.command.join(" ")) },
    })];
    let mut tids: Vec<u64> = events.iter().map(|event| event.tid).collect();
    tids.push(MAIN_TID);
    tids.sort_unstable();
    tids.dedup();
    out.extend(tids.into_iter().map(|tid| match tid {
        MAIN_TID => thread_name(tid, "main"),
        CRITICAL_TID => thread_name(tid, "critical path"),
        worker => thread_name(worker, &format!("worker {}", worker - 1)),
    }));
    out
}

fn counter(at: u64, running: u32) -> Value {
    json!({
        "name": "processes",
        "ph": "C",
        "ts": at,
        "pid": PID,
        "args": { "running": running },
    })
}

fn counters(profile: &Profile) -> Vec<Value> {
    let steps = steps(profile);
    let mut out: Vec<Value> = steps
        .iter()
        .map(|step| counter(step.start, step.running))
        .collect();
    if let Some(last) = steps.last() {
        out.push(counter(last.end, 0));
    }
    out
}

pub(crate) fn render(profile: &Profile) -> anyhow::Result<String> {
    let events = timed_events(profile);
    let mut trace = metadata(profile, &events);
    trace.extend(events.into_iter().map(|event| event.value));
    trace.extend(counters(profile));
    let document = json!({
        "traceEvents": trace,
        "displayTimeUnit": "ms",
        "otherData": {
            "id": profile.id,
            "command": profile.command.join(" "),
            "started": utc(profile.started_at_ms),
            "wall_ms": ms(profile.wall_us),
            "jobs": profile.jobs,
            "cores": profile.cores,
        },
    });
    Ok(serde_json::to_string(&document)?)
}
