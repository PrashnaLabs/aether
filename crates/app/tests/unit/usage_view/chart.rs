use super::*;

#[test]
fn nice_scales_round_up_to_readable_steps() {
    let (max, ticks) = nice_scale(97.0, 4);
    assert_eq!(max, 100.0);
    assert_eq!(ticks, vec![0.0, 50.0, 100.0]);
    let (max, _) = nice_scale(0.37, 4);
    assert!(max >= 0.37);
    let (max, ticks) = nice_scale(0.0, 4);
    assert_eq!(max, 0.0);
    assert_eq!(ticks, vec![0.0]);
}

#[test]
fn monotone_smoothing_never_overshoots_flat_runs() {
    let points = [(0.0, 100.0), (10.0, 100.0), (20.0, 0.0), (30.0, 100.0), (40.0, 100.0)];
    let tangents = monotone_tangents(&points);
    assert_eq!(tangents[0], 0.0);
    assert_eq!(tangents[1], 0.0);
    assert_eq!(tangents[3], 0.0);
    assert_eq!(tangents[4], 0.0);
    let segments = smooth_curve(&points);
    assert_eq!(segments.len(), 4);
    for segment in &segments {
        for y in [segment.c1.1, segment.c2.1] {
            assert!((-0.001..=100.001).contains(&y), "control point left range: {y}");
        }
    }
}
