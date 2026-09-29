use crate::manifest::scalar;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default)]
struct ConfigDoc {
    build: Option<Build>,
    target: Option<BTreeMap<String, TargetFlags>>,
    #[serde(default)]
    profile: Option<toml::Value>,
    #[serde(default)]
    env: Option<toml::Value>,
    #[serde(default)]
    unstable: Option<toml::Value>,
}

#[derive(Deserialize, Default)]
struct TargetFlags {
    rustflags: Option<toml::Value>,
    linker: Option<String>,
    runner: Option<String>,
}

#[derive(Deserialize, Default)]
struct Build {
    rustflags: Option<toml::Value>,
    #[serde(rename = "target-dir")]
    target_dir: Option<String>,
    rustc: Option<String>,
    rustdoc: Option<String>,
    rustdocflags: Option<toml::Value>,
    target: Option<String>,
    #[serde(rename = "rustc-wrapper")]
    rustc_wrapper: Option<String>,
    #[serde(rename = "rustc-workspace-wrapper")]
    rustc_workspace_wrapper: Option<String>,
}

const CONFIG_TABLES: [&str; 13] = [
    "build",
    "target",
    "profile",
    "env",
    "unstable",
    "alias",
    "term",
    "net",
    "http",
    "registries",
    "registry",
    "source",
    "install",
];

const BUILD_MODELED: [&str; 4] = [
    "rustflags",
    "target-dir",
    "rustc-wrapper",
    "rustc-workspace-wrapper",
];

const BUILD_IGNORED: [&str; 3] = ["jobs", "incremental", "dep-info-basedir"];

const BUILD_NAMED: [&str; 4] = ["rustc", "rustdoc", "rustdocflags", "target"];

const TARGET_KEYS: [&str; 3] = ["rustflags", "linker", "runner"];

#[derive(Clone, Default)]
pub struct Config {
    pub rustflags: Vec<String>,
    pub target_dir: Option<std::path::PathBuf>,
    pub target_rustflags: Vec<(String, Vec<String>)>,
    pub rustc_wrapper: Option<String>,
    pub rustc_workspace_wrapper: Option<String>,
    pub unmodeled: Vec<String>,
    pub unmodeled_doctest: Vec<String>,
}

#[must_use]
pub fn config(dir: &Path) -> Config {
    let files = config_files(dir, &crate::cargo::cargo_home());
    let mut out = Config::default();
    for path in files {
        let (doc, names) = match read_config(&path) {
            Ok(parsed) => parsed,
            Err(reason) => {
                push_once(&mut out.unmodeled, &reason);
                continue;
            }
        };
        for name in names {
            push_once(&mut out.unmodeled, &name);
        }
        if doc.profile.is_some() {
            push_once(
                &mut out.unmodeled,
                &format!("[profile] in {}", path.display()),
            );
        }
        if doc.env.is_some() {
            push_once(&mut out.unmodeled, &format!("[env] in {}", path.display()));
        }
        if doc.unstable.is_some() {
            push_once(
                &mut out.unmodeled,
                &format!("[unstable] in {}", path.display()),
            );
        }
        if let Some(build) = doc.build {
            if !out.rustflags.is_empty() && build.rustflags.is_some() {
                push_once(
                    &mut out.unmodeled,
                    "rustflags merged across Cargo configuration files",
                );
            }
            if out.rustflags.is_empty() {
                out.rustflags = match build.rustflags {
                    Some(toml::Value::String(s)) => {
                        s.split_whitespace().map(str::to_string).collect()
                    }
                    Some(toml::Value::Array(items)) => items.iter().filter_map(scalar).collect(),
                    _ => Vec::new(),
                };
            }
            if out.target_dir.is_none() {
                out.target_dir = build.target_dir.map(|value| {
                    let base = path.parent().and_then(Path::parent).unwrap_or(dir);
                    base.join(value)
                });
            }
            if out.rustc_wrapper.is_none() {
                out.rustc_wrapper = build
                    .rustc_wrapper
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty() && v != "rustc");
            }
            if out.rustc_workspace_wrapper.is_none() {
                out.rustc_workspace_wrapper = build
                    .rustc_workspace_wrapper
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty() && v != "rustc");
            }
            if build.rustc.is_some() {
                push_once(&mut out.unmodeled, "build.rustc");
            }
            if build.target.is_some() {
                push_once(&mut out.unmodeled, "build.target");
            }
            if build.rustdoc.is_some() {
                push_once(&mut out.unmodeled_doctest, "build.rustdoc");
            }
            if build.rustdocflags.is_some() {
                push_once(&mut out.unmodeled_doctest, "build.rustdocflags");
            }
        }
        for (matcher, t) in doc.target.unwrap_or_default() {
            if t.linker.is_some() {
                push_once(&mut out.unmodeled, &format!("[target.{matcher}.linker]"));
            }
            if t.runner.is_some() {
                push_once(&mut out.unmodeled, &format!("[target.{matcher}.runner]"));
            }
            if out.target_rustflags.iter().any(|(m, _)| m == &matcher) {
                if t.rustflags.is_some() {
                    push_once(
                        &mut out.unmodeled,
                        "target rustflags merged across Cargo configuration files",
                    );
                }
                continue;
            }
            let flags = match t.rustflags {
                Some(toml::Value::String(s)) => s.split_whitespace().map(str::to_string).collect(),
                Some(toml::Value::Array(items)) => items.iter().filter_map(scalar).collect(),
                _ => Vec::new(),
            };
            out.target_rustflags.push((matcher, flags));
        }
    }
    out
}

pub(crate) fn target_dir(dir: &Path, workspace: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(|path| dir.join(path))
        .or_else(|| config(dir).target_dir)
        .unwrap_or_else(|| workspace.join("target"))
}

fn push_once(list: &mut Vec<String>, reason: &str) {
    if !list.iter().any(|r| r == reason) {
        list.push(reason.to_string());
    }
}

pub(crate) fn config_files(dir: &Path, cargo_home: &Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for ancestor in dir.ancestors() {
        for name in ["config.toml", "config"] {
            let path = ancestor.join(".cargo").join(name);
            if path.is_file() {
                files.push(path);
            }
        }
    }
    for name in ["config.toml", "config"] {
        let path = cargo_home.join(name);
        if path.is_file() {
            files.push(path);
        }
    }
    files
}

fn read_config(path: &Path) -> Result<(ConfigDoc, Vec<String>), String> {
    let body = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let doc: ConfigDoc =
        toml::from_str(&body).map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
    let raw: toml::Value =
        toml::from_str(&body).map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
    let mut unmodeled = Vec::new();
    let shown = path.display();
    if let Some(table) = raw.as_table() {
        for key in table.keys() {
            if !CONFIG_TABLES.contains(&key.as_str()) {
                unmodeled.push(format!("[{key}] in {shown}"));
            }
        }
        if let Some(build) = table.get("build").and_then(toml::Value::as_table) {
            for key in build.keys() {
                let known = BUILD_MODELED.contains(&key.as_str())
                    || BUILD_IGNORED.contains(&key.as_str())
                    || BUILD_NAMED.contains(&key.as_str());
                if !known {
                    unmodeled.push(format!("build.{key} in {shown}"));
                }
            }
        }
        if let Some(target) = table.get("target").and_then(toml::Value::as_table) {
            for (matcher, flags) in target {
                let Some(flags) = flags.as_table() else {
                    continue;
                };
                for key in flags.keys() {
                    if !TARGET_KEYS.contains(&key.as_str()) {
                        unmodeled.push(format!("target.{matcher}.{key} in {shown}"));
                    }
                }
            }
        }
    }
    Ok((doc, unmodeled))
}

fn host_cfgs(rustc_print: &[String]) -> BTreeMap<String, Option<String>> {
    let mut map = BTreeMap::new();
    for line in rustc_print {
        match line.split_once('=') {
            Some((k, v)) if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 => {
                map.entry(k.to_string())
                    .or_insert_with(|| Some(v[1..v.len() - 1].to_string()));
            }
            _ => {
                map.entry(line.trim().to_string()).or_insert(None);
            }
        }
    }
    map
}

fn eval_cfg(expr: &str, host: &BTreeMap<String, Option<String>>) -> Option<bool> {
    let expr = expr.trim();
    if let Some(inner) = expr.strip_prefix("all(").and_then(|e| e.strip_suffix(')')) {
        for e in split_top(inner)? {
            match eval_cfg(e, host) {
                Some(false) => return Some(false),
                Some(true) => {}
                None => return None,
            }
        }
        return Some(true);
    }
    if let Some(inner) = expr.strip_prefix("any(").and_then(|e| e.strip_suffix(')')) {
        for e in split_top(inner)? {
            match eval_cfg(e, host) {
                Some(true) => return Some(true),
                Some(false) => {}
                None => return None,
            }
        }
        return Some(false);
    }
    if let Some(inner) = expr.strip_prefix("not(").and_then(|e| e.strip_suffix(')')) {
        return eval_cfg(inner, host).map(|v| !v);
    }
    match expr.split_once('=') {
        Some((k, v)) => {
            let v = v.trim().trim_matches('"');
            host.get(k.trim()).map(|got| got.as_deref() == Some(v))
        }
        None => Some(host.contains_key(expr)),
    }
}

fn split_top(expr: &str) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, false, 0usize);
    for (i, c) in expr.char_indices() {
        match c {
            '"' => quote = !quote,
            '(' if !quote => depth += 1,
            ')' if !quote => depth -= 1,
            ',' if !quote && depth == 0 => {
                parts.push(&expr[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&expr[start..]);
    if quote || depth != 0 {
        return None;
    }
    Some(parts.into_iter().map(str::trim).collect())
}

pub fn resolve_target_flags(
    cfg: &Config,
    host_triple: &str,
    rustc_print: &[String],
) -> Result<Option<Vec<String>>, String> {
    if cfg.target_rustflags.is_empty() {
        return Ok(None);
    }
    let host = host_cfgs(rustc_print);
    let mut matched: Vec<(String, Vec<String>)> = Vec::new();
    for (matcher, flags) in &cfg.target_rustflags {
        let hit = if let Some(expr) = matcher
            .strip_prefix("cfg(")
            .and_then(|e| e.strip_suffix(')'))
        {
            eval_cfg(expr, &host)
                .ok_or_else(|| format!("[target.{matcher}] could not be evaluated"))?
        } else {
            matcher == host_triple
        };
        if hit && !flags.is_empty() {
            matched.push((matcher.clone(), flags.clone()));
        }
    }
    if matched.is_empty() {
        return Ok(None);
    }
    matched.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Some(matched.into_iter().flat_map(|(_, f)| f).collect()))
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
