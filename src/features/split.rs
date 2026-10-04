use super::{Selection, Sides, dbg_sel};
use crate::cargo::{Dep, Metadata, host_id, node, package};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

fn differ(sides: &Sides) -> bool {
    match (&sides.normal, &sides.host) {
        (Some(normal), Some(host)) => {
            normal.iter().collect::<BTreeSet<_>>() != host.iter().collect::<BTreeSet<_>>()
        }
        _ => false,
    }
}

fn carries_split(meta: &Metadata, id: &str, split: &HashSet<String>) -> bool {
    node(meta, id).is_ok_and(|n| {
        n.deps.iter().any(|d| {
            split.contains(&d.pkg)
                && (d.dep_kinds.is_empty() || d.dep_kinds.iter().any(|k| !k.is_build()))
        })
    })
}

pub(super) fn plan(meta: &Metadata, feats: HashMap<String, Sides>) -> Option<Selection> {
    let mut split: HashSet<String> = feats
        .iter()
        .filter(|(_, sides)| differ(sides))
        .map(|(id, _)| id.clone())
        .collect();
    loop {
        let grown: Vec<String> = feats
            .iter()
            .filter(|(id, sides)| {
                sides.normal.is_some()
                    && sides.host.is_some()
                    && !split.contains(*id)
                    && carries_split(meta, id, &split)
            })
            .map(|(id, _)| id.clone())
            .collect();
        if grown.is_empty() {
            break;
        }
        split.extend(grown);
    }
    if let Some(id) = split.iter().find(|id| meta.workspace_members.contains(id)) {
        dbg_sel(&format!(
            "workspace member {id} resolves different host features"
        ));
        return None;
    }
    Some(Selection { feats, split })
}

fn rewire(deps: &[Dep], host: bool, split: &HashSet<String>) -> Vec<Dep> {
    let mut out = Vec::with_capacity(deps.len());
    for d in deps {
        if !split.contains(&d.pkg) {
            out.push(d.clone());
            continue;
        }
        let (build, rest): (Vec<_>, Vec<_>) =
            d.dep_kinds.iter().cloned().partition(|k| k.is_build());
        if host || (!build.is_empty() && rest.is_empty()) {
            out.push(Dep {
                pkg: host_id(&d.pkg),
                ..d.clone()
            });
            continue;
        }
        if build.is_empty() {
            out.push(d.clone());
            continue;
        }
        out.push(Dep {
            dep_kinds: rest,
            ..d.clone()
        });
        out.push(Dep {
            pkg: host_id(&d.pkg),
            dep_kinds: build,
            ..d.clone()
        });
    }
    out
}

pub fn narrow(meta: &mut Metadata, sel: &Selection) {
    let clones: Vec<_> = sel
        .split
        .iter()
        .filter_map(|id| package(meta, id).ok())
        .map(|p| {
            let mut p = p.clone();
            p.id = host_id(&p.id);
            p
        })
        .collect();
    let Some(resolve) = meta.resolve.as_mut() else {
        return;
    };
    resolve.nodes.retain(|n| sel.feats.contains_key(&n.id));
    let mut hosts = Vec::new();
    for node in &mut resolve.nodes {
        let Some(sides) = sel.feats.get(&node.id) else {
            continue;
        };
        node.deps.retain(|d| sel.feats.contains_key(&d.pkg));
        if sel.split.contains(&node.id) {
            let mut host = node.clone();
            host.id = host_id(&node.id);
            host.features = sides.host.clone().unwrap_or_default();
            host.deps = rewire(&node.deps, true, &sel.split);
            hosts.push(host);
        }
        if let Some(f) = sides.normal.as_ref().or(sides.host.as_ref()) {
            node.features.clone_from(f);
        }
        node.deps = rewire(&node.deps, sides.normal.is_none(), &sel.split);
    }
    resolve.nodes.extend(hosts);
    meta.packages.extend(clones);
    meta.node_ix = OnceLock::new();
    meta.pkg_ix = OnceLock::new();
}
