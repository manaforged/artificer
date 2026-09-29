use super::*;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn pkg(id: &str) -> Unit {
    Unit::Pkg(id.to_string())
}

fn extra(id: &str) -> Unit {
    Unit::Extra(id.to_string())
}

fn state<T>(ids: &[Unit], deps: &HashMap<Unit, Vec<Unit>>) -> State<T> {
    State {
        want: ids.iter().cloned().collect(),
        remaining: ids.iter().cloned().collect(),
        done: HashSet::new(),
        in_flight: 0,
        out: Vec::new(),
        failed: Vec::new(),
        reported: false,
        score: scores(ids, deps),
    }
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
    let mut state: State<()> = state(&[pkg("a"), pkg("b")], &HashMap::new());
    let mut deps: HashMap<Unit, Vec<Unit>> = HashMap::new();
    deps.insert(pkg("a"), vec![pkg("b")]);
    deps.insert(pkg("b"), vec![pkg("outside")]);
    assert_eq!(state.pick(&deps), Some(pkg("b")));
    assert!(state.pick(&deps).is_none());
    state.done.insert(pkg("b"));
    state.in_flight = 0;
    assert_eq!(state.pick(&deps), Some(pkg("a")));
}

#[test]
fn spine_beats_leaves() {
    let ids: Vec<Unit> = ["a", "b", "c", "l1", "l2"].map(pkg).to_vec();
    let mut deps: HashMap<Unit, Vec<Unit>> = HashMap::new();
    deps.insert(pkg("a"), vec![pkg("b")]);
    deps.insert(pkg("b"), vec![pkg("c")]);
    let score = scores(&ids, &deps);
    assert_eq!(score[&pkg("c")], 2);
    assert_eq!(score[&pkg("b")], 1);
    assert_eq!(score[&pkg("l1")], 0);
    let mut state: State<()> = state(&ids, &deps);
    assert_eq!(state.pick(&deps), Some(pkg("c")), "spine first");
}

#[test]
fn extras_wait_for_their_package_and_deps() {
    let ids = vec![pkg("root"), pkg("dep"), extra("root")];
    let mut deps: HashMap<Unit, Vec<Unit>> = HashMap::new();
    deps.insert(pkg("root"), vec![pkg("dep")]);
    deps.insert(extra("root"), vec![pkg("dep"), pkg("root")]);
    let mut state: State<()> = state(&ids, &deps);
    assert_eq!(state.pick(&deps), Some(pkg("dep")), "dep first");
    state.done.insert(pkg("dep"));
    state.in_flight = 0;
    assert_eq!(state.pick(&deps), Some(pkg("root")), "then the lib");
    state.done.insert(pkg("root"));
    state.in_flight = 0;
    assert_eq!(state.pick(&deps), Some(extra("root")), "harness last");
}
