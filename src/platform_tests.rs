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

#[cfg(target_os = "linux")]
struct Stop<'a>(&'a std::sync::atomic::AtomicBool);

#[cfg(target_os = "linux")]
impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_copied_executable_runs_while_other_threads_spawn() {
    let tmp = tempfile::tempdir().unwrap();
    let stop = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    drop(std::process::Command::new("/bin/true").status());
                }
            });
        }
        let _stop = Stop(&stop);
        for i in 0..200 {
            let dst = tmp.path().join(format!("true-{i}"));
            copy_file(Path::new("/bin/true"), &dst).unwrap();
            let status = std::process::Command::new(&dst).status().unwrap();
            assert!(status.success());
        }
    });
}
