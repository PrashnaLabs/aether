use super::*;

#[test]
fn stale_pane_ops_return_no_longer_exists() {
    let mut m = WorkspaceModel::default();
    let stale: PaneId = 999_999;
    assert_eq!(m.close(stale), Err("Pane no longer exists"));
    assert_eq!(m.float(stale, Rect::new(0., 0., 400., 300.)), Err("Pane no longer exists"));
    assert_eq!(m.dock(stale, None, DockSide::Left), Err("Pane no longer exists"));
    assert_eq!(m.transfer(stale, 7), Err("Pane no longer exists"));
    assert_eq!(
        m.split(stale, 22, SplitAxis::Horizontal, [1000., 600.]),
        Err("Pane no longer exists")
    );
}

#[test]
fn split_collapse_float_dock_preserve_sessions() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.split(a, 22, SplitAxis::Horizontal, [1000., 600.]).unwrap();
    let c = m.split(b, 33, SplitAxis::Vertical, [498., 600.]).unwrap();
    assert_eq!(m.windows[&0].root.as_ref().unwrap().leaves(), vec![a, b, c]);
    m.float(b, Rect::new(20., 30., 400., 300.)).unwrap();
    assert_eq!(m.windows[&0].root.as_ref().unwrap().leaves(), vec![a, c]);
    assert_eq!(m.windows[&0].floats[0].pane, b);
    m.dock(b, Some(a), DockSide::Left).unwrap();
    assert_eq!(m.windows[&0].root.as_ref().unwrap().leaves(), vec![b, a, c]);
    m.close(a).unwrap();
    assert_eq!(m.panes.get(&b), Some(&22));
    assert!(!m.panes.contains_key(&a));
}

#[test]
fn duplicate_open_focuses_original_and_small_split_is_atomic() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    assert_eq!(m.open(11, 99), a);
    assert_eq!(m.panes.len(), 1);
    assert!(m.split(a, 22, SplitAxis::Horizontal, [643., 600.]).is_err());
    assert_eq!(m.panes.len(), 1);
    assert_eq!(m.windows[&0].focused, Some(a));
}

#[test]
fn closing_last_docked_pane_keeps_floats_and_clears_maximize() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.split(a, 22, SplitAxis::Horizontal, [1000., 600.]).unwrap();
    m.float(b, Rect::new(0., 0., 400., 300.)).unwrap();
    m.windows.get_mut(&0).unwrap().maximized = Some(a);
    m.close(a).unwrap();
    assert!(m.windows[&0].root.is_none());
    assert_eq!(m.windows[&0].maximized, None);
    assert_eq!(m.windows[&0].focused, Some(b));
}

#[test]
fn geometry_honors_minimum_and_rejects_non_finite_bounds() {
    assert_eq!(split_sizes(644., 320., 320., 0.5), Some((320., 320.)));
    assert_eq!(split_sizes(643., 320., 320., 0.5), None);
    assert_eq!(split_sizes(484., 240., 240., 0.5), Some((240., 240.)));
    let r = Rect::new(f32::NAN, f32::INFINITY, 1e30, -4.).sanitize([800., 600.]);
    assert!(r.x.is_finite() && r.y.is_finite());
    assert_eq!([r.w, r.h], [800., 240.]);
    assert!(r.x >= 0. && r.x + r.w <= 800.);
}

#[test]
fn deleted_source_cannot_commit_a_window_transfer() {
    let mut m = WorkspaceModel::default();
    let pane = m.open(11, 0);
    let transfer = m.prepare_transfer(pane, 7).unwrap();
    m.close(pane).unwrap();
    assert!(m.commit_transfer(transfer).is_err());
    assert!(!m.windows.contains_key(&7));
}

#[test]
fn transfer_keeps_exactly_one_placement_and_return_order() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.split(a, 22, SplitAxis::Horizontal, [1000., 600.]).unwrap();
    let ticket = m.prepare_transfer(b, 7).unwrap();
    assert_eq!(m.window_of(b), Some(0));
    m.commit_transfer(ticket).unwrap();
    assert_eq!(m.windows[&0].panes(), vec![a]);
    assert_eq!(m.windows[&7].panes(), vec![b]);
    m.transfer(b, 0).unwrap();
    assert_eq!(m.windows[&0].panes(), vec![a, b]);
    assert_eq!(m.panes.len(), 2);
}

#[test]
fn raising_floats_preserves_focus_cycle_order() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.open(22, 0);
    let c = m.open(33, 0);
    m.float(b, Rect::new(10., 10., 400., 300.)).unwrap();
    m.float(c, Rect::new(20., 20., 400., 300.)).unwrap();
    m.focus(b);
    assert_eq!(m.windows[&0].stack, vec![c, b]);
    assert_eq!(m.cycle(0, false), Some(c));
    assert_eq!(m.cycle(0, false), Some(a));
}

#[test]
fn directional_focus_chooses_visible_neighbor_in_same_window() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.open(22, 0);
    let c = m.open(33, 0);
    let bounds = std::collections::BTreeMap::from([
        (a, Rect::new(0., 0., 400., 300.)),
        (b, Rect::new(404., 0., 400., 300.)),
        (c, Rect::new(0., 304., 400., 300.)),
    ]);
    m.focus(a);
    assert_eq!(m.directional(0, Direction::Right, &bounds, false), Some(b));
    assert_eq!(m.directional(0, Direction::Left, &bounds, false), Some(a));
    assert_eq!(m.directional(0, Direction::Down, &bounds, false), Some(c));
    assert_eq!(m.directional(0, Direction::Left, &bounds, false), None);
}

#[test]
fn removed_display_moves_window_onscreen_but_connected_negative_display_stays() {
    let saved = Rect::new(-1000., 40., 900., 700.);
    let left = Rect::new(-1440., 0., 1440., 900.);
    let main = Rect::new(0., 0., 1440., 900.);
    assert_eq!(restore_window_rect(saved, &[left, main]), saved);
    let repaired = restore_window_rect(saved, &[main]);
    assert!(repaired.x >= 0. && repaired.x + repaired.w <= 1440.);
    assert!(repaired.y >= 0. && repaired.y + repaired.h <= 900.);
}

#[test]
fn cycling_away_from_maximized_pane_reveals_destination() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.open(22, 0);
    m.focus(a);
    m.windows.get_mut(&0).unwrap().maximized = Some(a);
    assert_eq!(m.cycle(0, false), Some(b));
    assert_eq!(m.windows[&0].maximized, None);
    assert_eq!(m.cycle(0, true), Some(a));
}

fn shape(node: &DockNode) -> String {
    match node {
        DockNode::Leaf(p) => p.to_string(),
        DockNode::Split { axis, ratio, first, second, .. } => {
            let axis = if *axis == SplitAxis::Horizontal { "H" } else { "V" };
            format!("{axis}{:.2}({},{})", ratio, shape(first), shape(second))
        }
    }
}

#[test]
fn tile_builds_stack_columns_and_grid_shapes() {
    let mut m = WorkspaceModel::default();
    let tiled = |m: &mut WorkspaceModel, layout, n: u64| {
        let leaves: Vec<_> = (1..=n).collect();
        let root = m.tile(layout, &leaves, 0.6).unwrap();
        assert_eq!(root.leaves(), leaves);
        shape(&root)
    };
    use aether_core::TileLayout::*;
    for layout in [Stack, Columns, Grid] {
        assert_eq!(tiled(&mut m, layout, 1), "1");
        assert!(m.tile(layout, &[], 0.6).is_none());
    }
    assert_eq!(tiled(&mut m, Stack, 2), "H0.60(1,2)");
    assert_eq!(tiled(&mut m, Stack, 3), "H0.60(1,V0.50(2,3))");
    assert_eq!(tiled(&mut m, Stack, 5), "H0.60(1,V0.25(2,V0.33(3,V0.50(4,5))))");
    assert_eq!(tiled(&mut m, Columns, 2), "H0.50(1,2)");
    assert_eq!(tiled(&mut m, Columns, 4), "H0.25(1,H0.33(2,H0.50(3,4)))");
    assert_eq!(tiled(&mut m, Grid, 2), "H0.50(1,2)");
    assert_eq!(tiled(&mut m, Grid, 3), "V0.50(H0.50(1,2),3)");
    assert_eq!(tiled(&mut m, Grid, 4), "V0.50(H0.50(1,2),H0.50(3,4))");
    assert_eq!(tiled(&mut m, Grid, 5), "V0.50(H0.33(1,H0.50(2,3)),H0.50(4,5))");
    assert!(m.tile(Manual, &[1, 2], 0.6).is_none());
    assert!(m.tile(Monocle, &[1, 2], 0.6).is_none());
}

#[test]
fn swap_and_promote_reorder_docked_leaves_only() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.open(22, 0);
    let c = m.open(33, 0);
    let f = m.open(44, 0);
    m.float(f, Rect::new(0., 0., 400., 300.)).unwrap();
    let leaves = |m: &WorkspaceModel| m.windows[&0].root.as_ref().unwrap().leaves();
    m.swap(a, c);
    assert_eq!(leaves(&m), vec![c, b, a]);
    m.swap(a, f);
    assert_eq!(leaves(&m), vec![c, b, a]);
    m.promote(b);
    assert_eq!(leaves(&m), vec![b, c, a]);
    m.promote(f);
    assert_eq!(leaves(&m), vec![b, c, a]);
}

#[test]
fn retile_reapplies_after_close_and_keeps_manual_trees() {
    use aether_core::TileLayout::*;
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.split(a, 22, SplitAxis::Vertical, [1000., 600.]).unwrap();
    let c = m.open(33, 0);
    let root = |m: &WorkspaceModel| shape(m.windows[&0].root.as_ref().unwrap());
    let manual = root(&m);
    m.retile(0, Manual, 0.55);
    assert_eq!(root(&m), manual);
    m.retile(0, Stack, 0.55);
    assert_eq!(root(&m), format!("H0.55({a},V0.50({b},{c}))"));
    let DockNode::Split { id, .. } = m.windows[&0].root.clone().unwrap() else { panic!() };
    m.windows.get_mut(&0).unwrap().root.as_mut().unwrap().set_ratio(id, 0.7);
    m.retile(0, Stack, 0.55);
    assert_eq!(root(&m), format!("H0.70({a},V0.50({b},{c}))"));
    m.close(b).unwrap();
    m.retile(0, Stack, 0.55);
    assert_eq!(root(&m), format!("H0.55({a},{c})"));
    m.retile(0, Columns, 0.55);
    m.focus(a);
    m.swap_toward(0, Direction::Left, &Default::default(), true);
    assert_eq!(m.windows[&0].root.as_ref().unwrap().leaves(), vec![a, c]);
    m.focus(c);
    m.swap_toward(0, Direction::Left, &Default::default(), true);
    assert_eq!(m.windows[&0].root.as_ref().unwrap().leaves(), vec![c, a]);
}

#[test]
fn monocle_focus_steps_through_tabs_without_floats() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.open(22, 0);
    let f = m.open(33, 0);
    m.float(f, Rect::new(0., 0., 400., 300.)).unwrap();
    m.focus(a);
    let none = std::collections::BTreeMap::new();
    assert_eq!(m.directional(0, Direction::Left, &none, true), None);
    assert_eq!(m.directional(0, Direction::Right, &none, true), Some(b));
    assert_eq!(m.directional(0, Direction::Down, &none, true), None);
    assert_eq!(m.directional(0, Direction::Up, &none, true), Some(a));
}

#[test]
fn split_in_unmeasured_surface_is_not_disabled() {
    let m = {
        let mut m = WorkspaceModel::default();
        m.open(11, 0);
        m
    };
    let a = m.pane_for(11).unwrap();
    assert_eq!(m.can_split_in(a, DockSide::Right, None, 8.), Ok(()));
    assert_eq!(m.can_split_in(a, DockSide::Right, Some([0., 0.]), 8.), Ok(()));
    assert_eq!(m.can_split_in(a, DockSide::Right, Some([f32::NAN, 600.]), 8.), Ok(()));
}

#[test]
fn split_in_checks_whole_tree_minimum_and_gap() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    assert_eq!(m.can_split_in(a, DockSide::Right, Some([644., 240.]), 0.), Ok(()));
    assert!(m.can_split_in(a, DockSide::Right, Some([644., 240.]), 8.).is_err());
    assert_eq!(m.can_split_in(a, DockSide::Right, Some([652., 240.]), 8.), Ok(()));
    assert_eq!(m.can_split_in(a, DockSide::Bottom, Some([320., 484.]), 0.), Ok(()));
    assert!(m.can_split_in(a, DockSide::Bottom, Some([320., 483.]), 0.).is_err());
    let b = m.split_in(a, 22, DockSide::Right, Some([1000., 600.]), 0.).unwrap();
    assert!(m.can_split_in(b, DockSide::Right, Some([800., 700.]), 0.).is_err());
    assert_eq!(m.can_split_in(b, DockSide::Right, Some([968., 700.]), 0.), Ok(()));
}

#[test]
fn failed_split_in_leaves_model_untouched() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    assert!(m.split_in(a, 22, DockSide::Left, Some([100., 100.]), 0.).is_err());
    assert_eq!(m.panes.len(), 1);
    assert_eq!(m.windows[&0].root.as_ref().unwrap().leaves(), vec![a]);
    assert!(m.split_in(a, 11, DockSide::Left, None, 0.).is_err());
    assert_eq!(m.panes.len(), 1);
}

#[test]
fn split_in_left_and_up_place_new_pane_first() {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.split_in(a, 22, DockSide::Left, None, 0.).unwrap();
    assert_eq!(m.windows[&0].root.as_ref().unwrap().leaves(), vec![b, a]);
    let c = m.split_in(a, 33, DockSide::Top, None, 0.).unwrap();
    assert_eq!(m.windows[&0].root.as_ref().unwrap().leaves(), vec![b, c, a]);
    let Some(DockNode::Split { second, .. }) = &m.windows[&0].root else { panic!() };
    let DockNode::Split { axis, .. } = &**second else { panic!() };
    assert_eq!(*axis, SplitAxis::Vertical);
    assert_eq!(m.windows[&0].focused, Some(c));
}
