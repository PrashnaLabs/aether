use super::*;

fn branch(upstream: Option<&str>, ab: Option<(u32, u32)>, remote: Option<&str>) -> Status {
    Status {
        branch: Some("aether/fix".into()),
        commit: Some("abc1234".into()),
        upstream: upstream.map(String::from),
        ahead_behind: ab,
        remote: remote.map(String::from),
        ..Status::default()
    }
}

fn ready(label: &str, tip: &str, op: GitOp) -> Push {
    Push::Ready { label: label.into(), tip: tip.into(), op }
}

#[test]
fn push_shows_only_with_something_to_send() {
    assert_eq!(
        push_state(&branch(Some("origin/x"), Some((2, 0)), None)),
        ready("Push 2 commits", "Push to origin/x", GitOp::Push)
    );
    assert_eq!(push_state(&branch(Some("origin/x"), Some((0, 0)), None)), Push::Hidden);
    assert!(
        matches!(push_state(&branch(Some("origin/x"), None, None)), Push::Ready { ref label, op: GitOp::Push, .. } if label == "Push")
    );
    assert_eq!(
        push_state(&branch(None, None, Some("origin"))),
        ready("Publish branch", "Push aether/fix to origin and track it", GitOp::Push)
    );
    assert_eq!(push_state(&branch(None, None, None)), Push::Hidden);
    let detached = Status { commit: Some("abc1234".into()), ..Status::default() };
    assert!(matches!(push_state(&detached), Push::Blocked(_)));
    let unborn =
        Status { branch: Some("main".into()), remote: Some("origin".into()), ..Status::default() };
    assert_eq!(push_state(&unborn), Push::Hidden);
}

#[test]
fn commits_to_pull_turn_push_into_sync() {
    assert_eq!(
        push_state(&branch(Some("origin/x"), Some((0, 3)), None)),
        ready("Sync Changes", "Pull 3 commits from origin/x", GitOp::Pull)
    );
    assert_eq!(
        push_state(&branch(Some("origin/x"), Some((2, 1)), None)),
        ready("Sync Changes", "Pull 1 commit from origin/x, then push 2 commits", GitOp::Sync)
    );
}

#[test]
fn remote_actions_need_somewhere_to_go() {
    let on = |status: &Status| remote_ops(status).map(|(_, ok)| ok);
    assert_eq!(on(&branch(Some("origin/x"), Some((0, 0)), None)), [true, true, true, true]);
    assert_eq!(on(&branch(None, None, Some("origin"))), [true, false, true, false]);
    assert_eq!(on(&branch(None, None, None)), [false, false, false, false]);
    let detached =
        Status { commit: Some("abc1234".into()), ..branch(Some("origin/x"), None, None) };
    let detached = Status { branch: None, ..detached };
    assert_eq!(on(&detached), [true, false, false, false]);
}
