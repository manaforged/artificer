use super::*;

#[test]
fn threads_clamped() {
    assert_eq!(threads_n(1), 1);
    assert_eq!(threads_n(4), 4);
    assert_eq!(threads_n(10), 8);
    assert_eq!(threads_n(64), 8);
}
