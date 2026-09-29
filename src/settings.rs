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
    pub host: String,
    pub wrapper_all: Option<String>,
    pub wrapper_local: Option<String>,
    pub members: HashSet<String>,
    pub rustflags: Vec<String>,
    pub codegen: Vec<String>,
    pub linker: Vec<String>,
    pub threads: Vec<String>,
    pub profile: Vec<String>,
    pub overrides: Overrides,
    pub lto: bool,
    pub release: bool,
    pub mods: Mods,
    lints: HashMap<String, Vec<String>>,
}

impl Settings {
    pub(crate) fn profile_dir(&self) -> PathBuf {
        self.target_dir
            .join(if self.release { "release" } else { "debug" })
    }

    pub(crate) fn profile_value<'a>(&'a self, pkg: &Package, key: &str) -> Option<&'a str> {
        self.profile
            .iter()
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
    ) -> Result<Self> {
        let rustc = crate::out::timed("rustc -vV", || key::rustc_version_in(home, dir))?;
        let host = key::rustc_host(&rustc)?;
        if let Some(name) = crate::gate::gated_env(false) {
            anyhow::bail!("artificer cannot model this environment: {name} is set");
        }
        let mods = crate::mods::load(home)?;
        let release = matches!(name, "release" | "bench");
        let cfg = crate::config::config(dir);
        if let Some(reason) = cfg.unmodeled.first() {
            anyhow::bail!("artificer cannot model this workspace: {reason}");
        }
        if name == "test"
            && let Some(reason) = cfg.unmodeled_doctest.first()
        {
            anyhow::bail!("artificer cannot model this workspace: {reason}");
        }
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
        let mut rustflags = rustflags();
        if rustflags.is_none() {
            let print = crate::out::timed("rustc --print cfg", || key::rustc_print_cfg(home, dir))?;
            rustflags = Some(
                crate::config::resolve_target_flags(&cfg, &host, &print)
                    .map_err(|e| anyhow::anyhow!("artificer cannot model this workspace: {e}"))?
                    .unwrap_or(cfg.rustflags),
            );
        }
        let rustflags = rustflags.unwrap_or_default();
        let codegen = if mods.cranelift && !release {
            flags::codegen(home, &rustc, dir)
        } else {
            Vec::new()
        };
        let linker = if mods.linker {
            flags::linker(home, &rustc, dir)
        } else {
            Vec::new()
        };
        let threads = if mods.threads {
            flags::threads(home, &rustc, dir)
        } else {
            Vec::new()
        };
        let profile = crate::manifest::profile(ws, name);
        crate::manifest::profile_gate(ws, name)
            .map_err(|e| anyhow::anyhow!("artificer cannot model this workspace: {e}"))?;
        let mut lints = HashMap::new();
        for pkg in packages.iter().filter(|p| p.source.is_none()) {
            let root = crate::manifest::package_root(&pkg.manifest_path);
            let args = crate::manifest::lints(&pkg.manifest_path, &root)
                .map_err(|e| anyhow::anyhow!("artificer cannot model this workspace: {e}"))?;
            lints.insert(pkg.id.clone(), args);
        }
        let overrides = crate::manifest::overrides(ws, name)
            .map_err(|e| anyhow::anyhow!("artificer cannot model this workspace: {e}"))?;
        let lto = profile
            .iter()
            .any(|a| a.starts_with("lto=") && a != "lto=false" && a != "lto=off");
        Ok(Self {
            target_dir: crate::config::target_dir(dir, ws),
            workspace_root: ws.to_path_buf(),
            home: crate::resolve_path(home),
            toolchain_dir: dir.to_path_buf(),
            rustc,
            host,
            wrapper_all,
            wrapper_local,
            members: members.iter().cloned().collect(),
            rustflags,
            codegen,
            linker,
            threads,
            profile,
            overrides,
            lto,
            release,
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

    pub(crate) fn rustc_cmd(&self, pkg: &Package) -> Command {
        let chain = self.wrapper_chain(pkg);
        let rustc = key::rustc_bin();
        let mut cmd = match chain.split_first() {
            Some((program, args)) => {
                let mut cmd = Command::new(program);
                cmd.args(args);
                cmd.arg(rustc);
                cmd
            }
            None => Command::new(rustc),
        };
        cmd.current_dir(&self.toolchain_dir);
        cmd
    }
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
