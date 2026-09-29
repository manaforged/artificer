use super::*;

type OverrideArgs = Vec<String>;

pub(super) fn decode_override(table: &toml::Value) -> Result<OverrideArgs, String> {
    let Some(map) = table.as_table() else {
        return Err("override table is not a table".into());
    };
    let mut args = Vec::new();
    for (key, value) in map {
        let flag = match key.as_str() {
            "opt-level" => "opt-level",
            "debug" => "debuginfo",
            "codegen-units" => "codegen-units",
            "strip" => "strip",
            "debug-assertions" => "debug-assertions",
            "overflow-checks" => "overflow-checks",
            _ => return Err(format!("[profile.*.package.{key}] is not modeled")),
        };
        let Some(v) = scalar(value) else {
            return Err(format!("[profile.*.package.{key}] is not a scalar"));
        };
        let Some(v) = rustc_value(key, v) else {
            continue;
        };
        args.push("-C".to_string());
        args.push(format!("{flag}={v}"));
    }
    Ok(args)
}

#[derive(Debug)]
pub struct Overrides {
    pub named: Vec<(String, OverrideArgs)>,
    pub wildcard: Option<OverrideArgs>,
}

impl Overrides {
    pub fn for_package(&self, name: &str, sourced: bool) -> &[String] {
        static EMPTY: Vec<String> = Vec::new();
        for (spec, args) in &self.named {
            if spec == name {
                return args;
            }
        }
        if sourced && let Some(args) = &self.wildcard {
            return args;
        }
        &EMPTY
    }
}

pub fn overrides(workspace_root: &Path, name: &str) -> Result<Overrides, String> {
    let doc = read(&workspace_root.join("Cargo.toml"));
    let mut cursor = Some(name.to_string());
    let mut named: Vec<(String, OverrideArgs)> = Vec::new();
    let mut wildcard: Option<OverrideArgs> = None;
    while let Some(key) = cursor.take() {
        if let Some(p) = doc.profile.get(&key) {
            cursor = p.inherits.clone().filter(|i| i != &key);
            if p.build_override.is_some() {
                return Err("[profile.*.build-override] is not modeled".into());
            }
            for (spec, table) in &p.package {
                let args = decode_override(table)?;
                let dup = |s: &str| spec == s;
                if spec == "*" {
                    if wildcard.is_none() {
                        wildcard = Some(args);
                    }
                } else if !named.iter().any(|(s, _)| dup(s)) {
                    named.push((spec.clone(), args));
                }
            }
        }
        if cursor.is_none() {
            cursor = implicit_parent(&key).map(str::to_string);
        }
        if named.len() + usize::from(wildcard.is_some()) > 64 {
            break;
        }
    }
    Ok(Overrides { named, wildcard })
}
