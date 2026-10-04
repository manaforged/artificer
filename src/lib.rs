#![doc = "This library exists only to build the `artificer` binary and is not a stable API. See docs/api.md for the supported interface: the `artificer` CLI, the `cargo` shim, exit codes, and `artificer stat --json`."]

pub use store::LAYOUT;

mod action;
mod artifact;
mod build;
mod cargo;
mod compile;
mod config;
mod digest;
mod features;
mod flags;
mod gate;
mod home;
mod inputs;
mod install;
mod invoke;
mod jobs;
mod key;
mod keylog;
mod maintenance;
mod manifest;
mod mods;
mod out;
mod platform;
mod profile;
mod remote;
mod schedule;
mod script;
mod serve;
mod session;
mod settings;
mod store;
mod sweep;
mod transfer;
mod unit_key;
mod volume;

pub use build::{
    CheckOpts, Pick, Report, TargetSel, Targets, TestOpts, check, check_cmd, check_package,
    check_selected, run_cmd, test_package,
};
pub use cargo::{Unmodeled, cargo_home, stock_cargo, toolchain_path};
pub use compile::{RustcOutcome, ScriptOutcome};
pub use gate::passthrough_reason;
pub(crate) use home::resolve_path;
pub use home::{control_home, default_home, env_script, purge, ready};
pub use install::{
    InstallReport, cargo_package, check_real_cargo, install, path_prepend, path_remove,
    refresh_shim, uninstall,
};
#[cfg(unix)]
pub use install::{
    add_to_profiles, installed_profiles, profile_line, profiles, remove_from_profiles,
};
#[cfg(windows)]
pub use install::{add_to_user_path, remove_from_user_path};
pub use jobs::isolate;
pub use keylog::why_miss;
pub use maintenance::{StoreStat, doctor, fallback_report, store_stat, sweep_dir};
pub use mods::{Mods, enabled, load as load_mods, save as save_mods};
pub use out::{ColorChoice, Reported, error as report_error, set_color, set_quiet, set_trace};
#[cfg(unix)]
pub use platform::raise_open_file_limit;
pub use profile::{
    Phase, ProfileCommand, Recording as ProfileRecording, RunPhase, begin as begin_profile,
    profile_command, request_timings, span as profile_span,
};
pub use remote::{
    Location, PULL_EVERY, REMOTE_ENV, Source as RemoteSource, configured as remote, pull, push,
    set as set_remote, spawn_pull,
};
pub use schedule::set_jobs;
pub use serve::{
    Request as ServeRequest, listen as serve_listen, ping as serve_ping, stop as serve_stop,
    try_run as serve_try,
};
pub use store::note_fallback;
pub use sweep::Report as SweepReport;
pub use transfer::{TransferReport, export, import};
