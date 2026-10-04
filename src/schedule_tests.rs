use super::*;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn pkg(id: &str) -> Unit {
    Unit::Pkg(id.to_string())
}

fn extra(id: &str) -> Unit {
    Unit::Extra(id.to_string())
}

fn ready(units: &[Unit], deps: &HashMap<Unit, Vec<Unit>>, links: &[Unit]) -> Ready {
    Ready::new(&Plan {
        units: units.to_vec(),
        deps: deps.clone(),
        links: links.iter().cloned().collect(),
    })
}

#[test]
fn configured_job_cap_has_a_floor() {
    let _guard = ENV.lock().expect("environment test lock");
    unsafe {
        std::env::set_var("ARTIFICER_JOBS", "0");
    }
    assert_eq!(job_cap(), 1);
    unsafe {
        std::env::remove_var("ARTIFICER_JOBS");
    }
}

#[test]
fn pick_waits_for_in_scope_deps_only() {
    let mut deps: HashMap<Unit, Vec<Unit>> = HashMap::new();
    deps.insert(pkg("a"), vec![pkg("b")]);
    deps.insert(pkg("b"), vec![pkg("outside")]);
    let mut state = ready(&[pkg("a"), pkg("b")], &deps, &[]);
    assert_eq!(state.pick(), Some(pkg("b")));
    assert!(state.pick().is_none());
    state.done.insert(pkg("b"));
    state.in_flight = 0;
    assert_eq!(state.pick(), Some(pkg("a")));
}

#[test]
fn spine_beats_leaves() {
    let ids: Vec<Unit> = ["a", "b", "c", "l1", "l2"].map(pkg).to_vec();
    let mut deps: HashMap<Unit, Vec<Unit>> = HashMap::new();
    deps.insert(pkg("a"), vec![pkg("b")]);
    deps.insert(pkg("b"), vec![pkg("c")]);
    let score = plan::scores(&ids, &deps);
    assert_eq!(score[&pkg("c")], 2);
    assert_eq!(score[&pkg("b")], 1);
    assert_eq!(score[&pkg("l1")], 0);
    let mut state = ready(&ids, &deps, &[]);
    assert_eq!(state.pick(), Some(pkg("c")), "spine first");
}

#[test]
fn extras_wait_for_their_package_and_deps() {
    let ids = vec![pkg("root"), pkg("dep"), extra("root")];
    let mut deps: HashMap<Unit, Vec<Unit>> = HashMap::new();
    deps.insert(pkg("root"), vec![pkg("dep")]);
    deps.insert(extra("root"), vec![pkg("dep"), pkg("root")]);
    let mut state = ready(&ids, &deps, &[extra("root")]);
    assert_eq!(state.pick(), Some(pkg("dep")), "dep first");
    state.done.insert(pkg("dep"));
    state.in_flight = 0;
    assert_eq!(state.pick(), Some(pkg("root")), "then the lib");
    state.done.insert(pkg("root"));
    state.in_flight = 0;
    assert_eq!(state.pick(), Some(extra("root")), "harness last");
}

#[test]
fn a_library_starts_once_its_dependency_has_metadata() {
    let mut deps: HashMap<Unit, Vec<Unit>> = HashMap::new();
    deps.insert(pkg("user"), vec![pkg("base")]);
    let mut state = ready(&[pkg("base"), pkg("user")], &deps, &[]);
    assert_eq!(state.pick(), Some(pkg("base")));
    assert!(state.pick().is_none(), "no metadata yet");
    state.meta.insert(pkg("base"));
    assert_eq!(
        state.pick(),
        Some(pkg("user")),
        "started before base finished"
    );
}

#[test]
fn a_linking_unit_waits_for_every_transitive_dependency() {
    let ids = vec![pkg("a"), pkg("b"), pkg("bin")];
    let mut deps: HashMap<Unit, Vec<Unit>> = HashMap::new();
    deps.insert(pkg("b"), vec![pkg("a")]);
    deps.insert(pkg("bin"), vec![pkg("b")]);
    let mut state = ready(&ids, &deps, &[pkg("bin")]);
    state.remaining.remove(&pkg("a"));
    state.remaining.remove(&pkg("b"));
    state.meta.insert(pkg("a"));
    state.done.insert(pkg("b"));
    assert!(state.pick().is_none(), "a has only metadata");
    state.done.insert(pkg("a"));
    assert_eq!(state.pick(), Some(pkg("bin")));
}
