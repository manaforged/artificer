use super::*;

const MEMBERS: usize = 70;
const SOFT_LIMIT: usize = 48;

#[test]
fn a_build_with_more_units_than_the_soft_open_file_limit_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let members: Vec<(String, String)> = (0..MEMBERS)
        .map(|n| {
            (
                format!("m{n}"),
                format!("pub fn f{n}() -> usize {{\n    {n}\n}}\n"),
            )
        })
        .collect();
    let named: Vec<(&str, &str)> = members
        .iter()
        .map(|(name, source)| (name.as_str(), source.as_str()))
        .collect();
    write_workspace(&root, &named);
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("ulimit -S -n {SOFT_LIMIT} && exec \"$0\" build"))
        .arg(env!("CARGO_BIN_EXE_artificer"))
        .env("ARTIFICER_HOME", tmp.path().join("home"))
        .env("ARTIFICER_NOSERVE", "1")
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
