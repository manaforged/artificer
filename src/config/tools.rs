use super::{host_cfgs, target_applies};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Linker,
    Runner,
}

impl ToolKind {
    pub const ALL: [Self; 2] = [Self::Linker, Self::Runner];

    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Linker => "linker",
            Self::Runner => "runner",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetTool {
    pub matcher: String,
    pub kind: ToolKind,
    pub command: Vec<String>,
    pub listed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostTools {
    pub linker: Option<String>,
    pub runner: Option<Vec<String>>,
}

pub(super) fn parse(
    matcher: &str,
    kind: ToolKind,
    value: &toml::Value,
    root: &Path,
    shown: &str,
    out: &mut Vec<TargetTool>,
) -> Result<(), String> {
    let refused = || format!("target.{matcher}.{} in {shown}", kind.key());
    let (words, listed): (Vec<String>, bool) = match (kind, value) {
        (ToolKind::Linker, toml::Value::String(text)) => (vec![text.clone()], false),
        (ToolKind::Runner, toml::Value::String(text)) => {
            (text.split_whitespace().map(str::to_string).collect(), false)
        }
        (ToolKind::Runner, toml::Value::Array(items)) => (
            items
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(refused)?,
            true,
        ),
        _ => return Err(refused()),
    };
    let Some((first, args)) = words.split_first() else {
        return Err(refused());
    };
    if let Some(known) = out
        .iter()
        .find(|tool| tool.matcher == matcher && tool.kind == kind)
    {
        if known.listed || listed {
            return Err(format!(
                "target.{matcher}.{} merged across Cargo configuration files",
                kind.key()
            ));
        }
        return Ok(());
    }
    let mut command = vec![program(first, root)];
    command.extend(args.iter().cloned());
    out.push(TargetTool {
        matcher: matcher.to_string(),
        kind,
        command,
        listed,
    });
    Ok(())
}

fn program(value: &str, root: &Path) -> String {
    let path_like = value.contains('/') || (cfg!(windows) && value.contains('\\'));
    if path_like {
        root.join(value).display().to_string()
    } else {
        value.to_string()
    }
}

pub fn resolve_host_tools(
    tools: &[TargetTool],
    host_triple: &str,
    rustc_print: &[String],
) -> Result<HostTools, String> {
    let mut resolved = HostTools::default();
    if tools.is_empty() {
        return Ok(resolved);
    }
    let host = host_cfgs(rustc_print);
    for kind in ToolKind::ALL {
        let mut exact = None;
        let mut matched = Vec::new();
        for tool in tools.iter().filter(|tool| tool.kind == kind) {
            if tool.matcher == host_triple {
                exact = Some(tool);
            } else if target_applies(&tool.matcher, host_triple, &host)? {
                matched.push(tool);
            }
        }
        if exact.is_none() && matched.len() > 1 {
            return Err(format!(
                "several [target.cfg(..).{}] entries match the host",
                kind.key()
            ));
        }
        let Some(tool) = exact.or(matched.first().copied()) else {
            continue;
        };
        match kind {
            ToolKind::Linker => resolved.linker = tool.command.first().cloned(),
            ToolKind::Runner => resolved.runner = Some(tool.command.clone()),
        }
    }
    Ok(resolved)
}
