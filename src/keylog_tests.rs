use super::{record, why_miss};

#[test]
fn a_miss_explains_what_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();

    record(home, "widget", "aaa", "source: one\nfeatures: [a]\n");
    let first = why_miss(home, "widget").expect("one record");
    assert!(first.contains("now: source: one"), "{first}");
    assert!(
        !first.contains("was:"),
        "a first record has nothing to compare: {first}"
    );

    record(home, "widget", "bbb", "source: two\nfeatures: [a]\n");
    let report = why_miss(home, "widget").expect("two records");
    assert!(report.contains("now: source: two"), "{report}");
    assert!(report.contains("was: source: one"), "{report}");
    assert!(report.contains("now: digest=bbb"), "{report}");
    assert!(
        !report.contains("features"),
        "unchanged groups stay out: {report}"
    );
}

#[test]
fn an_unchanged_digest_keeps_the_previous_trace() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    record(home, "widget", "aaa", "source: one\n");
    record(home, "widget", "bbb", "source: two\n");
    record(home, "widget", "bbb", "source: two\n");
    let prev = std::fs::read_to_string(home.join("keys/widget.prev.txt")).unwrap_or_default();
    assert!(prev.contains("digest=aaa"), "{prev}");
}

#[test]
fn an_unknown_crate_has_no_report() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(why_miss(tmp.path(), "nothing").is_none());
}

#[test]
fn a_crate_name_finds_its_hyphenated_package() {
    let tmp = tempfile::tempdir().unwrap();
    record(tmp.path(), "grep-searcher", "aaa", "source: one\n");
    let report = why_miss(tmp.path(), "grep_searcher").expect("found by crate name");
    assert!(report.contains("now: source: one"), "{report}");
}
