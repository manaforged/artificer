use crate::cargo::{self, Package, Target, Unmodeled};
use anyhow::{Result, bail};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Pick {
    #[default]
    None,
    All,
    Named(Vec<String>),
}

impl Pick {
    pub fn add(&mut self, name: &str) {
        match self {
            Pick::All => {}
            Pick::Named(names) => {
                if !names.iter().any(|n| n == name) {
                    names.push(name.to_string());
                }
            }
            Pick::None => *self = Pick::Named(vec![name.to_string()]),
        }
    }

    pub fn names(&self) -> &[String] {
        match self {
            Pick::Named(names) => names,
            _ => &[],
        }
    }

    pub fn is_none(&self) -> bool {
        *self == Pick::None
    }

    pub(crate) fn wants(&self, name: &str) -> bool {
        match self {
            Pick::None => false,
            Pick::All => true,
            Pick::Named(names) => names.iter().any(|n| n == name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Lib,
    Bin,
    Example,
    Test,
    Bench,
}

impl Kind {
    pub const NAMED: [Kind; 4] = [Kind::Bin, Kind::Example, Kind::Test, Kind::Bench];

    pub fn of(pkg: &Package, target: &Target) -> Option<Kind> {
        if pkg
            .lib_target()
            .is_some_and(|lib| lib.name == target.name && lib.kind == target.kind)
        {
            return Some(Kind::Lib);
        }
        let has = |k: &str| target.kind.iter().any(|kind| kind == k);
        [
            ("bin", Kind::Bin),
            ("example", Kind::Example),
            ("test", Kind::Test),
            ("bench", Kind::Bench),
        ]
        .into_iter()
        .find_map(|(name, kind)| has(name).then_some(kind))
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Lib => "lib",
            Kind::Bin => "bin",
            Kind::Example => "example",
            Kind::Test => "test",
            Kind::Bench => "bench",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Normal,
    Test,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TargetSel {
    pub lib: bool,
    pub bins: Pick,
    pub examples: Pick,
    pub tests: Pick,
    pub benches: Pick,
}

impl TargetSel {
    pub fn all_targets() -> Self {
        Self {
            lib: true,
            bins: Pick::All,
            examples: Pick::All,
            tests: Pick::All,
            benches: Pick::All,
        }
    }

    pub fn from_targets(targets: super::Targets) -> Self {
        if targets.all {
            return Self::all_targets();
        }
        Self {
            tests: if targets.tests { Pick::All } else { Pick::None },
            ..Self::default()
        }
    }

    pub fn targets(&self) -> Option<super::Targets> {
        let tests = Self {
            tests: Pick::All,
            ..Self::default()
        };
        [
            (Self::default(), super::Targets::default()),
            (
                tests,
                super::Targets {
                    tests: true,
                    all: false,
                },
            ),
            (
                Self::all_targets(),
                super::Targets {
                    tests: false,
                    all: true,
                },
            ),
        ]
        .into_iter()
        .find_map(|(sel, targets)| (sel == *self).then_some(targets))
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn wants_dev(&self) -> bool {
        !(self.examples.is_none() && self.tests.is_none() && self.benches.is_none())
    }

    pub fn pick(&self, kind: Kind) -> &Pick {
        match kind {
            Kind::Lib => &Pick::None,
            Kind::Bin => &self.bins,
            Kind::Example => &self.examples,
            Kind::Test => &self.tests,
            Kind::Bench => &self.benches,
        }
    }

    fn effective_bins(&self) -> &Pick {
        if self.is_default() {
            &Pick::All
        } else {
            &self.bins
        }
    }

    pub fn wants_bin(&self, name: &str) -> bool {
        self.effective_bins().wants(name)
    }

    pub fn units<'a>(
        &self,
        pkg: &'a Package,
        features: &[String],
    ) -> Result<Vec<(&'a Target, Kind, Mode)>> {
        let mut out: Vec<(&'a Target, Kind, Mode)> = Vec::new();
        for t in &pkg.targets {
            let Some(kind) = Kind::of(pkg, t) else {
                continue;
            };
            for (mode, named) in self.modes(kind, t) {
                if !covered(pkg, t, features, named)? {
                    continue;
                }
                if !out
                    .iter()
                    .any(|(o, k, m)| o.name == t.name && *k == kind && *m == mode)
                {
                    out.push((t, kind, mode));
                }
            }
        }
        Ok(out)
    }

    fn modes(&self, kind: Kind, t: &Target) -> Vec<(Mode, bool)> {
        [
            self.normal(kind, t).map(|named| (Mode::Normal, named)),
            self.tested(kind, t).map(|named| (Mode::Test, named)),
            self.benched(kind, t).map(|named| (Mode::Test, named)),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn normal(&self, kind: Kind, t: &Target) -> Option<bool> {
        let pick = match kind {
            Kind::Lib => return (self.lib || self.is_default()).then_some(false),
            Kind::Bin => self.effective_bins(),
            Kind::Example => &self.examples,
            Kind::Test | Kind::Bench => return None,
        };
        pick.wants(&t.name)
            .then_some(matches!(pick, Pick::Named(_)))
    }

    fn tested(&self, kind: Kind, t: &Target) -> Option<bool> {
        match &self.tests {
            Pick::All => t.test.then_some(false),
            Pick::Named(names) => (kind == Kind::Test && names.contains(&t.name)).then_some(true),
            Pick::None => None,
        }
    }

    fn benched(&self, kind: Kind, t: &Target) -> Option<bool> {
        match &self.benches {
            Pick::All => matches!(kind, Kind::Lib | Kind::Bin | Kind::Bench).then_some(false),
            Pick::Named(names) => (kind == Kind::Bench && names.contains(&t.name)).then_some(true),
            Pick::None => None,
        }
    }

    pub fn validate(&self, meta: &cargo::Metadata, roots: &[String]) -> Result<()> {
        let pkgs = roots
            .iter()
            .map(|id| cargo::package(meta, id))
            .collect::<Result<Vec<_>>>()?;
        self.validate_names(&pkgs)?;
        self.validate_lib(&pkgs)?;
        if self.is_default() {
            return Ok(());
        }
        for (id, pkg) in roots.iter().zip(&pkgs) {
            let node = cargo::node(meta, id)?;
            self.validate_root(pkg, &node.features, pkgs.len())?;
        }
        Ok(())
    }

    fn validate_names(&self, pkgs: &[&Package]) -> Result<()> {
        for kind in Kind::NAMED {
            for name in self.pick(kind).names() {
                let found = pkgs.iter().any(|pkg| {
                    pkg.targets
                        .iter()
                        .any(|t| t.name == *name && Kind::of(pkg, t) == Some(kind))
                });
                if !found {
                    bail!("no {} target named `{name}`", kind.label());
                }
            }
        }
        Ok(())
    }

    fn validate_lib(&self, pkgs: &[&Package]) -> Result<()> {
        if !self.lib || *self == Self::all_targets() {
            return Ok(());
        }
        match pkgs.iter().find(|pkg| pkg.lib_target().is_none()) {
            Some(pkg) if pkgs.len() == 1 => {
                bail!("no library targets found in package `{}`", pkg.name)
            }
            Some(_) => Err(unmodeled("--lib across packages without a library")),
            None => Ok(()),
        }
    }

    fn validate_root(&self, pkg: &Package, features: &[String], roots: usize) -> Result<()> {
        let units = self.units(pkg, features)?;
        if roots > 1 && units.is_empty() {
            return Err(unmodeled("target selection that skips a selected package"));
        }
        let lib_test = units
            .iter()
            .any(|(_, kind, mode)| *kind == Kind::Lib && *mode == Mode::Test);
        if pkg.is_proc_macro() && lib_test {
            return Err(unmodeled("testing a proc-macro library"));
        }
        let main = pkg.bin_target().filter(|_| pkg.lib_target().is_none());
        if main.is_some_and(|main| !self.wants_bin(&main.name)) {
            return Err(unmodeled(
                "target selection that skips the first binary of a package without a library",
            ));
        }
        Ok(())
    }
}

fn covered(pkg: &Package, t: &Target, features: &[String], named: bool) -> Result<bool> {
    if Package::covered(t, features) {
        return Ok(true);
    }
    if named {
        bail!(
            "target `{}` in package `{}` requires the features: {}",
            t.name,
            pkg.name,
            t.required_features
                .iter()
                .map(|f| format!("`{f}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(false)
}

pub(crate) fn unmodeled(what: &str) -> anyhow::Error {
    Unmodeled(format!("{what} is not modeled")).into()
}
