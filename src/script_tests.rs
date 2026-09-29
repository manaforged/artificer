use super::parse_print_cfg;

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
