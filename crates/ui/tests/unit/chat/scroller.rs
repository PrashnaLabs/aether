use super::*;

#[test]
fn wheel_flag_needs_a_wheel_and_a_scrolled_up_list() {
    assert!(wheeled_up(true, true));
    assert!(!wheeled_up(true, false), "back at the end clears it");
    assert!(!wheeled_up(false, true), "programmatic scroll never counts");
    assert!(!wheeled_up(false, false));
}

#[test]
fn only_leaving_measure_all_mode_takes_a_fresh_list() {
    assert!(needs_fresh_list(true, MEASURE_ALL_ROWS + 1));
    assert!(!needs_fresh_list(true, MEASURE_ALL_ROWS), "small chats keep measuring every row");
    assert!(!needs_fresh_list(false, MEASURE_ALL_ROWS + 1), "a visible-only list keeps its hints");
    assert!(!needs_fresh_list(false, 0), "the same list can start measuring every row");
}

#[test]
fn bottom_fade_only_paints_over_an_opaque_surface() {
    let surface = gpui_kit::hsla(0.6, 0.15, 0.2, 1.);
    assert_eq!(opaque_fade(surface), Some(surface));
    assert_eq!(opaque_fade(surface.opacity(0.9)), None);
}
