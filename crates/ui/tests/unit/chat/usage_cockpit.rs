use super::*;

#[test]
fn usage_limit_row_clamps_percent() {
    let r1 = UsageLimitRow::new("Session", 120.0, "resets in 2h");
    assert_eq!(r1.percent_left, 100.0);

    let r2 = UsageLimitRow::new("Session", -10.0, "resets in 2h");
    assert_eq!(r2.percent_left, 0.0);

    let r3 = UsageLimitRow::new("Weekly", 50.0, "resets in 5d");
    assert_eq!(r3.percent_left, 50.0);
    assert_eq!(r3.threshold, Some(75.0));
}
