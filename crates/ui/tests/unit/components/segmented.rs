use super::key_target;

#[test]
fn arrows_wrap_and_home_end_jump() {
    assert_eq!(key_target("right", 0, 3), Some(1));
    assert_eq!(key_target("right", 2, 3), Some(0));
    assert_eq!(key_target("left", 0, 3), Some(2));
    assert_eq!(key_target("left", 1, 3), Some(0));
    assert_eq!(key_target("home", 2, 3), Some(0));
    assert_eq!(key_target("end", 0, 3), Some(2));
    assert_eq!(key_target("end", 2, 3), None);
    assert_eq!(key_target("right", 0, 1), None);
    assert_eq!(key_target("up", 0, 3), None);
    assert_eq!(key_target("left", 0, 0), None);
}
