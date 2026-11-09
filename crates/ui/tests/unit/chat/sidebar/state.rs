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

fn chat(depth: u8, pin: bool) -> Tree {
    Tree::Chat { depth, pin }
}

#[test]
fn left_on_grandchild_goes_to_sub_lead() {
    let rows = [chat(0, false), chat(1, false), chat(2, false), Tree::Inside, chat(1, false)];
    assert_eq!(parent_rows(rows), [None, Some(0), Some(1), None, Some(0)]);
}

#[test]
fn left_on_pinned_child_without_lead_does_nothing() {
    let rows = [chat(0, false), Tree::Break, chat(1, false), chat(0, false), chat(0, true)];
    assert_eq!(parent_rows(rows), [None; 5]);
    assert_eq!(parent_rows([chat(0, true), chat(1, true)]), [None, Some(0)]);
    assert_eq!(parent_rows([chat(0, false), chat(1, true)]), [None, None]);
}

#[test]
fn right_on_closed_lead_opens() {
    assert_eq!(crew_fold(Some(false), true, false), Some(true));
    assert_eq!(crew_fold(Some(true), true, false), None);
    assert_eq!(crew_fold(None, true, false), None);
    assert_eq!(crew_fold(Some(false), true, true), None);
}

#[test]
fn left_on_open_lead_closes() {
    assert_eq!(crew_fold(Some(true), false, false), Some(false));
    assert_eq!(crew_fold(Some(false), false, false), None);
}
