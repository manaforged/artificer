use super::phase::Phase;
use serde::{Deserialize, Serialize};

pub(crate) const SCHEMA: u32 = 1;
pub(crate) const US_PER_MS: u64 = 1000;

pub(crate) fn ms(us: u64) -> u64 {
    us / US_PER_MS
}

pub(crate) fn label(name: &str, version: &str, role: Role) -> String {
    match role {
        Role::Package => format!("{name} {version}"),
        Role::Script => format!("{name} {version} (build script)"),
        Role::Targets => format!("{name} {version} (targets)"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Hit,
    Miss,
    Failed,
}

impl Outcome {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Package,
    Script,
    Targets,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub user_us: u64,
    pub system_us: u64,
    pub peak_rss_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pass {
    pub name: String,
    pub us: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub phase: Phase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<u16>,
    pub start_us: u64,
    pub end_us: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passes: Vec<Pass>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetaStage {
    Early,
    Full,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitRecord {
    pub package: String,
    pub name: String,
    pub version: String,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub early_us: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub links: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub early: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
}

impl UnitRecord {
    pub(crate) fn duration_us(&self) -> Option<u64> {
        Some(self.end_us?.saturating_sub(self.start_us?))
    }

    pub(crate) fn label(&self) -> String {
        label(&self.name, &self.version, self.role)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Part {
    #[serde(default)]
    pub units: Vec<UnitRecord>,
    #[serde(default)]
    pub spans: Vec<Span>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_dir: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub schema: u32,
    pub id: String,
    pub started_at_ms: u64,
    pub wall_us: u64,
    pub command: Vec<String>,
    pub dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_dir: Option<String>,
    pub jobs: u32,
    pub cores: u32,
    pub passes: bool,
    pub failed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    pub units: Vec<UnitRecord>,
    pub spans: Vec<Span>,
}
