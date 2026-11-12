use crate::app::{Queued, chat_with};

use super::*;

#[test]
fn queue_only_advances_after_an_unstopped_run() {
    let mut chat = chat_with("t", &[]);
    let file =
        Attachment { name: "a.png".into(), path: "/x".into(), mime: "image/png".into(), size: 1 };
    let a = Queued {
        text: "a".into(),
        attachments: vec![file],
        delivering: false,
        from_children: false,
        deliveries: vec![],
    };
    let b = Queued {
        text: "b".into(),
        attachments: vec![],
        delivering: false,
        from_children: false,
        deliveries: vec![],
    };
    chat.queue = vec![a.clone(), b.clone()];
    assert_eq!(next_queued(&mut chat), Some(a));
    chat.stopped = true;
    assert_eq!(next_queued(&mut chat), None);
    assert_eq!(chat.queue, [b]);
}

#[test]
fn a_delivering_steer_holds_the_queue() {
    let mut chat = chat_with("t", &[]);
    let q = |t: &str, d: bool| Queued {
        text: t.into(),
        attachments: vec![],
        delivering: d,
        from_children: false,
        deliveries: vec![],
    };
    chat.queue = vec![q("a", false), q("b", true)];
    assert_eq!(next_queued(&mut chat), None);
    chat.queue.pop();
    assert_eq!(next_queued(&mut chat), Some(q("a", false)));
}

#[test]
fn steered_removes_the_matching_delivering_chip() {
    let q = |t: &str, d: bool| Queued {
        text: t.into(),
        attachments: vec![],
        delivering: d,
        from_children: false,
        deliveries: vec![],
    };
    let mut queue = vec![q("a", false), q("b", true), q("c", true)];
    assert_eq!(take_delivered(&mut queue, "c").map(|x| x.text), Some("c".into()));
    assert_eq!(take_delivered(&mut queue, "zzz").map(|x| x.text), Some("b".into()));
    assert_eq!(take_delivered(&mut queue, "a"), None, "plain queued items stay");
    assert_eq!(queue.len(), 1);
}

#[test]
fn saves_at_most_every_two_seconds() {
    assert!(!save_due(10_000, 9_000));
    assert!(save_due(11_000, 9_000));
}

#[test]
fn a_files_only_message_is_titled_by_its_first_file() {
    let mut m = Message::user("");
    assert_eq!(title_text(&m), "");
    m.attachments = vec![Attachment {
        name: "a.png".into(),
        path: "/x".into(),
        mime: "image/png".into(),
        size: 1,
    }];
    assert_eq!(title_text(&m), "a.png");
    m.text = "fix it".into();
    assert_eq!(title_text(&m), "fix it");
}

#[test]
fn auto_title_is_the_first_line_capped_at_40_chars() {
    assert_eq!(auto_title("fix the bug\nmore detail"), "fix the bug");
    assert_eq!(auto_title(&"x".repeat(50)), "x".repeat(40));
}

#[test]
fn a_late_steer_belongs_to_its_waiting_chip_in_the_same_epoch() {
    let q = |t: &str, d: bool| Queued {
        text: t.into(),
        attachments: vec![],
        delivering: d,
        from_children: false,
        deliveries: vec![],
    };
    let queue = vec![q("a", true), q("b", false)];
    assert!(still_owns(1, 1, &queue, "a"));
    assert!(!still_owns(2, 1, &queue, "a"), "rewound since");
    assert!(!still_owns(1, 1, &queue, "b"), "returned to the queue by stop");
    assert!(!still_owns(1, 1, &queue, "zzz"), "no oldest-chip fallback");
}

#[test]
fn a_delivering_steer_keeps_the_chat_busy() {
    let mut chat = chat_with("t", &[]);
    assert!(!chat.busy());
    chat.queue = vec![Queued {
        text: "a".into(),
        attachments: vec![],
        delivering: false,
        from_children: false,
        deliveries: vec![],
    }];
    assert!(!chat.busy());
    chat.queue[0].delivering = true;
    assert!(chat.busy());
    chat.queue.clear();
    chat.cancel = Some(aether_agent::Cancel::inert());
    assert!(chat.busy());
}

#[test]
fn a_turn_the_cli_resumes_continues_only_an_unfinished_trailing_reply() {
    assert_eq!(reusable_reply(&[]), None);
    let mut chat = chat_with("t", &["hi"]);
    assert_eq!(reusable_reply(&chat.messages), None, "user message last");
    chat.messages.push(Message::assistant());
    assert_eq!(reusable_reply(&chat.messages), Some(1));
    chat.messages[1].finish();
    assert_eq!(reusable_reply(&chat.messages), None, "finished");
}

#[test]
fn only_a_reply_no_run_streams_into_is_left_unfinished() {
    let mut chat = chat_with("t", &["hi"]);
    chat.messages.push(Message::assistant());
    assert_eq!(left_unfinished(&chat), Some(1));
    chat.cancel = Some(aether_agent::Cancel::inert());
    assert_eq!(left_unfinished(&chat), None, "a run still owns it");
    chat.cancel = None;
    chat.messages[1].finish();
    assert_eq!(left_unfinished(&chat), None);
}

#[test]
fn an_unreadable_placeholder_is_not_an_unfinished_reply() {
    let mut chat = chat_with("t", &["hi"]);
    chat.messages.push(Message::unreadable());
    assert_eq!(reusable_reply(&chat.messages), None);
}

#[test]
fn a_restored_reply_finished_before_a_send_stays_finished() {
    let mut chat = chat_with("t", &["hi"]);
    chat.messages.push(Message::assistant());
    crate::app::chat::create::restore_messages(Provider::OpenCode, &mut chat.messages);
    let at = left_unfinished(&chat).expect("restored reply never finished");
    chat.messages[at].finish();
    assert_eq!(left_unfinished(&chat), None);
}

#[test]
fn refused_steer_returns_to_the_queue() {
    let q = |t: &str, d: bool| Queued {
        text: t.into(),
        attachments: vec![],
        delivering: d,
        from_children: false,
        deliveries: vec![],
    };
    let mut queue = vec![q("a", false), q("b", true)];
    assert!(refuse_delivered(&mut queue, "b"));
    assert_eq!(queue, [q("a", false), q("b", false)]);
    assert!(!refuse_delivered(&mut queue, "b"), "nothing left delivering");
}

#[test]
fn a_refusal_after_the_run_ended_moves_the_queue_on() {
    let q = |t: &str, d: bool| Queued {
        text: t.into(),
        attachments: vec![],
        delivering: d,
        from_children: false,
        deliveries: vec![],
    };
    let mut chat = chat_with("t", &[]);
    chat.queue = vec![q("b", true), q("c", false)];
    chat.cancel = Some(aether_agent::Cancel::inert());
    assert_eq!(after_refusal(&mut chat, "b"), None, "the running turn submits it when it ends");
    assert_eq!(chat.queue, [q("b", false), q("c", false)]);
    chat.queue[0].delivering = true;
    chat.cancel = None;
    assert_eq!(after_refusal(&mut chat, "b"), Some(q("b", false)));
    assert_eq!(chat.queue, [q("c", false)]);
}

#[test]
fn a_refusal_matches_the_chip_without_its_files_block() {
    let q = |t: &str, d: bool| Queued {
        text: t.into(),
        attachments: vec![],
        delivering: d,
        from_children: false,
        deliveries: vec![],
    };
    let mut queue = vec![q("a", true), q("b", true)];
    assert!(refuse_delivered(&mut queue, "b\n\nAttached files:\n- /x/a.png (image/png)"));
    assert_eq!(queue, [q("a", true), q("b", false)]);
}

#[test]
fn a_failed_or_stopped_run_releases_its_steers() {
    let q = |t: &str, d: bool| Queued {
        text: t.into(),
        attachments: vec![],
        delivering: d,
        from_children: false,
        deliveries: vec![],
    };
    let mut chat = chat_with("t", &[]);
    chat.queue = vec![q("a", true)];
    release_steers(&mut chat, false);
    assert!(chat.busy(), "a finished run's steer may still start its own turn");
    release_steers(&mut chat, true);
    assert_eq!(chat.queue, [q("a", false)]);
    assert!(!chat.busy());
    chat.queue[0].delivering = true;
    chat.stopped = true;
    release_steers(&mut chat, false);
    assert!(!chat.busy());
}

#[test]
fn handoff_window_uses_session_reply_learned_and_provider_order() {
    let mut chat = chat_with("t", &["question", "answer"]);
    chat.provider = Provider::Claude;
    let mut settings = Settings::default();
    settings.model_windows.insert("model".into(), 300_000);
    chat.messages[1].model = Some("other".into());
    chat.messages[1].context_window = Some(400_000);
    assert_eq!(handoff_window(&chat, &settings, "model"), Some(300_000));
    chat.messages[1].model = Some("model".into());
    assert_eq!(handoff_window(&chat, &settings, "model"), Some(400_000));
    chat.handoff_seen = Some(aether_agent::HandoffSeen {
        seen: 1,
        pending: false,
        context_tokens: None,
        context_window: Some(500_000),
    });
    assert_eq!(handoff_window(&chat, &settings, "model"), Some(500_000));
    chat.handoff_seen = None;
    chat.messages[1].context_window = None;
    settings.model_windows.clear();
    assert_eq!(handoff_window(&chat, &settings, "model"), Some(200_000));
    assert_eq!(handoff_window(&chat, &settings, "model[1m]"), Some(1_000_000));
    chat.provider = Provider::Codex;
    assert_eq!(handoff_window(&chat, &settings, "model"), None);
}

#[test]
fn reported_windows_are_learned_for_the_reply_model() {
    let mut settings = Settings::default();
    let mut reply = Message::assistant();
    reply.model = Some("model".into());
    learn_window(&mut settings, &reply, &Update::Window(200_000));
    learn_window(&mut settings, &reply, &Update::Window(300_000));
    assert_eq!(settings.model_windows.get("model"), Some(&300_000));
}

#[test]
fn missed_turn_keeps_earliest_undelivered_message() {
    let mut chat = chat_with("t", &[]);
    chat.provider = Provider::Claude;
    chat.cli_session = "session".into();
    chat.messages = vec![
        Message::user("first"),
        Message::assistant(),
        Message::user("second"),
        Message::assistant(),
    ];
    record_missed_turn(&mut chat, 1, false);
    record_missed_turn(&mut chat, 3, false);
    assert_eq!(chat.handoff_seen.as_ref().unwrap().seen, 0);
}

#[test]
fn accepted_failed_turn_is_not_replayed() {
    let mut chat = chat_with("t", &[]);
    chat.provider = Provider::Claude;
    chat.cli_session = "session".into();
    let mut reply = Message::assistant();
    reply.apply(Update::Error("failed".into()));
    chat.messages = vec![Message::user("hello"), reply];
    record_missed_turn(&mut chat, 1, true);
    assert!(chat.handoff_seen.is_none());
    chat.messages[1].apply(Update::Delta("partial".into()));
    record_missed_turn(&mut chat, 1, false);
    assert_eq!(chat.handoff_seen.unwrap().seen, 0);
}

#[gpui_kit::test]
fn apply_keeps_ask_and_resolution_when_chat_run_advances(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    window
        .update(cx, |app, window, cx| {
            app.chats[0].messages.push(Message::assistant());
            app.chats[0].messages.push(Message::assistant());
            app.chats[0].run = 2;
            let ask = Ask::Approval(aether_core::Approval {
                tool: "Bash".into(),
                input: "pwd".into(),
                ..aether_core::Approval::default()
            });
            app.apply(0, 0, 1, Update::Ask { id: 810, ask }, window, cx);
            assert!(app.request_inputs.contains_key(&810));
            assert_eq!(app.chats[0].messages[0].requests.len(), 1);
            assert_eq!(
                app.chats[0].messages[0].requests[0].status,
                aether_core::RequestStatus::Pending
            );

            app.apply(0, 0, 1, Update::AskResolved(810), window, cx);
            assert!(!app.request_inputs.contains_key(&810));
            assert_eq!(
                app.chats[0].messages[0].requests[0].status,
                aether_core::RequestStatus::Withdrawn
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn non_active_visible_session_syncs_rows_on_ask_and_answer(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    window
        .update(cx, |app, window, cx| {
            let mut second = crate::app::chat_with("second", &[]);
            second.id = 444;
            second.messages.push(Message::assistant());
            app.chats.push(second);
            app.ensure_session_ui(444, window, cx);
            assert_eq!(app.active, 0);

            let ask = Ask::Approval(aether_core::Approval {
                tool: "Bash".into(),
                input: "echo hi".into(),
                ..aether_core::Approval::default()
            });
            let run = app.chats[1].run;
            app.apply(1, 0, run, Update::Ask { id: 840, ask }, window, cx);
            let has_req_row = app
                .in_session(444, cx, |this, _| {
                    this.rows.iter().any(|r| matches!(r.block, aether_ui::Block::Request(0)))
                })
                .unwrap_or(false);
            assert!(has_req_row, "split-pane session rows must include the new ask");
        })
        .unwrap();
}
