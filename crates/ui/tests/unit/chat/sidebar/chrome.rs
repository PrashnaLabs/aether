use super::*;

#[test]
fn tabular_font_features_contains_tnum() {
    let features = tabular();
    assert_eq!(features.0.len(), 1);
    assert_eq!(&features.0[0].0[..], "tnum");
    assert_eq!(features.0[0].1, 1);
}
