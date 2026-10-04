use super::model::{Outcome, Part, Pass, Profile, SCHEMA, Span, UnitRecord, Usage};
use super::phase::{Phase, ProcessPhase, WrapperPhase};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) struct Recorder {
    epoch: Instant,
    id: Option<String>,
    state: Mutex<Part>,
    fallback: Mutex<Option<String>>,
    timings: AtomicBool,
}

impl Recorder {
    fn new(id: Option<String>) -> Self {
        Self {
            epoch: Instant::now(),
            id,
            state: Mutex::new(Part::default()),
            fallback: Mutex::new(None),
            timings: AtomicBool::new(false),
        }
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, Part> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> u64 {
        micros(self.epoch.elapsed())
    }

    pub(super) fn mark(&self, index: u32, edit: impl FnOnce(&mut UnitRecord, u64)) {
        let now = self.now();
        let mut state = self.lock();
        if let Some(unit) = usize::try_from(index)
            .ok()
            .and_then(|index| state.units.get_mut(index))
        {
            edit(unit, now);
        }
    }
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Recorder>>> = const { RefCell::new(None) };
    static WORKER: Cell<Option<u16>> = const { Cell::new(None) };
    static UNIT: Cell<Option<u32>> = const { Cell::new(None) };
    static OPEN: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static SCRIPTS: Cell<u32> = const { Cell::new(0) };
}

pub(super) fn current_worker() -> Option<u16> {
    WORKER.get()
}

pub(super) fn current_unit() -> Option<u32> {
    UNIT.get()
}

pub(super) fn set_unit(unit: Option<u32>) {
    UNIT.set(unit);
}

pub(crate) fn current() -> Option<Arc<Recorder>> {
    CURRENT.with_borrow(Clone::clone)
}

pub(crate) fn attach(recorder: Option<Arc<Recorder>>) {
    CURRENT.with_borrow_mut(|slot| *slot = recorder);
    OPEN.with_borrow_mut(Vec::clear);
    UNIT.set(None);
    SCRIPTS.set(0);
}

pub(crate) fn worker(worker: usize) {
    WORKER.set(u16::try_from(worker).ok());
}

pub(crate) fn id() -> Option<String> {
    current().and_then(|recorder| recorder.id.clone())
}

pub(crate) fn fallback(reason: &str) {
    if let Some(recorder) = current() {
        *recorder
            .fallback
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(reason.to_string());
    }
}

pub fn request_timings() {
    if let Some(recorder) = current() {
        recorder.timings.store(true, Ordering::Relaxed);
    }
}

pub struct Recording {
    recorder: Arc<Recorder>,
    started_at_ms: u64,
    command: Vec<String>,
    dir: PathBuf,
}

pub fn begin(command: &[String], dir: &Path) -> Recording {
    let started_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(0));
    let id = format!("{started_at_ms:013}-{}", std::process::id());
    let recorder = Arc::new(Recorder::new(Some(id)));
    attach(Some(Arc::clone(&recorder)));
    Recording {
        recorder,
        started_at_ms,
        command: command.to_vec(),
        dir: dir.to_path_buf(),
    }
}

impl Recording {
    pub fn finish(self, home: &Path, failed: bool) {
        attach(None);
        let wall_us = self.recorder.now();
        let part = std::mem::take(&mut *self.recorder.lock());
        let fallback = self
            .recorder
            .fallback
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let profile = Profile {
            schema: SCHEMA,
            id: self.recorder.id.clone().unwrap_or_default(),
            started_at_ms: self.started_at_ms,
            wall_us,
            command: self.command,
            dir: self.dir.display().to_string(),
            target_dir: part.target_dir,
            jobs: u32::try_from(crate::schedule::job_cap()).unwrap_or(u32::MAX),
            cores: std::thread::available_parallelism()
                .map_or(1, |cores| u32::try_from(cores.get()).unwrap_or(u32::MAX)),
            passes: super::passes_enabled(),
            failed,
            fallback,
            units: part.units,
            spans: part.spans,
        };
        if let Err(error) = super::files::save(home, &profile) {
            crate::out::err(format!("artificer: profile not saved: {error:#}"));
        }
        if self.recorder.timings.load(Ordering::Relaxed) {
            match super::files::write_timings(&profile) {
                Ok(Some(path)) => crate::out::status(
                    crate::out::Status::Timing,
                    format!("report saved to {}", path.display()),
                ),
                Ok(None) => {}
                Err(error) => {
                    crate::out::err(format!("artificer: timing report not saved: {error:#}"));
                }
            }
        }
    }
}

pub(crate) fn capture<R>(id: Option<String>, work: impl FnOnce() -> R) -> (R, Part) {
    let previous = current();
    let recorder = Arc::new(Recorder::new(id));
    attach(Some(Arc::clone(&recorder)));
    let result = work();
    attach(previous);
    let part = std::mem::take(&mut *recorder.lock());
    (result, part)
}

pub(crate) fn merge(part: Part, sent: Instant) {
    let Some(recorder) = current() else {
        return;
    };
    let offset = micros(sent.saturating_duration_since(recorder.epoch));
    let mut state = recorder.lock();
    let base = u32::try_from(state.units.len()).unwrap_or(u32::MAX);
    if state.target_dir.is_none() {
        state.target_dir = part.target_dir;
    }
    for mut unit in part.units {
        for dep in &mut unit.deps {
            *dep = dep.saturating_add(base);
        }
        unit.start_us = unit.start_us.map(|at| at.saturating_add(offset));
        unit.end_us = unit.end_us.map(|at| at.saturating_add(offset));
        unit.meta_us = unit.meta_us.map(|at| at.saturating_add(offset));
        unit.early_us = unit.early_us.map(|at| at.saturating_add(offset));
        state.units.push(unit);
    }
    for mut span in part.spans {
        span.unit = span.unit.map(|unit| unit.saturating_add(base));
        span.start_us = span.start_us.saturating_add(offset);
        span.end_us = span.end_us.saturating_add(offset);
        state.spans.push(span);
    }
}

pub fn span<T>(phase: impl Into<Phase>, work: impl FnOnce() -> T) -> T {
    let phase = phase.into();
    let started = Instant::now();
    let opened = open(phase);
    let result = work();
    close(opened, phase);
    if super::timing_enabled() {
        print_timing(phase, started.elapsed());
    }
    result
}

fn open(phase: Phase) -> Option<(Arc<Recorder>, usize)> {
    let recorder = current()?;
    let unit = UNIT.get();
    let in_script = SCRIPTS.get() > 0;
    if phase == Phase::Wrapper(WrapperPhase::Script) {
        SCRIPTS.set(SCRIPTS.get() + 1);
    }
    let start_us = recorder.now();
    let index = {
        let mut state = recorder.lock();
        if phase == Phase::Process(ProcessPhase::ScriptRun)
            || (phase == Phase::Process(ProcessPhase::Rustc) && !in_script)
        {
            mark_miss(&mut state, unit);
        }
        state.spans.push(Span {
            phase,
            unit,
            worker: WORKER.get(),
            start_us,
            end_us: start_us,
            usage: None,
            passes: Vec::new(),
        });
        state.spans.len() - 1
    };
    OPEN.with_borrow_mut(|open| open.push(index));
    Some((recorder, index))
}

fn mark_miss(state: &mut Part, unit: Option<u32>) {
    let Some(unit) = unit
        .and_then(|unit| usize::try_from(unit).ok())
        .and_then(|unit| state.units.get_mut(unit))
    else {
        return;
    };
    if unit.outcome != Some(Outcome::Failed) {
        unit.outcome = Some(Outcome::Miss);
    }
}

fn close(opened: Option<(Arc<Recorder>, usize)>, phase: Phase) {
    let Some((recorder, index)) = opened else {
        return;
    };
    if phase == Phase::Wrapper(WrapperPhase::Script) {
        SCRIPTS.set(SCRIPTS.get().saturating_sub(1));
    }
    OPEN.with_borrow_mut(|open| {
        open.pop();
    });
    let end_us = recorder.now();
    if let Some(span) = recorder.lock().spans.get_mut(index) {
        span.end_us = end_us;
    }
}

fn innermost(edit: impl FnOnce(&mut Span)) {
    let Some(recorder) = current() else {
        return;
    };
    let Some(index) = OPEN.with_borrow(|open| open.last().copied()) else {
        return;
    };
    if let Some(span) = recorder.lock().spans.get_mut(index) {
        edit(span);
    }
}

pub(crate) fn note_usage(usage: Usage) {
    innermost(|span| span.usage = Some(usage));
}

pub(crate) fn note_passes(passes: Vec<Pass>) {
    innermost(|span| span.passes = passes);
}

pub(crate) fn note_target(dir: &Path) {
    if let Some(recorder) = current() {
        recorder.lock().target_dir = Some(dir.display().to_string());
    }
}

fn print_timing(phase: Phase, elapsed: Duration) {
    let unit = UNIT.get().and_then(|unit| {
        let recorder = current()?;
        let state = recorder.lock();
        state
            .units
            .get(usize::try_from(unit).ok()?)
            .map(|unit| unit.name.clone())
    });
    let label = match unit {
        Some(name) => format!("{} {name}", phase.name()),
        None => phase.name().to_string(),
    };
    crate::out::err(format!(
        "artificer: time {label} {:.1}ms",
        elapsed.as_secs_f64() * 1e3
    ));
}
