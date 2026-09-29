use crate::home::{dirs_home, ready, resolve_path};
#[cfg(unix)]
use crate::jobs;
use crate::serve::{GC_CAP_EVERY, GC_EVERY};
use crate::sweep::Report as SweepReport;
use crate::{cargo, key, serve::ping as serve_ping, store, sweep};
use anyhow::{Context, Result};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Default)]
pub struct StoreStat {
    pub units: u32,
    pub bytes: u64,
    pub meta: u32,
    pub meta_bytes: u64,
    pub scratch: u32,
    pub scratch_bytes: u64,
    pub hits: u64,
    pub misses: u64,
    pub fallbacks: u64,
    pub fallback_last: Option<String>,
    pub builds: u64,
    pub last_build: Option<String>,
}

pub fn store_stat(home: &Path) -> Result<StoreStat> {
    let mut stat = StoreStat::default();
    (stat.hits, stat.misses) = store::stats(home)?;
    (stat.fallbacks, stat.fallback_last) = store::fallbacks(home);
    let records = store::build_records(home);
    stat.builds = records.len() as u64;
    stat.last_build = records.last().map(last_build_line);
    let root = home.join("units").join(store::LAYOUT);
    if root.is_dir() {
        fn walk(dir: &Path, stat: &mut StoreStat) -> Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if ft.is_dir() {
                    if path.join("ok").is_file() {
                        stat.units += 1;
                    }
                    walk(&path, stat)?;
                } else if ft.is_file() {
                    stat.bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
            Ok(())
        }
        walk(&root, &mut stat)?;
    }
    let cache = home.join("cargo-meta").join("cache");
    if cache.is_dir() {
        for entry in std::fs::read_dir(&cache)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                stat.meta += 1;
                stat.meta_bytes += entry.metadata()?.len();
            }
        }
    }
    let scratch = home.join("scratch");
    if scratch.is_dir() {
        for entry in std::fs::read_dir(&scratch)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                stat.scratch += 1;
                stat.scratch_bytes += store::size(&entry.path())?;
            }
        }
    }
    Ok(stat)
}

pub fn sweep_dir(dir: &Path, home: &Path) -> Result<SweepReport> {
    let mut report = match cargo::find_manifest(dir) {
        Ok(manifest) => sweep_workspace(&manifest, dir, home)?,
        Err(_) if dir.is_dir() => SweepReport {
            scratch_dirs: sweep::gc_scratch(home)?,
            ..SweepReport::default()
        },
        Err(error) => return Err(error),
    };
    let (units, bytes) = store::gc_units(home, store::AGE).context("evict expired units")?;
    let (capped, cap_bytes) = store::gc_cap(home, store_cap(home)?).context("enforce store cap")?;
    report.evicted_units = units + capped;
    report.evicted_bytes = bytes + cap_bytes;
    Ok(report)
}

fn sweep_workspace(manifest: &Path, dir: &Path, home: &Path) -> Result<SweepReport> {
    let meta = cargo::metadata(manifest, home)?;
    let fallback = manifest.parent().unwrap_or(dir);
    let workspace = if meta.workspace_root.as_os_str().is_empty() {
        fallback
    } else {
        meta.workspace_root.as_path()
    };
    let target = crate::config::target_dir(dir, workspace);
    sweep::workspace(&target, home, true)
}

fn last_build_line(record: &serde_json::Value) -> String {
    let op = record["op"].as_str().unwrap_or("?");
    if let Some(reason) = record["fallback"].as_str() {
        return format!("fallback: {reason}");
    }
    format!(
        "{op} {} hit, {} rustc, {} ms",
        record["hits"].as_u64().unwrap_or(0),
        record["misses"].as_u64().unwrap_or(0),
        record["ms"].as_u64().unwrap_or(0)
    )
}

pub fn fallback_report(home: &Path, limit: usize) -> String {
    let reasons = store::fallback_reasons(home);
    if reasons.is_empty() {
        return "no fallbacks recorded; every handled command went through Artificer\n".to_string();
    }
    let total: u64 = reasons.iter().map(|(_, count)| count).sum();
    let mut out = format!("{total} fallback(s):\n");
    for (reason, count) in reasons.into_iter().take(limit) {
        out.push_str(&format!("{count:>6}  {reason}\n"));
    }
    out
}

pub(crate) fn gc_daily(home: &Path) {
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    let full = stale(&home.join("gc.stamp"), GC_EVERY);
    if !full && !stale(&home.join("cap.stamp"), GC_CAP_EVERY) {
        return;
    }
    if STARTED.set(()).is_err() {
        return;
    }
    let home = home.to_path_buf();
    std::thread::spawn(move || {
        if let Err(error) = gc(&home, full) {
            crate::out::err(format!("artificer: gc skipped: {error:#}"));
        }
    });
}

fn stale(stamp: &Path, every: Duration) -> bool {
    !std::fs::metadata(stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|d| d < every)
}

fn gc(home: &Path, full: bool) -> Result<()> {
    if full {
        store::gc_units(home, store::AGE)?;
    }
    store::gc_cap(home, store_cap(home)?)?;
    if full {
        std::fs::write(home.join("gc.stamp"), "")?;
    }
    std::fs::write(home.join("cap.stamp"), "")?;
    Ok(())
}

pub(crate) fn store_cap(home: &Path) -> Result<u64> {
    let configured = match std::env::var_os("ARTIFICER_STORE_CAP_GB") {
        None => None,
        Some(raw) => {
            let raw = raw
                .to_str()
                .context("ARTIFICER_STORE_CAP_GB must be Unicode")?;
            let gb = raw.parse::<u64>().with_context(|| {
                format!("ARTIFICER_STORE_CAP_GB must be an integer, got `{raw}`")
            })?;
            Some(
                gb.checked_mul(1 << 30)
                    .context("ARTIFICER_STORE_CAP_GB is too large")?,
            )
        }
    };
    let Some((total, _)) = crate::volume::capacity(home) else {
        return Ok(configured.unwrap_or(store::CAP));
    };
    let ceiling = std::cmp::max(total / 100 * store::CAP_SHARE, store::CAP);
    let Some(cap) = configured else {
        return Ok(ceiling);
    };
    if cap <= ceiling {
        return Ok(cap);
    }
    crate::out::err(format!(
        "artificer: store cap {} GiB exceeds this volume; using {} GiB",
        cap >> 30,
        ceiling >> 30
    ));
    Ok(ceiling)
}

mod doctor;
#[cfg(test)]
use doctor::analyzer_wired;
pub use doctor::doctor;

#[cfg(test)]
#[path = "maintenance_tests.rs"]
mod tests;
