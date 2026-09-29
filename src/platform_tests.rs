use super::*;
use std::path::Path;

#[test]
fn env_path_strips_windows_verbatim_prefix() {
    let verbatim = Path::new(r"\\?\C:\Windows\SystemTemp\out");
    let got = env_path(verbatim);
    #[cfg(windows)]
    assert_eq!(got, Path::new(r"C:\Windows\SystemTemp\out"));
    #[cfg(not(windows))]
    assert_eq!(got, verbatim);
}

#[test]
fn current_process_is_alive() {
    assert!(alive(std::process::id()));
}
