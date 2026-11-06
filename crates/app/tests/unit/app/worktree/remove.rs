use super::*;

#[test]
fn remove_summary_names_what_is_lost_and_what_is_kept() {
    let path = "~/w/app-1/fix";
    let branch = Some("aether/fix".to_string());
    let clean = RemoveCheck {
        missing: false,
        changes: vec![],
        branch: branch.clone(),
        unmerged: 0,
        managed: true,
    };
    assert_eq!(
        remove_summary(&clean, path, false),
        [
            "Deletes the folder ~/w/app-1/fix. It has no uncommitted changes.",
            "Branch aether/fix is kept.",
            "Ignored files, such as build output, are deleted too.",
        ]
    );
    assert_eq!(remove_summary(&clean, path, true)[1], "Branch aether/fix is deleted too.");
    let changes = ["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"].map(String::from).to_vec();
    let dirty = RemoveCheck { missing: false, changes, branch, unmerged: 2, managed: true };
    let lines = remove_summary(&dirty, path, false);
    assert_eq!(
        lines[0],
        "Deletes the folder ~/w/app-1/fix and 5 uncommitted changes: a.rs, b.rs, c.rs and 2 more. \
         You'll confirm once more before they're deleted."
    );
    assert_eq!(lines[1], "Branch aether/fix is kept, with its 2 commits that no other branch has.");
    let one = RemoveCheck {
        missing: false,
        changes: vec!["a.rs".into()],
        branch: None,
        unmerged: 1,
        managed: false,
    };
    let lines = remove_summary(&one, path, false);
    assert!(
        lines[0].starts_with("Deletes the folder ~/w/app-1/fix and 1 uncommitted change: a.rs. ")
    );
    assert_eq!(lines[1], "1 commit that isn't on any branch will be lost.");
    let detached =
        RemoveCheck { missing: false, changes: vec![], branch: None, unmerged: 0, managed: true };
    assert_eq!(remove_summary(&detached, path, false).len(), 2);
}

#[test]
fn a_missing_worktrees_summary_warns_about_unplugged_drives() {
    let gone = RemoveCheck { missing: true, managed: true, ..RemoveCheck::default() };
    assert_eq!(
        remove_summary(&gone, "~/w/app-1/fix", false),
        [
            "The folder ~/w/app-1/fix is gone, so this only clears git's record of it.",
            "A worktree on a drive that isn't connected right now counts as gone.",
        ]
    );
}

#[test]
fn busy_notice_names_setup_or_a_running_agent() {
    let (title, text) = busy_notice("Fix bug", true);
    assert_eq!(title, "Cancel setup first");
    assert_eq!(
        text,
        "\u{201c}Fix bug\u{201d} is waiting on worktree setup. Cancel it, then remove the worktree."
    );
    let (title, text) = busy_notice("Fix bug", false);
    assert_eq!(title, "Stop the agent first");
    assert_eq!(
        text,
        "\u{201c}Fix bug\u{201d} is still working in this worktree. Stop it, then remove the worktree."
    );
}
