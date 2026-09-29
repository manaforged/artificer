use super::*;

#[test]
fn capture_includes_attached_workers() {
    let (_, out, err) = capture(|| {
        out("from handler");
        std::thread::scope(|scope| {
            let sink = current();
            scope
                .spawn(move || {
                    attach(sink);
                    out("from worker");
                    err("worker err");
                })
                .join()
                .expect("worker must not panic");
        });
    });
    assert!(out.contains("from handler"));
    assert!(out.contains("from worker"));
    assert!(err.contains("worker err"));
}

#[test]
fn uncaptured_worker_prints() {
    let (_, out, _) = capture(|| {
        std::thread::scope(|scope| {
            scope
                .spawn(|| out("stray"))
                .join()
                .expect("worker must not panic");
        });
    });
    assert!(!out.contains("stray"));
}
