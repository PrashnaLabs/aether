use super::*;

#[test]
fn activity_line_is_the_first_non_empty_line() {
    assert_eq!(activity_line("\n  \n  Reviewing #5  \nmore"), "Reviewing #5");
    assert_eq!(activity_line(""), "");
    assert_eq!(money(4.1), "$4.10");
}
