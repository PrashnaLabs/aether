use super::*;

#[test]
fn btw_question_takes_the_text_after_the_command() {
    assert_eq!(btw_question("/btw x"), Some("x"));
    assert_eq!(btw_question("/side x y"), Some("x y"));
    assert_eq!(btw_question("  /btw  x  "), Some("x"));
    assert_eq!(btw_question("/btw"), None);
    assert_eq!(btw_question("/btw   "), None);
    assert_eq!(btw_question("/btwx"), None);
    assert_eq!(btw_question("/BTW x"), None);
    assert_eq!(btw_question("hi /btw x"), None);
}

#[test]
fn the_side_prompt_wraps_the_question() {
    let prompt = side_prompt("which file?");
    assert!(prompt.starts_with("Side question from the user."));
    assert!(prompt.ends_with("Question: which file?"));
}

#[test]
fn follow_ups_carry_prior_turns() {
    let mut thread = BtwThread::new();
    thread.turns.push(BtwTurn {
        question: "which file?".into(),
        answer: "a.ts".into(),
        error: None,
        running: false,
    });
    let prompt = btw_prompt(&thread.turns, "and the other one?");
    assert!(prompt.contains("User: which file?"));
    assert!(prompt.contains("Assistant: a.ts"));
    assert!(prompt.contains("Question: and the other one?"));
    assert!(prompt.starts_with("Earlier in this side chat:\nUser: which file?\nAssistant: a.ts"));
    thread.turns[0].answer = "  first\n\n".to_owned() + &"界🧪".repeat(300);
    let prompt = btw_prompt(&thread.turns, "follow up");
    assert!(prompt.contains(&format!("Assistant: {}", thread.turns[0].answer)));
    assert_eq!(btw_prompt(&[], "first"), side_prompt("first"));
}

#[test]
fn empty_turns_say_thinking_or_no_answer() {
    let mut turn = BtwTurn::new("q".into());
    assert_eq!(empty_note(&turn), Some("Thinking…"));
    turn.running = false;
    assert_eq!(empty_note(&turn), Some("No answer"));
    turn.error = Some("boom".into());
    assert_eq!(empty_note(&turn), None);
    turn.error = None;
    turn.answer = "a".into();
    assert_eq!(empty_note(&turn), None);
}

#[gpui_kit::test]
fn a_btw_message_adds_a_turn_and_leaves_the_chat_alone(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    window
        .update(cx, |app, window, cx| {
            let id = app.chats[app.active].id;
            let sent = app.chats[app.active].messages.len();
            app.composer.update(cx, |c, cx| c.set_value("/btw hi", window, cx));
            app.send(window, cx);
            assert_eq!(app.chats[app.active].messages.len(), sent);
            assert!(app.chats[app.active].queue.is_empty());
            assert!(app.composer.read(cx).value().is_empty());
            assert_eq!(app.btw[&id].turns.len(), 1);
            assert_eq!(app.btw[&id].turns[0].question, "hi");
            assert!(app.btw_open);
            app.drop_btw(id);
            assert!(!app.btw.contains_key(&id));
        })
        .unwrap();
}

#[gpui_kit::test]
fn btw_panel_matches_tab_bar_height_and_uses_shared_composer(cx: &mut gpui_kit::TestAppContext) {
    let source = include_str!("../../../../src/app/chat/side.rs");
    assert!(source.contains("Composer::new(&self.btw_input)"));
    assert!(source.contains("SendButton::new()"));
    assert!(!source.contains("field_area(&self.btw_input)"));

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
        app.chats[app.active].provider = Provider::Codex;
        app.btw_open = true;
        app
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let title = visual.debug_bounds("btw-title-bar").expect("btw title bar should render");
    assert_eq!(title.size.height, px(38.));
    assert!(visual.debug_bounds("btw-composer").is_some());
}
