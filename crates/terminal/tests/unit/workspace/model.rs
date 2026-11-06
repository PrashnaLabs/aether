use super::*;
use gpui_kit::Axis;

#[test]
fn split_tree_inserts_horizontal_and_vertical_splits_and_computes_connectors() {
    let mut model = WorkspaceModel::new();
    let g1 = model.active_group_id();
    let p1 = model.active_pane_id();
    assert_eq!(model.active_group().root.tree_rows(), vec![(p1, TreeConnector::Single)]);

    let p2 = model.split_pane(p1, Axis::Horizontal).unwrap();
    let p3 = model.split_pane(p2, Axis::Vertical).unwrap();
    assert_eq!(model.active_pane_id(), p3);
    assert_eq!(
        model.group(g1).unwrap().root.tree_rows(),
        vec![(p1, TreeConnector::First), (p2, TreeConnector::Middle), (p3, TreeConnector::Last),]
    );
}

#[test]
fn closing_middle_pane_in_nested_split_promotes_sibling_and_updates_active_pane() {
    let mut model = WorkspaceModel::new();
    let p1 = model.active_pane_id();
    let p2 = model.split_pane(p1, Axis::Horizontal).unwrap();
    let p3 = model.split_pane(p2, Axis::Vertical).unwrap();

    // Focus middle pane p2 and close it
    model.focus_pane(p2);
    let outcome = model.close_pane(p2);
    assert_eq!(outcome, CloseOutcome::Focused(p3));
    assert_eq!(model.active_pane_id(), p3);
    assert_eq!(model.active_group().root.leaves(), vec![p1, p3]);
}

#[test]
fn closing_last_pane_in_last_group_resets_default_group_and_requests_hide() {
    let mut model = WorkspaceModel::new();
    let p1 = model.active_pane_id();
    let outcome = model.close_pane(p1);
    assert!(matches!(outcome, CloseOutcome::HideWorkspace { replacement_pane: _ }));
    assert_eq!(model.groups.len(), 1);
    assert_eq!(model.active_group().root.leaves().len(), 1);
}

#[test]
fn split_ratio_clamps_out_of_range_and_non_finite_values() {
    let mut model = WorkspaceModel::new();
    let p1 = model.active_pane_id();
    let _p2 = model.split_pane(p1, Axis::Horizontal).unwrap();
    let split_id = match &model.active_group().root {
        SplitNode::Split { id, .. } => *id,
        _ => panic!("expected split"),
    };

    model.set_split_ratio(split_id, 0.02);
    assert_eq!(model.active_group().root.ratio_of(split_id), Some(0.15));

    model.set_split_ratio(split_id, 0.99);
    assert_eq!(model.active_group().root.ratio_of(split_id), Some(0.85));

    model.set_split_ratio(split_id, f32::NAN);
    assert_eq!(model.active_group().root.ratio_of(split_id), Some(0.5));
}

#[test]
fn pane_and_group_cycling_wraps_around_deterministically() {
    let mut model = WorkspaceModel::new();
    let g1 = model.active_group_id();
    let p1 = model.active_pane_id();
    let p2 = model.split_active(Axis::Horizontal);
    let p3 = model.split_active(Axis::Vertical);

    assert_eq!(model.active_pane_id(), p3);
    assert_eq!(model.focus_next_pane(), p1);
    assert_eq!(model.focus_next_pane(), p2);
    assert_eq!(model.focus_prev_pane(), p1);
    assert_eq!(model.focus_prev_pane(), p3);

    let (g2, g2_p1) = model.new_group();
    assert_eq!(model.active_group_id(), g2);
    assert_eq!(model.active_pane_id(), g2_p1);

    assert_eq!(model.focus_next_group(), p3);
    assert_eq!(model.active_group_id(), g1);
    assert_eq!(model.focus_prev_group(), g2_p1);
    assert_eq!(model.active_group_id(), g2);

    // Closing g2 falls back to g1
    let outcome = model.close_group(g2);
    assert_eq!(outcome, CloseOutcome::Focused(p3));
    assert_eq!(model.active_group_id(), g1);
}
