use crate::home::dirs_home;
use crate::store;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub fn root<'a>(meta: &'a Metadata, dir: &'a std::path::Path) -> &'a std::path::Path {
    if meta.workspace_root.as_os_str().is_empty() {
        dir
    } else {
        meta.workspace_root.as_path()
    }
}

#[derive(Debug)]
pub struct Unmodeled(pub String);

impl std::fmt::Display for Unmodeled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unmodeled {}

#[derive(Debug, Deserialize, Clone)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub resolve: Option<Resolve>,
    #[serde(default)]
    pub workspace_members: Vec<String>,
    #[serde(skip)]
    pub(crate) pkg_ix: OnceLock<HashMap<String, usize>>,
    #[serde(skip)]
    pub(crate) node_ix: OnceLock<HashMap<String, usize>>,
    #[serde(default)]
    pub workspace_root: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub id: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub license_file: Option<PathBuf>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub rust_version: Option<String>,
    pub source: Option<String>,
    pub manifest_path: PathBuf,
    pub targets: Vec<Target>,
    #[serde(default)]
    pub links: Option<String>,
    #[serde(default, rename = "features")]
    pub declared: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Target {
    pub name: String,
    pub kind: Vec<String>,
    #[serde(default)]
    pub crate_types: Vec<String>,
    pub src_path: PathBuf,
    pub edition: String,
    #[serde(default, rename = "required-features")]
    pub required_features: Vec<String>,
    #[serde(default = "default_true")]
    pub test: bool,
    #[serde(default = "default_true")]
    pub doc: bool,
    #[serde(default = "default_true")]
    pub doctest: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
pub struct Resolve {
    pub root: Option<String>,
    pub nodes: Vec<Node>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Node {
    pub id: String,
    #[serde(default)]
    pub deps: Vec<Dep>,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Dep {
    pub name: String,
    pub pkg: String,
    #[serde(default)]
    pub dep_kinds: Vec<DepKind>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DepKind {
    pub kind: Option<String>,
    pub target: Option<String>,
}

impl Package {
    pub fn root(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(Path::new("."))
    }

    pub fn lib_target(&self) -> Option<&Target> {
        const LIB: [&str; 6] = ["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"];
        self.targets
            .iter()
            .find(|t| t.kind.iter().any(|k| LIB.contains(&k.as_str())))
    }

    pub fn script_target(&self) -> Option<&Target> {
        self.targets
            .iter()
            .find(|t| t.kind.iter().any(|k| k == "custom-build"))
    }

    pub fn is_proc_macro(&self) -> bool {
        self.lib_target()
            .is_some_and(|t| t.kind.iter().any(|k| k == "proc-macro"))
    }

    pub fn bin_target(&self) -> Option<&Target> {
        self.targets
            .iter()
            .find(|t| t.kind.iter().any(|k| k == "bin"))
    }

    pub fn covered(t: &Target, features: &[String]) -> bool {
        t.required_features
            .iter()
            .all(|f| features.iter().any(|x| x == f))
    }
}

impl DepKind {
    pub fn is_build(&self) -> bool {
        self.kind.as_deref() == Some("build")
    }
}

impl Dep {
    pub fn is_dev(&self) -> bool {
        self.dep_kinds
            .iter()
            .any(|k| k.kind.as_deref() == Some("dev"))
            && self
                .dep_kinds
                .iter()
                .all(|k| k.kind.as_deref() == Some("dev") || disabled_cfg(k.target.as_deref()))
    }

    pub fn usable_for_lib(&self) -> bool {
        self.dep_kinds.iter().any(|k| {
            (k.kind.is_none() || k.kind.as_deref() == Some("normal"))
                && !disabled_cfg(k.target.as_deref())
        }) || self.dep_kinds.is_empty()
    }

    pub fn usable_for_script(&self) -> bool {
        self.dep_kinds
            .iter()
            .any(|k| k.is_build() && !disabled_cfg(k.target.as_deref()))
    }

    pub fn usable_for_dev(&self) -> bool {
        self.dep_kinds
            .iter()
            .any(|k| k.kind.as_deref() == Some("dev") && !disabled_cfg(k.target.as_deref()))
    }
}

fn disabled_cfg(target: Option<&str>) -> bool {
    matches!(target, Some("cfg(any())"))
}

pub fn metadata(manifest: &Path, home: &Path) -> Result<Metadata> {
    metadata_extra(manifest, &[], home)
}

pub fn cargo_home() -> PathBuf {
    crate::resolve_path(&cargo_home_alias())
}

pub(crate) fn cargo_home_alias() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs_home().join(".cargo"))
}

pub fn rustup_cargo() -> PathBuf {
    cargo_home()
        .join("bin")
        .join(format!("cargo{}", std::env::consts::EXE_SUFFIX))
}

pub fn cargo_version(home: &Path) -> Result<String> {
    let dir = std::env::current_dir().context("cwd")?;
    let key = format!(
        "{}|{}",
        crate::key::toolchain_key(&dir),
        cargo_bin().display()
    );
    crate::key::probe_memo(home, "cargo", &key, || {
        let mut cmd = Command::new(cargo_bin());
        cmd.arg("--version").current_dir(&dir);
        crate::jobs::isolate(&mut cmd);
        let out = cmd.output().context("spawn cargo --version")?;
        anyhow::ensure!(out.status.success(), "cargo --version failed");
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    })
}

pub fn pinned_toolchain_cargo(p: &Path) -> bool {
    p.components().any(|c| c.as_os_str() == "toolchains")
}

pub(crate) fn cargo_bin() -> PathBuf {
    if let Ok(p) = std::env::var("ARTIFICER_REAL_CARGO") {
        let p = PathBuf::from(p);
        if p.is_file() && !pinned_toolchain_cargo(&p) {
            return p;
        }
    }
    let stamp = dirs_home().join(".artificer").join("real-cargo");
    if let Ok(p) = std::fs::read_to_string(&stamp) {
        let p = PathBuf::from(p.trim());
        if p.is_file() && !pinned_toolchain_cargo(&p) {
            return p;
        }
    }
    rustup_cargo()
}

pub fn toolchain_path() -> Option<std::ffi::OsString> {
    if crate::platform::on_path("rustc").is_some() {
        return None;
    }
    let dir = cargo_bin().parent()?.to_path_buf();
    if !dir
        .join(format!("rustc{}", std::env::consts::EXE_SUFFIX))
        .is_file()
    {
        return None;
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    dirs.push(dir);
    std::env::join_paths(dirs).ok()
}

pub fn set_package_env(cmd: &mut Command, pkg: &Package) {
    cmd.env("CARGO_PKG_NAME", &pkg.name);
    cmd.env("CARGO_PKG_VERSION", &pkg.version);
    cmd.env("CARGO_MANIFEST_DIR", crate::platform::env_path(pkg.root()));
    cmd.env(
        "CARGO_MANIFEST_PATH",
        crate::platform::env_path(&pkg.manifest_path),
    );
    let version = pkg
        .version
        .split_once('+')
        .map_or(pkg.version.as_str(), |(version, _)| version);
    let mut parts = version.splitn(3, '.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("0");
    let rest = parts.next().unwrap_or("0");
    let (patch, pre) = match rest.split_once('-') {
        Some((p, pre)) => (p, pre),
        None => (rest, ""),
    };
    cmd.env("CARGO_PKG_VERSION_MAJOR", major);
    cmd.env("CARGO_PKG_VERSION_MINOR", minor);
    cmd.env("CARGO_PKG_VERSION_PATCH", patch);
    cmd.env("CARGO_PKG_VERSION_PRE", pre);
    cmd.env("CARGO", cargo_bin());
    cmd.env("CARGO_PKG_AUTHORS", pkg.authors.join(":"));
    cmd.env(
        "CARGO_PKG_DESCRIPTION",
        pkg.description.as_deref().unwrap_or_default(),
    );
    cmd.env(
        "CARGO_PKG_HOMEPAGE",
        pkg.homepage.as_deref().unwrap_or_default(),
    );
    cmd.env(
        "CARGO_PKG_LICENSE",
        pkg.license.as_deref().unwrap_or_default(),
    );
    cmd.env(
        "CARGO_PKG_LICENSE_FILE",
        pkg.license_file
            .as_deref()
            .unwrap_or_else(|| Path::new(""))
            .as_os_str(),
    );
    cmd.env(
        "CARGO_PKG_REPOSITORY",
        pkg.repository.as_deref().unwrap_or_default(),
    );
    cmd.env(
        "CARGO_PKG_RUST_VERSION",
        pkg.rust_version.as_deref().unwrap_or_default(),
    );
}

fn lock_digest(manifest: &Path) -> String {
    let mut h = blake3::Hasher::new();
    if let Some(lock) = nearest_lock(manifest)
        && let Ok(bytes) = std::fs::read(&lock)
    {
        h.update(&bytes);
    }
    h.finalize().to_hex()[..32].to_string()
}

pub(crate) fn nearest_lock(manifest: &Path) -> Option<PathBuf> {
    let mut dir = manifest.parent()?.to_path_buf();
    loop {
        let lock = dir.join("Cargo.lock");
        if lock.is_file() {
            return Some(lock);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
#[path = "cargo_tests.rs"]
mod tests;

#[must_use]
pub fn stock_cargo() -> PathBuf {
    cargo_bin()
}

mod cache;
use cache::{disk_meta, local_only, meta_ram_get, meta_ram_put};

mod graph;
#[cfg(all(test, unix))]
use graph::same_package_id;
pub use graph::{
    closure_many, compile_deps, find_manifest, host_id, id_by_name, must_link, node, package,
    root_id, test_closure_many, test_compile_deps,
};

mod metadata;
pub use metadata::metadata_extra;

mod kind;
pub(crate) use kind::TargetKind;

pub(crate) use cache::meta_key;

#[cfg(test)]
use cache::reroot;
