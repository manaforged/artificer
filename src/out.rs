use std::cell::RefCell;
use std::fmt::Display;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static QUIET: AtomicBool = AtomicBool::new(false);
static TRACE: AtomicBool = AtomicBool::new(false);

pub fn set_quiet(on: bool) {
    QUIET.store(on, Ordering::Relaxed);
}

pub fn set_trace(on: bool) {
    TRACE.store(on, Ordering::Relaxed);
}

static COLOR: AtomicU8 = AtomicU8::new(UNSET);
const UNSET: u8 = u8::MAX;

pub fn set_color(choice: ColorChoice) {
    COLOR.store(choice as u8, Ordering::Relaxed);
}

#[derive(Debug)]
pub struct Reported;

impl Display for Reported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("build failed; errors already reported")
    }
}

impl std::error::Error for Reported {}

const COLOR_ENV: &str = "CARGO_TERM_COLOR";
const STATUS_STYLE: &str = "\x1b[1m\x1b[92m";
const ERROR_STYLE: &str = "\x1b[1m\x1b[91m";
const WARNING_STYLE: &str = "\x1b[1m\x1b[33m";
const RESET: &str = "\x1b[0m";
const STATUS_WIDTH: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }

    fn from_u8(value: u8) -> Option<Self> {
        [Self::Auto, Self::Always, Self::Never]
            .into_iter()
            .find(|choice| *choice as u8 == value)
    }
}

#[must_use]
pub fn color() -> bool {
    let choice = ColorChoice::from_u8(COLOR.load(Ordering::Relaxed))
        .or_else(|| {
            std::env::var(COLOR_ENV)
                .ok()
                .and_then(|value| ColorChoice::parse(&value))
        })
        .unwrap_or(ColorChoice::Auto);
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => current().is_none() && std::io::stderr().is_terminal(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Compiling,
    Checking,
    Finished,
    Running,
    Executable,
    Timing,
}

impl Status {
    fn verb(self) -> &'static str {
        match self {
            Self::Compiling => "Compiling",
            Self::Checking => "Checking",
            Self::Finished => "Finished",
            Self::Running => "Running",
            Self::Executable => "Executable",
            Self::Timing => "Timing",
        }
    }
}

pub fn status(kind: Status, message: impl Display) {
    let verb = format!("{:>STATUS_WIDTH$}", kind.verb());
    if color() {
        err(format!("{STATUS_STYLE}{verb}{RESET} {message}"));
    } else {
        err(format!("{verb} {message}"));
    }
}

pub fn error(message: impl Display) {
    labelled(ERROR_STYLE, "error", message);
}

pub fn warning(message: impl Display) {
    if QUIET.load(Ordering::Relaxed) {
        return;
    }
    labelled(WARNING_STYLE, "warning", message);
}

fn labelled(style: &str, label: &str, message: impl Display) {
    if color() {
        diag(format!("{style}{label}{RESET}: {message}"));
    } else {
        diag(format!("{label}: {message}"));
    }
}

#[must_use]
pub fn elapsed(time: Duration) -> String {
    let secs = time.as_secs();
    if secs >= 60 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{:.2}s", time.as_secs_f64())
    }
}

#[must_use]
pub fn trace() -> bool {
    TRACE.load(Ordering::Relaxed)
}

pub type Sink = Arc<(Mutex<Vec<u8>>, Mutex<Vec<u8>>)>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

pub fn capture<R>(f: impl FnOnce() -> R) -> (R, String, String) {
    let sink: Sink = Arc::new((Mutex::new(Vec::new()), Mutex::new(Vec::new())));
    SINK.with(|s| *s.borrow_mut() = Some(sink.clone()));
    let r = f();
    SINK.with(|s| *s.borrow_mut() = None);
    (r, drain(&sink.0), drain(&sink.1))
}

pub fn current() -> Option<Sink> {
    SINK.with(|s| s.borrow().clone())
}

pub fn attach(sink: Option<Sink>) {
    SINK.with(|s| *s.borrow_mut() = sink);
}

fn drain(m: &Mutex<Vec<u8>>) -> String {
    let bytes = m.lock().map(|b| b.clone()).unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn replay(stdout: &[u8], stderr: &[u8]) {
    if !stdout.is_empty() {
        out(String::from_utf8_lossy(stdout).trim_end().to_string());
    }
    if !stderr.is_empty() {
        err(String::from_utf8_lossy(stderr).trim_end().to_string());
    }
}

pub fn out(s: impl Display) {
    match current() {
        Some(sink) => {
            if let Ok(mut buf) = sink.0.lock() {
                drop(writeln!(buf, "{s}"));
            }
        }
        None => write_out(&format!("{s}\n")),
    }
}

pub fn write_out(text: &str) {
    drop(std::io::stdout().write_all(text.as_bytes()));
}

pub fn write_err(text: &str) {
    drop(std::io::stderr().write_all(text.as_bytes()));
}

pub fn err(s: impl Display) {
    if QUIET.load(Ordering::Relaxed) {
        return;
    }
    diag(s);
}

pub fn diag(s: impl Display) {
    match current() {
        Some(sink) => {
            if let Ok(mut buf) = sink.1.lock() {
                drop(writeln!(buf, "{s}"));
            }
        }
        None => write_err(&format!("{s}\n")),
    }
}

#[cfg(test)]
#[path = "out_tests.rs"]
mod tests;
