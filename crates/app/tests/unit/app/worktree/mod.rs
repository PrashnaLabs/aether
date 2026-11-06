use crate::app::chat_with;

use super::*;

#[test]
fn branch_switch_requires_confirmation_for_the_initiating_chat() {
    let source = [
        include_str!("../../../../src/app/new.rs"),
        include_str!("../../../../src/app/session_ui/bindings.rs"),
        include_str!("../../../../src/app/worktree/branch.rs"),
        include_str!("../../../../src/app/worktree/mod.rs"),
    ]
    .map(|s| s.split("#[cfg(test)]").next().unwrap())
    .concat();
    let dispatch = source
        .split("WorkspaceSelectEvent::SwitchBranch(branch) =>")
        .nth(1)
        .unwrap()
        .split("WorkspaceSelectEvent::NewWorktree")
        .next()
        .unwrap();
    assert!(dispatch.contains("app.confirm_switch_workspace_branch(branch.clone(), window, cx)"));
    assert!(!dispatch.contains("this.switch_workspace_branch("));
    let dialog = source
        .split("fn confirm_switch_workspace_branch(")
        .nth(1)
        .unwrap()
        .split("fn switch_workspace_branch(")
        .next()
        .unwrap();
    let (before_confirm, on_confirm) = dialog.split_once(".on_ok(").unwrap();
    assert!(before_confirm.contains("let id = self.chats[self.active].id;"));
    assert!(before_confirm.contains(".confirm()"));
    assert!(before_confirm.contains("{branch}"));
    assert!(before_confirm.contains("other chats and tools using the same checkout"));
    for mutation in [
        "switch_workspace_branch(",
        "switch_branch(",
        "arm_local_worktree(",
        "worktree_job =",
        "on_cancel(",
        "on_close(",
    ] {
        assert!(!before_confirm.contains(mutation), "premature side effect: {mutation}");
    }
    assert!(on_confirm.contains("this.switch_workspace_branch(id, branch.clone(), cx)"));
    let switch = source
        .split("fn switch_workspace_branch(")
        .nth(1)
        .unwrap()
        .split("fn worktree_blocks_send(")
        .next()
        .unwrap();
    let (guards, _) = switch.split_once("self.agent.switch_branch(").unwrap();
    let guards = guards.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(guards.contains("let Some(ix) = self.chat_ix(id) else { return; };"));
    assert!(guards.contains("!self.workspace_can_change(ix)"));
    assert!(guards.contains("chat.cancel.is_some() || self.locked(other)"));
    assert!(!guards.contains("self.active"));
}

#[test]
fn workspace_trigger_names_current_and_pending_targets() {
    use aether_ui::WorkspaceSelection;
    assert_eq!(workspace_label(&WorkspaceSelection::Local, Some("main")), "main");
    assert_eq!(workspace_label(&WorkspaceSelection::Local, None), "Local");
    assert_eq!(
        workspace_label(&WorkspaceSelection::NewWorktree("dev".into()), Some("main")),
        "dev"
    );
    assert_eq!(
        workspace_label(&WorkspaceSelection::Worktree("/code/review".into()), Some("main")),
        "review"
    );
    assert_eq!(workspace_label(&WorkspaceSelection::Local, Some("Detached HEAD")), "Detached HEAD");
}

#[test]
fn workspace_worktrees_match_resolved_roots_and_exclude_missing() {
    let row = |path: &str, repo: &str, branch: Option<&str>, missing| Listed {
        path: path.into(),
        repo: repo.into(),
        branch: branch.map(String::from),
        missing,
        ..Listed::default()
    };
    let listed = [
        row("/trees/review", "/code/app", Some("review"), false),
        row("/trees/detached", "/code/app", None, false),
        row("/trees/gone", "/code/app", Some("gone"), true),
        row("/trees/other", "/code/other", Some("other"), false),
    ];
    let expected = vec![
        (PathBuf::from("/trees/review"), "review".into()),
        ("/trees/detached".into(), "Detached HEAD".into()),
    ];
    assert_eq!(selectable_worktrees(Path::new("/code/app"), &listed), expected);
    assert!(selectable_worktrees(Path::new("/code/unrelated"), &listed).is_empty());
}

#[test]
fn review_pending_worktree_send_checks_the_target_folder() {
    let mut switching = chat_with("switching", &[]);
    switching.cwd = "/trees/review".into();
    switching.worktree_job = Some(WorktreeJob::Switching);
    let mut pending = chat_with("pending", &[]);
    pending.provider = Provider::Claude;
    pending.cwd = "/code/app".into();
    pending.arm_existing_worktree("/trees/review".into());
    let mut chats = vec![switching, pending];
    assert!(switch_blocks_send(&chats, 1, "/fallback"));
    assert_eq!(chats[1].cwd, "/code/app");
    assert_eq!(chats[1].new_worktree_existing.as_deref(), Some(Path::new("/trees/review")));
    chats[0].worktree_job = None;
    assert!(!switch_blocks_send(&chats, 1, "/fallback"));
}

#[test]
fn review_nested_repository_never_offers_parent_worktrees() {
    let listed = [Listed {
        path: "/trees/parent".into(),
        repo: "/code/app".into(),
        branch: Some("parent".into()),
        ..Listed::default()
    }];
    assert!(selectable_worktrees(Path::new("/code/app/nested"), &listed).is_empty());
    assert_eq!(selectable_worktrees(Path::new("/code/app"), &listed).len(), 1);
}

#[test]
fn removed_or_missing_worktree_blocks_send() {
    assert!(worktree_send_block(true, false).is_some());
    assert!(worktree_send_block(false, true).is_some());
    assert_eq!(worktree_send_block(false, false), None);
}

#[test]
fn custom_worktree_name_wins_over_the_message() {
    assert_eq!(worktree_name_hint("", "Fix login"), "Fix login");
    assert_eq!(worktree_name_hint("  ", "Fix login"), "Fix login");
    assert_eq!(worktree_name_hint("my-fix", "Fix login"), "my-fix");
    assert_eq!(worktree_preview_branch("", "Fix login"), "aether/fix-login");
    assert_eq!(worktree_preview_branch("My Fix!", "ignored"), "aether/my-fix");
}

#[test]
fn composer_toggle_arms_create_on_send_with_existing_picker() {
    let mut chat = chat_with("t", &[]);
    assert_eq!(worktree_target(&chat), WorktreeTarget::Local);
    chat.arm_new_worktree("main".into());
    assert_eq!(worktree_target(&chat), WorktreeTarget::Create("main"));
    chat.arm_existing_worktree(PathBuf::from("/w/app-1/fix"));
    assert!(chat.new_worktree.is_none(), "picking an existing worktree disarms the new one");
    assert_eq!(worktree_target(&chat), WorktreeTarget::Share(Path::new("/w/app-1/fix")));
    chat.arm_new_worktree("dev".into());
    assert!(chat.new_worktree_existing.is_none(), "picking new disarms the existing one");
    assert_eq!(worktree_target(&chat), WorktreeTarget::Create("dev"));
    chat.arm_local_worktree();
    assert_eq!(worktree_target(&chat), WorktreeTarget::Local);
}
