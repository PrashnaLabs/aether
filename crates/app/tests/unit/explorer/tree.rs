use super::*;

#[test]
fn keys_walk_open_and_climb_the_tree() {
    // src/ (open), src/a.rs, docs/ (closed), b.rs
    let rows = [(0, Some(true)), (1, None), (0, Some(false)), (0, None)];
    assert_eq!(tree_step(&rows, 0, "down"), Some(TreeStep::Focus(1)));
    assert_eq!(tree_step(&rows, 3, "down"), None);
    assert_eq!(tree_step(&rows, 2, "home"), Some(TreeStep::Focus(0)));
    assert_eq!(tree_step(&rows, 0, "end"), Some(TreeStep::Focus(3)));
    assert_eq!(tree_step(&rows, 0, "right"), Some(TreeStep::Focus(1)));
    assert_eq!(tree_step(&rows, 0, "left"), Some(TreeStep::Toggle));
    assert_eq!(tree_step(&rows, 2, "right"), Some(TreeStep::Toggle));
    assert_eq!(tree_step(&rows, 1, "left"), Some(TreeStep::Focus(0)));
    assert_eq!(tree_step(&rows, 3, "left"), None);
    assert_eq!(tree_step(&rows, 1, "enter"), Some(TreeStep::Open));
    assert_eq!(tree_step(&rows, 2, "space"), Some(TreeStep::Toggle));
    assert_eq!(tree_step(&[(0, Some(true)), (0, None)], 0, "right"), None);
}
