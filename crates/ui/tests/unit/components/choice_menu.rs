use super::{matching, next_enabled};

#[test]
fn filter_keeps_titles_containing_the_text() {
    let titles = ["Local", "main", "feature/Login", "dev"];
    assert_eq!(matching(&titles, "", 0), [0, 1, 2, 3]);
    assert_eq!(matching(&titles, "in", 0), [1, 2]);
    assert_eq!(matching(&titles, "LOG", 0), [2]);
    assert!(matching(&titles, "zzz", 0).is_empty());
    assert_eq!(matching(&titles, "dev", 1), [0, 3]);
    assert_eq!(matching(&titles, "zzz", 1), [0]);
}

#[test]
fn highlight_wraps_and_skips_disabled() {
    let d = [false, true, false];
    assert_eq!(next_enabled(None, &d, 1), Some(0));
    assert_eq!(next_enabled(None, &d, -1), Some(2));
    assert_eq!(next_enabled(Some(0), &d, 1), Some(2));
    assert_eq!(next_enabled(Some(2), &d, 1), Some(0));
    assert_eq!(next_enabled(Some(0), &d, -1), Some(2));
    assert_eq!(next_enabled(Some(9), &d, 1), Some(0));
    assert_eq!(next_enabled(Some(0), &[true, true], 1), None);
    assert_eq!(next_enabled(None, &[], 1), None);
}
