use super::*;

#[test]
fn close_refused_only_for_last_main_pane() {
    let mut m = WorkspaceModel::default();
    let p = m.open(1, 0);
    assert!(close_refusal(&m, p).is_some());
    let q = m.open(2, 0);
    assert!(close_refusal(&m, q).is_none());
    m.transfer(q, 5).unwrap();
    assert!(close_refusal(&m, q).is_none(), "a detached window's last pane may close");
}
