use crate::cargo::Package;
use crate::settings::Settings;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(crate) type Leases = Arc<Mutex<Vec<Arc<std::fs::File>>>>;

#[derive(Clone)]
pub struct Artifact {
    pub crate_name: String,
    pub path: PathBuf,
    pub rmeta: Option<PathBuf>,
    pub proc_macro: bool,
}

pub struct Session {
    pub(crate) leases: Leases,
    pub settings: Settings,
    pub artifacts: Mutex<HashMap<String, Artifact>>,
    pub(crate) natives: Mutex<HashMap<String, String>>,
    link_edges: Mutex<HashMap<String, Vec<(String, LinkEdge)>>>,
    pub(crate) source_keys: Mutex<HashMap<String, String>>,
    artifact_hashes: Mutex<HashMap<PathBuf, String>>,
    env_names: Mutex<HashMap<String, Arc<Vec<String>>>>,
    pub(crate) published: Mutex<HashMap<String, Vec<(String, String)>>>,
    pub(crate) scripts: Mutex<HashMap<String, crate::script::Script>>,
    pub json: bool,
    pub(crate) target_tmpdir: Option<PathBuf>,
    pub meta_only: bool,
    pub must_link: HashSet<String>,
    pub(crate) build_only: HashSet<String>,
    pub(crate) build_pool: Option<crate::jobs::BuildPool>,
    pub ship: HashSet<String>,
    pub(crate) select: Option<crate::build::TargetSel>,
    pub primary: HashSet<String>,
    announced: Mutex<HashSet<(String, crate::out::Status)>>,
    shown: Mutex<HashSet<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkEdge {
    Normal,
    Dev,
}

pub(crate) fn package_label(pkg: &Package) -> String {
    let place = if pkg.source.is_none() {
        format!(" ({})", crate::platform::env_path(pkg.root()).display())
    } else {
        String::new()
    };
    format!("{} v{}{place}", pkg.name, pkg.version)
}

impl Session {
    pub(crate) fn profile(&self, pkg: &Package) -> &[String] {
        if self.build_only.contains(&pkg.id) {
            &self.settings.build_profile
        } else {
            &self.settings.profile
        }
    }

    pub(crate) fn profile_args(&self, pkg: &Package, takes_lto: bool) -> Vec<String> {
        let mut args = crate::settings::profile_for(self.profile(pkg), takes_lto);
        if self.slims(pkg) {
            args.extend(["-C".to_string(), "debuginfo=line-tables-only".to_string()]);
        }
        args
    }

    fn slims(&self, pkg: &Package) -> bool {
        self.settings.mods.slim_deps
            && pkg.source.is_some()
            && self
                .profile_value(pkg, "debuginfo")
                .is_some_and(|v| matches!(v, "1" | "2" | "limited" | "full"))
    }

    pub(crate) fn profile_value(&self, pkg: &Package, key: &str) -> Option<&str> {
        self.settings.profile_value(pkg, self.profile(pkg), key)
    }

    pub fn with_profile(
        home: &Path,
        dir: &Path,
        ws: &Path,
        name: &str,
        members: &[String],
        packages: &[Package],
        target: Option<&Path>,
    ) -> Result<Self> {
        let settings = Settings::load(home, dir, ws, name, members, packages, target)?;
        let build_pool = crate::jobs::BuildPool::new(&settings.home, crate::schedule::job_cap())
            .ok()
            .flatten();
        Ok(Self {
            leases: Arc::default(),
            settings,
            artifacts: Mutex::new(HashMap::new()),
            natives: Mutex::new(HashMap::new()),
            link_edges: Mutex::new(HashMap::new()),
            source_keys: Mutex::new(HashMap::new()),
            artifact_hashes: Mutex::new(HashMap::new()),
            env_names: Mutex::new(HashMap::new()),
            published: Mutex::new(HashMap::new()),
            scripts: Mutex::new(HashMap::new()),
            json: false,
            target_tmpdir: None,
            meta_only: false,
            must_link: HashSet::new(),
            build_only: HashSet::new(),
            build_pool,
            ship: HashSet::new(),
            select: None,
            primary: HashSet::new(),
            announced: Mutex::new(HashSet::new()),
            shown: Mutex::new(HashSet::new()),
        })
    }

    pub(crate) fn first_sight(&self, rendered: &str) -> bool {
        self.shown
            .lock()
            .is_ok_and(|mut seen| seen.insert(rendered.to_string()))
    }

    pub(crate) fn announce(&self, pkg: &Package, build_script: bool) {
        let kind = if self.meta_only && !pkg.is_proc_macro() && !build_script {
            crate::out::Status::Checking
        } else {
            crate::out::Status::Compiling
        };
        let first = self
            .announced
            .lock()
            .is_ok_and(|mut seen| seen.insert((pkg.id.clone(), kind)));
        if first {
            crate::out::status(kind, package_label(pkg));
        }
    }

    pub fn needs_link(&self, pkg: &Package) -> bool {
        if !self.settings.mods.rmeta || !self.meta_only {
            return true;
        }
        pkg.is_proc_macro() || self.must_link.contains(&pkg.id)
    }

    pub fn set_target_tmpdir(&mut self, path: PathBuf) {
        self.target_tmpdir = Some(path);
    }

    pub(crate) fn env_names(&self, pkg: &Package, tests: bool) -> Arc<Vec<String>> {
        let slot = format!("{}|{tests}", pkg.id);
        if let Some(hit) = self.env_names.lock().expect("env_names").get(&slot) {
            return Arc::clone(hit);
        }
        let names = Arc::new(crate::key::env_names(
            pkg.root(),
            &[&self.settings.home, &self.settings.target_dir],
            tests,
        ));
        self.env_names
            .lock()
            .expect("env_names")
            .insert(slot, Arc::clone(&names));
        names
    }

    pub(crate) fn put(&self, id: String, art: Artifact) {
        self.artifacts.lock().expect("artifacts").insert(id, art);
    }

    pub(crate) fn retain(&self, lease: Arc<std::fs::File>) {
        self.leases.lock().expect("unit leases").push(lease);
    }

    pub(crate) fn get(&self, id: &str) -> Option<Artifact> {
        self.artifacts.lock().expect("artifacts").get(id).cloned()
    }

    pub(crate) fn artifact_hash(&self, path: &Path) -> Result<String> {
        if let Some(hash) = self
            .artifact_hashes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(path)
        {
            return Ok(hash.clone());
        }
        let hash = crate::digest::file(Some(&self.settings.home), path)?[..32].to_string();
        self.artifact_hashes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(path.to_path_buf(), hash.clone());
        Ok(hash)
    }

    pub(crate) fn source_key(&self, pkg: &Package) -> Result<String> {
        let cached = self
            .source_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&pkg.id)
            .cloned();
        if let Some(key) = cached {
            return Ok(key);
        }
        let key = crate::profile::span(crate::profile::UnitPhase::Key, || {
            crate::key::lib(
                Some(&self.settings.home),
                pkg.root(),
                &self.settings.rustc,
                &pkg.name,
                pkg.lib_target()
                    .map(|t| t.edition.as_str())
                    .unwrap_or("2021"),
                &[&self.settings.home, &self.settings.target_dir],
            )
        })?;
        self.source_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(pkg.id.clone(), key.clone());
        Ok(key)
    }

    pub(crate) fn learn_links(&self, meta: &crate::cargo::Metadata) {
        let Some(resolve) = meta.resolve.as_ref() else {
            return;
        };
        let mut edges = self
            .link_edges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for node in &resolve.nodes {
            let deps = node
                .deps
                .iter()
                .filter_map(|d| {
                    if d.usable_for_lib() {
                        Some((d.pkg.clone(), LinkEdge::Normal))
                    } else if d.usable_for_dev() {
                        Some((d.pkg.clone(), LinkEdge::Dev))
                    } else {
                        None
                    }
                })
                .collect();
            edges.insert(node.id.clone(), deps);
        }
    }

    pub(crate) fn link_set(&self, root: &str, dev: bool) -> HashSet<String> {
        let edges = self
            .link_edges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut seen = HashSet::from([root.to_string()]);
        let mut stack: Vec<String> = edges
            .get(root)
            .into_iter()
            .flatten()
            .filter(|(_, edge)| *edge == LinkEdge::Normal || dev)
            .map(|(id, _)| id.clone())
            .collect();
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            stack.extend(
                edges
                    .get(&id)
                    .into_iter()
                    .flatten()
                    .filter(|(_, edge)| *edge == LinkEdge::Normal)
                    .map(|(dep, _)| dep.clone()),
            );
        }
        seen
    }
}
