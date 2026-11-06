use crate::app::chat_with;
use aether_core::Message;

use super::*;

#[test]
fn composer_history_only_for_chats_with_history() {
    let new_chat = chat_with("test", &[]);
    let has_history =
        new_chat.messages.iter().any(|m| m.role == Role::User && !m.text.trim().is_empty());
    assert!(!has_history, "new chat without messages must not have history");

    let mut chat_with_history = chat_with("test", &[]);
    chat_with_history.messages.push(Message::user("Hello"));
    let has_history = chat_with_history
        .messages
        .iter()
        .any(|m| m.role == Role::User && !m.text.trim().is_empty());
    assert!(has_history, "chat with user message has history");
}

#[test]
fn chat_header_back_affordance_for_non_agent_chat() {
    let normal_chat = chat_with("direct", &[]);
    let is_agent = normal_chat.is_agent();
    assert!(!is_agent);

    let source = include_str!("../../../../src/app/composer/header.rs");
    assert!(source.contains("\"chat-header-back\""));
    assert!(source.contains("Back to Agent Projects"));
}

#[test]
fn composer_drops_the_footnote_and_the_effort_chip() {
    let card = include_str!("../../../../src/app/composer/mod.rs");
    let chips = include_str!("../../../../src/app/composer/chips.rs");
    assert!(!card.contains("Aether can make mistakes"));
    assert!(!chips.contains("composer_chip(\"effort\""));
}

#[test]
fn transcript_hands_jumping_to_the_fold_bar() {
    let render = include_str!("../../../../src/app/render.rs");
    let card = include_str!("../../../../src/app/composer/mod.rs");
    assert!(render.contains(".jump_button(false)"));
    assert!(card.contains("user_scrolled_up()"));
}

#[test]
fn input_focus_prevents_scroll_folding_until_focus_leaves() {
    assert!(!composer_folds(true, true, true), "focused input stays expanded while scrolled up");
    assert!(composer_folds(true, true, false), "unfocused input may fold while scrolled up");
    assert!(!composer_folds(true, false, false), "returning to the tail expands the input");
    assert!(!composer_folds(false, true, false), "an empty chat never folds");
}

#[gpui_kit::test]
fn wheeling_the_transcript_unfocuses_the_composer(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::Focusable;
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
        app.chats[app.active].messages.push(Message::user("Hello"));
        app
    });
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        app.update(cx, |app, cx| app.focus_composer(window, cx));
        let _ = window.draw(cx);
    });
    let focused = |visual: &mut gpui_kit::VisualTestContext| {
        visual.update(|window, cx| app.read(cx).composer.focus_handle(cx).is_focused(window))
    };
    assert!(focused(&mut visual));
    let transcript = visual.debug_bounds("transcript").unwrap();
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: transcript.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(40.))),
        ..Default::default()
    });
    assert!(!focused(&mut visual));
}

#[gpui_kit::test]
fn background_panel_hides_per_chat_until_its_tasks_end(cx: &mut gpui_kit::TestAppContext) {
    use aether_core::{ToolCall, ToolDetail, ToolStatus, Update};
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
        .update(cx, |app, _, cx| {
            let mut call = ToolCall::new("bg", "dev", ToolDetail::default());
            call.status = ToolStatus::Background;
            let mut reply = Message::assistant();
            reply.apply(Update::Tool(call));
            app.chats[app.active].messages.push(reply);
            let id = app.chats[app.active].id;
            assert!(matches!(app.background_panel(cx), (1, Some(_))));
            app.background_hidden.insert(id);
            assert!(matches!(app.background_panel(cx), (1, None)));
            app.chats[app.active].messages.clear();
            assert!(matches!(app.background_panel(cx), (0, None)));
            assert!(!app.background_hidden.contains(&id), "next task shows the panel again");
        })
        .unwrap();
}

#[gpui_kit::test]
fn the_transcript_keeps_its_height_beside_the_background_panel(cx: &mut gpui_kit::TestAppContext) {
    use aether_core::{ToolCall, ToolDetail, ToolStatus, Update};
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
        let mut call = ToolCall::new("bg", "dev", ToolDetail::default());
        call.status = ToolStatus::Background;
        let mut reply = Message::assistant();
        reply.apply(Update::Tool(call));
        app.chats[app.active].messages.extend([Message::user("Hello"), reply]);
        app
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let transcript = visual.debug_bounds("transcript").unwrap();
    assert!(transcript.size.height > px(200.), "{:?}", transcript.size);
}

#[gpui_kit::test]
fn the_stall_line_can_show_a_foreground_row_in_the_panel(cx: &mut gpui_kit::TestAppContext) {
    use aether_core::{ToolCall, ToolDetail, Update};
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
        .update(cx, |app, _, cx| {
            let mut reply = Message::assistant();
            reply.apply(Update::Tool(ToolCall::new("run", "Run", ToolDetail::default())));
            app.chats[app.active].messages.push(reply);
            let id = app.chats[app.active].id;
            assert!(matches!(app.background_panel(cx), (0, None)));
            app.inspect = Some((id, "run".into()));
            assert!(matches!(app.background_panel(cx), (0, Some(_))), "running row alone opens it");
            app.inspect = Some((id + 1, "run".into()));
            assert!(
                matches!(app.background_panel(cx), (0, None)),
                "another chat's pick stays there"
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn a_clicked_queued_message_opens_in_the_panel(cx: &mut gpui_kit::TestAppContext) {
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
        .update(cx, |app, _, cx| {
            let id = app.chats[app.active].id;
            app.chats[app.active].messages.push(Message::user("Hello"));
            app.chats[app.active].queue.push(crate::app::Queued {
                text: "line one\nline two".into(),
                attachments: Vec::new(),
                delivering: false,
                from_children: false,
                deliveries: vec![],
            });
            assert!(matches!(app.background_panel(cx), (0, None)));
            app.inspect_queued = Some((id, "line one\nline two".into()));
            assert!(matches!(app.background_panel(cx), (0, Some(_))));
            app.chats[app.active].queue.clear();
            assert!(matches!(app.background_panel(cx), (0, None)), "sent messages leave the panel");
        })
        .unwrap();
}
