use super::*;

const PREFIX: &str = "CARGO_PROFILE_";
const OVERRIDE_TABLES: [&str; 2] = ["_PACKAGE_", "_BUILD_OVERRIDE_"];
const BOOLS: &[&str] = &["true", "false"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    OptLevel,
    Debug,
    SplitDebuginfo,
    Strip,
    DebugAssertions,
    OverflowChecks,
    Lto,
    Panic,
    Incremental,
    CodegenUnits,
    Rpath,
}

enum Accepts {
    OneOf(&'static [&'static str]),
    Count,
}

impl Accepts {
    fn allows(&self, value: &str) -> bool {
        match self {
            Self::OneOf(values) => values.contains(&value),
            Self::Count => value.parse::<u32>().is_ok_and(|n| n > 0),
        }
    }
}

const KEYS: [(Key, &str, Accepts); 11] = [
    (
        Key::OptLevel,
        "OPT_LEVEL",
        Accepts::OneOf(&["0", "1", "2", "3", "s", "z"]),
    ),
    (
        Key::Debug,
        "DEBUG",
        Accepts::OneOf(&[
            "0",
            "1",
            "2",
            "true",
            "false",
            "none",
            "limited",
            "full",
            "line-tables-only",
            "line-directives-only",
        ]),
    ),
    (
        Key::SplitDebuginfo,
        "SPLIT_DEBUGINFO",
        Accepts::OneOf(&["off", "packed", "unpacked"]),
    ),
    (
        Key::Strip,
        "STRIP",
        Accepts::OneOf(&["none", "debuginfo", "symbols", "true", "false"]),
    ),
    (
        Key::DebugAssertions,
        "DEBUG_ASSERTIONS",
        Accepts::OneOf(BOOLS),
    ),
    (
        Key::OverflowChecks,
        "OVERFLOW_CHECKS",
        Accepts::OneOf(BOOLS),
    ),
    (
        Key::Lto,
        "LTO",
        Accepts::OneOf(&["true", "false", "fat", "thin", "off"]),
    ),
    (Key::Panic, "PANIC", Accepts::OneOf(&["unwind", "abort"])),
    (Key::Incremental, "INCREMENTAL", Accepts::OneOf(BOOLS)),
    (Key::CodegenUnits, "CODEGEN_UNITS", Accepts::Count),
    (Key::Rpath, "RPATH", Accepts::OneOf(BOOLS)),
];

fn key_of(name: &str) -> Option<&'static (Key, &'static str, Accepts)> {
    let rest = name.strip_prefix(PREFIX)?;
    if OVERRIDE_TABLES.iter().any(|table| rest.contains(table)) {
        return None;
    }
    KEYS.iter().find(|(_, suffix, _)| {
        rest.strip_suffix(suffix)
            .and_then(|profile| profile.strip_suffix('_'))
            .is_some_and(|profile| !profile.is_empty())
    })
}

#[must_use]
pub fn profile_env_modeled(name: &str) -> bool {
    key_of(name).is_some()
}

#[derive(Clone, Debug, Default)]
pub struct ProfileEnv(BTreeMap<String, String>);

impl ProfileEnv {
    pub fn from_process() -> Result<Self, String> {
        Self::from_vars(std::env::vars())
    }

    pub fn from_vars(vars: impl IntoIterator<Item = (String, String)>) -> Result<Self, String> {
        let mut values = BTreeMap::new();
        for (name, value) in vars {
            let Some((_, _, accepts)) = key_of(&name) else {
                continue;
            };
            if !accepts.allows(&value) {
                return Err(format!("{name}={value} is not a value Cargo accepts"));
            }
            values.insert(name, value);
        }
        Ok(Self(values))
    }

    fn get(&self, profile: &str, key: Key) -> Option<&str> {
        let (_, suffix, _) = KEYS.iter().find(|(k, _, _)| *k == key)?;
        let profile = profile.to_ascii_uppercase().replace('-', "_");
        self.0
            .get(&format!("{PREFIX}{profile}_{suffix}"))
            .map(String::as_str)
    }

    pub(super) fn apply(&self, profile: &str, p: &mut Profile) {
        let text = |key| {
            self.get(profile, key)
                .map(|v| toml::Value::String(v.to_string()))
        };
        let flag = |key| self.get(profile, key).map(|v| v == "true");
        p.opt_level = text(Key::OptLevel).or(p.opt_level.take());
        p.debug = text(Key::Debug).or(p.debug.take());
        p.split_debuginfo = text(Key::SplitDebuginfo).or(p.split_debuginfo.take());
        p.strip = text(Key::Strip).or(p.strip.take());
        p.lto = text(Key::Lto).or(p.lto.take());
        p.debug_assertions = flag(Key::DebugAssertions).or(p.debug_assertions);
        p.overflow_checks = flag(Key::OverflowChecks).or(p.overflow_checks);
        p.rpath = flag(Key::Rpath).or(p.rpath);
        p.panic = self
            .get(profile, Key::Panic)
            .map(str::to_string)
            .or(p.panic.take());
        p.codegen_units = self
            .get(profile, Key::CodegenUnits)
            .and_then(|v| v.parse().ok())
            .or(p.codegen_units);
    }

    pub(super) fn incremental(&self, profile: &str) -> Option<bool> {
        self.get(profile, Key::Incremental).map(|v| v == "true")
    }
}
