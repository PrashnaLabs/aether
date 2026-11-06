use super::*;

fn rows() -> WorkspaceRows {
    WorkspaceRows {
        branches: vec!["main".into(), "feature/Login".into()],
        worktrees: vec![WorktreeRow {
            path: "/code/review".into(),
            branch: "Detached HEAD".into(),
            running: true,
        }],
        selected: WorkspaceSelection::Branch("main".into()),
        ..Default::default()
    }
}

#[test]
fn filters_branches_and_worktree_paths() {
    let mut rows = rows();
    rows.filter("LOGIN");
    assert_eq!(rows.filtered, [1]);
    rows.change_tab(WorkspaceTab::Worktrees, "review");
    assert_eq!(rows.filtered, [0]);
    rows.filter("detached");
    assert_eq!(rows.filtered, [0]);
}

#[test]
fn empty_navigation_and_tab_changes_reset_highlight() {
    let mut rows = rows();
    rows.filter("absent");
    rows.navigate(-1);
    assert_eq!(rows.highlight, None);
    rows.change_tab(WorkspaceTab::Worktrees, "");
    assert_eq!(rows.highlight, Some(0));
    rows.navigate(1);
    assert_eq!(rows.highlight, Some(0));
    rows.change_tab(WorkspaceTab::Branches, "");
    rows.navigate(-1);
    assert_eq!(rows.highlight, Some(1));
}

#[test]
fn selected_checkmark_does_not_follow_highlight() {
    let mut rows = rows();
    rows.filter("");
    rows.navigate(1);
    assert_eq!(rows.highlight, Some(1));
    assert!(rows.is_selected(0));
    assert!(!rows.is_selected(1));
    rows.filter("login");
    assert!(!rows.is_selected(0));
    rows.selected = WorkspaceSelection::Worktree("/code/review".into());
    rows.change_tab(WorkspaceTab::Worktrees, "");
    rows.highlight = None;
    assert!(rows.is_selected(0));
}

#[test]
fn confirmation_emits_typed_actions_for_the_filtered_row() {
    let mut rows = rows();
    rows.filter("login");
    assert_eq!(rows.event(false), Some(WorkspaceSelectEvent::SwitchBranch("feature/Login".into())));
    assert_eq!(rows.event(true), Some(WorkspaceSelectEvent::NewWorktree("feature/Login".into())));
    rows.change_tab(WorkspaceTab::Worktrees, "review");
    assert_eq!(rows.event(false), Some(WorkspaceSelectEvent::UseWorktree("/code/review".into())));
    rows.filter("absent");
    assert_eq!(rows.event(false), None);
}

#[test]
fn new_branch_action_stays_available_when_search_has_no_matches() {
    let mut rows = rows();
    rows.filter("feature/new");
    assert_eq!(rows.event(false), None);
    assert_eq!(rows.new_branch_event(), Some(WorkspaceSelectEvent::NewBranch));
    rows.change_tab(WorkspaceTab::Worktrees, "");
    assert_eq!(rows.new_branch_event(), None);
    rows.change_tab(WorkspaceTab::Branches, "");
    assert_eq!(rows.new_branch_event(), Some(WorkspaceSelectEvent::NewBranch));
}

#[test]
fn new_branch_action_needs_an_existing_branch_to_start_from() {
    assert_eq!(WorkspaceRows::default().new_branch_event(), None);
}
