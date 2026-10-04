use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const STORE_TOKEN: &str = "@ARTIFICER_HOME@";
const STORE_TOKENS: [&str; 3] = [
    STORE_TOKEN,
    "@ARTIFICER_HOME_ESCAPED@",
    "@ARTIFICER_HOME_FORWARD@",
];
const CARGO_TOKENS: [&str; 3] = [
    "@CARGO_HOME@",
    "@CARGO_HOME_ESCAPED@",
    "@CARGO_HOME_FORWARD@",
];

#[derive(Clone, Copy)]
enum Base {
    Store,
    Cargo,
}

impl Base {
    const ALL: [Self; 2] = [Self::Store, Self::Cargo];

    fn path(self, home: &Path) -> PathBuf {
        match self {
            Self::Store => home.to_path_buf(),
            Self::Cargo => crate::cargo::cargo_home(),
        }
    }

    fn tokens(self) -> [&'static str; 3] {
        match self {
            Self::Store => STORE_TOKENS,
            Self::Cargo => CARGO_TOKENS,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spelling {
    Native,
    Escaped,
    Forward,
}

impl Spelling {
    const ALL: [Self; 3] = [Self::Native, Self::Escaped, Self::Forward];

    fn token(self, base: Base) -> &'static str {
        base.tokens()[self as usize]
    }

    fn render(self, text: &str) -> String {
        match self {
            Self::Native => text.to_string(),
            Self::Escaped => text.replace('\\', "\\\\"),
            Self::Forward => text.replace('\\', "/"),
        }
    }
}

pub(crate) fn spellings(path: &Path) -> Vec<(Spelling, String)> {
    let shown = crate::platform::env_path(path).display().to_string();
    let plain = path.display().to_string();
    let mut out: Vec<(Spelling, String)> = Vec::new();
    for text in [shown, plain] {
        for spelling in Spelling::ALL {
            let form = spelling.render(&text);
            if !form.is_empty() && !out.iter().any(|(_, seen)| *seen == form) {
                out.push((spelling, form));
            }
        }
    }
    out.sort_by_key(|(_, form)| std::cmp::Reverse(form.len()));
    out
}

fn replace(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = 0;
    for at in memchr::memmem::find_iter(bytes, from) {
        out.extend_from_slice(&bytes[rest..at]);
        out.extend_from_slice(to);
        rest = at + from.len();
    }
    out.extend_from_slice(&bytes[rest..]);
    out
}

pub(crate) fn portable_bytes(home: &Path, bytes: &[u8]) -> Vec<u8> {
    let mut forms: Vec<(Base, Spelling, String)> = Base::ALL
        .into_iter()
        .flat_map(|base| {
            spellings(&base.path(home))
                .into_iter()
                .map(move |(spelling, form)| (base, spelling, form))
        })
        .collect();
    forms.sort_by_key(|(_, _, form)| std::cmp::Reverse(form.len()));
    forms
        .iter()
        .fold(bytes.to_vec(), |text, (base, spelling, form)| {
            replace(&text, form.as_bytes(), spelling.token(*base).as_bytes())
        })
}

pub(crate) fn portable(home: &Path, text: &str) -> String {
    String::from_utf8(portable_bytes(home, text.as_bytes()))
        .unwrap_or_else(|err| String::from_utf8_lossy(err.as_bytes()).into_owned())
}

pub(crate) fn concrete(home: &Path, text: &str) -> String {
    Base::ALL.into_iter().fold(text.to_string(), |text, base| {
        let shown = crate::platform::env_path(&base.path(home))
            .display()
            .to_string();
        Spelling::ALL.into_iter().fold(text, |text, spelling| {
            text.replace(spelling.token(base), &spelling.render(&shown))
        })
    })
}

fn portable_path(home: &Path, path: &Path) -> PathBuf {
    Base::ALL
        .into_iter()
        .find_map(|base| {
            let root = base.path(home);
            [root.clone(), crate::platform::env_path(&root)]
                .iter()
                .find_map(|prefix| path.strip_prefix(prefix).ok())
                .map(|rest| Path::new(Spelling::Native.token(base)).join(rest))
        })
        .unwrap_or_else(|| path.to_path_buf())
}

fn concrete_path(home: &Path, path: &Path) -> PathBuf {
    Base::ALL
        .into_iter()
        .find_map(|base| {
            path.strip_prefix(Spelling::Native.token(base))
                .ok()
                .map(|rest| base.path(home).join(rest))
        })
        .unwrap_or_else(|| path.to_path_buf())
}

#[derive(Serialize, Deserialize)]
struct Inputs {
    files: Vec<(PathBuf, String)>,
    environment: Vec<(String, Option<String>)>,
}

fn env_digest(home: &Path, command: &Command, name: &str) -> Option<String> {
    let value = command
        .get_envs()
        .find(|(key, _)| *key == OsStr::new(name))
        .map(|(_, value)| value.map(OsString::from))
        .unwrap_or_else(|| std::env::var_os(name));
    value.map(|value| {
        let bytes = portable_bytes(home, value.as_encoded_bytes());
        blake3::hash(&bytes).to_hex().to_string()
    })
}

pub(crate) fn matches(home: &Path, out: &Path, root: &Path, command: &Command) -> bool {
    let Ok(bytes) = fs::read(out.join("inputs.json")) else {
        return false;
    };
    let Ok(inputs) = serde_json::from_slice::<Inputs>(&bytes) else {
        return false;
    };
    inputs.files.iter().all(|(path, digest)| {
        crate::digest::file(Some(home), &root.join(concrete_path(home, path)))
            .is_ok_and(|current| current == *digest)
    }) && inputs
        .environment
        .iter()
        .all(|(name, digest)| env_digest(home, command, name) == *digest)
}

pub(crate) fn record(home: &Path, out: &Path, root: &Path, command: &Command) -> Result<()> {
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
            let digest = crate::digest::file(Some(home), &source)?;
            let name = source
                .strip_prefix(root)
                .map_or_else(|_| portable_path(home, &source), Path::to_path_buf);
            inputs.files.push((name, digest));
        }
        for line in body
            .lines()
            .filter_map(|line| line.strip_prefix("# env-dep:"))
        {
            let name = line.split_once('=').map_or(line, |(name, _)| name);
            inputs
                .environment
                .push((name.to_string(), env_digest(home, command, name)));
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
