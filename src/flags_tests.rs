use super::*;

#[test]
fn threads_clamped() {
    assert_eq!(threads_n(1), 1);
    assert_eq!(threads_n(4), 1);
    assert_eq!(threads_n(8), 2);
    assert_eq!(threads_n(16), 4);
    assert_eq!(threads_n(64), 4);
}
