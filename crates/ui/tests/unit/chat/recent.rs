use super::*;

#[test]
fn row_label_says_what_opens() {
    assert_eq!(recent_row_label("Fix bug", "main", "5m"), "Open Fix bug, main, 5m");
}
