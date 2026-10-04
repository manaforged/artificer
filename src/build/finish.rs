use super::*;

pub(super) fn finished(dir: &Path, name: &str, started: std::time::Instant) {
    let profile = json_profile(dir, name, false);
    let optimized = profile["opt_level"]
        .as_str()
        .is_some_and(|level| level != "0");
    let debuginfo = profile["debuginfo"].as_u64().is_some_and(|level| level > 0);
    let shape = match (optimized, debuginfo) {
        (true, true) => "optimized + debuginfo",
        (true, false) => "optimized",
        (false, true) => "unoptimized + debuginfo",
        (false, false) => "unoptimized",
    };
    crate::out::status(
        crate::out::Status::Finished,
        format!(
            "`{name}` profile [{shape}] target(s) in {}",
            crate::out::elapsed(started.elapsed())
        ),
    );
}

pub(super) fn record_stats(
    home: &Path,
    compiled: &HashMap<String, compile::Compiled>,
    started: std::time::Instant,
    op: &str,
) {
    let hits = compiled
        .values()
        .filter(|c| c.rustc == RustcOutcome::Restored)
        .count() as u64;
    let misses = compiled
        .values()
        .filter(|c| c.rustc == RustcOutcome::Ran)
        .count() as u64;
    record_counts(home, hits, misses, started, op);
}

pub(super) fn record_counts(
    home: &Path,
    hits: u64,
    misses: u64,
    started: std::time::Instant,
    op: &str,
) {
    if let Err(error) = crate::store::bump_stats(home, hits, misses) {
        crate::out::err(format!("artificer: stats not recorded: {error:#}"));
    }
    crate::store::note_build(
        home,
        op,
        hits,
        misses,
        started.elapsed().as_millis() as u64,
        None,
    );
    crate::remote::push_built(home);
}
