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

fn oc_agent(pid: Option<u32>, session: &str, state: ExternalState) -> ExternalAgent {
    let mut a = agent(pid);
    a.provider = aether_core::Provider::OpenCode;
    a.session = session.into();
    a.state = state;
    a
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
fn opencode_service_session_opens_without_a_pid() {
    use ExternalState::*;
    assert_eq!(open_blocker(&oc_agent(None, "ses_x", Working)), None);
    assert_eq!(open_blocker(&oc_agent(None, "", Working)), Some("No session to open"));
    assert_eq!(
        open_blocker(&oc_agent(None, "s", Working)),
        Some("Busy in another OpenCode client")
    );
}

#[test]
fn transcript_becomes_finished_messages() {
    let turn = |role, text: &str, tools| aether_agent::Turn { role, text: text.into(), tools };
    let read = aether_core::ToolCall::new(
        "a",
        "Read",
        aether_core::ToolDetail::Read { path: "x".into(), range: None },
    );
    let msgs = transcript_messages(vec![
        turn(Role::User, "hi", vec![]),
        turn(Role::Assistant, "yo", vec![read]),
    ]);
    assert!(matches!(msgs[1].steps.as_slice(), [aether_core::Step::Tool(c)] if c.id == "a"));
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
    // updated to the new rule: a busy pid-None OpenCode service session locks by session id
    let busy_oc = oc_agent(None, "ses_x", ExternalState::Working);
    let found = [agent(Some(7)), busy_oc];
    assert_eq!(externally_locked(None, "ses_x", &found), Some(&found[1]));
}

#[test]
fn idle_opencode_service_session_does_not_lock() {
    let idle_oc = oc_agent(None, "ses_x", ExternalState::Idle);
    assert!(externally_locked(None, "ses_x", &[idle_oc]).is_none());
    let waiting_oc = oc_agent(None, "ses_x", ExternalState::Waiting("approval".into()));
    let found = [waiting_oc];
    assert_eq!(externally_locked(None, "ses_x", &found), Some(&found[0]));
}

#[gpui_kit::test]
fn locked_chat_offers_stop_to_continue_here(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        app.chats[app.active].cli_session = "s".into();
        app.external = vec![agent(Some(7))];
        app
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("composer-stop-external").is_some());
}
