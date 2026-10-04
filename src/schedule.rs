use crate::cargo;
use crate::compile::{self, Compiled, TestBin};
use crate::session::Session;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex};

const WAITING: &str = "build failed, waiting for other jobs to finish...";

pub type CompiledTests = (HashMap<String, Compiled>, HashMap<String, Vec<TestBin>>);

type Graph = (Vec<Unit>, HashMap<Unit, Vec<Unit>>);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Unit {
    Pkg(String),
    Extra(String),
}

struct State<T> {
    want: HashSet<Unit>,
    remaining: HashSet<Unit>,
    done: HashSet<Unit>,
    in_flight: usize,
    out: Vec<(Unit, T)>,
    failed: Vec<anyhow::Error>,
    reported: bool,
    score: HashMap<Unit, usize>,
}

impl<T> State<T> {
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

    fn pick(&mut self, deps: &HashMap<Unit, Vec<Unit>>) -> Option<Unit> {
        let id = self
            .remaining
            .iter()
            .filter(|id| {
                deps.get(*id).is_none_or(|d| {
                    d.iter()
                        .all(|dep| self.done.contains(dep) || !self.want.contains(dep))
                })
            })
            .max_by_key(|id| self.score.get(*id).copied().unwrap_or(0))
            .cloned()?;
        self.remaining.remove(&id);
        self.in_flight += 1;
        Some(id)
    }
}

fn scores(units: &[Unit], deps: &HashMap<Unit, Vec<Unit>>) -> HashMap<Unit, usize> {
    let want: HashSet<&Unit> = units.iter().collect();
    let mut waiters: HashMap<&Unit, Vec<&Unit>> = HashMap::new();
    for id in units {
        if let Some(ds) = deps.get(id) {
            for d in ds.iter().filter(|d| want.contains(d)) {
                waiters.entry(d).or_default().push(id);
            }
        }
    }
    fn reach<'a>(
        id: &'a Unit,
        waiters: &HashMap<&'a Unit, Vec<&'a Unit>>,
        memo: &mut HashMap<&'a Unit, HashSet<&'a Unit>>,
    ) -> HashSet<&'a Unit> {
        if let Some(hit) = memo.get(id) {
            return hit.clone();
        }
        let mut all = HashSet::new();
        memo.insert(id, HashSet::new());
        for w in waiters.get(id).map(Vec::as_slice).unwrap_or_default() {
            all.insert(*w);
            all.extend(reach(w, waiters, memo));
        }
        memo.insert(id, all.clone());
        all
    }
    let mut memo = HashMap::new();
    units
        .iter()
        .map(|id| {
            let n = reach(id, &waiters, &mut memo).len();
            (id.clone(), n)
        })
        .collect()
}

fn run_units<T: Send>(
    units: Vec<Unit>,
    deps: HashMap<Unit, Vec<Unit>>,
    work: impl Fn(&Unit) -> Result<T> + Sync,
) -> Result<Vec<(Unit, T)>> {
    let count = units.len();
    let order: HashMap<Unit, usize> = units
        .iter()
        .enumerate()
        .map(|(i, u)| (u.clone(), i))
        .collect();
    let state = Mutex::new(State {
        want: units.iter().cloned().collect(),
        remaining: units.iter().cloned().collect(),
        done: HashSet::new(),
        in_flight: 0,
        out: Vec::new(),
        failed: Vec::new(),
        reported: false,
        score: scores(&units, &deps),
    });
    let wake = Condvar::new();

    std::thread::scope(|s| {
        let work = &work;
        let mut joins = Vec::new();
        for _ in 0..job_cap().min(count) {
            let sink = crate::out::current();
            let state = &state;
            let wake = &wake;
            let deps = &deps;
            joins.push(s.spawn(move || {
                crate::out::attach(sink);
                loop {
                    let unit = {
                        let mut st = state.lock().expect("state");
                        loop {
                            if !st.failed.is_empty() || st.remaining.is_empty() {
                                return;
                            }
                            if let Some(unit) = st.pick(deps) {
                                break unit;
                            }
                            if st.in_flight == 0 {
                                st.fail(anyhow::anyhow!("cycle in compile graph"));
                                wake.notify_all();
                                return;
                            }
                            st = wake.wait(st).expect("state");
                        }
                    };
                    let r = work(&unit);
                    let mut st = state.lock().expect("state");
                    st.in_flight -= 1;
                    match r {
                        Ok(value) => {
                            st.done.insert(unit.clone());
                            st.out.push((unit, value));
                        }
                        Err(e) => st.fail(e),
                    }
                    wake.notify_all();
                }
            }));
        }
        for j in joins {
            j.join().expect("compile worker");
        }
    });

    let st = state
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(unmodeled) = st.failed.into_iter().find(|e| e.is::<cargo::Unmodeled>()) {
        return Err(unmodeled);
    }
    if st.reported {
        return Err(crate::out::Reported.into());
    }
    let mut out = st.out;
    out.sort_by_key(|(unit, _)| order.get(unit).copied().unwrap_or(usize::MAX));
    Ok(out)
}

pub fn compile_ids(
    sess: &Session,
    meta: &cargo::Metadata,
    ids: &[String],
) -> Result<HashMap<String, Compiled>> {
    let (units, deps) = graph_and_extras(meta, ids, &[], false)?;
    sess.learn_links(meta);
    let done = run_units(units, deps, |unit| match unit {
        Unit::Pkg(id) => compile::compile_pkg(sess, meta, id),
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
    let (units, deps) = graph_and_extras(meta, ids, roots, true)?;
    sess.learn_links(meta);
    enum Done {
        Lib(Option<Compiled>),
        Tests(Vec<TestBin>),
    }
    let done = run_units(units, deps, |unit| match unit {
        Unit::Pkg(id) => compile::compile_pkg(sess, meta, id).map(Done::Lib),
        Unit::Extra(id) => compile::compile_tests(sess, meta, id, sel).map(Done::Tests),
    })?;
    let mut libs = HashMap::new();
    let mut tests = HashMap::new();
    for (unit, done) in done {
        match (unit, done) {
            (Unit::Pkg(id), Done::Lib(Some(compiled))) => {
                libs.insert(id, compiled);
            }
            (Unit::Pkg(_), Done::Lib(None)) => {}
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
    tests: bool,
    all: bool,
) -> Result<HashMap<String, Compiled>> {
    let (units, deps) = graph_and_extras(meta, ids, roots, tests || all)?;
    sess.learn_links(meta);
    let done = run_units(units, deps, |unit| match unit {
        Unit::Pkg(id) => compile::compile_pkg(sess, meta, id),
        Unit::Extra(id) => compile::check_extras(sess, meta, id, tests, all).map(|()| None),
    })?;
    let mut out = HashMap::new();
    for (unit, compiled) in done {
        if let (Unit::Pkg(id), Some(compiled)) = (unit, compiled) {
            out.insert(id, compiled);
        }
    }
    Ok(out)
}

fn graph_and_extras(
    meta: &cargo::Metadata,
    ids: &[String],
    roots: &[String],
    dev: bool,
) -> Result<Graph> {
    let mut units = Vec::new();
    let mut deps = HashMap::new();
    for id in ids {
        units.push(Unit::Pkg(id.clone()));
        deps.insert(Unit::Pkg(id.clone()), pkg_deps(meta, id)?);
    }
    for root in roots {
        units.push(Unit::Extra(root.clone()));
        let direct = if dev {
            cargo::test_compile_deps(meta, root)?
        } else {
            cargo::compile_deps(meta, root)?
        };
        let mut d: Vec<Unit> = direct.into_iter().map(Unit::Pkg).collect();
        d.push(Unit::Pkg(root.clone()));
        deps.insert(Unit::Extra(root.clone()), d);
    }
    Ok((units, deps))
}

fn pkg_deps(meta: &cargo::Metadata, id: &str) -> Result<Vec<Unit>> {
    Ok(cargo::compile_deps(meta, id)?
        .into_iter()
        .map(Unit::Pkg)
        .collect())
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

#[cfg(test)]
#[path = "schedule_tests.rs"]
mod tests;
