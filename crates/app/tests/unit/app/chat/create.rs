use aether_core::{Ask, Request, Step, ToolCall, ToolDetail, ToolStatus, Update};

use crate::app::chat_with;

use super::*;

#[test]
fn new_chats_start_in_the_current_or_latest_project() {
    let cur = Some(PathBuf::from("/w/a"));
    let rec = Some(PathBuf::from("/w/b"));
    assert_eq!(new_chat_folder(true, cur.clone(), rec.clone()), cur);
    assert_eq!(new_chat_folder(true, None, rec.clone()), rec);
    assert_eq!(new_chat_folder(false, cur, rec), None);
    assert_eq!(new_chat_folder(true, None, None), None);
}

#[test]
fn restore_interrupts_live_rows_except_opencode() {
    let running = || {
        let mut m = Message::assistant();
        m.apply(Update::Tool(ToolCall::new("t", "Run", ToolDetail::Plain(String::new()))));
        m
    };
    let status = |ms: &[Message]| match &ms[0].steps[0] {
        Step::Tool(c) => (c.status, c.error.clone()),
        _ => panic!(),
    };
    let mut claude = vec![running()];
    restore_messages(Provider::Claude, &mut claude);
    assert_eq!(status(&claude), (ToolStatus::Stopped, Some("Interrupted: Aether closed".into())));
    let mut opencode = vec![running()];
    restore_messages(Provider::OpenCode, &mut opencode);
    assert_eq!(status(&opencode), (ToolStatus::Running, None));
}

#[test]
fn only_unfinished_opencode_replies_reattach() {
    let chat = |provider, finished| {
        let mut chat = chat_with("t", &["hi"]);
        chat.provider = provider;
        chat.cli_session = "ses_1".into();
        let mut reply = Message::assistant();
        if finished {
            reply.finish();
        }
        chat.messages.push(reply);
        chat
    };
    assert!(reattach_wanted(&chat(Provider::OpenCode, false)));
    assert!(!reattach_wanted(&chat(Provider::OpenCode, true)));
    assert!(!reattach_wanted(&chat(Provider::Claude, false)));
    let mut unsaved = chat(Provider::OpenCode, false);
    unsaved.cli_session.clear();
    assert!(!reattach_wanted(&unsaved));
    let asking = |status| {
        let mut chat = chat(Provider::OpenCode, true);
        let ask = Ask::Question { question: "q".into(), choices: vec![] };
        chat.messages[1].requests.push(Request { id: 1, at: 0, asked_ms: None, ask, status });
        chat
    };
    assert!(reattach_wanted(&asking(RequestStatus::Pending)));
    assert!(!reattach_wanted(&asking(RequestStatus::Expired)));
}

#[test]
fn a_reattach_hands_over_the_last_replys_background_rows() {
    let row = |id: &str, status| {
        Update::Tool(ToolCall {
            status,
            ..ToolCall::new(id, "Task", ToolDetail::Plain(String::new()))
        })
    };
    let mut chat = chat_with("t", &["hi"]);
    let mut earlier = Message::assistant();
    earlier.apply(row("bg0", ToolStatus::Background));
    let mut last = Message::assistant();
    for (id, status) in
        [("bg1", ToolStatus::Background), ("r1", ToolStatus::Running), ("d1", ToolStatus::Done)]
    {
        last.apply(row(id, status));
    }
    chat.messages.extend([earlier, Message::user("again"), last]);
    assert_eq!(restored_background(&chat), ["bg1"]);
}

#[test]
fn a_restored_reply_finishes_without_a_made_up_duration() {
    let mut messages = vec![Message::assistant()];
    restore_messages(Provider::OpenCode, &mut messages);
    messages[0].finish();
    assert_eq!(messages[0].elapsed_ms, None);
    assert!(messages[0].ended_at_ms.is_some());
}

#[test]
fn a_restored_reply_with_background_rows_reattaches_to_settle_them() {
    let mut chat = chat_with("t", &["hi", "done"]);
    chat.provider = Provider::OpenCode;
    chat.cli_session = "ses_1".into();
    chat.messages[1].finish();
    assert!(!reattach_wanted(&chat));
    let mut call = ToolCall::new("bg", "dev", ToolDetail::default());
    call.status = ToolStatus::Background;
    chat.messages[1].apply(Update::Tool(call));
    assert!(reattach_wanted(&chat));
}

#[gpui_kit::test]
fn new_chat_resets_to_agent_tab_for_fresh_and_reused_drafts(cx: &mut gpui_kit::TestAppContext) {
    use crate::app::{ChatViewMode, Page};
    use crate::side_panel::SideTab;
    use aether_agent::Agent;
    use aether_core::Settings;
    use gpui_kit::AppContext;

    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let (window, app) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| {
                ChatApp::with_state(
                    Agent::new().unwrap(),
                    Settings::default(),
                    None,
                    None,
                    false,
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.settings.provider = Provider::Codex;
            for (index, tab) in
                [SideTab::Explorer, SideTab::Terminal, SideTab::Browser].into_iter().enumerate()
            {
                for reuse in [false, true] {
                    for chat in &mut app.chats {
                        chat.draft = "Keep this draft".into();
                    }
                    let reused_id = -(index as i64) - 1;
                    if reuse {
                        let mut draft = chat_with("Unused draft", &[]);
                        draft.id = reused_id;
                        draft.provider = Provider::Codex;
                        draft.view = ChatViewMode::Explorer;
                        app.chats.push(draft);
                    }
                    let count = app.chats.len();
                    app.side_tab = tab;
                    app.new_chat_in_current_project(window, cx);
                    assert_eq!(
                        app.side_tab,
                        SideTab::Agent,
                        "starting from {tab:?}, reuse={reuse}"
                    );
                    assert_eq!(app.page, Page::Chat);
                    assert_eq!(app.chats[app.active].view, ChatViewMode::Chat);
                    assert_eq!(app.chats.len(), count + usize::from(!reuse));
                    if reuse {
                        assert_eq!(app.chats[app.active].id, reused_id);
                    }
                }
            }
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn switching_chats_always_lands_on_the_chat_tab(cx: &mut gpui_kit::TestAppContext) {
    use crate::app::ChatViewMode;
    use crate::side_panel::SideTab;
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
        let mut other = chat_with("Other", &["hi", "hello"]);
        other.id = 9_001;
        other.view = ChatViewMode::Explorer;
        app.chats.push(other);
        app
    });
    window
        .update(cx, |app, window, cx| {
            for tab in [SideTab::Terminal, SideTab::Explorer, SideTab::Browser] {
                app.side_tab = tab;
                let other =
                    app.chats.iter().position(|c| c.id != app.chats[app.active].id).unwrap();
                app.select(other, window, cx);
                assert_eq!(app.side_tab, SideTab::Agent, "from {tab:?}");
                assert_eq!(app.chats[app.active].view, ChatViewMode::Chat);
            }
        })
        .unwrap();
}

#[gpui_kit::test]
fn handoff_copies_the_transcript_to_a_fresh_chat_on_the_target(cx: &mut gpui_kit::TestAppContext) {
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
            let mut source = chat_with("Fix bug", &["hi", "done"]);
            source.provider = Provider::Claude;
            source.access = aether_core::AccessMode::Full;
            source.effort = "high".into();
            source.cli_session = "claude-ses".into();
            source.cwd = "/w/a".into();
            source.project = "/w/a".into();
            source.worktree_path = "/w/a-wt".into();
            source.worktree_branch = "fix".into();
            app.chats = vec![source];
            app.active = 0;
            app.handoff_chat(0, Provider::Codex, "gpt-x".into(), window, cx);
            assert_eq!(app.chats.len(), 2);
            let (source, new) = (&app.chats[0], &app.chats[1]);
            assert_eq!(
                (source.provider, source.cli_session.as_str()),
                (Provider::Claude, "claude-ses")
            );
            assert_eq!(source.title, "Fix bug");
            assert_eq!(new.provider, Provider::Codex);
            assert_eq!(new.provider_model, "gpt-x");
            assert!(new.cli_session.is_empty() && new.effort.is_empty());
            assert_eq!(new.title, "Fix bug \u{2192} Codex");
            assert_eq!(new.messages.len(), 2);
            assert_eq!(new.messages[0].text, "hi");
            assert_eq!(
                (&new.cwd, &new.project, &new.worktree_path, &new.worktree_branch),
                (&source.cwd, &source.project, &source.worktree_path, &source.worktree_branch)
            );
            assert!(aether_core::AccessMode::for_provider(Provider::Codex).contains(&new.access));
            assert_ne!(new.id, source.id);
            assert_eq!(app.active, 1);
            assert!(new.draft.starts_with("Continue where the previous agent left off."));
            assert_eq!(app.composer.read(cx).value().as_ref(), new.draft);
        })
        .unwrap();
}

#[gpui_kit::test]
fn handoff_finishes_a_running_reply_and_expires_its_requests(cx: &mut gpui_kit::TestAppContext) {
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
            let mut source = chat_with("Busy", &["hi"]);
            source.cwd = "/w/a".into();
            let mut running = aether_core::Message::assistant();
            running.apply(aether_core::Update::Ask { id: 1, ask: aether_core::Ask::Unknown });
            source.messages.push(running);
            app.chats = vec![source];
            app.active = 0;
            app.handoff_chat(0, Provider::Codex, "gpt-x".into(), window, cx);
            let last = app.chats[1].messages.last().unwrap();
            assert!(last.ended_at_ms.is_some());
            assert!(last.pending_request().is_none());
            assert!(app.chats[0].messages.last().unwrap().pending_request().is_some());
        })
        .unwrap();
}

fn reply(updates: Vec<Update>) -> Message {
    let mut m = Message::assistant();
    for u in updates {
        m.apply(u);
    }
    m
}

fn rate_limit(state: aether_core::NoticeState) -> Update {
    let mut n = aether_core::Notice::new("rl", aether_core::NoticeKind::RateLimit, "slow down");
    n.state = state;
    Update::Notice(n)
}

#[test]
fn limit_hit_reads_failed_rate_limit_notices_errors_and_warnings() {
    use aether_core::NoticeState;
    assert!(limit_hit(&reply(vec![rate_limit(NoticeState::Failed)])));
    assert!(!limit_hit(&reply(vec![rate_limit(NoticeState::Done)])));
    assert!(!limit_hit(&reply(vec![Update::Final("done".into())])));
    for why in [
        "You've hit your USAGE LIMIT",
        "rate_limit_error",
        "Quota exceeded",
        "exceeds the context window",
        "context length exceeded",
        "maximum context reached",
        "too many tokens",
        "HTTP 429",
        "error_max_turns: max_turns",
        "error_max_budget_usd: max_budget",
    ] {
        assert!(limit_hit(&reply(vec![Update::Error(why.into())])), "{why}");
    }
    assert!(!limit_hit(&reply(vec![Update::Error("spawn failed".into())])));
    assert!(!limit_hit(&reply(vec![Update::Error("build failed at line 4290".into())])));
    assert!(limit_hit(&reply(vec![Update::Error("429 Too Many Requests".into())])));
    assert!(limit_hit(&reply(vec![Update::Warning(
        "The reply hit the model's output limit".into()
    )])));
    assert!(!limit_hit(&reply(vec![Update::Warning("using Pi's current model".into())])));
    assert!(!limit_hit(&Message::user("what is a rate limit")));
}

#[gpui_kit::test]
fn limit_banner_shows_for_an_idle_chat_whose_last_reply_hit_a_limit(
    cx: &mut gpui_kit::TestAppContext,
) {
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
        let chat = &mut app.chats[app.active];
        chat.provider = Provider::Claude;
        chat.messages =
            vec![Message::user("hi"), reply(vec![rate_limit(aether_core::NoticeState::Failed)])];
        app.probes.insert(
            Provider::Codex,
            crate::app::ProviderProbe { installed: true, ..Default::default() },
        );
        app
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("composer-handoff-limit").is_some());
    window
        .update(&mut visual, |app, _, _| {
            let chat = &mut app.chats[app.active];
            chat.messages.pop();
            chat.messages.push(reply(vec![Update::Final("fine".into())]));
        })
        .unwrap();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("composer-handoff-limit").is_none());
}
