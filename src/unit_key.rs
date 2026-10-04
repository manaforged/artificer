use crate::action::Key;
use crate::cargo::{self, Package};
use crate::invoke;
use crate::session::Session;
use anyhow::Result;
use std::path::Path;

fn compile_env(sess: &Session, pkg: &Package, name: &str, target_tmpdir: bool) -> Option<String> {
    match name {
        "CARGO_MANIFEST_DIR" => Some(pkg.root().display().to_string()),
        "CARGO_MANIFEST_PATH" => Some(pkg.manifest_path.display().to_string()),
        "CARGO_PKG_NAME" => Some(pkg.name.clone()),
        "CARGO_PKG_VERSION" => Some(pkg.version.clone()),
        "CARGO_PRIMARY_PACKAGE" => sess.primary.contains(&pkg.id).then(|| "1".to_string()),
        "CARGO_TARGET_TMPDIR" => target_tmpdir
            .then_some(sess.target_tmpdir.as_ref())
            .flatten()
            .map(|path| path.display().to_string()),
        "OUT_DIR" => None,
        _ => sess
            .settings
            .env_value(name)
            .and_then(|value| value.into_string().ok()),
    }
}

fn from_registry(pkg: &Package) -> bool {
    from_registry_in(pkg, &cargo::cargo_home())
}

fn from_registry_in(pkg: &Package, cargo_home: &Path) -> bool {
    if pkg.source.is_none() {
        return false;
    }
    let home = crate::resolve_path(cargo_home);
    let registry = home.join("registry");
    let manifest = crate::resolve_path(&pkg.manifest_path);
    manifest.starts_with(&registry) || pkg.manifest_path.starts_with(cargo_home.join("registry"))
}

fn clippy(sess: &Session, pkg: &Package) -> Option<String> {
    let driver = sess.settings.wrapper_chain(pkg).iter().any(|w| {
        Path::new(w)
            .file_stem()
            .is_some_and(|s| s == "clippy-driver")
    });
    if !driver {
        return None;
    }
    let args = std::env::var("CLIPPY_ARGS").unwrap_or_default();
    let dir = std::env::var("CLIPPY_CONF_DIR").unwrap_or_default();
    let start = if dir.is_empty() {
        pkg.root().to_path_buf()
    } else {
        sess.settings.toolchain_dir.join(&dir)
    };
    let conf = start
        .ancestors()
        .map(|d| {
            [".clippy.toml", "clippy.toml"]
                .iter()
                .filter_map(|name| std::fs::read(d.join(name)).ok().map(|b| (name, b)))
                .map(|(name, b)| format!("{name}:{}", blake3::hash(&b).to_hex()))
                .collect::<Vec<_>>()
        })
        .find(|found| !found.is_empty())
        .unwrap_or_default();
    Some(format!(
        "args={args} dir={dir} conf={conf:?} primary={}",
        sess.primary.contains(&pkg.id)
    ))
}

mod feed;
use feed::{Fed, Feed, feed_inputs};

pub(crate) const DEPS_FILE: &str = "deps.blake3";

pub(crate) fn dep_manifest(
    sess: &Session,
    node: &cargo::Node,
    tests: bool,
    own_lib: Option<&str>,
) -> Result<String> {
    let mut lines = Vec::new();
    if let Some(id) = own_lib
        && let Some(art) = sess.get(id)
    {
        let name = art
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        lines.push(format!("{name} {}", sess.artifact_hash(&art.path)?));
    }
    for d in &node.deps {
        let usable = if tests {
            d.usable_for_lib() || d.usable_for_dev()
        } else {
            d.usable_for_lib() || d.usable_for_script()
        };
        if !usable {
            continue;
        }
        let Some(art) = sess.get(&d.pkg) else {
            continue;
        };
        let file = art.rmeta.clone().unwrap_or_else(|| art.path.clone());
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        lines.push(format!("{name} {}", sess.artifact_hash(&file)?));
    }
    lines.sort();
    Ok(lines.join("\n"))
}

pub(crate) fn deps_match(out: &Path, manifest: &str) -> bool {
    match std::fs::read_to_string(out.join(DEPS_FILE)) {
        Ok(recorded) => recorded.trim() == manifest.trim(),
        Err(_) => false,
    }
}

pub(crate) struct UnitKey {
    pub digest: String,
    pub lineage: Option<String>,
    pub metadata: String,
}

#[expect(
    clippy::too_many_arguments,
    reason = "the digest must receive every independent rustc unit input"
)]
pub(crate) fn unit_digest(
    sess: &Session,
    pkg: &Package,
    node: &cargo::Node,
    kind: &str,
    features: &[String],
    types: &[String],
    takes_lto: bool,
    script: Option<&str>,
    target_tmpdir: bool,
) -> Result<UnitKey> {
    let content = if from_registry(pkg) {
        None
    } else {
        Some(sess.source_key(pkg)?)
    };
    let mut key = Key::new();
    let Fed {
        types,
        feats,
        clippy,
        dev_deps,
        dep_trace,
    } = feed_inputs(
        &mut key,
        sess,
        pkg,
        node,
        kind,
        features,
        types,
        takes_lto,
        script,
        target_tmpdir,
        Feed::Unit {
            content: content.as_deref(),
        },
    );
    let digest = key.digest();
    let mut lineage_digest = None;
    let mut metadata = digest.clone();
    if pkg.source.is_none() {
        let mut lineage = Key::new();
        feed_inputs(
            &mut lineage,
            sess,
            pkg,
            node,
            kind,
            features,
            &types,
            takes_lto,
            script,
            target_tmpdir,
            Feed::Lineage,
        );
        metadata = lineage.digest();
        lineage.feed(sess.settings.workspace_root.as_os_str().as_encoded_bytes());
        lineage_digest = Some(lineage.digest());
        let source = if from_registry(pkg) {
            format!("registry:{}", pkg.id.rsplit('#').next().unwrap_or(&pkg.id))
        } else {
            sess.source_keys
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&pkg.id)
                .cloned()
                .unwrap_or_default()
        };
        let script_id = script.map(|s| {
            let mut h = blake3::Hasher::new();
            h.update(s.as_bytes());
            format!("{}:{}", s.len(), &h.finalize().to_hex()[..12])
        });
        let mut trace = String::new();
        trace.push_str(&format!(
            "rustc: {}\n",
            sess.settings.rustc.lines().next().unwrap_or_default()
        ));
        trace.push_str(&format!("kind: {kind}\n"));
        trace.push_str(&format!("types: {types:?}\n"));
        trace.push_str(&format!("features: {feats:?}\n"));
        trace.push_str(&format!("codegen: {:?}\n", sess.settings.codegen));
        trace.push_str(&format!(
            "wrappers: {:?}\n",
            sess.settings.wrapper_chain(pkg)
        ));
        if let Some(lint) = &clippy {
            trace.push_str(&format!("clippy: {lint:?}\n"));
        }
        trace.push_str(&format!("rustflags: {:?}\n", sess.settings.rustflags));
        trace.push_str(&format!("profile: {:?}\n", sess.profile(pkg)));
        trace.push_str(&format!(
            "overrides: {:?}\n",
            sess.settings
                .overrides
                .for_package(&pkg.name, pkg.source.is_some())
        ));
        trace.push_str(&format!("lints: {:?}\n", sess.settings.lints(pkg)));
        trace.push_str(&format!("check-cfg: {:?}\n", invoke::check_cfg_args(pkg)));
        trace.push_str(&format!(
            "slim: {} release: {}\n",
            sess.settings.mods.slim, sess.settings.release
        ));
        trace.push_str(&format!("linker: {:?}\n", sess.settings.linker));
        trace.push_str(&format!("threads: {:?}\n", sess.settings.threads));
        trace.push_str(&format!("script: {script_id:?}\n"));
        trace.push_str(&format!("source: {source}\n"));
        for line in &dep_trace {
            trace.push_str(line);
            trace.push('\n');
        }
        for name in sess.env_names(pkg, dev_deps).iter() {
            trace.push_str(&format!(
                "env: {name}={}\n",
                compile_env(sess, pkg, name, target_tmpdir)
                    .map(|value| blake3::hash(value.as_bytes()).to_hex().to_string())
                    .unwrap_or_else(|| "absent".into())
            ));
        }
        if std::env::var("ARTIFICER_DEBUG_KEY").is_ok_and(|w| w == pkg.name) {
            crate::out::diag(format!("KEY {} digest={digest}\n{trace}", pkg.name));
        }
        crate::keylog::record(&sess.settings.home, &pkg.name, &digest, &trace);
    }
    Ok(UnitKey {
        digest,
        lineage: lineage_digest,
        metadata,
    })
}

#[cfg(test)]
#[path = "unit_key_tests.rs"]
mod tests;
