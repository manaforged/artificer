use super::{parse_print_cfg, searches_resolve};

#[test]
fn print_cfg_joins_repeated_keys_like_cargo() {
    let rows = parse_print_cfg(
        "target_has_atomic=\"64\"\ntarget_has_atomic=\"8\"\ntarget_has_atomic=\"ptr\"\nunix\ntarget_os=\"macos\"\n",
    )
    .expect("parse rustc cfg output");
    let atomic = rows
        .iter()
        .find(|(key, _)| key == "TARGET_HAS_ATOMIC")
        .expect("target atomic row");
    assert_eq!(atomic.1, "64,8,ptr");
    assert!(
        rows.iter()
            .any(|(key, value)| key == "UNIX" && value == "true")
    );
    assert!(
        rows.iter()
            .any(|(key, value)| key == "TARGET_OS" && value == "macos")
    );
}

#[test]
fn a_recorded_link_search_from_another_machine_does_not_resolve() {
    let here = std::env::temp_dir();
    let local = format!("cargo:rustc-link-search=native={}\n", here.display());
    let foreign = "cargo:rustc-link-search=native=/opt/another-machine/onnxruntime/lib\n";
    assert!(searches_resolve(&local));
    assert!(searches_resolve("cargo:rustc-link-search=relative/dir\n"));
    assert!(!searches_resolve(foreign));
    assert!(!searches_resolve(&format!("{local}{foreign}")));
}
