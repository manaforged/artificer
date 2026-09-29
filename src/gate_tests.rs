use super::{gated_env, gated_name, model_fallback, older_than};

fn names<'a>(list: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
    list.iter().map(|n| (*n).to_string())
}

#[test]
fn cargo_owned_names_are_gated() {
    assert_eq!(
        gated_name(names(&["CARGO_PROFILE_DEV_OPT_LEVEL"]), false).as_deref(),
        Some("CARGO_PROFILE_DEV_OPT_LEVEL")
    );
    assert_eq!(
        gated_name(names(&["CARGO_BUILD_RUSTFLAGS"]), false).as_deref(),
        Some("CARGO_BUILD_RUSTFLAGS")
    );
    assert_eq!(
        gated_name(names(&["CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER"]), false).as_deref(),
        Some("CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER")
    );
    assert_eq!(
        gated_name(names(&["CARGO_UNSTABLE_CHECK_CFG"]), false).as_deref(),
        Some("CARGO_UNSTABLE_CHECK_CFG")
    );
}

#[test]
fn modeled_and_doctest_names_pass_the_always_check() {
    assert_eq!(gated_name(names(&["CARGO_TARGET_DIR"]), false), None);
    assert_eq!(gated_name(names(&["CARGO_TARGET_DIR"]), true), None);
    assert_eq!(
        gated_name(names(&["CARGO_BUILD_RUSTDOCFLAGS"]), false),
        None
    );
    assert_eq!(
        gated_name(names(&["CARGO_BUILD_RUSTDOCFLAGS"]), true).as_deref(),
        Some("CARGO_BUILD_RUSTDOCFLAGS")
    );
    assert_eq!(
        gated_name(names(&["CARGO_ENCODED_RUSTDOCFLAGS"]), false),
        None
    );
    assert_eq!(
        gated_name(names(&["CARGO_ENCODED_RUSTDOCFLAGS"]), true).as_deref(),
        Some("CARGO_ENCODED_RUSTDOCFLAGS")
    );
    assert_eq!(gated_name(names(&["CARGO_INCREMENTAL"]), false), None);
}

#[test]
fn the_process_environment_is_scanned() {
    let name = "CARGO_FUTURE_TEST_KNOB";
    unsafe { std::env::set_var(name, "1") };
    let hit = gated_env(false);
    unsafe { std::env::remove_var(name) };
    let hit = hit.expect("the scan must find an unmodeled CARGO_* name");
    assert!(hit.starts_with("CARGO_"));
}

#[test]
fn unknown_cargo_names_are_gated() {
    assert_eq!(
        gated_name(names(&["CARGO_FUTURE_KNOB"]), false).as_deref(),
        Some("CARGO_FUTURE_KNOB")
    );
    assert_eq!(gated_name(names(&["CARGO_PKG_NAME"]), false), None);
    assert_eq!(gated_name(names(&["CARGO_NET_OFFLINE"]), false), None);
    assert_eq!(
        gated_name(names(&["CARGO_BUILD_RUSTC_WRAPPER"]), false),
        None
    );
    assert_eq!(
        gated_name(names(&["CARGO_BUILD_RUSTC"]), false).as_deref(),
        Some("CARGO_BUILD_RUSTC")
    );
}

#[test]
fn only_unmodeled_cargo_errors_fall_back() {
    let unmodeled = anyhow::Error::new(crate::cargo::Unmodeled("format changed".into()));
    assert_eq!(
        model_fallback(&unmodeled).as_deref(),
        Some("format changed")
    );
    let real = anyhow::anyhow!("network down");
    assert_eq!(model_fallback(&real), None);
}

#[test]
fn older_cargo_falls_back() {
    assert!(older_than("cargo 1.97.0 (abc 2026-01-01)").is_some());
    assert!(older_than("cargo 1.98.0 (abc 2026-08-05)").is_none());
    assert!(older_than("cargo 1.99.0-nightly (abc 2026-09-01)").is_none());
    assert!(
        older_than("nonsense").is_some(),
        "a version format Artificer cannot read must fall back"
    );
}
