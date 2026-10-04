use super::model::{MetaStage, Outcome, Role, UnitRecord};
use super::record::{Recorder, current, current_unit, current_worker, set_unit};
use std::sync::Arc;

pub(crate) struct PlannedUnit {
    pub(crate) package: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) role: Role,
    pub(crate) links: bool,
    pub(crate) early: bool,
    pub(crate) deps: Vec<usize>,
}

#[derive(Clone)]
pub(crate) struct MetaMark {
    recorder: Arc<Recorder>,
    unit: u32,
}

impl MetaMark {
    pub(crate) fn mark(&self, stage: MetaStage) {
        self.recorder.mark(self.unit, |unit, now| {
            let at = match stage {
                MetaStage::Early => &mut unit.early_us,
                MetaStage::Full => &mut unit.meta_us,
            };
            at.get_or_insert(now);
        });
    }
}

pub(crate) fn meta_mark() -> Option<MetaMark> {
    Some(MetaMark {
        recorder: current()?,
        unit: current_unit()?,
    })
}

pub(crate) fn plan(units: Vec<PlannedUnit>) -> Option<u32> {
    let recorder = current()?;
    let mut state = recorder.lock();
    let base = u32::try_from(state.units.len()).ok()?;
    state.units.extend(units.into_iter().map(|unit| {
        UnitRecord {
            package: unit.package,
            name: unit.name,
            version: unit.version,
            role: unit.role,
            deps: unit
                .deps
                .into_iter()
                .filter_map(|dep| u32::try_from(dep).ok())
                .map(|dep| base.saturating_add(dep))
                .collect(),
            start_us: None,
            end_us: None,
            meta_us: None,
            early_us: None,
            links: unit.links,
            early: unit.early,
            worker: None,
            outcome: None,
        }
    }));
    Some(base)
}

pub(crate) fn unit<T, E>(index: Option<u32>, work: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    let (Some(index), Some(recorder)) = (index, current()) else {
        return work();
    };
    let worker = current_worker();
    recorder.mark(index, |unit, now| {
        unit.start_us = Some(now);
        unit.worker = worker;
    });
    set_unit(Some(index));
    let result = work();
    set_unit(None);
    let failed = result.is_err();
    recorder.mark(index, |unit, now| {
        unit.end_us = Some(now);
        unit.outcome = Some(match (failed, unit.outcome) {
            (true, _) => Outcome::Failed,
            (false, Some(Outcome::Miss)) => Outcome::Miss,
            _ => Outcome::Hit,
        });
    });
    result
}

#[cfg(test)]
#[path = "units_tests.rs"]
mod tests;
