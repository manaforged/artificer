use super::{Build, Config, TargetFlags, ToolKind, push_once, tools};
use crate::manifest::scalar;
use std::collections::BTreeMap;
use std::path::Path;

fn flags(value: Option<toml::Value>) -> Vec<String> {
    match value {
        Some(toml::Value::String(s)) => s.split_whitespace().map(str::to_string).collect(),
        Some(toml::Value::Array(items)) => items.iter().filter_map(scalar).collect(),
        _ => Vec::new(),
    }
}

fn wrapper(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v != "rustc")
}

pub(super) fn build(out: &mut Config, build: Build, root: &Path) {
    if !out.rustflags.is_empty() && build.rustflags.is_some() {
        push_once(
            &mut out.unmodeled,
            "rustflags merged across Cargo configuration files",
        );
    }
    if out.rustflags.is_empty() {
        out.rustflags = flags(build.rustflags);
    }
    if out.target_dir.is_none() {
        out.target_dir = build.target_dir.map(|value| root.join(value));
    }
    if out.rustc_wrapper.is_none() {
        out.rustc_wrapper = wrapper(build.rustc_wrapper);
    }
    if out.rustc_workspace_wrapper.is_none() {
        out.rustc_workspace_wrapper = wrapper(build.rustc_workspace_wrapper);
    }
    for (present, key) in [
        (build.rustc.is_some(), "build.rustc"),
        (build.target.is_some(), "build.target"),
    ] {
        if present {
            push_once(&mut out.unmodeled, key);
        }
    }
    for (present, key) in [
        (build.rustdoc.is_some(), "build.rustdoc"),
        (build.rustdocflags.is_some(), "build.rustdocflags"),
    ] {
        if present {
            push_once(&mut out.unmodeled_doctest, key);
        }
    }
}

pub(super) fn targets(
    out: &mut Config,
    targets: BTreeMap<String, TargetFlags>,
    root: &Path,
    shown: &str,
) {
    for (matcher, t) in targets {
        for (kind, value) in [(ToolKind::Linker, &t.linker), (ToolKind::Runner, &t.runner)] {
            if let Some(value) = value
                && let Err(reason) =
                    tools::parse(&matcher, kind, value, root, shown, &mut out.target_tools)
            {
                push_once(&mut out.unmodeled, &reason);
            }
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
        out.target_rustflags.push((matcher, flags(t.rustflags)));
    }
}
