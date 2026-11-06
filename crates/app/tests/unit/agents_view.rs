use super::*;

#[test]
fn poll_is_faster_on_the_page() {
    assert_eq!(poll_interval(true), Duration::from_secs(2));
    assert_eq!(poll_interval(false), Duration::from_secs(10));
}

#[test]
fn state_text_marks_estimates() {
    use ExternalState::*;
    assert_eq!(external_state_text(&Working, true), "Working");
    assert_eq!(external_state_text(&Idle, false), "Idle (estimated)");
    assert_eq!(external_state_text(&Waiting("approval".into()), true), "Needs approval");
    assert_eq!(external_state_text(&Waiting("input needed".into()), true), "Needs input");
    assert_eq!(external_state_text(&Waiting("input".into()), false), "Needs input (estimated)");
}

fn agent(pid: Option<u32>) -> ExternalAgent {
    ExternalAgent {
        provider: aether_core::Provider::Claude,
        pid,
        started: String::new(),
        elapsed: String::new(),
        cwd: "/tmp".into(),
        session: "s".into(),
        title: String::new(),
        state: ExternalState::Idle,
        exact: true,
        origin: String::new(),
    }
}

#[test]
fn open_needs_a_session_and_a_process() {
    assert_eq!(open_blocker(&agent(Some(7))), None);
    assert_eq!(open_blocker(&agent(None)), Some("Busy in another OpenCode client"));
    let mut bare = agent(Some(7));
    bare.session.clear();
    assert_eq!(open_blocker(&bare), Some("No session to open"));
}

#[test]
fn transcript_becomes_finished_messages() {
    let msgs = transcript_messages(vec![(Role::User, "hi".into()), (Role::Assistant, "yo".into())]);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, Role::User);
    assert_eq!(msgs[0].text, "hi");
    assert_eq!(msgs[1].role, Role::Assistant);
    assert_eq!(msgs[1].text, "yo");
    assert!(msgs[1].ended_at_ms.is_some());
}

#[test]
fn lock_follows_the_pid_or_session_in_the_scan() {
    let mut busy = agent(None);
    busy.session = "ses_x".into();
    let found = [agent(Some(7)), busy];
    assert_eq!(externally_locked(Some(7), "", &found), Some(&found[0]));
    assert!(externally_locked(Some(8), "", &found).is_none());
    assert!(externally_locked(None, "", &found).is_none());
    assert!(externally_locked(Some(7), "s", &[]).is_none());
    assert_eq!(externally_locked(None, "s", &found), Some(&found[0]));
    assert_eq!(externally_locked(Some(8), "s", &found), Some(&found[0]));
    assert!(externally_locked(None, "other", &found).is_none());
    assert!(externally_locked(None, "ses_x", &found).is_none());
}
