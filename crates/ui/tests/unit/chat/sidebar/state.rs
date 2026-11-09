use super::*;

#[test]
fn splice_keeps_rows_shared_at_both_ends() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(splice_range(&s(&["a", "b", "c"]), &s(&["a", "b", "c"])), (3..3, 0));
    assert_eq!(splice_range(&s(&["a", "b"]), &s(&["x", "a", "b"])), (0..0, 1));
    assert_eq!(splice_range(&s(&["h", "a", "b", "c", "d"]), &s(&["h", "d"])), (1..4, 0));
    assert_eq!(splice_range(&s(&["a", "b"]), &s(&["a", "b", "c"])), (2..2, 1));
    assert_eq!(splice_range(&s(&["a", "b", "c"]), &s(&["a", "c", "b"])), (1..3, 2));
    assert_eq!(splice_range(&s(&[]), &s(&["a"])), (0..0, 1));
    assert_eq!(splice_range(&s(&["a", "a"]), &s(&["a"])), (1..2, 0));
}

#[test]
fn arrows_skip_captions_and_stop_at_the_ends() {
    let rows = [false, true, true, false, true];
    assert_eq!(target_row(&rows, &[], 1, Nav::Down), Some(2));
    assert_eq!(target_row(&rows, &[], 2, Nav::Down), Some(4));
    assert_eq!(target_row(&rows, &[], 4, Nav::Down), None);
    assert_eq!(target_row(&rows, &[], 4, Nav::Up), Some(2));
    assert_eq!(target_row(&rows, &[], 1, Nav::Up), None);
    assert_eq!(target_row(&rows, &[], 2, Nav::Home), Some(1));
    assert_eq!(target_row(&rows, &[], 1, Nav::End), Some(4));
}

#[test]
fn right_goes_to_first_child_and_left_to_lead() {
    let rows = [true, true, false, true, true];
    let parents = [None, Some(0), None, None, None];
    assert_eq!(target_row(&rows, &parents, 0, Nav::Right), Some(1));
    assert_eq!(target_row(&rows, &parents, 1, Nav::Right), None);
    assert_eq!(target_row(&rows, &parents, 4, Nav::Right), None);
    assert_eq!(target_row(&rows, &parents, 1, Nav::Left), Some(0));
    assert_eq!(target_row(&rows, &parents, 0, Nav::Left), None);
}
