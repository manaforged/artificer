use super::*;

pub(super) fn rustc_value(key: &str, v: String) -> Option<String> {
    match (key, v.as_str()) {
        ("debug", "true" | "full") => Some("2".into()),
        ("debug", "false" | "none") => Some("0".into()),
        ("debug", "limited") => Some("1".into()),
        ("strip", "true") => Some("symbols".into()),
        ("strip", "false") => Some("none".into()),
        ("lto", "true") => Some("fat".into()),
        ("lto", "false") => None,
        _ => Some(v),
    }
}

pub(super) fn implicit_parent(name: &str) -> Option<&'static str> {
    match name {
        "test" => Some("dev"),
        "bench" => Some("release"),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitUse {
    Runtime,
    BuildOnly,
}

#[must_use]
pub fn profile(workspace_root: &Path, name: &str, unit: UnitUse) -> Vec<String> {
    let doc = read(&workspace_root.join("Cargo.toml"));
    let mut values = Values::defaults(matches!(name, "release" | "bench"));
    for p in chain(&doc, name) {
        values.apply(&p);
    }
    if unit == UnitUse::BuildOnly {
        values.build_only();
    }
    values.args()
}

fn chain(doc: &Doc, name: &str) -> Vec<Profile> {
    let mut chain: Vec<Profile> = Vec::new();
    let mut cursor = Some(name.to_string());
    while let Some(key) = cursor.take() {
        if let Some(p) = doc.profile.get(&key) {
            cursor = p.inherits.clone().filter(|i| i != &key);
            chain.push(p.clone());
        }
        if cursor.is_none() {
            cursor = implicit_parent(&key).map(str::to_string);
        }
        if chain.len() > 8 {
            break;
        }
    }
    chain.reverse();
    chain
}

struct Values {
    opt: String,
    assertions: bool,
    overflow: bool,
    debuginfo: String,
    lto: Option<String>,
    panic: Option<String>,
    units: Option<String>,
    strip: Option<String>,
    split_debuginfo: Option<String>,
    rpath: bool,
}

impl Values {
    fn defaults(release: bool) -> Self {
        Self {
            opt: if release { "3" } else { "0" }.to_string(),
            assertions: !release,
            overflow: !release,
            debuginfo: if release { "0" } else { "2" }.to_string(),
            lto: None,
            panic: None,
            units: None,
            strip: None,
            split_debuginfo: None,
            rpath: false,
        }
    }

    fn apply(&mut self, p: &Profile) {
        if let Some(v) = p.opt_level.as_ref().and_then(scalar) {
            self.opt = v;
        }
        self.assertions = p.debug_assertions.unwrap_or(self.assertions);
        self.overflow = p.overflow_checks.unwrap_or(self.overflow);
        if let Some(v) = p.debug.as_ref().and_then(scalar) {
            self.debuginfo = rustc_value("debug", v).unwrap_or_default();
        }
        if let Some(v) = p.lto.as_ref().and_then(scalar) {
            self.lto = rustc_value("lto", v);
        }
        self.panic = p.panic.clone().or(self.panic.take());
        self.units = p.codegen_units.map(|v| v.to_string()).or(self.units.take());
        if let Some(v) = p.strip.as_ref().and_then(scalar) {
            self.strip = rustc_value("strip", v);
        }
        if let Some(v) = p.split_debuginfo.as_ref().and_then(scalar) {
            self.split_debuginfo = Some(v);
        }
        self.rpath = p.rpath.unwrap_or(self.rpath);
    }

    fn build_only(&mut self) {
        self.opt = "0".to_string();
        self.debuginfo = "0".to_string();
        (self.lto, self.panic, self.units, self.split_debuginfo) = (None, None, None, None);
    }

    fn args(mut self) -> Vec<String> {
        if self.split_debuginfo.is_none() && cfg!(target_os = "macos") && self.debuginfo != "0" {
            self.split_debuginfo = Some("unpacked".to_string());
        }
        let mut args = vec![
            "-C".into(),
            format!("opt-level={}", self.opt),
            "-C".into(),
            format!("debug-assertions={}", self.assertions),
            "-C".into(),
            format!("overflow-checks={}", self.overflow),
            "-C".into(),
            format!("debuginfo={}", self.debuginfo),
        ];
        for (flag, value) in [
            ("lto", self.lto),
            ("panic", self.panic),
            ("codegen-units", self.units),
            ("strip", self.strip),
            ("split-debuginfo", self.split_debuginfo),
        ] {
            if let Some(v) = value {
                args.push("-C".into());
                args.push(format!("{flag}={v}"));
            }
        }
        if self.rpath {
            args.push("-C".into());
            args.push("rpath".into());
        }
        args
    }
}

const PROFILE_KEYS: [&str; 14] = [
    "inherits",
    "opt-level",
    "debug",
    "split-debuginfo",
    "strip",
    "debug-assertions",
    "overflow-checks",
    "lto",
    "panic",
    "incremental",
    "codegen-units",
    "rpath",
    "package",
    "build-override",
];

const INCREMENTAL_ROOT: &str = "dev";
const INCREMENTAL_KEY: &str = "incremental";

pub fn profile_gate(workspace_root: &Path, name: &str) -> Result<bool, String> {
    let body = cached_body(&workspace_root.join("Cargo.toml"))?;
    let doc: toml::Value =
        toml::from_str(&body).map_err(|e| format!("cannot parse the workspace manifest: {e}"))?;
    let mut cursor = Some(name.to_string());
    let mut root = name.to_string();
    let mut incremental = None;
    for _ in 0..8 {
        let Some(profile_name) = cursor.take() else {
            break;
        };
        root.clone_from(&profile_name);
        if let Some(table) = doc
            .get("profile")
            .and_then(|p| p.get(&profile_name))
            .and_then(toml::Value::as_table)
        {
            for key in table.keys() {
                if !PROFILE_KEYS.contains(&key.as_str()) {
                    return Err(format!("[profile.{profile_name}.{key}] is not modeled"));
                }
            }
            if incremental.is_none() {
                incremental = table.get(INCREMENTAL_KEY).and_then(toml::Value::as_bool);
            }
            cursor = table
                .get("inherits")
                .and_then(toml::Value::as_str)
                .map(str::to_string);
        }
        if cursor.is_none() {
            cursor = implicit_parent(&profile_name).map(str::to_string);
        }
    }
    Ok(incremental.unwrap_or(root == INCREMENTAL_ROOT))
}
