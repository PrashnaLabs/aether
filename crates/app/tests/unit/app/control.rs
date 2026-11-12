use aether_core::AccessMode;
use gpui_kit::TestAppContext;
use serde_json::json;

use super::*;
use crate::app::{Agent, Settings, WorktreeJob, chat_with};
use crate::board::model::Col;

#[test]
fn ages_parse_and_reject_garbage() {
    assert_eq!(parse_age("30m"), Some(1_800_000));
    assert_eq!(parse_age("12h"), Some(43_200_000));
    assert_eq!(parse_age(" 3d "), Some(259_200_000));
    assert_eq!(parse_age("2w"), Some(1_209_600_000));
    for bad in ["", "3", "3x", "-1d", "d", "1.5h"] {
        assert_eq!(parse_age(bad), None, "{bad}");
    }
}

#[test]
fn filters_parse_and_name_bad_fields() {
    let f = parse_filter(
        &json!({"filter": {"ids": [1, "2"], "project": "all", "older_than": "3d", "state": "done", "provider": "pi", "include_pinned": true}}),
        false,
    )
    .unwrap();
    assert_eq!(f.ids, Some(vec![1, 2]));
    assert_eq!(f.project.as_deref(), Some("all"));
    assert_eq!(f.older_than_ms, Some(259_200_000));
    assert_eq!(f.provider, Some(Provider::Pi));
    assert!(f.include_pinned && !f.archived);
    assert!(parse_filter(&json!({}), true).unwrap().archived);
    let err = |args| parse_filter(&args, false).unwrap_err();
    assert!(err(json!({"filter": {"state": "idle"}})).contains("unknown \"state\""));
    assert!(err(json!({"filter": {"provider": "gpt"}})).contains("unknown \"provider\""));
    assert!(err(json!({"filter": {"older_than": "soon"}})).contains("bad \"older_than\""));
    assert!(err(json!({"filter": {"ids": "1"}})).contains("list of chat ids"));
}

#[test]
fn states_follow_the_last_reply() {
    let mut chat = chat_with("c", &["q", "a"]);
    assert_eq!(state_of(&chat, true), "running");
    assert_eq!(state_of(&chat, false), "done");
    chat.messages.last_mut().unwrap().failed = true;
    assert_eq!(state_of(&chat, false), "failed");
    assert_eq!(state_of(&chat_with("e", &[]), false), "empty");
}

fn chat(id: i64, title: &str, dir: &str) -> Chat {
    let mut c = chat_with(title, &["q", "a"]);
    c.id = id;
    c.provider = Provider::Claude;
    c.cwd = dir.into();
    c.project = dir.into();
    for m in &mut c.messages {
        (m.sent_at_ms, m.ended_at_ms) = (None, None);
    }
    c
}

fn app(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<ChatApp> {
    cx.update(gpui_kit::init);
    cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        let mut lead = chat(10, "lead", "/tmp/proj");
        (lead.lead, lead.access, lead.stopped) = (true, AccessMode::AutoEdit, true);
        let mut pinned = chat(21, "pinned", "/tmp/proj");
        pinned.pinned = true;
        let mut busy = chat(23, "busy", "/tmp/proj");
        busy.worktree_job = Some(WorktreeJob::Switching);
        let mut fresh = chat(24, "fresh", "/tmp/proj");
        fresh.turn_at = now_ms();
        app.chats = vec![
            lead,
            chat(20, "old", "/tmp/proj"),
            pinned,
            chat(22, "elsewhere", "/tmp/other"),
            busy,
            fresh,
        ];
        app
    })
}

fn ids(v: &Value) -> Vec<i64> {
    v["chats"].as_array().unwrap().iter().map(|c| c["id"].as_i64().unwrap()).collect()
}

#[gpui_kit::test]
fn list_defaults_to_the_leads_project(cx: &mut TestAppContext) {
    let window = app(cx);
    window
        .update(cx, |app, _, _| {
            let list = |args| app.list_chats(0, &args).unwrap();
            assert_eq!(ids(&list(json!({}))), [10, 20, 21, 23, 24]);
            assert_eq!(ids(&list(json!({"filter": {"project": "all"}}))), [10, 20, 21, 22, 23, 24]);
            assert_eq!(ids(&list(json!({"filter": {"project": "other"}}))), [22]);
            assert_eq!(ids(&list(json!({"filter": {"older_than": "1h"}}))), [10, 20, 21, 23]);
            assert_eq!(ids(&list(json!({"filter": {"state": "running"}}))), [23]);
            assert_eq!(ids(&list(json!({"filter": {"ids": [22]}}))), [22]);
            assert_eq!(ids(&list(json!({"filter": {"search": "OLD"}}))), [20]);
            assert_eq!(ids(&list(json!({"filter": {"search": "el wh", "project": "all"}}))), [22]);
            let row = &list(json!({"filter": {"ids": [23]}}))["chats"][0];
            assert_eq!(
                (row["project"].as_str(), row["state"].as_str()),
                (Some("proj"), Some("running"))
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn bulk_guard_skips_lead_running_and_pinned(cx: &mut TestAppContext) {
    let window = app(cx);
    window
        .update(cx, |app, _, _| {
            let f = parse_filter(&json!({"filter": {"older_than": "1h"}}), false).unwrap();
            let (ok, skipped) = app.guard(0, &f, &app.matching(0, &f));
            assert_eq!(ok, [20]);
            let reasons: Vec<_> = skipped
                .iter()
                .map(|s| (s["id"].as_i64().unwrap(), s["reason"].as_str().unwrap()))
                .collect();
            assert_eq!(reasons, [(10, "it is you"), (21, "pinned"), (23, "running")]);
        })
        .unwrap();
}

#[gpui_kit::test]
fn approved_archive_rechecks_then_tells_the_lead(cx: &mut TestAppContext) {
    let window = app(cx);
    window
        .update(cx, |app, window, cx| {
            app.chats[5].turn_at = 0;
            let f = parse_filter(&json!({"filter": {"older_than": "1h"}}), false).unwrap();
            app.chats[5].pinned = true;
            app.finish_bulk(10, Bulk::Archive, f, vec![20, 24], window, cx);
            let archived: Vec<i64> =
                app.chats.iter().filter(|c| c.archived).map(|c| c.id).collect();
            assert_eq!(archived, [20]);
            let told = &app.chats[0].queue.last().unwrap().text;
            assert!(
                told.starts_with("[Aether] The user approved. Archived 1 chat(s): “old” (20)"),
                "{told}"
            );
            assert!(
                told.contains("changed since you asked (running, pinned or gone): “fresh” (24)")
            );
            assert_eq!(app.chats.len(), 6);
        })
        .unwrap();
}

#[gpui_kit::test]
fn unarchive_update_and_stop(cx: &mut TestAppContext) {
    let window = app(cx);
    window
        .update(cx, |app, window, cx| {
            app.chats[1].archived = true;
            let back = app.unarchive_chats(0, &json!({"filter": {}}), window, cx).unwrap();
            assert_eq!(back["unarchived"], 1);
            assert!(!app.chats[1].archived);
            let done = app
                .update_chat(&json!({"id": 20, "title": " Renamed ", "pinned": true}), cx)
                .unwrap();
            assert_eq!(done, json!({"id": 20, "title": "Renamed", "pinned": true}));
            assert!(
                app.update_chat(&json!({"id": 20}), cx).unwrap_err().contains("nothing to change")
            );
            assert_eq!(
                app.update_chat(&json!({"id": 99, "title": "x"}), cx).unwrap_err(),
                "chat 99 not found"
            );
            assert_eq!(
                app.stop_any_chat(0, &json!({"id": 10}), cx).unwrap_err(),
                "a lead can't stop itself"
            );
            assert_eq!(
                app.stop_any_chat(0, &json!({"id": 20}), cx).unwrap(),
                json!({"stopped": false})
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn nothing_to_archive_answers_without_a_dialog(cx: &mut TestAppContext) {
    let window = app(cx);
    window
        .update(cx, |app, window, cx| {
            let reply = app
                .ask_bulk(0, Bulk::Delete, &json!({"filter": {"ids": [21, 23]}}), window, cx)
                .unwrap();
            assert_eq!(reply["status"], "nothing_to_do");
            assert_eq!(reply["skipped"].as_array().unwrap().len(), 2);
        })
        .unwrap();
}

#[test]
fn card_filters_parse_and_name_bad_fields() {
    let f = parse_card_filter(
        &json!({"filter": {"ids": [3], "col": "Review", "project": "all", "added_by": "agent", "search": " parser "}}),
    )
    .unwrap();
    assert_eq!(f.ids, Some(vec![3]));
    assert_eq!(f.col, Some(Col::Review));
    assert_eq!((f.by_agent, f.search.as_str()), (Some(true), "parser"));
    let err = |args| parse_card_filter(&args).unwrap_err();
    assert!(err(json!({"filter": {"col": "later"}})).contains("unknown \"col\""));
    assert!(err(json!({"filter": {"added_by": "bot"}})).contains("unknown \"added_by\""));
}

#[gpui_kit::test]
fn card_delete_scopes_to_the_lead_and_reports(cx: &mut TestAppContext) {
    let window = app(cx);
    window
        .update(cx, |app, window, cx| {
            let other = app.board_add(
                "Other".into(),
                String::new(),
                Col::Todo,
                "/tmp/other".into(),
                None,
                false,
            );
            let reply =
                app.delete_cards(0, &json!({"filter": {"project": "proj"}}), window, cx).unwrap();
            assert_eq!(reply["status"], "nothing_to_do");
            let mine = app.board_add(
                "Mine".into(),
                String::new(),
                Col::Review,
                "/tmp/proj".into(),
                None,
                true,
            );
            app.board_picks.insert(mine);
            app.board_selected = Some(mine);
            app.finish_card_delete(&[mine, 999], Some(10), window, cx);
            assert_eq!(app.cards.iter().map(|c| c.id).collect::<Vec<_>>(), [other]);
            assert!(app.board_picks.is_empty() && app.board_selected.is_none());
            let told = &app.chats[0].queue.last().unwrap().text;
            assert_eq!(
                told,
                &format!("[Aether] The user approved. Deleted 1 card(s): #{mine} “Mine”")
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn read_history_pages_this_chats_messages_with_tools(cx: &mut TestAppContext) {
    use aether_core::{ToolCall, ToolDetail, Update};
    let window = app(cx);
    window
        .update(cx, |app, _, _| {
            let reply = &mut app.chats[1].messages[1];
            reply.apply(Update::Tool(ToolCall::new(
                "1",
                "T",
                ToolDetail::Read { path: "README.md".into(), range: None },
            )));
            let page = app.read_history(1, &json!({"offset": 1, "limit": 1})).unwrap();
            assert_eq!(page["total"], 2);
            assert_eq!(page["messages"][0]["n"], 1);
            assert_eq!(page["messages"][0]["role"], "assistant");
            assert_eq!(page["messages"][0]["tools"], json!(["- Read README.md"]));
            assert!(page.get("next_offset").is_none());
            let first = app.read_history(1, &json!({"limit": 1, "max_chars": 1})).unwrap();
            assert_eq!(first["next_offset"], 1);
            assert_eq!(first["messages"][0]["text"], "q");
            let last = app.read_history(1, &json!({"offset": -1})).unwrap();
            assert_eq!(last["messages"][0]["n"], 1);
            let all = app.read_history(1, &json!({"offset": -50})).unwrap();
            assert_eq!(all["messages"].as_array().unwrap().len(), 2);
            let other = app.read_history(0, &json!({"id": 22, "offset": -1})).unwrap();
            assert_eq!(other["messages"][0]["text"], "a");
            assert_eq!(
                app.read_history(1, &json!({"id": 22})),
                Err("only a lead can read another chat's history".into())
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn read_history_pages_and_reads_items(cx: &mut TestAppContext) {
    use aether_core::{ToolCall, ToolDetail, Update};
    let window = app(cx);
    window
        .update(cx, |app, _, _| {
            app.chats[1].messages[1].apply(Update::Tool(ToolCall::new(
                "1",
                "T",
                ToolDetail::Command {
                    command: "echo test".into(),
                    cwd: None,
                    output: "界".repeat(600),
                    exit_code: Some(0),
                },
            )));
            let first = app.read_history(1, &json!({"view": "items", "limit": 1})).unwrap();
            assert_eq!(first["items"][0]["item"], "0");
            assert_eq!(first["next_position"], 0);
            let next = app
                .read_history(
                    1,
                    &json!({"view": "items", "after_position": first["next_position"], "limit": 1}),
                )
                .unwrap();
            assert_eq!(next["items"][0]["position"], 1);
            assert_eq!(next["items"][0]["item"], "1.0");
            assert_eq!(next["items"][0]["kind"], "command_execution");
            assert_eq!(next["next_position"], 1);
            let last = app.read_history(1, &json!({"view": "items", "after_position": 1})).unwrap();
            assert_eq!(last["items"][0]["item"], "1");
            assert!(last.get("next_position").is_none());
            let one = app.read_history(1, &json!({"item": "1.0", "max_chars": 500})).unwrap();
            assert_eq!(one["item"], "1.0");
            assert_eq!(one["next_text_offset"], 500);
            let rest = app
                .read_history(1, &json!({"view": "items", "item": "1.0", "text_offset": 500}))
                .unwrap();
            assert_eq!(
                format!("{}{}", one["text"].as_str().unwrap(), rest["text"].as_str().unwrap()),
                format!("Command: echo test\nExit code: 0\n{}", "界".repeat(600))
            );
            assert_eq!(
                app.read_history(1, &json!({"item": "missing"})),
                Err("no item missing".into())
            );
            assert_eq!(
                app.read_history(0, &json!({"id": 20, "item": "1.0"})).unwrap()["item"],
                "1.0"
            );
            assert!(app.read_history(1, &json!({"id": 22, "view": "items"})).is_err());
        })
        .unwrap();
}

#[gpui_kit::test]
fn read_history_pages_long_message_text(cx: &mut TestAppContext) {
    use aether_core::{ToolCall, ToolDetail, Update};
    let window = app(cx);
    window
        .update(cx, |app, _, _| {
            let text = "abcdefghij".repeat(75);
            let reply = &mut app.chats[1].messages[1];
            reply.text = text.clone();
            reply.error = Some("failed\nmore detail".into());
            reply.apply(Update::Tool(ToolCall::new(
                "1",
                "T",
                ToolDetail::Read { path: "README.md".into(), range: None },
            )));
            let first =
                app.read_history(1, &json!({"offset": 1, "limit": 1, "max_chars": 500})).unwrap();
            let first = &first["messages"][0];
            assert_eq!(first["next_text_offset"], 500);
            assert_eq!(first["tools"], json!(["- Read README.md"]));
            assert_eq!(first["error"], "failed");
            let last = app
                .read_history(
                    1,
                    &json!({"offset": 1, "limit": 1, "max_chars": 500,
                    "text_offset": first["next_text_offset"]}),
                )
                .unwrap();
            let last = &last["messages"][0];
            assert!(last.get("next_text_offset").is_none());
            assert!(last.get("tools").is_none());
            assert!(last.get("error").is_none());
            assert_eq!(
                format!("{}{}", first["text"].as_str().unwrap(), last["text"].as_str().unwrap()),
                text
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn read_history_slices_multibyte_text_by_character(cx: &mut TestAppContext) {
    let window = app(cx);
    window
        .update(cx, |app, _, _| {
            app.chats[1].messages[1].text = format!("é{}終", "🦀".repeat(500));
            let first = app
                .read_history(
                    1,
                    &json!({"offset": 1, "limit": 1, "max_chars": 500, "text_offset": 1}),
                )
                .unwrap();
            assert_eq!(first["messages"][0]["text"], "🦀".repeat(500));
            assert_eq!(first["messages"][0]["next_text_offset"], 501);
            let last =
                app.read_history(1, &json!({"offset": 1, "limit": 1, "text_offset": 501})).unwrap();
            assert_eq!(last["messages"][0]["text"], "終");
            assert!(last["messages"][0].get("next_text_offset").is_none());
        })
        .unwrap();
}

#[gpui_kit::test]
fn non_leads_get_read_history_only(cx: &mut TestAppContext) {
    let window = app(cx);
    window
        .update(cx, |app, window, cx| {
            let mut call = |lead: &str, tool: &str| {
                let (reply, answer) = std::sync::mpsc::channel();
                let call = aether_agent::orchestrator::OrchCall {
                    deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
                    lead: lead.into(),
                    tool: tool.into(),
                    args: json!({}),
                    reply,
                };
                app.orchestrate(call, window, cx);
                answer.try_recv().unwrap()
            };
            let names = |v: Value| {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s["name"].as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                names(call("20", "list_tools").unwrap()),
                ["list_cards", "add_card", "read_card", "move_card", "update_card", "read_history"]
            );
            assert!(names(call("10", "list_tools").unwrap()).len() > 1);
            assert!(call("20", "read_history").is_ok());
            assert_eq!(call("20", "list_chats").unwrap_err(), "lead chat not found");
        })
        .unwrap();
}
