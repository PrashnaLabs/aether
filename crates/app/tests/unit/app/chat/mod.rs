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

#[test]
fn seed_history_keeps_one_entry_per_message() {
    use aether_core::{ToolCall, ToolDetail};
    let mut chat = chat_with("c", &["ask", "", "again", "done"]);
    chat.messages[1].apply(Update::Tool(ToolCall::new(
        "1",
        "T",
        ToolDetail::Read { path: "README.md".into(), range: None },
    )));
    let seeded = seed_history(&chat.messages);
    let texts: Vec<_> = seeded.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(texts, ["ask", "(tool calls only)", "again", "done"]);
    assert_eq!(history(&chat.messages).len(), 3, "OpenRouter history still skips empty replies");
}

#[test]
fn tool_records_show_what_ran_with_output_tails() {
    use aether_core::{ToolCall, ToolDetail, ToolStatus};
    let cmd = ToolCall::new(
        "2",
        "T",
        ToolDetail::Command {
            command: "ls api\n&& head -30 mcp_server.py".into(),
            cwd: None,
            output: format!("{}\nfrom fastmcp import FastMCP", "x".repeat(2_000)),
            exit_code: Some(0),
        },
    );
    let text = tool_record(&cmd, 600).unwrap();
    assert!(
        text.starts_with("- Ran `ls api && head -30 mcp_server.py` → exit 0\n  output:\n    …"),
        "{text}"
    );
    assert!(text.ends_with("    from fastmcp import FastMCP") && text.len() < 700, "{text}");
    let mut failed = ToolCall::new(
        "4",
        "T",
        ToolDetail::Search {
            pattern: "Product".into(),
            path: Some("api".into()),
            matches: Some(12),
            output: String::new(),
        },
    );
    (failed.status, failed.error) = (ToolStatus::Failed, Some("rg missing\nmore".into()));
    assert_eq!(
        tool_record(&failed, 600).unwrap(),
        "- Searched `Product` in api → 12 matches (failed: rg missing)"
    );
    let read = ToolCall::new("1", "T", ToolDetail::Read { path: "README.md".into(), range: None });
    assert_eq!(tool_record(&read, 600).unwrap(), "- Read README.md");
}

fn stream_app(window: &mut Window, cx: &mut Context<ChatApp>) -> ChatApp {
    ChatApp::with_state(
        aether_agent::Agent::new().unwrap(),
        aether_core::Settings::default(),
        None,
        None,
        false,
        window,
        cx,
    )
}

fn live_reply(app: &mut ChatApp, updates: Vec<Update>, cx: &mut App) {
    let chat = &mut app.chats[app.active];
    chat.messages = vec![Message::user("go"), Message::assistant()];
    for update in updates {
        chat.messages[1].apply(update);
    }
    chat.cancel = Some(aether_agent::Cancel::inert());
    app.rows = app.build_rows(0, cx);
}

fn stream(app: &mut ChatApp, update: Update) -> Changed {
    let reply = &mut app.chats[app.active].messages[1];
    let changed = Changed::of(&update, reply);
    reply.apply(update);
    changed
}

fn run(id: &str) -> Update {
    Update::Tool(aether_core::ToolCall::new(id, "Run", aether_core::ToolDetail::Plain("x".into())))
}

#[gpui_kit::test]
fn a_text_delta_on_the_last_block_extends_its_row_without_a_rebuild(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(stream_app);
    window
        .update(cx, |app, _, cx| {
            let start = vec![
                Update::Delta("Looking".into()),
                run("a"),
                Update::Delta(" closer\n".into()),
                run("b"),
                run("c"),
                Update::Delta("Found ".into()),
            ];
            live_reply(app, start, cx);
            let before = app.rows.len();
            let changed = stream(app, Update::Delta("it".into()));
            let resized = app.extend_tail(1, &changed).expect("fast path");
            assert_eq!(resized, vec![before - 1..before]);
            assert_eq!(app.rows, app.build_rows(0, cx));
            let changed = stream(app, Update::Delta(" at last\n".into()));
            app.sync(Some((1, changed)), cx);
            assert_eq!(app.rows, app.build_rows(0, cx));
        })
        .unwrap();
}

#[gpui_kit::test]
fn structural_deltas_and_unsure_cases_still_rebuild(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(stream_app);
    window
        .update(cx, |app, _, cx| {
            let full = |app: &mut ChatApp, update, cx: &mut App| {
                let changed = stream(app, update);
                assert!(app.extend_tail(1, &changed).is_none());
                app.rows = app.build_rows(0, cx);
            };
            live_reply(app, vec![Update::Delta("a".into()), run("a")], cx);
            full(app, Update::Delta("b".into()), cx);
            live_reply(
                app,
                vec![Update::Delta("`".into()), run("a"), Update::Delta("`".into())],
                cx,
            );
            full(app, Update::Delta("`".into()), cx);
            live_reply(app, vec![run("a")], cx);
            full(app, Update::Delta("first text".into()), cx);
            live_reply(
                app,
                vec![Update::Delta("a".into()), run("a"), Update::Delta("b".into())],
                cx,
            );
            full(app, run("b"), cx);
            live_reply(
                app,
                vec![Update::Delta("a".into()), run("a"), Update::Delta("b".into())],
                cx,
            );
            app.chats[app.active].cancel = None;
            full(app, Update::Delta("c".into()), cx);
        })
        .unwrap();
}

#[gpui_kit::test]
fn growing_the_open_thought_keeps_the_rows_and_a_new_one_rebuilds(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(stream_app);
    window
        .update(cx, |app, _, cx| {
            live_reply(app, vec![run("a"), Update::Thinking("hm".into())], cx);
            let changed = stream(app, Update::Thinking(" more".into()));
            assert_eq!(app.extend_tail(1, &changed), Some(Vec::new()));
            assert_eq!(app.rows, app.build_rows(0, cx));
            let changed = stream(app, run("b"));
            assert!(app.extend_tail(1, &changed).is_none());
            let changed = stream(app, Update::Thinking("again".into()));
            assert!(app.extend_tail(1, &changed).is_none());
        })
        .unwrap();
}

#[test]
fn crew_options_offer_all_and_finished_without_lead_alone() {
    use super::manage::crew_options;
    let labels = |o: &[(String, Vec<i64>)]| o.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>();
    let all = crew_options(1, Some(&[2, 3]), &[2]);
    assert_eq!(
        labels(&all),
        [
            "This chat and all its 2 agents",
            "Only its 1 finished agent (done or failed); this chat stays"
        ]
    );
    assert_eq!(all[0].1, [2, 3, 1]);
    assert_eq!(all[1].1, [2]);
    assert!(all.iter().all(|(_, ids)| ids != &[1]));
    assert_eq!(crew_options(1, Some(&[2]), &[])[0].1, [2, 1]);
    let busy = crew_options(1, None, &[2]);
    assert_eq!(busy.len(), 1, "with agents running, only the finished ones can go");
    assert_eq!(crew_options(1, Some(&[]), &[]), [("This chat".into(), vec![1])]);
}
