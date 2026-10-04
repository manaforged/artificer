mod svg;
mod tables;

use super::{Analysis, Profile, analyze, critical_path, utc};

const STYLE: &str = "body{font-family:system-ui,sans-serif;margin:24px;max-width:1200px;color:#1f2328}\
h1{font-size:22px}h2{font-size:17px;margin-top:28px}\
table{border-collapse:collapse;font-size:13px;margin:8px 0}\
th,td{border:1px solid #d0d7de;padding:3px 8px;text-align:left}\
td.n{text-align:right;font-variant-numeric:tabular-nums}\
th{background:#f6f8fa}svg{display:block;margin:8px 0}\
svg text{font-size:11px;fill:#1f2328}";

pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn result_text(analysis: &Analysis) -> String {
    match &analysis.fallback {
        Some(reason) => format!("{} ({reason})", analysis.result.name()),
        None => analysis.result.name().to_string(),
    }
}

fn header(out: &mut String, analysis: &Analysis) {
    out.push_str(&format!(
        "<h1>Artificer build {}</h1><p><code>{}</code><br>Started {} &middot; {}</p>",
        escape(&analysis.id),
        escape(&analysis.command.join(" ")),
        escape(&utc(analysis.started_at_ms)),
        escape(&result_text(analysis)),
    ));
}

pub(crate) fn render(profile: &Profile) -> String {
    let analysis = analyze(profile);
    let path = critical_path(profile);
    let mut out = String::new();
    out.push_str(&format!("<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Artificer build {}</title><style>{STYLE}</style></head><body>",
        escape(&analysis.id)
    ));
    header(&mut out, &analysis);
    tables::summary(&mut out, &analysis, &result_text(&analysis));
    out.push_str("<h2>Timeline</h2>");
    out.push_str(&svg::timeline(profile, &path));
    out.push_str("<h2>Concurrency</h2>");
    out.push_str(&svg::concurrency(profile));
    tables::details(&mut out, &analysis);
    out.push_str("</body></html>");
    out
}
