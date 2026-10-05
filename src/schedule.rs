use crate::cargo;
use crate::compile::{self, Compiled, TestBin};
use crate::session::Session;
use anyhow::Result;
use plan::{Plan, Waits};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

const WAITING: &str = "build failed, waiting for other jobs to finish...";

pub type CompiledTests = (HashMap<String, Compiled>, HashMap<String, Vec<TestBin>>);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Unit {
    Pkg(String),
    Script(String),
    Extra(String),
}

struct Ready {
    want: HashSet<Unit>,
    remaining: HashSet<Unit>,
    done: HashSet<Unit>,
    meta: HashSet<Unit>,
    early: HashSet<Unit>,
    early_ok: HashSet<Unit>,
    in_flight: usize,
    failed: Vec<anyhow::Error>,
    reported: bool,
    score: HashMap<Unit, usize>,
    waits: Waits,
}

impl Ready {
    fn new(plan: &Plan) -> Self {
        Self {
            want: plan.units.iter().cloned().collect(),
            remaining: plan.units.iter().cloned().collect(),
            done: HashSet::new(),
            meta: HashSet::new(),
            early: HashSet::new(),
            early_ok: plan.early_ok.clone(),
            in_flight: 0,
            failed: Vec::new(),
            reported: false,
            score: plan::scores(&plan.units, &plan.deps),
            waits: plan::waits(plan),
        }
    }

    fn fail(&mut self, error: anyhow::Error) {
        if !error.is::<cargo::Unmodeled>() {
            let first = !self.reported;
            self.reported = true;
            if !error.is::<crate::out::Reported>() {
                crate::out::error(format!("{error:#}"));
            }
            if first && self.in_flight > 0 {
                crate::out::warning(WAITING);
            }
        }
        self.failed.push(error);
    }

    fn has(&self, dep: &Unit, full: bool, early: bool) -> bool {
        !self.want.contains(dep)
            || self.done.contains(dep)
            || (!full && (self.meta.contains(dep) || (early && self.early.contains(dep))))
    }

    fn startable(&self, unit: &Unit) -> bool {
        let early = self.early_ok.contains(unit);
        let meta = self
            .waits
            .meta
            .get(unit)
            .is_none_or(|deps| deps.iter().all(|dep| self.has(dep, false, early)));
        let full = self
            .waits
            .full
            .get(unit)
            .is_none_or(|deps| deps.iter().all(|dep| self.has(dep, true, false)));
        meta && full
    }

    fn pick(&mut self) -> Option<Unit> {
        let id = self
            .remaining
            .iter()
            .filter(|id| self.startable(id))
            .max_by_key(|id| self.score.get(*id).copied().unwrap_or(0))
            .cloned()?;
        self.remaining.remove(&id);
        self.in_flight += 1;
        Some(id)
    }
}

struct Board {
    ready: Mutex<Ready>,
    wake: Condvar,
}

impl Board {
    fn lock(&self) -> MutexGuard<'_, Ready> {
        self.ready.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn metadata(&self, unit: &Unit, stage: MetaStage) {
        {
            let mut ready = self.lock();
            let set = match stage {
                MetaStage::Early => &mut ready.early,
                MetaStage::Full => &mut ready.meta,
            };
            set.insert(unit.clone());
        }
        self.wake.notify_all();
    }

    fn next(&self) -> Option<Unit> {
        let mut ready = self.lock();
        loop {
            if !ready.failed.is_empty() || ready.remaining.is_empty() {
                return None;
            }
            if let Some(unit) = ready.pick() {
                return Some(unit);
            }
            if ready.in_flight == 0 {
                ready.fail(anyhow::anyhow!("cycle in compile graph"));
                self.wake.notify_all();
                return None;
            }
            ready = self
                .wake
                .wait(ready)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn finish(&self, unit: Unit, failure: Option<anyhow::Error>) {
        let mut ready = self.lock();
        ready.in_flight -= 1;
        match failure {
            None => {
                ready.done.insert(unit);
            }
            Some(error) => ready.fail(error),
        }
        self.wake.notify_all();
    }
}

struct Running<'a> {
    board: &'a Board,
    unit: Option<Unit>,
}

impl Running<'_> {
    fn finish(mut self, failure: Option<anyhow::Error>) {
        if let Some(unit) = self.unit.take() {
            self.board.finish(unit, failure);
        }
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        if let Some(unit) = self.unit.take() {
            self.board
                .finish(unit, Some(anyhow::anyhow!("a build worker panicked")));
        }
    }
}

fn run_units<T: Send>(
    meta: &cargo::Metadata,
    plan: Plan,
    work: impl Fn(&Unit) -> Result<T> + Sync,
) -> Result<Vec<(Unit, T)>> {
    let count = plan.units.len();
    let order: HashMap<Unit, usize> = plan
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| (u.clone(), i))
        .collect();
    let base = crate::profile::current()
        .and_then(|_| crate::profile::plan(plan::planned(meta, &plan, &order)));
    let board = Arc::new(Board {
        ready: Mutex::new(Ready::new(&plan)),
        wake: Condvar::new(),
    });
    let outputs: Mutex<Vec<(Unit, T)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for worker in 0..job_cap().min(count) {
            let sink = crate::out::current();
            let recorder = crate::profile::current();
            let board = Arc::clone(&board);
            let (work, order, outputs) = (&work, &order, &outputs);
            s.spawn(move || {
                crate::out::attach(sink);
                crate::profile::attach(recorder);
                crate::profile::worker(worker);
                while let Some(unit) = board.next() {
                    let index = base.and_then(|base| {
                        let at = u32::try_from(*order.get(&unit)?).ok()?;
                        Some(base.saturating_add(at))
                    });
                    let running = Running {
                        board: &board,
                        unit: Some(unit.clone()),
                    };
                    let result = crate::profile::unit(index, || {
                        ready::with(Arc::clone(&board), &unit, || work(&unit))
                    });
                    match result {
                        Ok(value) => {
                            outputs
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .push((unit, value));
                            running.finish(None);
                        }
                        Err(error) => running.finish(Some(error)),
                    }
                }
            });
        }
    });
    let (failed, reported) = {
        let mut ready = board.lock();
        (std::mem::take(&mut ready.failed), ready.reported)
    };
    if let Some(unmodeled) = failed.into_iter().find(|e| e.is::<cargo::Unmodeled>()) {
        return Err(unmodeled);
    }
    if reported {
        return Err(crate::out::Reported.into());
    }
    let mut out = outputs.into_inner().unwrap_or_else(PoisonError::into_inner);
    out.sort_by_key(|(unit, _)| order.get(unit).copied().unwrap_or(usize::MAX));
    Ok(out)
}

pub fn compile_ids(
    sess: &Session,
    meta: &cargo::Metadata,
    ids: &[String],
) -> Result<HashMap<String, Compiled>> {
    let plan = plan::plan(sess, meta, ids, &[], false)?;
    sess.learn_links(meta);
    let done = run_units(meta, plan, |unit| match unit {
        Unit::Pkg(id) => compile::compile_pkg(sess, meta, id),
        Unit::Script(id) => compile::run_script(sess, meta, id).map(|()| None),
        Unit::Extra(_) => unreachable!("no extras scheduled"),
    })?;
    let mut out = HashMap::new();
    for (unit, compiled) in done {
        if let (Unit::Pkg(id), Some(compiled)) = (unit, compiled) {
            out.insert(id, compiled);
        }
    }
    Ok(out)
}

pub fn compile_ids_and_tests(
    sess: &Session,
    meta: &cargo::Metadata,
    ids: &[String],
    roots: &[String],
    sel: &compile::TestSel,
) -> Result<CompiledTests> {
    let plan = plan::plan(sess, meta, ids, roots, true)?;
    sess.learn_links(meta);
    enum Done {
        Lib(Option<Compiled>),
        Tests(Vec<TestBin>),
    }
    let done = run_units(meta, plan, |unit| match unit {
        Unit::Pkg(id) => compile::compile_pkg(sess, meta, id).map(Done::Lib),
        Unit::Script(id) => compile::run_script(sess, meta, id).map(|()| Done::Lib(None)),
        Unit::Extra(id) => compile::compile_tests(sess, meta, id, sel).map(Done::Tests),
    })?;
    let mut libs = HashMap::new();
    let mut tests = HashMap::new();
    for (unit, done) in done {
        match (unit, done) {
            (Unit::Pkg(id), Done::Lib(Some(compiled))) => {
                libs.insert(id, compiled);
            }
            (Unit::Pkg(_) | Unit::Script(_), Done::Lib(None)) => {}
            (Unit::Extra(id), Done::Tests(bins)) => {
                tests.insert(id, bins);
            }
            _ => unreachable!("unit kinds match their work"),
        }
    }
    Ok((libs, tests))
}

pub fn compile_ids_and_extras(
    sess: &Session,
    meta: &cargo::Metadata,
    ids: &[String],
    roots: &[String],
    sel: &crate::build::TargetSel,
) -> Result<HashMap<String, Compiled>> {
    let plan = plan::plan(sess, meta, ids, roots, sel.wants_dev())?;
    sess.learn_links(meta);
    let done = run_units(meta, plan, |unit| match unit {
        Unit::Pkg(id) => compile::compile_pkg(sess, meta, id),
        Unit::Script(id) => compile::run_script(sess, meta, id).map(|()| None),
        Unit::Extra(id) => compile::check_extras(sess, meta, id, sel).map(|()| None),
    })?;
    let mut out = HashMap::new();
    for (unit, compiled) in done {
        if let (Unit::Pkg(id), Some(compiled)) = (unit, compiled) {
            out.insert(id, compiled);
        }
    }
    Ok(out)
}

static JOBS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

pub fn set_jobs(jobs: usize) {
    if JOBS.set(jobs.max(1)).is_err() {}
}

pub(crate) fn explicit_jobs() -> bool {
    JOBS.get().is_some()
}

pub(crate) fn job_cap() -> usize {
    if let Some(jobs) = JOBS.get() {
        return *jobs;
    }
    let jobs = std::env::var("ARTIFICER_JOBS")
        .ok()
        .or_else(|| std::env::var("CARGO_BUILD_JOBS").ok());
    if let Some(jobs) = jobs
        && let Ok(jobs) = jobs.parse::<usize>()
    {
        return jobs.max(1);
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1)
}

mod fan;
pub(crate) use fan::fan_out;
mod plan;
mod ready;
pub(crate) use ready::{MetaStage, signal};

#[cfg(test)]
#[path = "schedule_tests.rs"]
mod tests;
