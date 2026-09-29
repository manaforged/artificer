use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize, Deserialize)]
struct Inputs {
    files: Vec<(PathBuf, String)>,
    environment: Vec<(String, Option<String>)>,
}

fn env_digest(command: &Command, name: &str) -> Option<String> {
    let value = command
        .get_envs()
        .find(|(key, _)| *key == OsStr::new(name))
        .map(|(_, value)| value.map(OsString::from))
        .unwrap_or_else(|| std::env::var_os(name));
    value.map(|value| blake3::hash(value.as_encoded_bytes()).to_hex().to_string())
}

fn file_digest(path: &Path) -> Result<String> {
    Ok(
        blake3::hash(&fs::read(path).with_context(|| format!("read {}", path.display()))?)
            .to_hex()
            .to_string(),
    )
}

pub(crate) fn matches(out: &Path, root: &Path, command: &Command) -> bool {
    let Ok(bytes) = fs::read(out.join("inputs.json")) else {
        return false;
    };
    let Ok(inputs) = serde_json::from_slice::<Inputs>(&bytes) else {
        return false;
    };
    inputs
        .files
        .iter()
        .all(|(path, digest)| file_digest(&root.join(path)).is_ok_and(|current| current == *digest))
        && inputs
            .environment
            .iter()
            .all(|(name, digest)| env_digest(command, name) == *digest)
}

pub(crate) fn record(out: &Path, root: &Path, command: &Command) -> Result<()> {
    let mut inputs = Inputs {
        files: Vec::new(),
        environment: Vec::new(),
    };
    let mut found = false;
    for entry in fs::read_dir(out)? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "d") {
            continue;
        }
        let body = fs::read_to_string(&path)?;
        let rule = body
            .lines()
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .context("compiler dependency record has no rule")?;
        let (_, sources) = rule
            .split_once(": ")
            .context("invalid compiler dependency rule")?;
        for name in paths(sources) {
            let source = command.get_current_dir().unwrap_or(root).join(name);
            let digest = file_digest(&source)?;
            let name = source.strip_prefix(root).unwrap_or(&source).to_path_buf();
            inputs.files.push((name, digest));
        }
        for line in body
            .lines()
            .filter_map(|line| line.strip_prefix("# env-dep:"))
        {
            let name = line.split_once('=').map_or(line, |(name, _)| name);
            inputs
                .environment
                .push((name.to_string(), env_digest(command, name)));
        }
        found = true;
    }
    if !found {
        bail!("compiler did not emit dependency information");
    }
    inputs.files.sort();
    inputs.files.dedup();
    inputs.environment.sort();
    inputs.environment.dedup();
    fs::write(out.join("inputs.json"), serde_json::to_vec(&inputs)?)?;
    Ok(())
}

fn paths(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if chars.peek() == Some(&' ') => {
                chars.next();
                word.push(' ');
            }
            ch if ch.is_whitespace() => {
                if !word.is_empty() {
                    paths.push(PathBuf::from(std::mem::take(&mut word)));
                }
            }
            ch => word.push(ch),
        }
    }
    if !word.is_empty() {
        paths.push(PathBuf::from(word));
    }
    paths
}
