use super::*;

#[test]
fn windows_and_month_enumeration_cover_calendars() {
    let day = NaiveDate::from_ymd_opt(2026, 8, 9).unwrap();
    let ymd = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
    assert_eq!(UsageWindow::TrailingDays(7).bounds(day), (ymd(2026, 8, 3), day));
    assert_eq!(UsageWindow::Months(12).bounds(day), (ymd(2025, 9, 1), day));
    assert_eq!(UsageWindow::ThisMonth.bounds(day), (ymd(2026, 8, 1), day));
    assert_eq!(UsageWindow::LastMonth.bounds(day), (ymd(2026, 7, 1), ymd(2026, 7, 31)));
    assert_eq!(
        UsageWindow::LastMonth.bounds(ymd(2026, 1, 15)),
        (ymd(2025, 12, 1), ymd(2025, 12, 31))
    );
    let months = enumerate_months(ymd(2025, 11, 15), ymd(2026, 2, 3));
    assert_eq!(months, vec![ymd(2025, 11, 1), ymd(2025, 12, 1), ymd(2026, 1, 1), ymd(2026, 2, 1)]);
    assert_eq!(days_in_month(ymd(2024, 2, 1)), 29);
    assert_eq!(days_in_month(ymd(2026, 2, 1)), 28);
    assert_eq!(enumerate_days(ymd(2026, 8, 1), ymd(2026, 8, 3)).len(), 3);
}
