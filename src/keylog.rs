use std::fs;
use std::path::{Path, PathBuf};

fn current_path(home: &Path, crate_name: &str) -> PathBuf {
    home.join("keys").join(format!("{crate_name}.txt"))
}

fn previous_path(home: &Path, crate_name: &str) -> PathBuf {
    home.join("keys").join(format!("{crate_name}.prev.txt"))
}

pub(crate) fn record(home: &Path, crate_name: &str, digest: &str, trace: &str) {
    let current = format!("digest={digest}\n{trace}");
    let path = current_path(home, crate_name);
    if fs::read_to_string(&path).is_ok_and(|was| was == current) {
        return;
    }
    if let Some(dir) = path.parent() {
        drop(fs::create_dir_all(dir));
    }
    if let Ok(was) = fs::read_to_string(&path) {
        drop(fs::write(previous_path(home, crate_name), was));
    }
    drop(fs::write(&path, current));
}

#[must_use]
pub fn why_miss(home: &Path, crate_name: &str) -> Option<String> {
    [
        crate_name.to_string(),
        crate_name.replace('_', "-"),
        crate_name.replace('-', "_"),
    ]
    .iter()
    .find_map(|name| difference(home, name))
}

fn difference(home: &Path, crate_name: &str) -> Option<String> {
    let current = fs::read_to_string(current_path(home, crate_name)).ok()?;
    let previous = fs::read_to_string(previous_path(home, crate_name)).unwrap_or_default();
    let mut out = String::new();
    for line in current
        .lines()
        .filter(|l| !previous.lines().any(|p| p == *l))
    {
        out.push_str("now: ");
        out.push_str(line);
        out.push('\n');
    }
    for line in previous
        .lines()
        .filter(|l| !current.lines().any(|c| c == *l))
    {
        out.push_str("was: ");
        out.push_str(line);
        out.push('\n');
    }
    if out.is_empty() {
        out.push_str(
            "no recorded difference; the miss came from a dependency artifact or the store\n",
        );
    }
    Some(out)
}

#[cfg(test)]
#[path = "keylog_tests.rs"]
mod tests;
