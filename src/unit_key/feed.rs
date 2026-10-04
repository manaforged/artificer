use super::{Key, clippy, compile_env, from_registry};
use crate::cargo::{self, Package};
use crate::session::Session;
use crate::{invoke, key, settings};

#[derive(Clone, Copy)]
pub(super) enum Feed<'a> {
    Unit { content: Option<&'a str> },
    Lineage,
}

pub(super) struct Fed {
    pub(super) types: Vec<String>,
    pub(super) feats: Vec<String>,
    pub(super) clippy: Option<String>,
    pub(super) dev_deps: bool,
    pub(super) dep_trace: Vec<String>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "the digest must receive every independent rustc unit input"
)]
pub(super) fn feed_inputs(
    key: &mut Key,
    sess: &Session,
    pkg: &Package,
    node: &cargo::Node,
    kind: &str,
    features: &[String],
    types: &[String],
    takes_lto: bool,
    script: Option<&str>,
    target_tmpdir: bool,
    feed: Feed<'_>,
) -> Fed {
    key.feed_str(&sess.settings.rustc);
    key.feed_str(&key::rustc_bin());
    key.feed_str(&key::explicit_rustc_identity().unwrap_or_default());
    for a in &sess.settings.codegen {
        key.feed(a.as_bytes());
    }
    for w in sess.settings.wrapper_chain(pkg) {
        key.feed(w.as_bytes());
    }
    let clippy = clippy(sess, pkg);
    if let Some(lint) = &clippy {
        key.feed_str(lint);
    }
    key.feed_str(kind);
    let mut types = types.to_vec();
    types.sort();
    key.feed_list(&types);
    if let Some(out) = script {
        key.feed(out.as_bytes());
    } else if pkg.script_target().is_some() {
        key.feed(b"script-pending");
    }
    key.feed_list(&sess.settings.rustflags);
    key.feed_list(settings::profile_for(&sess.settings.profile, takes_lto));
    key.feed_list(
        sess.settings
            .overrides
            .for_package(&pkg.name, pkg.source.is_some()),
    );
    key.feed_list(sess.settings.lints(pkg).iter());
    key.feed_list(invoke::check_cfg_args(pkg));
    key.feed(&[u8::from(sess.settings.mods.slim)]);
    key.feed(&[u8::from(sess.settings.release)]);
    key.feed_list(&sess.settings.linker);
    key.feed_list(&sess.settings.threads);
    let mut feats = features.to_vec();
    feats.sort();
    key.feed_list(&feats);
    let dev_deps = kind.starts_with("test-") || kind.starts_with("example-");
    if from_registry(pkg) {
        key.feed_str(&pkg.id);
    } else {
        if let Feed::Unit {
            content: Some(base),
        } = feed
        {
            key.feed_str(base);
        }
        for name in sess.env_names(pkg, dev_deps).iter() {
            key.feed_str(name);
            let value = compile_env(sess, pkg, name, target_tmpdir);
            key.feed(&[u8::from(value.is_some())]);
            if let Some(value) = value {
                key.feed(value.as_bytes());
            }
        }
    }
    let mut deps: Vec<_> = node
        .deps
        .iter()
        .filter(|d| {
            if dev_deps {
                d.usable_for_lib() || d.usable_for_dev()
            } else {
                d.usable_for_lib() || d.usable_for_script()
            }
        })
        .map(|d| d.pkg.as_str())
        .collect();
    deps.sort();
    let mut dep_trace = Vec::new();
    for d in deps {
        let id = d.rsplit('#').next().unwrap_or(d);
        key.feed_str(id);
        let mut artifact = String::new();
        if let Feed::Unit { .. } = feed
            && let Some(art) = sess.get(d)
            && let Some(name) = art.path.file_name()
        {
            key.feed(name.as_encoded_bytes());
            artifact = name.to_string_lossy().into_owned();
        }
        dep_trace.push(format!("dep: {id}=>{artifact}"));
    }
    Fed {
        types,
        feats,
        clippy,
        dev_deps,
        dep_trace,
    }
}
