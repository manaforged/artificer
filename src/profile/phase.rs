use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Setup,
    Wrapper,
    Unit,
    Process,
    Run,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SetupPhase {
    Gate,
    CargoVersion,
    Metadata,
    CargoMetadata,
    FeatureProbe,
    Session,
    RustcVersion,
    RustcCfg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WrapperPhase {
    Serve,
    Schedule,
    Script,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnitPhase {
    Key,
    Permit,
    Publish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProcessPhase {
    Rustc,
    ScriptRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunPhase {
    TestRun,
    Run,
    Fallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Phase {
    Setup(SetupPhase),
    Wrapper(WrapperPhase),
    Unit(UnitPhase),
    Process(ProcessPhase),
    Run(RunPhase),
}

impl Phase {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Setup(phase) => phase.name(),
            Self::Wrapper(phase) => phase.name(),
            Self::Unit(phase) => phase.name(),
            Self::Process(phase) => phase.name(),
            Self::Run(phase) => phase.name(),
        }
    }

    pub(crate) fn stage(self) -> Stage {
        match self {
            Self::Setup(_) => Stage::Setup,
            Self::Wrapper(_) => Stage::Wrapper,
            Self::Unit(_) => Stage::Unit,
            Self::Process(_) => Stage::Process,
            Self::Run(_) => Stage::Run,
        }
    }
}

impl SetupPhase {
    fn name(self) -> &'static str {
        match self {
            Self::Gate => "gate",
            Self::CargoVersion => "cargo-version",
            Self::Metadata => "metadata",
            Self::CargoMetadata => "cargo-metadata",
            Self::FeatureProbe => "feature-probe",
            Self::Session => "session",
            Self::RustcVersion => "rustc-version",
            Self::RustcCfg => "rustc-cfg",
        }
    }
}

impl WrapperPhase {
    fn name(self) -> &'static str {
        match self {
            Self::Serve => "serve",
            Self::Schedule => "schedule",
            Self::Script => "script",
        }
    }
}

impl UnitPhase {
    fn name(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Permit => "permit",
            Self::Publish => "publish",
        }
    }
}

impl ProcessPhase {
    fn name(self) -> &'static str {
        match self {
            Self::Rustc => "rustc",
            Self::ScriptRun => "script-run",
        }
    }
}

impl RunPhase {
    fn name(self) -> &'static str {
        match self {
            Self::TestRun => "test-run",
            Self::Run => "run",
            Self::Fallback => "fallback",
        }
    }
}

impl From<SetupPhase> for Phase {
    fn from(phase: SetupPhase) -> Self {
        Self::Setup(phase)
    }
}

impl From<WrapperPhase> for Phase {
    fn from(phase: WrapperPhase) -> Self {
        Self::Wrapper(phase)
    }
}

impl From<UnitPhase> for Phase {
    fn from(phase: UnitPhase) -> Self {
        Self::Unit(phase)
    }
}

impl From<ProcessPhase> for Phase {
    fn from(phase: ProcessPhase) -> Self {
        Self::Process(phase)
    }
}

impl From<RunPhase> for Phase {
    fn from(phase: RunPhase) -> Self {
        Self::Run(phase)
    }
}
