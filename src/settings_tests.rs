use super::{profile_for, rustdocflags, rustflags, wrapper_var};

#[test]
fn reads_rustflags_the_way_cargo_does() {
    unsafe {
        std::env::remove_var("CARGO_ENCODED_RUSTFLAGS");
        std::env::remove_var("RUSTFLAGS");
    }
    assert!(rustflags().is_none(), "unset means the config tables apply");
    unsafe { std::env::set_var("RUSTFLAGS", "-D warnings --cfg foo") }
    assert_eq!(
        rustflags(),
        Some(vec![
            "-D".to_string(),
            "warnings".to_string(),
            "--cfg".to_string(),
            "foo".to_string()
        ])
    );
    unsafe { std::env::set_var("CARGO_ENCODED_RUSTFLAGS", "-D\x1fwarnings\x1f--cfg\x1fbar") }
    assert_eq!(
        rustflags(),
        Some(vec![
            "-D".to_string(),
            "warnings".to_string(),
            "--cfg".to_string(),
            "bar".to_string()
        ]),
        "the encoded form wins over the plain one"
    );
    unsafe {
        std::env::remove_var("CARGO_ENCODED_RUSTFLAGS");
        std::env::set_var("RUSTFLAGS", "")
    }
    assert_eq!(
        rustflags(),
        Some(Vec::new()),
        "an empty value clears config flags instead of deferring to them"
    );
    unsafe {
        std::env::remove_var("CARGO_ENCODED_RUSTFLAGS");
        std::env::remove_var("RUSTFLAGS");
    }
}

#[test]
fn reads_rustdocflags_the_way_cargo_does() {
    unsafe {
        std::env::remove_var("CARGO_ENCODED_RUSTDOCFLAGS");
        std::env::remove_var("RUSTDOCFLAGS");
    }
    assert!(rustdocflags().is_empty());
    unsafe { std::env::set_var("RUSTDOCFLAGS", "--cfg docflag") }
    assert_eq!(
        rustdocflags(),
        vec!["--cfg".to_string(), "docflag".to_string()]
    );
    unsafe { std::env::set_var("CARGO_ENCODED_RUSTDOCFLAGS", "--cfg\x1fencoded") }
    assert_eq!(
        rustdocflags(),
        vec!["--cfg".to_string(), "encoded".to_string()],
        "the encoded form wins over the plain one"
    );
    unsafe {
        std::env::remove_var("CARGO_ENCODED_RUSTDOCFLAGS");
        std::env::remove_var("RUSTDOCFLAGS");
    }
}

#[test]
fn wrapper_resolution_matches_cargo_precedence() {
    unsafe {
        std::env::remove_var("ART_TEST_WRAP");
        std::env::remove_var("CARGO_BUILD_ART_TEST_WRAP");
    }
    assert_eq!(
        wrapper_var(
            "ART_TEST_WRAP",
            "CARGO_BUILD_ART_TEST_WRAP",
            Some("file-wrap")
        ),
        Some("file-wrap".to_string())
    );
    assert_eq!(
        wrapper_var("ART_TEST_WRAP", "CARGO_BUILD_ART_TEST_WRAP", Some("rustc")),
        None,
        "rustc is not a wrapper"
    );
    unsafe { std::env::set_var("CARGO_BUILD_ART_TEST_WRAP", "env-wrap") }
    assert_eq!(
        wrapper_var(
            "ART_TEST_WRAP",
            "CARGO_BUILD_ART_TEST_WRAP",
            Some("file-wrap")
        ),
        Some("env-wrap".to_string())
    );
    unsafe { std::env::set_var("ART_TEST_WRAP", "dedicated-wrap") }
    assert_eq!(
        wrapper_var(
            "ART_TEST_WRAP",
            "CARGO_BUILD_ART_TEST_WRAP",
            Some("file-wrap")
        ),
        Some("dedicated-wrap".to_string())
    );
    unsafe { std::env::set_var("ART_TEST_WRAP", "") }
    assert_eq!(
        wrapper_var(
            "ART_TEST_WRAP",
            "CARGO_BUILD_ART_TEST_WRAP",
            Some("file-wrap")
        ),
        None,
        "an empty value resets the wrapper"
    );
    unsafe {
        std::env::remove_var("ART_TEST_WRAP");
        std::env::remove_var("CARGO_BUILD_ART_TEST_WRAP");
    }
}

fn args(pairs: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    for pair in pairs {
        values.push("-C".to_string());
        values.push((*pair).to_string());
    }
    values
}

#[test]
fn lto_is_dropped_only_where_rustc_rejects_it() {
    let flags = args(&["opt-level=3", "lto=thin", "codegen-units=1"]);
    assert_eq!(profile_for(&flags, true), flags);
    assert_eq!(
        profile_for(&flags, false),
        args(&["opt-level=3", "codegen-units=1"])
    );
    let tail = args(&["opt-level=3", "lto=true"]);
    assert_eq!(profile_for(&tail, false), args(&["opt-level=3"]));
}
