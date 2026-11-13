use super::*;

#[test]
fn seats_start_at_twelve_and_go_clockwise_at_equal_angles() {
    let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
    let (a, x, y) = seat_pos(0, 4, 660.);
    assert!(close(a, -std::f32::consts::FRAC_PI_2));
    assert!(close(x, 330.) && close(y, 60.));
    let (_, x, y) = seat_pos(1, 4, 660.);
    assert!(close(x, 600.) && close(y, 330.));
    let (_, x, y) = seat_pos(2, 4, 660.);
    assert!(close(x, 330.) && close(y, 600.));
    let (_, x, y) = seat_pos(1, 4, 330.);
    assert!(close(x, 300.) && close(y, 165.));
    let (a1, ..) = seat_pos(1, 3, 660.);
    let (a2, ..) = seat_pos(2, 3, 660.);
    assert!(close(a2 - a1, std::f32::consts::TAU / 3.));
    let (_, x, y) = seat_pos(0, 0, 660.);
    assert!(close(x, 330.) && close(y, 60.));
}

#[test]
fn short_takes_the_first_line_and_clips() {
    assert_eq!(short("\n  hello world\nmore", 5), "hello…");
    assert_eq!(short("hi", 5), "hi");
    assert_eq!(short("", 5), "");
}
