use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;

type BodyStamp = Option<(u64, Option<std::time::SystemTime>)>;
type BodyCache = HashMap<PathBuf, (BodyStamp, Result<String, String>)>;

fn cached_body(path: &Path) -> Result<String, String> {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let stamp = std::fs::metadata(&key)
        .ok()
        .map(|m| (m.len(), m.modified().ok()));
    static BODIES: OnceLock<Mutex<BodyCache>> = OnceLock::new();
    let mut map = BODIES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((old, hit)) = map.get(&key)
        && *old == stamp
    {
        return hit.clone();
    }
    let body =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()));
    map.insert(key, (stamp, body.clone()));
    body
}

#[derive(Deserialize, Default)]
struct Doc {
    lints: Option<Lints>,
    #[serde(default)]
    profile: BTreeMap<String, Profile>,
    workspace: Option<Workspace>,
    lib: Option<TargetSpec>,
    #[serde(default)]
    bin: Vec<TargetSpec>,
    #[serde(default)]
    test: Vec<TargetSpec>,
    #[serde(default)]
    bench: Vec<TargetSpec>,
    #[serde(default)]
    example: Vec<TargetSpec>,
}

#[derive(Deserialize, Default)]
struct TargetSpec {
    name: Option<String>,
    harness: Option<bool>,
}

#[derive(Deserialize, Default)]
struct Workspace {
    #[serde(default)]
    lints: BTreeMap<String, BTreeMap<String, Lint>>,
}

#[derive(Deserialize, Default)]
struct Lints {
    #[serde(default)]
    workspace: bool,
    #[serde(flatten)]
    groups: BTreeMap<String, BTreeMap<String, Lint>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Lint {
    Level(String),
    Detail {
        level: String,
        #[serde(default)]
        priority: i32,
        #[serde(default, rename = "check-cfg")]
        check_cfg: Vec<String>,
    },
}

impl Lint {
    fn parts(&self) -> (&str, i32) {
        match self {
            Self::Level(level) => (level, 0),
            Self::Detail {
                level, priority, ..
            } => (level, *priority),
        }
    }

    fn check_cfg(&self) -> &[String] {
        match self {
            Self::Level(_) => &[],
            Self::Detail { check_cfg, .. } => check_cfg,
        }
    }
}

#[derive(Deserialize, Default, Clone)]
struct Profile {
    inherits: Option<String>,
    #[serde(rename = "opt-level")]
    opt_level: Option<toml::Value>,
    debug: Option<toml::Value>,
    #[serde(rename = "debug-assertions")]
    debug_assertions: Option<bool>,
    #[serde(rename = "overflow-checks")]
    overflow_checks: Option<bool>,
    lto: Option<toml::Value>,
    panic: Option<String>,
    #[serde(rename = "codegen-units")]
    codegen_units: Option<u32>,
    strip: Option<toml::Value>,
    #[serde(rename = "split-debuginfo")]
    split_debuginfo: Option<toml::Value>,
    rpath: Option<bool>,
    #[serde(default)]
    package: BTreeMap<String, toml::Value>,
    #[serde(rename = "build-override", default)]
    build_override: Option<toml::Value>,
}

fn read(path: &Path) -> Doc {
    cached_body(path)
        .ok()
        .and_then(|body| toml::from_str(&body).ok())
        .unwrap_or_default()
}

pub(crate) fn harness(manifest: &Path, target: &crate::cargo::Target) -> bool {
    use crate::cargo::TargetKind;
    let doc = read(manifest);
    let specs = match TargetKind::of(target) {
        TargetKind::Lib => return doc.lib.and_then(|lib| lib.harness).unwrap_or(true),
        TargetKind::BuildScript => return true,
        TargetKind::Bin => doc.bin,
        TargetKind::Test => doc.test,
        TargetKind::Bench => doc.bench,
        TargetKind::Example => doc.example,
    };
    specs
        .into_iter()
        .find(|spec| spec.name.as_deref() == Some(target.name.as_str()))
        .and_then(|spec| spec.harness)
        .unwrap_or(true)
}

pub(crate) fn package_root(manifest: &Path) -> PathBuf {
    let dir = manifest.parent().unwrap_or(Path::new("."));
    for ancestor in dir.ancestors() {
        let candidate = ancestor.join("Cargo.toml");
        let Ok(body) = std::fs::read_to_string(&candidate) else {
            continue;
        };
        let Ok(doc) = toml::from_str::<toml::Value>(&body) else {
            continue;
        };
        if doc.get("workspace").is_some() {
            return ancestor.to_path_buf();
        }
    }
    dir.to_path_buf()
}

pub(crate) fn scalar(v: &toml::Value) -> Option<String> {
    match v {
        toml::Value::Integer(n) => Some(n.to_string()),
        toml::Value::Boolean(b) => Some(b.to_string()),
        toml::Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

pub fn lints(pkg_manifest: &Path, workspace_root: &Path) -> Result<Vec<String>, String> {
    let doc = read_strict(pkg_manifest)?;
    let Some(table) = doc.lints else {
        return Ok(Vec::new());
    };
    let groups = if table.workspace {
        read_strict(&workspace_root.join("Cargo.toml"))?
            .workspace
            .unwrap_or_default()
            .lints
    } else {
        table.groups
    };

    let mut flat: Vec<(i32, String, String)> = Vec::new();
    let mut check_cfg: Vec<String> = Vec::new();
    for (group, lints) in &groups {
        for (name, lint) in lints {
            check_cfg.extend(lint.check_cfg().iter().cloned());
            let (level, priority) = lint.parts();
            let qualified = match group.as_str() {
                "rust" => name.clone(),
                tool => format!("{tool}::{name}"),
            };
            flat.push((priority, level.to_string(), qualified));
        }
    }
    flat.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));

    let mut args = Vec::new();
    for (_, level, name) in flat {
        let flag = match level.as_str() {
            "allow" => "--allow",
            "warn" => "--warn",
            "deny" => "--deny",
            "forbid" => "--forbid",
            _ => return Err(format!("lint level `{level}` is not modeled")),
        };
        args.push(flag.to_string());
        args.push(name);
    }
    for cfg in check_cfg {
        args.push("--check-cfg".to_string());
        args.push(cfg);
    }
    Ok(args)
}

fn read_strict(path: &Path) -> Result<Doc, String> {
    let body = cached_body(path)?;
    toml::from_str(&body).map_err(|e| format!("cannot parse {}: {e}", path.display()))
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;

mod profile;
pub use profile::{Overrides, overrides};

mod profile_args;
pub use profile_args::{profile, profile_gate};

#[cfg(test)]
use profile::decode_override;

use profile_args::{implicit_parent, rustc_value};
