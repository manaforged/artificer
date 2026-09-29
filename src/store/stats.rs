use super::*;

fn read_counter(home: &Path, name: &str) -> u64 {
    std::fs::read_to_string(home.join(name))
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

pub fn stats(home: &Path) -> Result<(u64, u64)> {
    Ok((
        read_counter(home, "stat.hits"),
        read_counter(home, "stat.misses"),
    ))
}

pub(crate) fn fallbacks(home: &Path) -> (u64, Option<String>) {
    let last = std::fs::read_to_string(home.join("fallback.last"))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    (read_counter(home, "stat.fallbacks"), last)
}

pub fn note_fallback(home: &Path, reason: &str) {
    drop(note_fallback_inner(home, reason));
}

fn note_fallback_inner(home: &Path, reason: &str) -> Result<()> {
    if !crate::home::ready(home) {
        return Ok(());
    }
    let _hold = hold(home, "fallback")?;
    let count = read_counter(home, "stat.fallbacks") + 1;
    std::fs::write(home.join("stat.fallbacks"), count.to_string())?;
    let mut last = reason.replace('\n', " ");
    last.truncate(200);
    std::fs::write(home.join("fallback.last"), &last)?;
    drop(note_build_inner(home, "fallback", 0, 0, 0, Some(&last)));
    Ok(())
}

pub fn bump_stats(home: &Path, hits: u64, misses: u64) -> Result<()> {
    let _hold = hold(home, "stats")?;
    let (was_hits, was_misses) = stats(home)?;
    std::fs::write(home.join("stat.hits"), (was_hits + hits).to_string())?;
    std::fs::write(home.join("stat.misses"), (was_misses + misses).to_string())?;
    Ok(())
}

pub fn note_build(home: &Path, op: &str, hits: u64, misses: u64, ms: u64, fallback: Option<&str>) {
    drop(note_build_inner(home, op, hits, misses, ms, fallback));
}

fn note_build_inner(
    home: &Path,
    op: &str,
    hits: u64,
    misses: u64,
    ms: u64,
    fallback: Option<&str>,
) -> Result<()> {
    const MAX_BYTES: u64 = 1 << 20;
    const KEEP: usize = 200;
    let _hold = hold(home, "builds")?;
    let path = home.join("builds.jsonl");
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        let lines: Vec<&str> = body.lines().collect();
        let keep = lines.len().saturating_sub(KEEP);
        std::fs::write(&path, format!("{}\n", lines[keep..].join("\n")))?;
    }
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = serde_json::json!({
        "at": at,
        "op": op,
        "hits": hits,
        "misses": misses,
        "ms": ms,
        "fallback": fallback,
    });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

pub(crate) fn build_records(home: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(home.join("builds.jsonl"))
        .map(|body| {
            body.lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn fallback_reasons(home: &Path) -> Vec<(String, u64)> {
    let mut counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for record in build_records(home) {
        if let Some(reason) = record.get("fallback").and_then(serde_json::Value::as_str) {
            *counts.entry(reason.to_string()).or_default() += 1;
        }
    }
    let mut out: Vec<(String, u64)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}
