use super::*;

#[test]
fn agent_state_labels() {
    assert_eq!(AgentState::Working(0).label(), "Working");
    assert_eq!(AgentState::Working(2).label(), "Working · 2 queued");
    assert_eq!(AgentState::Waiting("Needs approval").label(), "Needs approval");
    assert_eq!(AgentState::Queued(3).label(), "Queued 3");
    assert_eq!(AgentState::Failed.label(), "Failed");
    assert_eq!(AgentState::Done.label(), "Done");
    assert_eq!(AgentState::Open.label(), "Idle · session open");
    assert!(AgentState::Working(0).busy() && AgentState::Waiting("x").busy());
    assert!(!AgentState::Queued(1).busy() && !AgentState::Open.busy());
}

#[test]
fn removed_row_git_chip_is_not_clickable() {
    let line = |removed: bool, changed: usize| ChatLine {
        provider: Provider::Claude,
        project: None,
        model: SharedString::from(""),
        effort: SharedString::default(),
        place: None,
        time: SharedString::from(""),
        path: SharedString::from("/repo"),
        removed,
        changed,
        added: 0,
        deleted: 0,
        ahead_behind: Some((1, 0)),
        background: 0,
    };
    assert!(line(false, 2).status_clickable());
    assert!(!line(true, 2).status_clickable());
    let clean = ChatLine { changed: 0, ahead_behind: None, ..line(false, 0) };
    assert!(!clean.status_clickable());
}

#[test]
fn sidebar_collapse_handler_can_be_set() {
    let sidebar = ChatSidebar::new("Aether").on_collapse(|_, _, _| {});
    assert!(sidebar.on_collapse.is_some());
}

#[test]
fn status_text_pairs_every_state_with_words() {
    let now = 1_400_000;
    assert_eq!(ChatStatus::Working { since_ms: 80_000 }.text(now), "Working · 22m 00s");
    assert_eq!(ChatStatus::Waiting.text(now), "Needs you");
    assert_eq!(ChatStatus::Background(2).text(now), "2 in background");
    assert_eq!(
        ChatStatus::Quiet { since_ms: now - 120_000, turn_ms: 0 }.text(now),
        "No output 2m 00s"
    );
    assert_eq!(ChatStatus::Retrying.text(now), "Retrying");
    assert_eq!(ChatStatus::Idle.text(now), "");
}
