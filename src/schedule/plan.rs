use super::Unit;
use crate::cargo;
use crate::profile::{PlannedUnit, Role};
use crate::session::Session;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

pub(super) struct Plan {
    pub(super) units: Vec<Unit>,
    pub(super) deps: HashMap<Unit, Vec<Unit>>,
    pub(super) links: HashSet<Unit>,
}

pub(super) struct Waits {
    pub(super) meta: HashMap<Unit, Vec<Unit>>,
    pub(super) full: HashMap<Unit, Vec<Unit>>,
}

pub(super) fn plan(
    sess: &Session,
    meta: &cargo::Metadata,
    ids: &[String],
    roots: &[String],
    dev: bool,
) -> Result<Plan> {
    let mut units = Vec::new();
    let mut deps = HashMap::new();
    for id in ids {
        package_units(meta, id, &mut units, &mut deps)?;
    }
    for root in roots {
        units.push(Unit::Extra(root.clone()));
        let direct = if dev {
            cargo::test_compile_deps(meta, root)?
        } else {
            cargo::compile_deps(meta, root)?
        };
        let mut d: Vec<Unit> = direct.into_iter().map(Unit::Pkg).collect();
        d.push(Unit::Pkg(root.clone()));
        deps.insert(Unit::Extra(root.clone()), d);
    }
    let links = units
        .iter()
        .filter(|unit| unit_links(sess, meta, unit))
        .cloned()
        .collect();
    Ok(Plan { units, deps, links })
}

fn unit_links(sess: &Session, meta: &cargo::Metadata, unit: &Unit) -> bool {
    match unit {
        Unit::Extra(_) | Unit::Script(_) => true,
        Unit::Pkg(id) => {
            cargo::package(meta, id).map_or(true, |pkg| crate::compile::waits_for_link(sess, pkg))
        }
    }
}

fn package_units(
    meta: &cargo::Metadata,
    id: &str,
    units: &mut Vec<Unit>,
    deps: &mut HashMap<Unit, Vec<Unit>>,
) -> Result<()> {
    let pkg = cargo::package(meta, id)?;
    let node = cargo::node(meta, id)?;
    let mut lib: Vec<Unit> = node
        .deps
        .iter()
        .filter(|d| !d.is_dev() && d.usable_for_lib())
        .map(|d| Unit::Pkg(d.pkg.clone()))
        .collect();
    if pkg.script_target().is_some() {
        let script = Unit::Script(id.to_string());
        deps.insert(script.clone(), script_deps(meta, node));
        units.push(script.clone());
        lib.push(script);
    }
    units.push(Unit::Pkg(id.to_string()));
    deps.insert(Unit::Pkg(id.to_string()), lib);
    Ok(())
}

fn script_deps(meta: &cargo::Metadata, node: &cargo::Node) -> Vec<Unit> {
    let mut out: Vec<Unit> = node
        .deps
        .iter()
        .filter(|d| !d.is_dev() && d.usable_for_script())
        .map(|d| Unit::Pkg(d.pkg.clone()))
        .collect();
    out.extend(
        node.deps
            .iter()
            .filter(|d| !d.is_dev() && (d.usable_for_lib() || d.usable_for_script()))
            .filter(|d| {
                cargo::package(meta, &d.pkg)
                    .is_ok_and(|pkg| pkg.links.is_some() && pkg.script_target().is_some())
            })
            .map(|d| Unit::Script(d.pkg.clone())),
    );
    out
}

pub(super) fn waits(plan: &Plan) -> Waits {
    let mut meta = HashMap::new();
    for unit in plan.units.iter().filter(|unit| !plan.links.contains(*unit)) {
        if let Some(direct) = plan.deps.get(unit) {
            meta.insert(unit.clone(), direct.clone());
        }
    }
    Waits {
        meta,
        full: closures(plan),
    }
}

fn closures(plan: &Plan) -> HashMap<Unit, Vec<Unit>> {
    let index: HashMap<&Unit, usize> = plan
        .units
        .iter()
        .enumerate()
        .map(|(at, unit)| (unit, at))
        .collect();
    let edges: Vec<Vec<usize>> = plan
        .units
        .iter()
        .map(|unit| {
            plan.deps
                .get(unit)
                .map(|ds| ds.iter().filter_map(|d| index.get(d).copied()).collect())
                .unwrap_or_default()
        })
        .collect();
    let mut memo: Vec<Option<Vec<usize>>> = vec![None; plan.units.len()];
    plan.units
        .iter()
        .enumerate()
        .filter(|(_, unit)| plan.links.contains(*unit))
        .map(|(at, unit)| {
            let reached = reach(at, &edges, &mut memo)
                .into_iter()
                .filter_map(|dep| plan.units.get(dep).cloned())
                .collect();
            (unit.clone(), reached)
        })
        .collect()
}

fn reach(at: usize, edges: &[Vec<usize>], memo: &mut [Option<Vec<usize>>]) -> Vec<usize> {
    if let Some(Some(hit)) = memo.get(at) {
        return hit.clone();
    }
    if let Some(slot) = memo.get_mut(at) {
        *slot = Some(Vec::new());
    }
    let mut all = Vec::new();
    for &dep in edges.get(at).map(Vec::as_slice).unwrap_or_default() {
        all.push(dep);
        all.extend(reach(dep, edges, memo));
    }
    all.sort_unstable();
    all.dedup();
    if let Some(slot) = memo.get_mut(at) {
        *slot = Some(all.clone());
    }
    all
}

pub(super) fn scores(units: &[Unit], deps: &HashMap<Unit, Vec<Unit>>) -> HashMap<Unit, usize> {
    let want: HashSet<&Unit> = units.iter().collect();
    let mut waiters: HashMap<&Unit, Vec<&Unit>> = HashMap::new();
    for id in units {
        if let Some(ds) = deps.get(id) {
            for d in ds.iter().filter(|d| want.contains(d)) {
                waiters.entry(d).or_default().push(id);
            }
        }
    }
    fn reach<'a>(
        id: &'a Unit,
        waiters: &HashMap<&'a Unit, Vec<&'a Unit>>,
        memo: &mut HashMap<&'a Unit, HashSet<&'a Unit>>,
    ) -> HashSet<&'a Unit> {
        if let Some(hit) = memo.get(id) {
            return hit.clone();
        }
        let mut all = HashSet::new();
        memo.insert(id, HashSet::new());
        for w in waiters.get(id).map(Vec::as_slice).unwrap_or_default() {
            all.insert(*w);
            all.extend(reach(w, waiters, memo));
        }
        memo.insert(id, all.clone());
        all
    }
    let mut memo = HashMap::new();
    units
        .iter()
        .map(|id| {
            let n = reach(id, &waiters, &mut memo).len();
            (id.clone(), n)
        })
        .collect()
}

pub(super) fn planned(
    meta: &cargo::Metadata,
    plan: &Plan,
    order: &HashMap<Unit, usize>,
) -> Vec<PlannedUnit> {
    plan.units
        .iter()
        .map(|unit| {
            let (id, role) = match unit {
                Unit::Pkg(id) => (id, Role::Package),
                Unit::Script(id) => (id, Role::Script),
                Unit::Extra(id) => (id, Role::Targets),
            };
            let (name, version) = cargo::package(meta, id).map_or_else(
                |_| (id.clone(), String::new()),
                |pkg| (pkg.name.clone(), pkg.version.clone()),
            );
            PlannedUnit {
                package: id.clone(),
                name,
                version,
                role,
                links: plan.links.contains(unit),
                deps: plan
                    .deps
                    .get(unit)
                    .map(|ds| ds.iter().filter_map(|d| order.get(d).copied()).collect())
                    .unwrap_or_default(),
            }
        })
        .collect()
}
