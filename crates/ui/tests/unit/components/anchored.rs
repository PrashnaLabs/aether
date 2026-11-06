use super::popup_height;

#[test]
fn height_fits_the_room_minus_gap_and_edge_then_caps_and_floors() {
    assert_eq!(popup_height(1000., 390.), 390.);
    assert_eq!(popup_height(360., 390.), 346.);
    assert_eq!(popup_height(404., 390.), 390.);
    assert_eq!(popup_height(50., 390.), 120.);
    assert_eq!(popup_height(50., 80.), 80.);
    assert_eq!(popup_height(300., f32::INFINITY), 286.);
}
