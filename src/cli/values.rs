use super::*;

pub(super) const COMMANDS: [&str; 6] = ["check", "build", "test", "run", "clean", "warm"];
pub(super) const DIRECT_ONLY: [&str; 2] = ["clean", "warm"];
pub(super) const VALUED: [&str; 11] = [
    "--package",
    "--target-dir",
    "--bin",
    "--example",
    "--manifest-path",
    "--profile",
    "--jobs",
    "--features",
    "--test",
    "--message-format",
    "--color",
];

#[derive(Clone, Copy)]
pub(super) enum Toggle {
    NoRun,
    Doc,
    Lib,
    Tests,
    AllTargets,
}

const TEST_ONLY: &[&str] = &["test"];
const CHECK_OR_BUILD: &[&str] = &["check", "build"];
pub(super) const TOGGLES: [(&str, &[&str], Toggle, Option<&str>); 5] = [
    (
        "--no-run",
        TEST_ONLY,
        Toggle::NoRun,
        Some("--no-run is not modeled for this command"),
    ),
    ("--doc", TEST_ONLY, Toggle::Doc, None),
    (
        "--lib",
        TEST_ONLY,
        Toggle::Lib,
        Some("--lib target selection belongs to cargo"),
    ),
    (
        "--tests",
        CHECK_OR_BUILD,
        Toggle::Tests,
        Some("--tests is not modeled for this command"),
    ),
    (
        "--all-targets",
        CHECK_OR_BUILD,
        Toggle::AllTargets,
        Some("--all-targets is not modeled for this command"),
    ),
];

pub(super) fn value<'a>(args: &'a [String], i: &mut usize) -> Option<&'a str> {
    let value = args
        .get(*i + 1)
        .filter(|v| !v.starts_with('-'))
        .filter(|v| !v.is_empty())?;
    *i += 1;
    Some(value)
}

pub(super) fn attached<'a>(flag: &'a str, prefix: &str) -> Option<&'a str> {
    flag.strip_prefix(prefix).filter(|value| !value.is_empty())
}

pub(super) fn manifest_dir(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.file_name() != Some(std::ffi::OsStr::new("Cargo.toml")) || !path.is_file() {
        return None;
    }
    Some(
        path.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf(),
    )
}

pub(super) fn color_choice(value: Option<&str>) -> Result<ColorChoice, Parsed> {
    match value {
        None => Err(fallback("--color needs a value")),
        Some(value) => ColorChoice::parse(value)
            .ok_or_else(|| fallback(format!("color `{value}` belongs to cargo"))),
    }
}
