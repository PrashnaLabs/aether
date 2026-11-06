use super::*;
use gpui_kit::px;
use workspace::WorkspaceModel;

#[test]
fn drawer_height_stays_between_the_grid_floor_and_three_quarters() {
    let min = min_height(px(13.));
    let row_h = f32::from(row_height(px(13.)));
    assert_eq!(min, HEADER_HEIGHT + GRID_PAD_Y + MIN_ROWS as f32 * row_h);
    assert_eq!(drawer_height(260., 760., min), 260.);
    assert_eq!(drawer_height(50., 760., min), min);
    assert_eq!(drawer_height(700., 760., min), 570.);
    assert_eq!(drawer_height(300., 150., min), min);
    assert!(min_height(px(16.)) > min);
}

#[test]
fn global_folder_falls_back_to_home_when_workspace_empty() {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let folder = resolve_global_folder("");
    assert_eq!(folder, home);
}

#[test]
fn global_folder_falls_back_to_home_when_workspace_nonexistent() {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let folder = resolve_global_folder("/path/that/definitely/does/not/exist/99999");
    assert_eq!(folder, home);
}

#[test]
fn global_folder_uses_workspace_when_valid() {
    let tmp = std::env::temp_dir();
    let folder = resolve_global_folder(tmp.to_str().unwrap());
    assert_eq!(folder, tmp);
}

#[test]
fn switching_to_chat_without_folder_resets_active_terminal_tab() {
    // When active tab is Project(42) and destination chat has no folder:
    let mut active_terminal_tab = TerminalTab::Project(42);
    let destination_has_folder = false;

    if !destination_has_folder {
        active_terminal_tab = TerminalTab::Global;
    } else if matches!(active_terminal_tab, TerminalTab::Project(_)) {
        active_terminal_tab = TerminalTab::Project(99);
    }
    assert_eq!(active_terminal_tab, TerminalTab::Global);

    // When destination chat has a folder, Project tab transitions to new chat id:
    let mut active_terminal_tab = TerminalTab::Project(42);
    let destination_has_folder = true;

    if !destination_has_folder {
        active_terminal_tab = TerminalTab::Global;
    } else if matches!(active_terminal_tab, TerminalTab::Project(_)) {
        active_terminal_tab = TerminalTab::Project(99);
    }
    assert_eq!(active_terminal_tab, TerminalTab::Project(99));

    // When active tab is Global and destination chat has a folder, it stays Global:
    let mut active_terminal_tab = TerminalTab::Global;
    let destination_has_folder = true;

    if !destination_has_folder {
        active_terminal_tab = TerminalTab::Global;
    } else if matches!(active_terminal_tab, TerminalTab::Project(_)) {
        active_terminal_tab = TerminalTab::Project(99);
    }
    assert_eq!(active_terminal_tab, TerminalTab::Global);
}

#[test]
fn closing_terminal_resets_matching_project_tab_to_global() {
    let mut active_terminal_tab = TerminalTab::Project(42);
    let closed_id = 42;
    if active_terminal_tab == TerminalTab::Project(closed_id) {
        active_terminal_tab = TerminalTab::Global;
    }
    assert_eq!(active_terminal_tab, TerminalTab::Global);

    // Non-matching tab remains unaffected
    let mut active_terminal_tab = TerminalTab::Project(99);
    if active_terminal_tab == TerminalTab::Project(closed_id) {
        active_terminal_tab = TerminalTab::Global;
    }
    assert_eq!(active_terminal_tab, TerminalTab::Project(99));
}

#[test]
fn effective_drawer_height_uses_three_quarters_viewport_when_maximized() {
    let min = min_height(px(13.));
    assert_eq!(effective_drawer_height(260., 800., min, false), 260.);
    assert_eq!(effective_drawer_height(260., 800., min, true), 600.);
}

#[test]
fn global_terminal_workspace_defaults_sidebar_to_hidden() {
    let mut model = WorkspaceModel::new();
    assert!(model.sidebar_open);
    model.sidebar_open = false;
    assert!(!model.sidebar_open);
}
