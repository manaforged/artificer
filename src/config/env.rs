use std::path::Path;

const DISALLOWED: [&str; 3] = ["CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN"];
const VALUE: &str = "value";
const FORCE: &str = "force";
const RELATIVE: &str = "relative";
const KEYS: [&str; 3] = [VALUE, FORCE, RELATIVE];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
    pub force: bool,
}

pub(super) fn parse(
    table: &toml::Value,
    root: &Path,
    shown: &str,
    out: &mut Vec<EnvVar>,
) -> Result<(), String> {
    let Some(table) = table.as_table() else {
        return Err(format!("[env] in {shown}"));
    };
    for (name, entry) in table {
        if DISALLOWED.contains(&name.as_str()) {
            return Err(format!("env.{name} in {shown}"));
        }
        let var = entry_var(name, entry, root).ok_or_else(|| format!("env.{name} in {shown}"))?;
        if !out.iter().any(|known| known.name == var.name) {
            out.push(var);
        }
    }
    Ok(())
}

fn entry_var(name: &str, entry: &toml::Value, root: &Path) -> Option<EnvVar> {
    let plain = |value: &str| EnvVar {
        name: name.to_string(),
        value: value.to_string(),
        force: false,
    };
    match entry {
        toml::Value::String(value) => Some(plain(value)),
        toml::Value::Table(fields) => {
            if fields.keys().any(|key| !KEYS.contains(&key.as_str())) {
                return None;
            }
            let value = fields.get(VALUE)?.as_str()?;
            let flag = |key: &str| match fields.get(key) {
                None => Some(false),
                Some(value) => value.as_bool(),
            };
            let value = if flag(RELATIVE)? {
                root.join(value).display().to_string()
            } else {
                value.to_string()
            };
            Some(EnvVar {
                force: flag(FORCE)?,
                ..plain(&value)
            })
        }
        _ => None,
    }
}

#[must_use]
pub fn effective_env(vars: &[EnvVar]) -> Vec<(String, String)> {
    vars.iter()
        .filter(|var| var.force || std::env::var_os(&var.name).is_none())
        .map(|var| (var.name.clone(), var.value.clone()))
        .collect()
}
