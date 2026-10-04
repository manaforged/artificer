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

#[must_use]
pub fn profile(workspace_root: &Path, name: &str) -> Vec<String> {
    let doc = read(&workspace_root.join("Cargo.toml"));
    let release = matches!(name, "release" | "bench");
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

    let mut opt = if release {
        "3".to_string()
    } else {
        "0".to_string()
    };
    let mut assertions = !release;
    let mut overflow = !release;
    let mut debuginfo = if release { "0" } else { "2" }.to_string();
    let (mut lto, mut panic, mut units, mut strip) = (None, None, None, None);
    let mut split_debuginfo = None;
    let mut rpath = false;

    for p in chain {
        if let Some(v) = p.opt_level.as_ref().and_then(scalar) {
            opt = v;
        }
        if let Some(v) = p.debug_assertions {
            assertions = v;
        }
        if let Some(v) = p.overflow_checks {
            overflow = v;
        }
        if let Some(v) = p.debug.as_ref().and_then(scalar) {
            debuginfo = rustc_value("debug", v).unwrap_or_default();
        }
        if let Some(v) = p.lto.as_ref().and_then(scalar) {
            lto = rustc_value("lto", v);
        }
        if let Some(v) = p.panic.clone() {
            panic = Some(v);
        }
        if let Some(v) = p.codegen_units {
            units = Some(v.to_string());
        }
        if let Some(v) = p.strip.as_ref().and_then(scalar) {
            strip = rustc_value("strip", v);
        }
        if let Some(v) = p.split_debuginfo.as_ref().and_then(scalar) {
            split_debuginfo = Some(v);
        }
        if let Some(v) = p.rpath {
            rpath = v;
        }
    }

    if split_debuginfo.is_none() && cfg!(target_os = "macos") && debuginfo != "0" {
        split_debuginfo = Some("unpacked".to_string());
    }
    let mut args = vec![
        "-C".into(),
        format!("opt-level={opt}"),
        "-C".into(),
        format!("debug-assertions={assertions}"),
        "-C".into(),
        format!("overflow-checks={overflow}"),
        "-C".into(),
        format!("debuginfo={debuginfo}"),
    ];
    for (flag, value) in [
        ("lto", lto),
        ("panic", panic),
        ("codegen-units", units),
        ("strip", strip),
        ("split-debuginfo", split_debuginfo),
    ] {
        if let Some(v) = value {
            args.push("-C".into());
            args.push(format!("{flag}={v}"));
        }
    }
    if rpath {
        args.push("-C".into());
        args.push("rpath".into());
    }
    args
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
