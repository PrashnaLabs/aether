use super::*;
use aether_core::ChatLayout;

#[test]
fn next_chat_layout_cycles_all_three() {
    assert_eq!(next_chat_layout(ChatLayout::Tabbed), ChatLayout::Sidebar);
    assert_eq!(next_chat_layout(ChatLayout::Sidebar), ChatLayout::Split);
    assert_eq!(next_chat_layout(ChatLayout::Split), ChatLayout::Tabbed);
}

#[test]
fn only_split_disables_monocle() {
    assert!(chat_layout_allows_monocle(ChatLayout::Tabbed));
    assert!(chat_layout_allows_monocle(ChatLayout::Sidebar));
    assert!(!chat_layout_allows_monocle(ChatLayout::Split));
}

fn model_with_floats(docked: bool) -> (WorkspaceModel, PaneId, PaneId) {
    let mut m = WorkspaceModel::default();
    let a = m.open(11, 0);
    let b = m.split(a, 22, SplitAxis::Horizontal, [1000., 600.]).unwrap();
    m.float(b, Rect::new(500., 400., 600., 500.)).unwrap();
    if !docked {
        m.float(a, Rect::new(0., 0., 400., 300.)).unwrap();
    }
    (m, a, b)
}

#[test]
fn drawn_bounds_clamp_without_touching_stored_bounds() {
    let (m, _, b) = model_with_floats(true);
    let layout = &m.windows[&0];
    let drawn = float_draw_bounds(layout, [500., 400.]);
    assert_eq!(drawn, vec![(b, layout.floats[0].bounds.sanitize([500., 400.]))]);
    assert_ne!(drawn[0].1, layout.floats[0].bounds);
    assert_eq!(layout.floats[0].bounds, Rect::new(500., 400., 600., 500.));
}

#[test]
fn drawn_floats_skip_maximized_pane_but_keep_others() {
    let (mut m, a, b) = model_with_floats(false);
    m.windows.get_mut(&0).unwrap().maximized = Some(a);
    let ids: Vec<_> =
        float_draw_bounds(&m.windows[&0], [1000., 600.]).iter().map(|f| f.0).collect();
    assert_eq!(ids, vec![b]);
}

#[test]
fn leading_pane_is_never_a_float() {
    let (mut m, a, b) = model_with_floats(false);
    assert!(m.windows[&0].root.is_none());
    assert_eq!(leading_pane(&m.windows[&0]), None);
    m.windows.get_mut(&0).unwrap().maximized = Some(b);
    assert_eq!(leading_pane(&m.windows[&0]), None);
    m.dock(a, None, DockSide::Right).unwrap();
    m.windows.get_mut(&0).unwrap().maximized = Some(b);
    assert_eq!(leading_pane(&m.windows[&0]), Some(a));
}

#[test]
fn dock_all_floats_docks_every_float_of_the_window() {
    let (mut m, a, b) = model_with_floats(false);
    dock_all_floats(&mut m, 0);
    let layout = &m.windows[&0];
    assert!(layout.floats.is_empty());
    let mut leaves = layout.root.as_ref().unwrap().leaves();
    leaves.sort();
    assert_eq!(leaves, vec![a, b]);
}
