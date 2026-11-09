use super::*;

fn main_with(chats: &[i64]) -> (WorkspaceModel, Vec<PaneId>) {
    let mut m = WorkspaceModel::default();
    let panes = chats.iter().map(|c| m.open(*c, 0)).collect();
    (m, panes)
}

#[test]
fn detach_refused_for_last_docked_main_pane() {
    let (m, p) = main_with(&[1]);
    assert!(pane_refusal(&m, p[0], false).is_some());
}

#[test]
fn detach_refused_when_only_floats_would_remain() {
    let (mut m, p) = main_with(&[1, 2]);
    m.float(p[1], Rect::new(0., 0., 400., 300.)).unwrap();
    assert!(pane_refusal(&m, p[0], false).is_some());
    assert!(pane_refusal(&m, p[1], false).is_none());
}

#[test]
fn detach_allowed_with_other_docked_pane_or_from_detached() {
    let (mut m, p) = main_with(&[1, 2]);
    assert!(pane_refusal(&m, p[0], false).is_none());
    m.transfer(p[1], 5).unwrap();
    assert!(pane_refusal(&m, p[1], false).is_none());
}

#[test]
fn close_refused_only_for_last_main_pane() {
    let (mut m, p) = main_with(&[1]);
    assert!(pane_refusal(&m, p[0], true).is_some());
    let q = m.open(2, 0);
    assert!(pane_refusal(&m, q, true).is_none());
    m.transfer(q, 5).unwrap();
    assert!(pane_refusal(&m, q, true).is_none());
}
