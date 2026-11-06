use super::*;
use crate::git_panel::change;
use aether_git::{Change, ChangeKind};

#[test]
fn commit_says_why_it_is_off() {
    assert_eq!(commit_state(0, 0, "x"), Err("Nothing to commit"));
    assert_eq!(commit_state(1, 2, "  \n"), Err("Write a commit message to commit"));
    assert_eq!(commit_state(1, 3, "fix").as_deref(), Ok("Commit 1 staged file"));
    assert_eq!(commit_state(2, 3, "fix").as_deref(), Ok("Commit 2 staged files"));
    assert_eq!(commit_state(0, 3, "fix").as_deref(), Ok("Commit 3 files"));
    assert_eq!(commit_state(0, 1200, "fix").as_deref(), Ok("Commit 1,200 files"));
}

#[test]
fn commit_notes_name_the_commit() {
    assert_eq!(
        commit_note("Committed a1b2c3d", "\nFix login\n\nbody"),
        "Committed a1b2c3d · Fix login"
    );
    let long = "x".repeat(80);
    assert_eq!(commit_note("Committed a", &long), format!("Committed a · {}…", "x".repeat(60)));
}

#[test]
fn remote_hints_point_at_sync_or_the_terminal() {
    assert!(remote_hint("! [rejected] main -> main (fetch first)").unwrap().contains("Sync"));
    assert!(
        remote_hint("fatal: could not read Username: terminal prompts disabled")
            .unwrap()
            .contains("terminal")
    );
    assert!(
        remote_hint("fatal: Not possible to fast-forward, aborting.").unwrap().contains("diverged")
    );
    assert_eq!(remote_hint("CONFLICT (content): Merge conflict in a.rs"), None);
}

fn repo(changes: Vec<Change>) -> Status {
    Status {
        changes,
        branch: Some("main".into()),
        commit: Some("abc1234".into()),
        upstream: Some("origin/main".into()),
        ahead_behind: Some((1, 0)),
        parent: true,
        ..Status::default()
    }
}

#[test]
fn commit_actions_say_why_they_are_off() {
    use CommitAction::*;
    let modified = || vec![change("a.rs", ChangeKind::Modified, false)];
    let status = repo(modified());
    for action in [Commit, All, Push, Sync, Amend, Undo] {
        assert_eq!(commit_allowed(&status, "fix", action), Ok(()), "{action:?}");
    }
    assert_eq!(commit_allowed(&status, " ", Commit), Err("Write a commit message to commit"));
    assert_eq!(commit_allowed(&repo(vec![]), "fix", All), Err("Nothing to commit"));
    // amend may keep HEAD's message; undo needs none
    assert_eq!(commit_allowed(&status, "", Amend), Ok(()));
    assert_eq!(commit_allowed(&repo(vec![]), "", Undo), Ok(()));

    let unpublished = Status { upstream: None, ahead_behind: None, ..repo(modified()) };
    assert_eq!(commit_allowed(&unpublished, "fix", Push), Err("No remote to push to"));
    assert!(commit_allowed(&unpublished, "fix", Sync).is_err());
    let publishable = Status { remote: Some("origin".into()), ..unpublished.clone() };
    assert_eq!(commit_allowed(&publishable, "fix", Push), Ok(()));
    assert_eq!(commit_allowed(&unpublished, "", Amend), Ok(()));
    assert_eq!(commit_allowed(&unpublished, "", Undo), Ok(()));
}

#[test]
fn amend_and_undo_stay_off_once_pushed_and_undo_off_on_the_first_commit() {
    use CommitAction::*;
    let pushed = Status { ahead_behind: Some((0, 0)), ..repo(vec![]) };
    let reason = Err("Last commit is pushed: can't amend or undo");
    assert_eq!(commit_allowed(&pushed, "x", Amend), reason);
    assert_eq!(commit_allowed(&pushed, "x", Undo), reason);
    let behind = Status { ahead_behind: Some((0, 2)), ..repo(vec![]) };
    assert_eq!(commit_allowed(&behind, "x", Undo), reason);
    let gone = Status { ahead_behind: None, ..repo(vec![]) };
    assert_eq!(commit_allowed(&gone, "x", Amend), Ok(()));
    let first = Status { parent: false, ..repo(vec![]) };
    assert_eq!(commit_allowed(&first, "x", Amend), Ok(()));
    assert_eq!(commit_allowed(&first, "x", Undo), Err("The first commit can't be undone"));
    let unborn = Status { commit: None, parent: false, ..repo(vec![]) };
    assert_eq!(commit_allowed(&unborn, "x", Amend), Err("No commit to amend or undo yet"));
}

#[test]
fn a_captured_op_runs_only_while_its_folder_reads_as_the_same_repo() {
    use aether_git::StatusError;
    let (a, b, top) = (Path::new("/w/a"), Path::new("/w/b"), Path::new("/w/a"));
    let read = |status| Loaded { status, at: 0 };
    let mut statuses = HashMap::new();
    statuses.insert(a.to_path_buf(), read(Ok(Status { top: top.into(), ..repo(vec![]) })));
    assert!(still_at(&statuses, a, top));
    assert!(!still_at(&statuses, a, Path::new("/w")));
    assert!(!still_at(&statuses, b, top));
    statuses.insert(a.to_path_buf(), read(Err(StatusError::NotRepo)));
    assert!(!still_at(&statuses, a, top));
    assert!(!still_at(&HashMap::new(), a, top));
}
