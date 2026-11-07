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
