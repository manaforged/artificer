use super::select::unmodeled;
use super::*;
use std::collections::HashSet;

const SHIPPED_LIB_TYPES: [&str; 3] = ["cdylib", "dylib", "staticlib"];

pub(super) fn ship(plan: &Plan, sel: &TargetSel) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    for id in &plan.roots {
        let pkg = cargo::package(&plan.meta, id)?;
        let node = cargo::node(&plan.meta, id)?;
        let bins: Vec<&str> = pkg
            .targets
            .iter()
            .filter(|t| t.kind.iter().any(|k| k == "bin"))
            .filter(|t| cargo::Package::covered(t, &node.features))
            .map(|t| t.name.as_str())
            .collect();
        let wanted = bins.iter().filter(|name| sel.wants_bin(name)).count();
        let first_kept =
            pkg.lib_target().is_some() || bins.first().is_some_and(|b| sel.wants_bin(b));
        if wanted > 0 && first_kept || wanted == bins.len() && (sel.lib || sel.is_default()) {
            out.insert(id.clone());
        } else {
            unshipped(pkg, sel, wanted, bins.len())?;
        }
        if !sel.tests.is_none() {
            exact_tests(pkg, sel)?;
        }
        for t in pkg.targets.iter().filter(|t| wanted_example(sel, t)) {
            if !t.crate_types.is_empty() && t.crate_types.iter().any(|c| c != "bin") {
                return Err(unmodeled("building a library example"));
            }
        }
    }
    Ok(out)
}

fn unshipped(pkg: &cargo::Package, sel: &TargetSel, wanted: usize, bins: usize) -> Result<()> {
    if wanted > 0 {
        return Err(unmodeled("building a subset of a package's binaries"));
    }
    if pkg.lib_target().is_some_and(|lib| {
        artifact::link_types(lib, pkg.is_proc_macro())
            .iter()
            .any(|t| SHIPPED_LIB_TYPES.contains(&t.as_str()))
    }) {
        return Err(unmodeled(
            "a build that skips a library's cdylib, dylib or staticlib output",
        ));
    }
    if bins > 0 && !sel.tests.is_none() {
        return Err(unmodeled("building tests without their package's binaries"));
    }
    Ok(())
}

fn exact_tests(pkg: &cargo::Package, sel: &TargetSel) -> Result<()> {
    if !matches!(sel.tests, Pick::All) {
        return Ok(());
    }
    let diverges = pkg.targets.iter().any(|t| match Kind::of(pkg, t) {
        Some(Kind::Lib) => pkg.is_proc_macro() == t.test,
        Some(Kind::Test) => !t.test,
        Some(Kind::Example | Kind::Bench) => t.test,
        _ => false,
    });
    if diverges {
        return Err(unmodeled("--tests with a non-default test manifest flag"));
    }
    Ok(())
}

fn wanted_example(sel: &TargetSel, t: &cargo::Target) -> bool {
    t.kind.iter().any(|k| k == "example") && sel.examples.wants(&t.name)
}

pub(super) fn build(
    sess: &Session,
    plan: &Plan,
    order: &[String],
    sel: &TargetSel,
) -> Result<HashMap<String, compile::Compiled>> {
    let (meta, roots) = (&plan.meta, &plan.roots);
    let profile_dir = sess.settings.profile_dir();
    let compiled = if sel.tests.is_none() {
        schedule::compile_ids(sess, meta, order)?
    } else {
        let test_sel = compile::TestSel {
            lib: false,
            only: sel.tests.names().to_vec(),
        };
        let (compiled, mut harnesses) =
            schedule::compile_ids_and_tests(sess, meta, order, roots, &test_sel)?;
        place_tests(harnesses.values_mut().flatten(), &profile_dir)?;
        compiled
    };
    artifact::deliver(roots, &compiled, &profile_dir)?;
    let examples = profile_dir.join("examples");
    for id in roots {
        let pkg = cargo::package(meta, id)?;
        let node = cargo::node(meta, id)?;
        for t in pkg.targets.iter().filter(|t| wanted_example(sel, t)) {
            if !cargo::Package::covered(t, &node.features) {
                continue;
            }
            let exe = compile::compile_example(sess, meta, id, &t.name)?;
            artifact::place_exe(&exe, &examples, &artifact::bin_name(&t.name))?;
        }
    }
    Ok(compiled)
}
