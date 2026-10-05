use crate::cargo::Package;
use crate::flags;
use crate::key;
use crate::manifest::Overrides;
use crate::mods::Mods;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Settings {
    pub home: PathBuf,
    pub(crate) target_dir: PathBuf,
    pub(crate) workspace_root: PathBuf,
    pub toolchain_dir: PathBuf,
    pub rustc: String,
    pub rustc_exe: String,
    pub host: String,
    pub wrapper_all: Option<String>,
    pub wrapper_local: Option<String>,
    pub members: HashSet<String>,
    pub rustflags: Vec<String>,
    pub host_linker: Vec<String>,
    pub runner: Option<Vec<String>>,
    pub env: Vec<(String, String)>,
    pub threads: Vec<String>,
    pub fork: flags::ForkFlags,
    pub profile: Vec<String>,
    pub build_profile: Vec<String>,
    pub overrides: Overrides,
    pub lto: bool,
    pub release: bool,
    pub incremental: bool,
    pub mods: Mods,
    lints: HashMap<String, Vec<String>>,
}

fn unmodeled(reason: String) -> anyhow::Error {
    anyhow::anyhow!("artificer cannot model this workspace: {reason}")
}

fn checked_config(dir: &Path, name: &str) -> Result<crate::config::Config> {
    let cfg = crate::config::config(dir);
    let doctest = if name == "test" {
        cfg.unmodeled_doctest.first()
    } else {
        None
    };
    if let Some(reason) = cfg.unmodeled.first().or(doctest) {
        return Err(unmodeled(reason.clone()));
    }
    Ok(cfg)
}

struct TargetFlags {
    rustflags: Vec<String>,
    host_linker: Vec<String>,
    runner: Option<Vec<String>>,
}

fn target_flags(
    cfg: &crate::config::Config,
    host: &str,
    home: &Path,
    dir: &Path,
) -> Result<TargetFlags> {
    let explicit = rustflags();
    let print = if explicit.is_none() || !cfg.target_tools.is_empty() {
        crate::profile::span(crate::profile::SetupPhase::RustcCfg, || {
            key::rustc_print_cfg(home, dir)
        })?
    } else {
        Vec::new()
    };
    let rustflags = match explicit {
        Some(flags) => flags,
        None => crate::config::resolve_target_flags(cfg, host, &print)
            .map_err(unmodeled)?
            .unwrap_or_else(|| cfg.rustflags.clone()),
    };
    let tools =
        crate::config::resolve_host_tools(&cfg.target_tools, host, &print).map_err(unmodeled)?;
    Ok(TargetFlags {
        rustflags,
        host_linker: tools
            .linker
            .map(|path| vec!["-C".into(), format!("linker={path}")])
            .unwrap_or_default(),
        runner: tools.runner,
    })
}

fn workspace_lints(packages: &[Package]) -> Result<HashMap<String, Vec<String>>> {
    packages
        .iter()
        .filter(|p| p.source.is_none())
        .map(|pkg| {
            let root = crate::manifest::package_root(&pkg.manifest_path);
            crate::manifest::lints(&pkg.manifest_path, &root)
                .map(|args| (pkg.id.clone(), args))
                .map_err(unmodeled)
        })
        .collect()
}

impl Settings {
    pub(crate) fn profile_dir(&self) -> PathBuf {
        self.target_dir
            .join(if self.release { "release" } else { "debug" })
    }

    pub(crate) fn compile_dir(&self, pkg: &Package, lineage: Option<&str>) -> Option<PathBuf> {
        let lineage = lineage?;
        self.incremental_dir(pkg)?;
        Some(self.profile_dir().join(COMPILE_DIR).join(lineage))
    }

    pub(crate) fn incremental_dir(&self, pkg: &Package) -> Option<PathBuf> {
        (self.incremental && pkg.source.is_none())
            .then(|| self.profile_dir().join(crate::sweep::INCREMENTAL_DIR))
    }

    pub(crate) fn profile_value<'a>(
        &'a self,
        pkg: &Package,
        base: &'a [String],
        key: &str,
    ) -> Option<&'a str> {
        base.iter()
            .chain(
                self.overrides
                    .for_package(&pkg.name, pkg.source.is_some())
                    .iter(),
            )
            .filter_map(|arg| arg.split_once('='))
            .filter(|(name, _)| *name == key)
            .map(|(_, value)| value)
            .next_back()
    }

    pub fn load(
        home: &Path,
        dir: &Path,
        ws: &Path,
        name: &str,
        members: &[String],
        packages: &[Package],
        target: Option<&Path>,
    ) -> Result<Self> {
        let rustc = crate::profile::span(crate::profile::SetupPhase::RustcVersion, || {
            key::rustc_version_in(home, dir)
        })?;
        let host = key::rustc_host(&rustc)?;
        let rustc_exe = key::rustc_exe(home, dir);
        if let Some(name) = crate::gate::gated_env(false) {
            anyhow::bail!("artificer cannot model this environment: {name} is set");
        }
        let mods = crate::mods::load(home)?;
        let release = matches!(name, "release" | "bench");
        let cfg = checked_config(dir, name)?;
        let wrapper_all = wrapper_var(
            "RUSTC_WRAPPER",
            "CARGO_BUILD_RUSTC_WRAPPER",
            cfg.rustc_wrapper.as_deref(),
        );
        let wrapper_local = wrapper_var(
            "RUSTC_WORKSPACE_WRAPPER",
            "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
            cfg.rustc_workspace_wrapper.as_deref(),
        );
        let target_flags = target_flags(&cfg, &host, home, dir)?;
        let env = crate::config::effective_env(&cfg.env);
        let fork = flags::fork_flags(home, &rustc, dir, &mods);
        let threads = if mods.threads {
            flags::threads(home, &rustc, dir)
        } else {
            Vec::new()
        };
        let profile = crate::manifest::profile(ws, name, crate::manifest::UnitUse::Runtime);
        let build_profile = crate::manifest::profile(ws, name, crate::manifest::UnitUse::BuildOnly);
        let incremental = incremental(crate::manifest::profile_gate(ws, name).map_err(unmodeled)?);
        let lints = workspace_lints(packages)?;
        let overrides = crate::manifest::overrides(ws, name).map_err(unmodeled)?;
        let lto = profile
            .iter()
            .any(|a| a.starts_with("lto=") && a != "lto=false" && a != "lto=off");
        let target_dir = crate::config::target_dir(target, dir, ws);
        crate::profile::note_target(&target_dir);
        Ok(Self {
            target_dir,
            workspace_root: ws.to_path_buf(),
            home: crate::resolve_path(home),
            toolchain_dir: dir.to_path_buf(),
            rustc,
            rustc_exe,
            host,
            wrapper_all,
            wrapper_local,
            members: members.iter().cloned().collect(),
            rustflags: target_flags.rustflags,
            host_linker: target_flags.host_linker,
            runner: target_flags.runner,
            env,
            threads,
            fork,
            profile,
            build_profile,
            overrides,
            lto,
            release,
            incremental,
            mods,
            lints,
        })
    }

    pub(crate) fn lints(&self, pkg: &Package) -> &[String] {
        self.lints.get(&pkg.id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub(crate) fn wrapper_chain(&self, pkg: &Package) -> Vec<String> {
        let mut chain = Vec::new();
        if let Some(w) = &self.wrapper_all {
            chain.push(w.clone());
        }
        if self.members.contains(&pkg.id)
            && let Some(w) = &self.wrapper_local
        {
            chain.push(w.clone());
        }
        chain
    }

    pub(crate) fn apply_env(&self, cmd: &mut Command) {
        for (name, value) in &self.env {
            cmd.env(name, value);
        }
    }

    pub(crate) fn exec_cmd(&self, exe: &Path) -> Command {
        let mut cmd = match self.runner.as_deref() {
            Some([program, args @ ..]) => {
                let mut cmd = Command::new(program);
                cmd.args(args).arg(exe);
                cmd
            }
            _ => Command::new(exe),
        };
        self.apply_env(&mut cmd);
        cmd
    }

    pub(crate) fn env_value(&self, name: &str) -> Option<std::ffi::OsString> {
        self.env
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value.into())
            .or_else(|| std::env::var_os(name))
    }

    pub(crate) fn portable_env(&self) -> Vec<String> {
        self.env
            .iter()
            .map(|(name, value)| format!("{name}={}", crate::inputs::portable(&self.home, value)))
            .collect()
    }

    pub(crate) fn rustc_cmd(&self, pkg: &Package) -> Command {
        let chain = self.wrapper_chain(pkg);
        let rustc = &self.rustc_exe;
        let mut cmd = match chain.split_first() {
            Some((program, args)) => {
                let mut cmd = Command::new(program);
                cmd.args(args);
                cmd.arg(rustc);
                cmd
            }
            None => Command::new(rustc),
        };
        self.apply_env(&mut cmd);
        cmd.current_dir(&self.toolchain_dir);
        cmd
    }
}

const COMPILE_DIR: &str = "artificer";
const INCREMENTAL_ENV: &str = "CARGO_INCREMENTAL";
const BUILD_INCREMENTAL_ENV: &str = "CARGO_BUILD_INCREMENTAL";

pub(crate) fn incremental(profile: bool) -> bool {
    if let Some(value) = std::env::var_os(INCREMENTAL_ENV) {
        return value == "1";
    }
    std::env::var_os(BUILD_INCREMENTAL_ENV)
        .and_then(|value| value.to_string_lossy().trim().parse::<bool>().ok())
        .unwrap_or(profile)
}

#[must_use]
fn rustflags() -> Option<Vec<String>> {
    if let Some(encoded) = std::env::var_os("CARGO_ENCODED_RUSTFLAGS") {
        return Some(
            encoded
                .to_string_lossy()
                .split('\x1f')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        );
    }
    std::env::var_os("RUSTFLAGS").map(|plain| {
        plain
            .to_string_lossy()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    })
}

pub(crate) fn rustdocflags() -> Vec<String> {
    if let Some(encoded) = std::env::var_os("CARGO_ENCODED_RUSTDOCFLAGS") {
        return encoded
            .to_string_lossy()
            .split('\x1f')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
    }
    std::env::var_os("RUSTDOCFLAGS")
        .map(|plain| {
            plain
                .to_string_lossy()
                .split_whitespace()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn wrapper_var(dedicated: &str, config_env: &str, file: Option<&str>) -> Option<String> {
    for name in [dedicated, config_env] {
        if let Some(value) = std::env::var_os(name) {
            let value = value.to_string_lossy();
            let value = value.trim();
            return (!value.is_empty() && value != "rustc").then(|| value.to_string());
        }
    }
    file.map(str::trim)
        .filter(|v| !v.is_empty() && *v != "rustc")
        .map(str::to_string)
}

pub(crate) fn profile_for(args: &[String], takes_lto: bool) -> Vec<String> {
    if takes_lto {
        return args.to_vec();
    }
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-C" && args.get(i + 1).is_some_and(|v| v.starts_with("lto=")) {
            i += 2;
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }
    out
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
