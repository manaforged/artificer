mod analyze;
mod command;
mod concurrency;
mod diff;
mod files;
mod html;
mod model;
mod passes;
mod phase;
mod record;
mod report;
mod trace;
mod usage;

pub use analyze::{Analysis, BuildResult, CategoryTotal, PhaseTotal, UnitTime, Window};
pub(crate) use analyze::{analyze, critical_path, unit_times, utc};
pub use command::{ProfileCommand, profile_command};
pub(crate) use concurrency::steps;
pub(crate) use files::{latest, list, load};
pub use model::{Outcome, Part, Profile, Role, Span, UnitRecord};
pub(crate) use model::{US_PER_MS, label, ms};
pub(crate) use passes::{BOOTSTRAP, FLAGS, harvest};
pub use phase::{Phase, ProcessPhase, RunPhase, SetupPhase, Stage, UnitPhase, WrapperPhase};
pub(crate) use record::{
    PlannedUnit, attach, capture, current, fallback, id, merge, note_passes, note_target, plan,
    unit, worker,
};
pub use record::{Recording, begin, request_timings, span};
pub(crate) use usage::{output, status};

pub(crate) const PASSES_VAR: &str = "ARTIFICER_PASSES";
pub(crate) const TIMING_VAR: &str = "ARTIFICER_TIMING";

pub(crate) fn passes_enabled() -> bool {
    std::env::var_os(PASSES_VAR).is_some()
}

pub(crate) fn timing_enabled() -> bool {
    std::env::var_os(TIMING_VAR).is_some()
}
