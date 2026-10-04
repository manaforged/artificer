use super::*;
use serde_json::Value;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn write_chain(root: &Path) {
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"a\", \"b\", \"c\"]\n",
    );
    let crates = [
        ("a", "", "pub fn a() -> u8 { 1 }\n"),
        (
            "b",
            "a = { path = \"../a\" }\n",
            "pub fn b() -> u8 { a::a() + 1 }\n",
        ),
        (
            "c",
            "b = { path = \"../b\" }\n",
            "pub fn c() -> u8 { b::b() + 1 }\n",
        ),
    ];
    for (name, deps, source) in crates {
        write(
            &root.join(name).join("Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}"
            ),
        );
        write(&root.join(name).join("src/lib.rs"), source);
    }
}

fn ok(out: &std::process::Output) -> String {
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn json(home: &Path, root: &Path, args: &[&str]) -> Value {
    let out = artificer(home, root)
        .arg("profile")
        .args(args)
        .output()
        .unwrap();
    ok(&out);
    serde_json::from_slice(&out.stdout).unwrap()
}

fn names(steps: &Value) -> Vec<String> {
    steps
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_build_profile_follows_the_dependency_chain_and_cache_outcomes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let home = tmp.path().join("home");
    write_chain(&root);

    ok(&artificer(&home, &root).arg("build").output().unwrap());
    let cold = json(&home, &root, &["--json"]);
    assert_eq!(cold["result"], "built");
    assert_eq!(cold["units"]["total"], 3);
    assert_eq!(cold["units"]["miss"], 3);
    assert_eq!(names(&cold["critical_path"]), ["a", "b", "c"]);
    let steps = cold["critical_path"].as_array().unwrap();
    for pair in steps.windows(2) {
        assert!(
            pair[1]["start_ms"].as_u64() >= pair[0]["end_ms"].as_u64(),
            "a dependent started before its dependency ended: {pair:?}"
        );
    }
    assert!(cold["cpu_ms"].as_u64().unwrap() > 0, "{cold}");
    assert!(
        cold["top_units"][0]["peak_rss_bytes"].as_u64().unwrap() > 0,
        "{cold}"
    );

    ok(&artificer(&home, &root).arg("build").output().unwrap());
    let warm = json(&home, &root, &["--json"]);
    assert_eq!(warm["units"]["hit"], 3);
    assert_eq!(warm["units"]["miss"], 0);

    let list = json(&home, &root, &["list", "--json"]);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [warm["id"].as_str().unwrap(), cold["id"].as_str().unwrap()]
    );

    let diff = json(&home, &root, &["diff", ids[1], ids[0], "--json"]);
    let changed = diff["units"].as_array().unwrap();
    assert!(
        changed
            .iter()
            .any(|unit| unit["base_outcome"] == "miss" && unit["head_outcome"] == "hit"),
        "{diff}"
    );
}

#[test]
fn timings_write_the_report_without_falling_back() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let home = tmp.path().join("home");
    write_chain(&root);

    let stderr = ok(&shim(&home, &root)
        .args(["build", "--timings"])
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .unwrap());
    assert!(stderr.contains("Timing report saved to"), "{stderr}");
    let report = fs::read_to_string(root.join("target/cargo-timings/cargo-timing.html")).unwrap();
    assert!(report.contains("Artificer build"), "Cargo wrote the report");
    let stat: Value = serde_json::from_slice(
        &artificer(&home.join("store"), &root)
            .args(["stat", "--json"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(stat["fallbacks"], 0, "{stat}");
}

#[test]
fn pass_timings_land_in_the_profile_without_changing_unit_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let home = tmp.path().join("home");
    write_chain(&root);

    let stderr = ok(&artificer(&home, &root)
        .arg("build")
        .env("ARTIFICER_PASSES", "1")
        .output()
        .unwrap());
    assert!(!stderr.contains("time:"), "{stderr}");
    let timed = json(&home, &root, &["--json"]);
    assert!(!timed["compiler"].as_array().unwrap().is_empty(), "{timed}");

    let stderr = ok(&artificer(&home, &root).arg("build").output().unwrap());
    assert!(!stderr.contains("time:"), "{stderr}");
    let plain = json(&home, &root, &["--json"]);
    assert_eq!(plain["units"]["hit"], 3, "{plain}");
}

#[test]
fn a_profile_exports_a_trace_and_an_html_report() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let home = tmp.path().join("home");
    write_chain(&root);
    ok(&artificer(&home, &root).arg("build").output().unwrap());

    let trace = tmp.path().join("out/trace.json");
    let html = tmp.path().join("out/report.html");
    let out = artificer(&home, &root)
        .arg("profile")
        .arg("--trace")
        .arg(&trace)
        .arg("--html")
        .arg(&html)
        .output()
        .unwrap();
    ok(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("critical path"));

    let trace: Value = serde_json::from_str(&fs::read_to_string(&trace).unwrap()).unwrap();
    let events = trace["traceEvents"].as_array().unwrap();
    for name in ["a 0.1.0", "b 0.1.0", "c 0.1.0"] {
        assert!(
            events
                .iter()
                .any(|event| event["cat"] == "unit" && event["name"] == name && event["ph"] == "X"),
            "no unit event for {name}"
        );
    }
    assert!(events.iter().any(|event| event["ph"] == "C"));

    let html = fs::read_to_string(&html).unwrap();
    assert!(html.contains("<svg") && html.contains("c 0.1.0"), "{html}");
}

#[test]
fn a_fallback_build_records_its_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let home = tmp.path().join("home");
    write_chain(&root);

    ok(&shim(&home, &root)
        .args(["build", "--keep-going"])
        .output()
        .unwrap());
    let profile = json(&home.join("store"), &root, &["--json"]);
    assert_eq!(profile["result"], "fallback", "{profile}");
    assert!(
        profile["fallback"]
            .as_str()
            .unwrap()
            .contains("--keep-going"),
        "{profile}"
    );
    assert!(
        profile["phases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["phase"] == "fallback"),
        "{profile}"
    );
}
