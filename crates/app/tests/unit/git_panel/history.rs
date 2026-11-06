use super::*;

fn commit(sha: &str) -> Commit {
    Commit {
        sha: sha.into(),
        short: sha[..7].into(),
        subject: "Fix it".into(),
        author: "Ada".into(),
        time: 1_700_000_000,
        flow: None,
    }
}

#[test]
fn commit_menus_copy_always_and_switch_only_when_free() {
    use CommitItem::*;
    let labels: Vec<&str> =
        commit_items(false, false, true).iter().map(|(i, _)| i.label()).collect();
    assert_eq!(
        labels,
        [
            "Copy commit ID",
            "Copy message",
            "Open changes",
            "Create branch here…",
            "Check out (detached)…"
        ]
    );
    assert_eq!(
        commit_items(false, false, true),
        [(CopyId, true), (CopyMessage, true), (Open, true), (Branch, true), (Detach, true)]
    );
    assert_eq!(
        commit_items(true, false, true),
        [(CopyId, true), (CopyMessage, true), (Open, true), (Branch, false), (Detach, false)]
    );
    // the open commit's own header has no Open; HEAD already there can't check it out
    assert_eq!(
        commit_items(false, true, false),
        [(CopyId, true), (CopyMessage, true), (Branch, true), (Detach, false)]
    );
}

#[test]
fn detached_means_no_branch_and_head_on_the_commit() {
    let sha = "f00dcafe0123456789abcdef0123456789abcdef";
    let at = |branch: Option<&str>, head: &str| Status {
        branch: branch.map(String::from),
        commit: Some(head.into()),
        ..Status::default()
    };
    assert!(detached_at(Some(&at(None, "f00dcaf")), &commit(sha)));
    assert!(!detached_at(Some(&at(Some("main"), "f00dcaf")), &commit(sha)));
    assert!(!detached_at(Some(&at(None, "abc1234")), &commit(sha)));
    assert!(!detached_at(None, &commit(sha)));
}
