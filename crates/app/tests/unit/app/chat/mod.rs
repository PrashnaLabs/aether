use super::*;
use crate::app::chat_with;

#[gpui_kit::test]
fn live_status_stays_out_of_the_scroller_but_finished_footers_remain(
    cx: &mut gpui_kit::TestAppContext,
) {
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
            let chat = &mut app.chats[app.active];
            chat.messages = vec![Message::user("hello"), Message::assistant()];
            chat.messages[1].text = "Working on it".into();
            chat.cancel = Some(aether_agent::Cancel::inert());
            let rows = app.build_rows(0, cx);
            assert!(rows.iter().any(|row| matches!(row.block, Block::Text(_))));
            assert!(!rows.iter().any(|row| matches!(row.block, Block::Live | Block::Footer)));
            app.chats[app.active].cancel = None;
            let rows = app.build_rows(0, cx);
            assert!(rows.iter().any(|row| row.block == Block::Footer));
        })
        .unwrap();
}

#[test]
fn new_chat_access_defaults_follow_the_provider() {
    assert_eq!(default_access(Provider::Pi), AccessMode::ReadOnly);
    assert_eq!(default_access(Provider::Claude), AccessMode::Ask);
    assert_eq!(fit_access(Provider::Pi, AccessMode::Ask), AccessMode::ReadOnly);
    assert_eq!(fit_access(Provider::Claude, AccessMode::Full), AccessMode::Full);
}

#[test]
fn drafts_are_stashed_by_chat_id_not_index() {
    let mut chats = vec![chat_with("a", &[]), chat_with("b", &[])];
    chats[0].id = 1;
    chats[1].id = 2;
    assert!(stash_draft(&mut chats, Some(2), "half typed".into()));
    chats.remove(0);
    assert_eq!(chats[0].draft, "half typed");
    assert!(!stash_draft(&mut chats, Some(1), "gone".into()));
    assert!(!stash_draft(&mut chats, None, "x".into()));
}

#[test]
fn history_names_earlier_attachments() {
    let file = |name: &str| aether_core::Attachment {
        name: name.into(),
        path: format!("/x/{name}").into(),
        mime: "image/png".into(),
        size: 1,
    };
    let mut m = Message::user("see this");
    m.attachments = vec![file("a.png")];
    let mut bare = Message::user("");
    bare.attachments = vec![file("a.png"), file("b.pdf")];
    let h = history(&[m, bare, Message::user("")]);
    assert_eq!(h.len(), 2);
    assert_eq!(h[0].1, "see this\n[attached: a.png]");
    assert_eq!(h[1].1, "[attached: a.png, b.pdf]");
}

#[test]
fn a_sent_prompt_is_cleared_from_where_it_waits() {
    assert_eq!(sent_copy(true, " fix it ", "", "fix it"), Some(SentCopy::Composer));
    assert_eq!(sent_copy(true, "something new", "fix it", "fix it"), None);
    assert_eq!(sent_copy(false, "fix it", "fix it\n", "fix it"), Some(SentCopy::Draft));
    assert_eq!(sent_copy(false, "fix it", "fix it, then more", "fix it"), None);
}

#[test]
fn a_recovered_file_leaves_the_missing_set() {
    let mut missing: HashSet<PathBuf> = ["/a".into(), "/b".into()].into();
    merge_missing(
        &mut missing,
        vec![(true, "/a".into()), (false, "/c".into()), (false, "/b".into())],
    );
    let want: HashSet<PathBuf> = ["/b".into(), "/c".into()].into();
    assert_eq!(missing, want);
}

#[gpui_kit::test]
fn activity_expansion_is_scoped_to_its_message_when_rows_rebuild(
    cx: &mut gpui_kit::TestAppContext,
) {
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
            reply.steps = vec![Step::Skill("design".into()), Step::Skill("review".into())];
            reply.text = "Answer".into();
            reply.step_positions = vec![0, 0];
            let chat = &mut app.chats[app.active];
            chat.messages = vec![Message::user("hello"), reply.clone(), reply];
            let key = aether_ui::tool_key(&chat.session, "1-activity-0");
            aether_ui::open_tool(cx, key);
            let rows = app.build_rows(0, cx);
            assert!(
                rows.iter().any(|r| r.message == 1 && matches!(r.block, Block::Activity { .. }))
            );
            assert!(
                !rows.iter().any(|r| r.message == 2 && matches!(r.block, Block::Activity { .. }))
            );
            assert_eq!(
                app.build_rows(1, cx),
                rows.into_iter().filter(|r| r.message >= 1).collect::<Vec<_>>()
            );
        })
        .unwrap();
}

#[test]
fn updates_find_tools_inside_activity_groups() {
    let steps = vec![Step::Tool(aether_core::ToolCall::new(
        "c",
        "Run",
        aether_core::ToolDetail::Plain("test".into()),
    ))];
    let block =
        Block::Activity { kind: aether_ui::ActivityKind::Tools, steps: vec![Block::Tool(0)] };
    assert!(shows_tool(&steps, &block, "c"));
    assert!(!shows_tool(&steps, &block, "other"));
}
