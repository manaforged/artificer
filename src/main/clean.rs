use super::*;

pub(super) fn clean(dir: &Path, target: Option<&Path>, home: &Path) -> Result<Dispatch> {
    let report = artificer::sweep_dir(dir, target, home)?;
    eprintln!(
        "artificer: removed {} incremental dir(s), {} scratch copy(ies)",
        report.incremental_dirs, report.scratch_dirs
    );
    eprintln!(
        "artificer: evicted {} unit(s), {:.1} MB",
        report.evicted_units,
        report.evicted_bytes as f64 / 1_048_576.0
    );
    Ok(ExitCode::from(0).into())
}
