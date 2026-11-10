use aether_agent::orchestrator::OrchCall;
use aether_core::ToolDetail;
use aether_driver::CliModel;
use aether_driver::antigravity::AgyEntry;
use gpui_kit::TestAppContext;
use serde_json::{Value, json};

use super::*;
use crate::app::orchestrate::{
    SpawnArgs, child_access, child_prompt, child_status, clip, delivery_text,
    needs_agy_registration, spawned_child, stranded_texts, unread,
};
use crate::board::model::Col;

#[gpui_kit::test]
fn toggle_lead_flips_and_persists_flag(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        app.chats.push(chat_with("a", &[]));
        app
    });
    window
        .update(cx, |app, window, cx| {
            let ix = app.chats.len() - 1;
            app.toggle_lead(ix, window, cx);
            assert!(app.chats[ix].lead);
            assert!(app.cli_request(ix).1.lead);
            app.toggle_lead(ix, window, cx);
            assert!(!app.chats[ix].lead);
            assert!(!app.cli_request(ix).1.lead);
        })
        .unwrap();
}

#[test]
fn antigravity_lead_needs_a_live_entry_for_this_exe() {
    let exe = std::path::Path::new("/x/aether");
    let entry = |command: &str, disabled| Some(AgyEntry { command: command.into(), disabled });
    let ag = Provider::Antigravity;
    assert!(needs_agy_registration(ag, None, Some(exe)));
    assert!(!needs_agy_registration(ag, entry("/x/aether", false).as_ref(), Some(exe)));
    assert!(needs_agy_registration(ag, entry("/x/aether", true).as_ref(), Some(exe)));
    assert!(needs_agy_registration(ag, entry("/old/aether", false).as_ref(), Some(exe)));
    assert!(needs_agy_registration(ag, entry("/x/aether", false).as_ref(), None));
    for p in [Provider::Claude, Provider::Codex, Provider::Pi, Provider::OpenCode] {
        assert!(!needs_agy_registration(p, None, Some(exe)));
    }
}

fn codex_probes(models: &[&str]) -> HashMap<Provider, ProviderProbe> {
    let models = models
        .iter()
        .map(|id| CliModel {
            id: (*id).into(),
            efforts: Some(vec!["low".into(), "high".into()]),
            ..CliModel::default()
        })
        .collect();
    HashMap::from([
        (Provider::Codex, ProviderProbe { installed: true, models, ..ProviderProbe::default() }),
        (Provider::Pi, ProviderProbe::default()),
    ])
}

fn spawn_args(provider: &str, model: &str) -> Value {
    json!({"provider": provider, "model": model, "title": "Fix tests", "task": "make it green"})
}

fn lead_chat(id: i64) -> Chat {
    let mut lead = chat_with("lead", &[]);
    lead.id = id;
    lead.provider = Provider::Claude;
    lead.lead = true;
    lead.cwd = "/tmp/proj".into();
    lead.project = "/tmp/proj".into();
    lead.access = AccessMode::AutoEdit;
    lead
}

fn lead_app(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<ChatApp> {
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
        app.chats = vec![chat_with("idle", &[]), lead_chat(10)];
        app.active = 0;
        app.probes = codex_probes(&["gpt-5-mini", "gpt-5"]);
        app
    })
}

#[test]
fn parse_rejects_missing_task() {
    let mut args = spawn_args("codex", "gpt-5");
    args.as_object_mut().unwrap().remove("task");
    let err = SpawnArgs::parse(&args, &codex_probes(&["gpt-5"])).unwrap_err();
    assert!(err.contains("task"), "{err}");
}

#[test]
fn parse_rejects_unknown_provider() {
    for provider in ["openrouter", "gpt"] {
        let err = SpawnArgs::parse(&spawn_args(provider, "x"), &codex_probes(&[])).unwrap_err();
        assert!(err.contains("claude, codex, pi, opencode, antigravity"), "{err}");
    }
}

#[test]
fn parse_rejects_model_not_in_probe() {
    let err = SpawnArgs::parse(&spawn_args("codex", "x"), &codex_probes(&["gpt-5"])).unwrap_err();
    assert_eq!(err, r#"unknown model "x" for codex; call the list_agents tool"#);
}

#[test]
fn parse_accepts_any_model_when_probe_lists_none() {
    let a = SpawnArgs::parse(&spawn_args("codex", "anything"), &codex_probes(&[])).unwrap();
    assert_eq!(a.provider, Provider::Codex);
    assert_eq!(a.model, "anything");
    assert_eq!(a.title, "Fix tests");
    assert_eq!(a.task, "make it green");
    assert_eq!(a.effort, "");
}

#[test]
fn parse_rejects_uninstalled_provider() {
    for provider in ["pi", "claude"] {
        let err = SpawnArgs::parse(&spawn_args(provider, "x"), &codex_probes(&[])).unwrap_err();
        assert!(err.contains("not installed"), "{err}");
    }
}

#[test]
fn child_prompt_asks_for_a_summary() {
    assert_eq!(
        child_prompt("do it"),
        "do it\n\nWhen you finish, end your reply with a short summary of what you did and which files you changed."
    );
}

#[gpui_kit::test]
fn create_child_links_parent_and_keeps_focus(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let mut args = spawn_args("codex", "gpt-5-mini");
            args["effort"] = "high".into();
            let a = SpawnArgs::parse(&args, &app.probes).unwrap();
            let ix = app.create_child(1, &a).unwrap();
            assert_eq!(app.active, 0);
            assert_eq!(app.chats.len(), 3);
            let child = &app.chats[ix];
            assert_eq!(child.parent, Some(10));
            assert!(!child.lead);
            assert_eq!(child.provider, Provider::Codex);
            assert_eq!(child.provider_model, "gpt-5-mini");
            assert_eq!(child.effort, "high");
            assert_eq!(child.cwd, "/tmp/proj");
            assert_eq!(child.project, "/tmp/proj");
            assert_eq!(child.title.as_ref(), "Fix tests");
            assert_eq!(child.access, AccessMode::AutoEdit);
            assert!(child.messages.is_empty());
            assert_ne!(child.id, 10);
        })
        .unwrap();
}

#[test]
fn child_access_never_exceeds_lead() {
    assert_eq!(child_access(AccessMode::AutoEdit, Provider::Pi), Ok(AccessMode::ReadOnly));
    assert_eq!(child_access(AccessMode::AutoEdit, Provider::Claude), Ok(AccessMode::AutoEdit));
    assert_eq!(
        child_access(AccessMode::Ask, Provider::Antigravity),
        Err("antigravity can't run as restricted as this lead (Ask)".into())
    );
    assert_eq!(child_access(AccessMode::Full, Provider::Codex), Ok(AccessMode::Full));
}

#[gpui_kit::test]
fn list_agents_shows_installed_clis_and_own_children_only(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            app.chats.push(lead_chat(20));
            let a = SpawnArgs::parse(&spawn_args("codex", "gpt-5"), &app.probes).unwrap();
            let mine = app.create_child(1, &a).unwrap();
            let theirs = app.create_child(2, &a).unwrap();
            let listed = app.list_agents(1);
            assert_eq!(
                listed["agents"],
                json!([{
                    "provider": "codex",
                    "label": "Codex",
                    "models": [
                        {"id": "gpt-5-mini", "efforts": ["low", "high"]},
                        {"id": "gpt-5", "efforts": ["low", "high"]},
                    ],
                }])
            );
            let children = listed["children"].as_array().unwrap();
            assert_eq!(children.len(), 1);
            assert_eq!(children[0]["id"], app.chats[mine].id);
            assert_ne!(children[0]["id"], app.chats[theirs].id);
            assert_eq!(children[0]["title"], "Fix tests");
            assert_eq!(children[0]["provider"], "codex");
            assert_eq!(children[0]["model"], "gpt-5");
            assert_eq!(children[0]["status"], "idle");
        })
        .unwrap();
}

#[test]
fn child_status_reads_the_last_reply() {
    let mut chat = chat_with("c", &["hi", "done"]);
    assert_eq!(child_status(&chat), "idle");
    chat.messages.last_mut().unwrap().failed = true;
    assert_eq!(child_status(&chat), "failed");
}

#[gpui_kit::test]
fn call_for_missing_lead_replies_error(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let (reply, answer) = std::sync::mpsc::channel();
            let call = OrchCall {
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
                lead: "999".into(),
                tool: "spawn_agent".into(),
                args: spawn_args("codex", "gpt-5"),
                reply,
            };
            app.orchestrate(call, window, cx);
            assert_eq!(answer.try_recv().unwrap(), Err("lead chat not found".into()));
            assert!(answer.try_recv().is_err());
            assert_eq!(app.chats.len(), 2);
        })
        .unwrap();
}

#[gpui_kit::test]
fn unknown_tool_replies_error(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let (reply, answer) = std::sync::mpsc::channel();
            let call = OrchCall {
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
                lead: "10".into(),
                tool: "x".into(),
                args: json!({}),
                reply,
            };
            app.orchestrate(call, window, cx);
            assert_eq!(answer.try_recv().unwrap(), Err(r#"unknown tool "x""#.into()));
        })
        .unwrap();
}

#[gpui_kit::test]
fn call_for_non_lead_chat_replies_error(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            app.chats[1].lead = false;
            let (reply, answer) = std::sync::mpsc::channel();
            let call = OrchCall {
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
                lead: "10".into(),
                tool: "list_agents".into(),
                args: json!({}),
                reply,
            };
            app.orchestrate(call, window, cx);
            assert_eq!(answer.try_recv().unwrap(), Err("lead chat not found".into()));
        })
        .unwrap();
}

#[gpui_kit::test]
fn blocked_spawn_replies_error_and_leaves_no_chat(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            app.chats[1].worktree_job = Some(crate::app::WorktreeJob::Switching);
            let (reply, answer) = std::sync::mpsc::channel();
            let call = OrchCall {
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
                lead: "10".into(),
                tool: "spawn_agent".into(),
                args: spawn_args("codex", "gpt-5"),
                reply,
            };
            app.orchestrate(call, window, cx);
            assert_eq!(answer.try_recv().unwrap(), Err("Switching branches…".into()));
            assert_eq!(app.chats.len(), 2);
        })
        .unwrap();
}

fn with_child(app: &mut ChatApp, lead: i64) -> i64 {
    let mut child = chat_with("kid", &["q1", "first", "q2", "second"]);
    child.id = lead * 100 + app.chats.len() as i64;
    child.provider = Provider::Codex;
    child.provider_model = "gpt-5".into();
    child.parent = Some(lead);
    let id = child.id;
    app.chats.push(child);
    id
}

fn child_ix(app: &ChatApp, id: i64) -> usize {
    app.chats.iter().position(|c| c.id == id).unwrap()
}

#[gpui_kit::test]
fn read_agent_returns_last_replies_oldest_first(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let id = with_child(app, 10);
            let v = app.read_agent(1, &json!({"id": id})).unwrap();
            assert_eq!(v["replies"], json!(["second"]));
            assert_eq!(v["status"], "idle");
            assert_eq!(v["provider"], "codex");
            assert_eq!(v["model"], "gpt-5");
            assert_eq!(v["title"], "kid");
            let v = app.read_agent(1, &json!({"id": id, "last": 2})).unwrap();
            assert_eq!(v["replies"], json!(["first", "second"]));
            let v = app.read_agent(1, &json!({"id": id, "last": 99})).unwrap();
            assert_eq!(v["replies"], json!(["first", "second"]));
        })
        .unwrap();
}

#[gpui_kit::test]
fn read_agent_accepts_string_id_and_rejects_missing(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let id = with_child(app, 10);
            let v = app.read_agent(1, &json!({"id": id.to_string()})).unwrap();
            assert_eq!(v["id"], id);
            let missing = Err(r#"missing "id""#.to_string());
            assert_eq!(app.read_agent(1, &json!({})), missing);
            assert_eq!(app.read_agent(1, &json!({"id": 1.5})), missing);
        })
        .unwrap();
}

#[gpui_kit::test]
fn tools_refuse_another_leads_child(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            app.chats.push(lead_chat(20));
            let id = with_child(app, 20);
            let want = Err(format!("agent {id} isn't one of this lead's children"));
            assert_eq!(app.read_agent(1, &json!({"id": id})), want);
            let ix = child_ix(app, id);
            app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
            let sent = app.message_agent(1, &json!({"id": id, "text": "x"}), window, cx).unwrap();
            assert_eq!(sent["status"], "queued");
            assert!(sent["reply"].as_str().unwrap().starts_with("not your child"));
            app.chats[ix].cancel = None;
            assert_eq!(
                app.message_agent(1, &json!({"id": 10, "text": "x"}), window, cx),
                Err("a lead can't message itself".into())
            );
            assert_eq!(app.stop_agent(1, &json!({"id": id}), cx), want);
        })
        .unwrap();
}

#[gpui_kit::test]
fn message_agent_queues_behind_busy_child(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let id = with_child(app, 10);
            let ix = child_ix(app, id);
            app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
            let v = app.message_agent(1, &json!({"id": id, "text": "more"}), window, cx);
            assert_eq!(v, Ok(json!({"status": "queued"})));
            assert_eq!(app.chats[ix].queue.len(), 1);
            assert_eq!(app.chats[ix].queue[0].text, "more");
            let v = app.message_agent(1, &json!({"id": id}), window, cx);
            assert_eq!(v, Err(r#"missing "text""#.into()));
        })
        .unwrap();
}

#[gpui_kit::test]
fn stop_agent_reports_whether_it_stopped(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, cx| {
            let id = with_child(app, 10);
            let ix = child_ix(app, id);
            assert_eq!(app.stop_agent(1, &json!({"id": id}), cx), Ok(json!({"stopped": false})));
            app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
            assert_eq!(app.stop_agent(1, &json!({"id": id}), cx), Ok(json!({"stopped": true})));
            assert!(app.chats[ix].stopped);
        })
        .unwrap();
}

#[test]
fn clip_cuts_at_4000_chars_on_a_char_boundary() {
    assert_eq!(clip(&"é".repeat(4001)), format!("{}…", "é".repeat(4000)));
    assert_eq!(clip(&"é".repeat(4000)), "é".repeat(4000));
    assert_eq!(clip("short"), "short");
}

#[test]
fn delivery_text_formats_finished_failed_and_stopped() {
    let mut child = chat_with("kid", &["q", "done"]);
    child.id = 7;
    child.provider = Provider::Codex;
    child.provider_model = "gpt-5".into();
    let mut reply = child.messages[1].clone();
    assert_eq!(
        delivery_text(&child, &reply, "finished"),
        "[Aether child result] Agent 7 \"kid\" (codex/gpt-5) finished.\n\ndone\n\nUse the read_agent tool with id 7 for more."
    );
    assert!(
        delivery_text(&child, &reply, "was stopped by the user")
            .contains("(codex/gpt-5) was stopped by the user.\n\ndone\n\n")
    );
    reply.text = "partial".into();
    reply.error = Some("boom".into());
    assert!(
        delivery_text(&child, &reply, "failed")
            .contains(" failed.\n\nboom\n\npartial\n\nUse the read_agent tool")
    );
    reply.text.clear();
    assert!(
        delivery_text(&child, &reply, "failed")
            .contains(" failed.\n\nboom\n\nUse the read_agent tool")
    );
    reply.error = None;
    reply.text = "x".repeat(4001);
    assert!(
        delivery_text(&child, &reply, "finished")
            .contains(&format!("{}…\n\nUse the read_agent tool", "x".repeat(4000)))
    );
}

fn busy_lead(app: &mut ChatApp) {
    app.chats[1].cancel = Some(aether_agent::Cancel::inert());
}

fn deliver(app: &mut ChatApp, id: i64, window: &mut Window, cx: &mut Context<ChatApp>) {
    let ix = child_ix(app, id);
    let reply_ix = app.chats[ix].messages.len() - 1;
    app.deliver_to_lead(ix, reply_ix, false, window, cx);
}

#[gpui_kit::test]
fn busy_lead_coalesces_deliveries_into_one_queued_item(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            let a = with_child(app, 10);
            let b = with_child(app, 10);
            deliver(app, a, window, cx);
            deliver(app, b, window, cx);
            let text = |id| {
                let c = &app.chats[child_ix(app, id)];
                delivery_text(c, c.messages.last().unwrap(), "finished")
            };
            let want = format!("{}\n\n---\n\n{}", text(a), text(b));
            let queue = &app.chats[1].queue;
            assert_eq!(queue.len(), 1);
            assert!(queue[0].from_children);
            assert_eq!(queue[0].text, want);
        })
        .unwrap();
}

#[gpui_kit::test]
fn delivery_does_not_merge_into_users_queued_text(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            app.chats[1].queue = vec![Queued {
                text: "user note".into(),
                attachments: vec![],
                delivering: false,
                from_children: false,
                deliveries: vec![],
            }];
            let a = with_child(app, 10);
            deliver(app, a, window, cx);
            let queue = &app.chats[1].queue;
            assert_eq!(queue.len(), 2);
            assert_eq!(queue[0].text, "user note");
            assert!(queue[1].from_children);
        })
        .unwrap();
}

#[gpui_kit::test]
fn no_delivery_without_a_live_lead(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            app.chats[0].id = 5;
            app.chats[0].cancel = Some(aether_agent::Cancel::inert());
            let a = with_child(app, 5);
            let b = with_child(app, 9999);
            deliver(app, a, window, cx);
            deliver(app, b, window, cx);
            assert!(app.chats.iter().all(|c| c.queue.is_empty()));
        })
        .unwrap();
}

// keyless OpenRouter: a wrong submit errors in place instead of spawning a CLI
fn idle_lead(app: &mut ChatApp) {
    app.chats[1].provider = Provider::OpenRouter;
}

fn queued_texts(app: &ChatApp) -> Vec<(String, bool)> {
    app.chats[1].queue.iter().map(|q| (q.text.clone(), q.from_children)).collect()
}

#[gpui_kit::test]
fn stopped_idle_lead_holds_the_delivery(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            idle_lead(app);
            app.chats[1].stopped = true;
            let a = with_child(app, 10);
            deliver(app, a, window, cx);
            assert_eq!(app.chats[1].queue.len(), 1);
            assert!(app.chats[1].queue[0].from_children);
        })
        .unwrap();
}

#[gpui_kit::test]
fn idle_lead_with_leftover_queue_delivers_behind_it(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            idle_lead(app);
            app.chats[1].queue = vec![Queued {
                text: "older".into(),
                attachments: vec![],
                delivering: false,
                from_children: false,
                deliveries: vec![],
            }];
            let a = with_child(app, 10);
            deliver(app, a, window, cx);
            let queue = queued_texts(app);
            assert_eq!(queue.len(), 2);
            assert_eq!(queue[0], ("older".into(), false));
            assert!(queue[1].1);
        })
        .unwrap();
}

fn lock_externally(app: &mut ChatApp, ix: usize) {
    app.chats[ix].external_pid = Some(42);
    app.external = vec![aether_agent::ExternalAgent {
        provider: Provider::Claude,
        pid: Some(42),
        started: String::new(),
        elapsed: String::new(),
        cwd: "/tmp/proj".into(),
        session: String::new(),
        title: String::new(),
        state: aether_agent::ExternalState::Idle,
        exact: true,
        origin: String::new(),
    }];
    assert!(app.locked(ix));
}

#[gpui_kit::test]
fn externally_locked_lead_holds_the_delivery(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            idle_lead(app);
            lock_externally(app, 1);
            let a = with_child(app, 10);
            deliver(app, a, window, cx);
            assert_eq!(app.chats[1].queue.len(), 1);
            assert!(app.chats[1].queue[0].from_children);
        })
        .unwrap();
}

#[gpui_kit::test]
fn message_agent_refuses_a_stopping_child(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let id = with_child(app, 10);
            let ix = child_ix(app, id);
            app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
            app.stop_agent(1, &json!({"id": id}), cx).unwrap();
            let v = app.message_agent(1, &json!({"id": id, "text": "more"}), window, cx);
            assert_eq!(
                v,
                Err(format!(
                    "agent {id} is stopping; message it after its \"was stopped\" result arrives"
                ))
            );
            assert!(app.chats[ix].queue.is_empty());
        })
        .unwrap();
}

#[gpui_kit::test]
fn message_agent_refuses_a_terminal_locked_child(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let id = with_child(app, 10);
            let ix = child_ix(app, id);
            app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
            lock_externally(app, ix);
            let v = app.message_agent(1, &json!({"id": id, "text": "more"}), window, cx);
            assert_eq!(
                v,
                Err(format!(
                    "agent {id} is open in a terminal; it can't take messages from Aether right now"
                ))
            );
            assert!(app.chats[ix].queue.is_empty());
        })
        .unwrap();
}

fn last_user_text(chat: &Chat) -> &str {
    &chat.messages.iter().rev().find(|m| m.role == aether_core::Role::User).unwrap().text
}

#[gpui_kit::test]
fn message_agent_runs_the_oldest_queued_text_first(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let id = with_child(app, 10);
            let ix = child_ix(app, id);
            app.chats[ix].provider = Provider::OpenRouter;
            app.chats[ix].queue = vec![Queued {
                text: "old".into(),
                attachments: vec![],
                delivering: false,
                from_children: false,
                deliveries: vec![],
            }];
            let v = app.message_agent(1, &json!({"id": id, "text": "new"}), window, cx);
            let v = v.unwrap();
            assert_eq!(v["status"], "queued");
            assert!(v["note"].as_str().unwrap().contains("oldest queued text was sent"));
            let queue: Vec<_> =
                app.chats[ix].queue.iter().map(|q| (q.text.as_str(), q.from_children)).collect();
            assert_eq!(queue, [("new", false)]);
            assert_eq!(last_user_text(&app.chats[ix]), "old");
        })
        .unwrap();
}

#[gpui_kit::test]
fn read_agent_keeps_a_failed_replys_error(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let id = with_child(app, 10);
            let ix = child_ix(app, id);
            let reply = app.chats[ix].messages.last_mut().unwrap();
            reply.failed = true;
            reply.error = Some("boom".into());
            reply.text.clear();
            let v = app.read_agent(1, &json!({"id": id})).unwrap();
            assert_eq!(v["replies"], json!(["boom"]));
        })
        .unwrap();
}

#[gpui_kit::test]
fn idle_lead_gets_the_delivery_as_its_next_turn(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            idle_lead(app);
            let a = with_child(app, 10);
            deliver(app, a, window, cx);
            assert!(app.chats[1].queue.is_empty());
            assert!(last_user_text(&app.chats[1]).contains("[Aether child result] Agent"));
        })
        .unwrap();
}

#[gpui_kit::test]
fn delivery_says_who_stopped_the_child(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            let by_lead = with_child(app, 10);
            let ix = child_ix(app, by_lead);
            app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
            app.stop_agent(1, &json!({"id": by_lead}), cx).unwrap();
            deliver(app, by_lead, window, cx);
            let by_user = with_child(app, 10);
            let ix = child_ix(app, by_user);
            app.chats[ix].stopped = true;
            deliver(app, by_user, window, cx);
            let stale = with_child(app, 10);
            app.stopped_by_lead.insert(stale);
            deliver(app, stale, window, cx);
            let text = &app.chats[1].queue[0].text;
            for (id, outcome) in [
                (by_lead, "was stopped by the lead"),
                (by_user, "was stopped by the user"),
                (stale, "finished"),
            ] {
                let want = format!("Agent {id} \"kid\" (codex/gpt-5) {outcome}.");
                assert!(text.contains(&want), "{text}");
            }
            assert!(app.stopped_by_lead.is_empty());
        })
        .unwrap();
}

#[gpui_kit::test]
fn spawn_queue_is_capped_per_lead(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            for _ in 0..8 {
                let id = with_child(app, 10);
                let ix = child_ix(app, id);
                app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
            }
            for _ in 0..32 {
                app.spawn_agent(1, &spawn_args("codex", "gpt-5"), window, cx).unwrap();
            }
            let err = app.spawn_agent(1, &spawn_args("codex", "gpt-5"), window, cx).unwrap_err();
            assert!(err.contains("already queued"), "{err}");
            assert_eq!(app.spawn_queue.len(), 32);
        })
        .unwrap();
}

#[gpui_kit::test]
fn message_agent_refuses_a_chat_with_more_access_than_the_lead(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let mut other = lead_chat(20);
            other.access = AccessMode::Full;
            other.cancel = Some(aether_agent::Cancel::inert());
            app.chats.push(other);
            let err =
                app.message_agent(1, &json!({"id": 20, "text": "x"}), window, cx).unwrap_err();
            assert!(err.contains("more access"), "{err}");
            app.chats[2].access = AccessMode::AutoEdit;
            let sent = app.message_agent(1, &json!({"id": 20, "text": "x"}), window, cx).unwrap();
            assert_eq!(sent["status"], "queued");
        })
        .unwrap();
}

#[gpui_kit::test]
fn spawn_queues_at_running_cap_and_starts_when_a_slot_frees(cx: &mut TestAppContext) {
    // the drained spawn starts a real CLI run whose worker thread sends updates
    cx.background_executor.allow_parking();
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let kids: Vec<i64> = (0..8)
                .map(|_| {
                    let id = with_child(app, 10);
                    let ix = child_ix(app, id);
                    app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
                    id
                })
                .collect();
            let n = app.chats.len();
            let mut args = spawn_args("codex", "gpt-5");
            args["nonce"] = "q-1".into();
            let first = app.spawn_agent(1, &args, window, cx).unwrap();
            assert_eq!(
                (first["status"].as_str(), first["position"].as_u64()),
                (Some("queued"), Some(1))
            );
            let again = app.spawn_agent(1, &args, window, cx).unwrap();
            assert_eq!(again["position"], 1, "a retried nonce isn't queued twice");
            let second = app.spawn_agent(1, &spawn_args("codex", "gpt-5"), window, cx).unwrap();
            assert_eq!(second["position"], 2);
            assert_eq!((app.chats.len(), app.spawn_queue.len()), (n, 2));
            app.drain_spawn_queue(window, cx);
            assert_eq!(app.spawn_queue.len(), 2, "still full: nothing starts");
            let freed = child_ix(app, kids[0]);
            app.chats[freed].cancel = None;
            app.drain_spawn_queue(window, cx);
            assert_eq!((app.chats.len(), app.spawn_queue.len()), (n + 1, 1));
            let lead = &app.chats[1];
            let told = lead.queue.iter().map(|q| q.text.as_str()).chain(
                lead.messages
                    .iter()
                    .filter(|m| m.role == aether_core::Role::User)
                    .map(|m| m.text.as_str()),
            );
            let told: Vec<&str> = told.collect();
            assert!(
                told.iter().any(|t| t.contains("Queued spawn \"Fix tests\" started")),
                "{told:?}"
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn idle_children_dont_count_toward_cap(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            for i in 0..8 {
                let id = with_child(app, 10);
                let ix = child_ix(app, id);
                if i > 0 {
                    app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
                }
            }
            assert_eq!(app.running_children(1), 7);
        })
        .unwrap();
}

#[gpui_kit::test]
fn spawn_refuses_when_lead_has_no_folder(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            app.chats[1].provider = Provider::OpenRouter;
            app.chats[1].cwd.clear();
            app.settings.workspace_root.clear();
            let n = app.chats.len();
            let err = app.spawn_agent(1, &spawn_args("codex", "gpt-5"), window, cx).unwrap_err();
            assert_eq!(
                err,
                "this lead has no project folder; set a workspace root in Settings or lead from a project chat"
            );
            assert_eq!(app.chats.len(), n);
            app.settings.workspace_root = "/tmp/ws".into();
            assert!(app.create_child(1, &SpawnArgs::parse(&spawn_args("codex", "gpt-5"), &app.probes).unwrap()).is_ok());
        })
        .unwrap();
}

#[gpui_kit::test]
fn lead_chip_shows_for_top_level_chats_only(cx: &mut TestAppContext) {
    use crate::app::orchestrate::shows_lead_chip;
    let window = lead_app(cx);
    window
        .update(cx, |_, _, _| {
            let mut chat = chat_with("a", &[]);
            chat.provider = Provider::Claude;
            assert!(shows_lead_chip(&chat));
            chat.parent = Some(1);
            assert!(!shows_lead_chip(&chat));
            chat.parent = None;
            chat.provider = Provider::Antigravity;
            assert!(shows_lead_chip(&chat));
        })
        .unwrap();
}

#[gpui_kit::test]
fn lead_chip_draws_and_toggles_on_click(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        let mut chat = chat_with("a", &[]);
        chat.id = 2;
        chat.provider = Provider::OpenRouter;
        app.chats.push(chat);
        app.select(1, window, cx);
        app
    });
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    let bounds = |visual: &mut gpui_kit::VisualTestContext| {
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        visual.debug_bounds("lead-chip")
    };
    let chip = bounds(&mut visual).expect("top-level OpenRouter chat shows the chip");
    visual.simulate_click(chip.center(), gpui_kit::Modifiers::default());
    assert!(visual.update(|_, cx| app.read(cx).chats[1].lead));
    visual.update(|_, cx| app.update(cx, |app, _| app.chats[1].parent = Some(1)));
    assert!(bounds(&mut visual).is_none(), "children never get the chip");
}

#[gpui_kit::test]
fn crew_strip_lists_own_children_in_order(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            app.chats.push(lead_chat(20));
            for (id, parent, archived, running) in [
                (1005, 10, false, true),
                (2001, 20, false, true),
                (1003, 10, false, true),
                (1004, 10, true, true),
                (1006, 10, false, false),
            ] {
                let mut child = chat_with("kid", &[]);
                child.id = id;
                child.parent = Some(parent);
                child.archived = archived;
                if running {
                    child.cancel = Some(aether_agent::Cancel::inert());
                }
                app.chats.push(child);
            }
            let ids: Vec<i64> = app.crew(1).into_iter().map(|ix| app.chats[ix].id).collect();
            assert_eq!(ids, [1003, 1005]);
            assert!(app.crew(0).is_empty());
        })
        .unwrap();
}

#[gpui_kit::test]
fn crew_strip_orders_needs_you_failed_running_unread(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let done = finished_child(app, 10);
            let failed = finished_child(app, 10);
            let ix = child_ix(app, failed);
            app.chats[ix].messages.last_mut().unwrap().failed = true;
            let waiting = finished_child(app, 10);
            let ix = child_ix(app, waiting);
            app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
            let request = aether_core::Request {
                id: 1,
                at: 0,
                ask: aether_core::Ask::Unknown,
                status: aether_core::RequestStatus::Pending,
                asked_ms: None,
            };
            app.chats[ix].messages.last_mut().unwrap().requests.push(request);
            let running = finished_child(app, 10);
            let ix = child_ix(app, running);
            app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
            // crew strip is running-only: finished/failed children stay out
            let ids: Vec<i64> = app.crew(1).into_iter().map(|ix| app.chats[ix].id).collect();
            assert_eq!(ids, [waiting, running]);
            assert!(!ids.contains(&done));
            assert!(!ids.contains(&failed));
        })
        .unwrap();
}

fn crew_app(cx: &mut TestAppContext, children: i64) -> gpui_kit::WindowHandle<ChatApp> {
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
        let mut lead = chat_with("lead", &[]);
        lead.id = 2;
        lead.provider = Provider::OpenRouter;
        lead.lead = true;
        app.chats.push(lead);
        for i in 0..children {
            let title = if i == 0 {
                "Überprüfe die Größenänderung der Fenster".to_string()
            } else {
                format!("child number {i} with a rather long title")
            };
            let mut child = chat_with(&title, &[]);
            child.id = 100 + i;
            child.provider = Provider::OpenRouter;
            child.parent = Some(2);
            child.cancel = Some(aether_agent::Cancel::inert());
            app.chats.push(child);
        }
        app.select(1, window, cx);
        app
    })
}

fn draw(visual: &mut gpui_kit::VisualTestContext) {
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn open_crew_pop(visual: &mut gpui_kit::VisualTestContext) {
    let strip = visual.debug_bounds("crew-strip").expect("crew trigger");
    visual.simulate_click(strip.center(), gpui_kit::Modifiers::default());
    draw(visual);
    assert!(visual.debug_bounds("crew-pop").is_some(), "crew popover open");
}

#[gpui_kit::test]
fn no_crew_strip_without_children(cx: &mut TestAppContext) {
    let window = crew_app(cx, 0);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    draw(&mut visual);
    assert!(visual.debug_bounds("crew-strip").is_none());
}

#[gpui_kit::test]
fn crew_strip_with_many_children_draws(cx: &mut TestAppContext) {
    let window = crew_app(cx, 12);
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    draw(&mut visual);
    let strip = visual.debug_bounds("crew-strip").expect("lead with children shows the trigger");
    assert!(strip.size.height <= gpui_kit::px(32.), "single footer chip: {strip:?}");
    open_crew_pop(&mut visual);
    let chip = visual.debug_bounds("crew-chip-100").expect("oldest child chip");
    visual.simulate_click(chip.center(), gpui_kit::Modifiers::default());
    let active = visual.update(|_, cx| {
        let app = app.read(cx);
        app.chats[app.active].id
    });
    assert_eq!(active, 100);
}

#[gpui_kit::test]
fn crew_chip_opens_child_in_its_own_pane(cx: &mut TestAppContext) {
    let window = crew_app(cx, 2);
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    draw(&mut visual);
    assert!(visual.update(|_, cx| app.read(cx).workspace.model.pane_for(2).is_some()));
    open_crew_pop(&mut visual);
    let chip = visual.debug_bounds("crew-chip-101").expect("child chip");
    visual.simulate_click(chip.center(), gpui_kit::Modifiers::default());
    visual.update(|_, cx| {
        let model = &app.read(cx).workspace.model;
        let (lead, child) = (model.pane_for(2), model.pane_for(101));
        assert!(lead.is_some(), "lead keeps its pane");
        assert!(child.is_some_and(|p| Some(p) != lead), "child gets its own pane");
        assert_eq!(model.windows[&0].focused, child);
    });
}

#[gpui_kit::test]
fn crew_chips_in_a_narrow_window_reuse_the_other_pane(cx: &mut TestAppContext) {
    let window = crew_app(cx, 2);
    cx.simulate_window_resize(*window, gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(800.)));
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    draw(&mut visual);
    open_crew_pop(&mut visual);
    let chip = visual.debug_bounds("crew-chip-100").expect("first child chip");
    visual.simulate_click(chip.center(), gpui_kit::Modifiers::default());
    draw(&mut visual);
    assert_eq!(visual.update(|_, cx| app.read(cx).workspace.model.windows[&0].panes().len()), 2);
    open_crew_pop(&mut visual);
    let chip = visual.debug_bounds("crew-chip-101").expect("second child chip");
    visual.simulate_click(chip.center(), gpui_kit::Modifiers::default());
    visual.update(|_, cx| {
        let model = &app.read(cx).workspace.model;
        assert_eq!(model.windows[&0].panes().len(), 2, "no third sliver pane");
        let (lead, child) = (model.pane_for(2), model.pane_for(101));
        assert!(lead.is_some(), "lead keeps its pane");
        assert!(child.is_some_and(|p| Some(p) != lead), "second child takes the other pane");
        assert!(model.pane_for(100).is_none());
        assert_eq!(model.windows[&0].focused, child);
    });
}

#[gpui_kit::test]
fn tools_treat_archived_children_as_absent(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let id = with_child(app, 10);
            let ix = child_ix(app, id);
            app.chats[ix].archived = true;
            assert_eq!(app.list_agents(1)["children"], json!([]));
            let want = Err(format!("agent {id} isn't one of this lead's children"));
            assert_eq!(app.read_agent(1, &json!({"id": id})), want);
            assert_eq!(
                app.message_agent(1, &json!({"id": id, "text": "x"}), window, cx),
                Err(format!("chat {id} is archived; unarchive it first"))
            );
            assert_eq!(app.stop_agent(1, &json!({"id": id}), cx), want);
        })
        .unwrap();
}

#[gpui_kit::test]
fn delegated_by_names_live_lead(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let id = with_child(app, 10);
            assert_eq!(app.delegated_by(child_ix(app, id)), Some(1));
        })
        .unwrap();
}

#[gpui_kit::test]
fn delegated_by_hidden_for_orphan_or_non_lead(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let id = with_child(app, 10);
            let ix = child_ix(app, id);
            app.chats[ix].parent = Some(999);
            assert_eq!(app.delegated_by(ix), None);
            app.chats[ix].parent = Some(app.chats[0].id);
            assert_eq!(app.delegated_by(ix), None);
            app.chats[ix].parent = None;
            assert_eq!(app.delegated_by(ix), None);
        })
        .unwrap();
}

#[gpui_kit::test]
fn delegated_by_link_draws_and_selects_lead(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let id = with_child(app, 10);
            let ix = child_ix(app, id);
            app.chats[1].provider = Provider::OpenRouter;
            app.chats[ix].provider = Provider::OpenRouter;
            app.select(ix, window, cx);
        })
        .unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let link = visual.debug_bounds("delegated-by").expect("child shows the link");
    visual.simulate_click(link.center(), gpui_kit::Modifiers::default());
    assert_eq!(visual.update(|_, cx| app.read(cx).active), 1);
}

#[gpui_kit::test]
fn delegated_by_link_opens_lead_in_its_own_pane(cx: &mut TestAppContext) {
    let window = crew_app(cx, 1);
    window.update(cx, |app, window, cx| app.select(2, window, cx)).unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    draw(&mut visual);
    visual.update(|_, cx| {
        let model = &app.read(cx).workspace.model;
        assert!(model.pane_for(100).is_some());
        assert!(model.pane_for(2).is_none());
    });
    let link = visual.debug_bounds("delegated-by").expect("child shows the link");
    visual.simulate_click(link.center(), gpui_kit::Modifiers::default());
    visual.update(|_, cx| {
        let model = &app.read(cx).workspace.model;
        let (lead, child) = (model.pane_for(2), model.pane_for(100));
        assert!(child.is_some(), "child keeps its pane");
        assert!(lead.is_some_and(|p| Some(p) != child), "lead gets its own pane");
        assert_eq!(model.windows[&0].focused, lead);
    });
}

fn other(name: &str, output: &str) -> ToolDetail {
    ToolDetail::Other { name: name.into(), input: String::new(), output: output.into() }
}

#[test]
fn spawned_child_reads_id_from_each_harness_name() {
    let out = r#"{"id":42,"title":"Fix"}"#;
    for name in ["mcp__aether__spawn_agent", "aether_spawn_agent", "spawn_agent"] {
        assert_eq!(spawned_child(&other(name, out)), Some(42), "{name}");
    }
    for name in ["mcp__aether__read_agent", "aether_list_agents", "respawn_agent", "Bash"] {
        assert_eq!(spawned_child(&other(name, out)), None, "{name}");
    }
    let edit = ToolDetail::Edit { path: out.into(), diff: String::new(), added: 0, removed: 0 };
    assert_eq!(spawned_child(&edit), None);
}

#[test]
fn spawned_child_ignores_bad_output() {
    for out in ["not json", r#"{"title":"Fix"}"#, r#"{"id":"42"}"#, ""] {
        assert_eq!(spawned_child(&other("spawn_agent", out)), None, "{out}");
    }
}

fn temp_folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aether-orch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn parse_reads_worktree_flag() {
    let probes = codex_probes(&["gpt-5"]);
    let mut args = spawn_args("codex", "gpt-5");
    assert!(!SpawnArgs::parse(&args, &probes).unwrap().worktree);
    args["worktree"] = true.into();
    assert!(SpawnArgs::parse(&args, &probes).unwrap().worktree);
    args["worktree"] = "yes".into();
    assert_eq!(
        SpawnArgs::parse(&args, &probes).unwrap_err(),
        r#""worktree" must be true or false"#
    );
}

#[gpui_kit::test]
fn spawn_worktree_rejects_non_git_folder(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    let folder = temp_folder("nogit");
    window
        .update(cx, |app, window, cx| {
            app.chats[1].cwd = folder.display().to_string();
            let n = app.chats.len();
            let mut args = spawn_args("codex", "gpt-5");
            args["worktree"] = true.into();
            let err = app.spawn_agent(1, &args, window, cx).unwrap_err();
            assert_eq!(err, "the lead's folder isn't a git repository; spawn without worktree");
            assert_eq!(app.chats.len(), n);
        })
        .unwrap();
}

#[test]
fn worktree_base_is_leads_branch() {
    use crate::app::orchestrate::worktree_base;
    let repo = temp_folder("repo");
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "feat-x"]);
    git(&["commit", "-q", "--allow-empty", "-m", "init"]);
    let mut lead = lead_chat(10);
    assert_eq!(worktree_base(&lead, &repo).unwrap(), "feat-x");
    lead.worktree_branch = "aether/lead-work".into();
    assert_eq!(worktree_base(&lead, &repo).unwrap(), "aether/lead-work");
    assert_eq!(
        worktree_base(&lead, &temp_folder("plain")).unwrap_err(),
        "the lead's folder isn't a git repository; spawn without worktree"
    );
    git(&["checkout", "-q", "--detach"]);
    assert_eq!(worktree_base(&lead, &repo).unwrap(), "aether/lead-work");
    lead.worktree_branch.clear();
    assert_eq!(
        worktree_base(&lead, &repo).unwrap_err(),
        "the lead's folder is on a detached HEAD; check out a branch or spawn without worktree"
    );
}

#[gpui_kit::test]
fn worktree_failure_is_delivered_to_lead(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            app.active = 1;
            app.composer.update(cx, |s, cx| s.set_value("lead draft", window, cx));
            let a = with_child(app, 10);
            let ix = child_ix(app, a);
            app.deliver_failure_to_lead(ix, "Couldn't create its worktree: boom", window, cx);
            let queue = &app.chats[1].queue;
            assert_eq!(queue.len(), 1);
            assert!(queue[0].from_children);
            assert_eq!(
                queue[0].text,
                format!(
                    "[Aether child result] Agent {a} \"kid\" (codex/gpt-5) failed.\n\nCouldn't create its worktree: boom"
                )
            );
            assert_eq!(app.composer.read(cx).value(), "lead draft");
        })
        .unwrap();
}

#[gpui_kit::test]
fn child_send_in_worktree_leaves_lead_composer_alone(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = lead_app(cx);
    let folder = temp_folder("child-nogit");
    let ix = window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            app.active = 1;
            app.composer.update(cx, |s, cx| s.set_value("lead draft", window, cx));
            app.worktree_name_input.update(cx, |s, cx| s.set_value("lead-name", window, cx));
            let id = with_child(app, 10);
            let ix = child_ix(app, id);
            app.chats[ix].cwd = folder.display().to_string();
            app.send_in_new_worktree(
                ix,
                "main".into(),
                "kid".into(),
                "do it".into(),
                vec![],
                window,
                cx,
            );
            assert!(app.chats[ix].worktree_job.is_some());
            ix
        })
        .unwrap();
    for _ in 0..500 {
        cx.run_until_parked();
        if window.update(cx, |app, _, _| app.chats[ix].worktree_job.is_none()).unwrap() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    window
        .update(cx, |app, _, cx| {
            let child = &app.chats[ix];
            assert_eq!(
                child.worktree_error.as_deref(),
                Some("This folder isn't in a git repository.")
            );
            let queue = &app.chats[1].queue;
            assert_eq!(queue.len(), 1);
            assert!(queue[0].text.contains(
                "failed.\n\nCouldn't create its worktree: This folder isn't in a git repository."
            ));
            assert_eq!(app.active, 1);
            assert_eq!(app.composer.read(cx).value(), "lead draft");
            assert_eq!(app.worktree_name_input.read(cx).value(), "lead-name");
        })
        .unwrap();
}

#[gpui_kit::test]
fn worktree_lead_child_keeps_leads_project(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let lead = &mut app.chats[1];
            lead.cwd = "/wt/proj".into();
            lead.worktree_path = "/wt/proj".into();
            lead.worktree_branch = "aether/lead".into();
            let a = SpawnArgs::parse(&spawn_args("codex", "gpt-5"), &app.probes).unwrap();
            let ix = app.create_child(1, &a).unwrap();
            assert_eq!(app.chats[ix].cwd, "/wt/proj");
            assert_eq!(app.chats[ix].project, "/tmp/proj");
            assert!(app.chats[ix].worktree_path.is_empty());
        })
        .unwrap();
}

fn creating() -> Option<crate::app::WorktreeJob> {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    Some(crate::app::WorktreeJob::Creating("main".into(), cancel))
}

#[gpui_kit::test]
fn starting_worktree_children_count_toward_cap(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            for _ in 0..8 {
                let id = with_child(app, 10);
                let ix = child_ix(app, id);
                app.chats[ix].worktree_job = creating();
            }
            let queued = app.spawn_agent(1, &spawn_args("codex", "gpt-5"), window, cx).unwrap();
            assert_eq!(queued["status"], "queued");
        })
        .unwrap();
}

#[test]
fn child_status_says_starting_while_its_worktree_is_made_or_set_up() {
    use crate::app::worktree::{SetupFailure, SetupJob};
    let mut chat = chat_with("c", &[]);
    chat.worktree_job = creating();
    assert_eq!(child_status(&chat), "starting");
    chat.worktree_job = None;
    let script = aether_core::ProjectScript {
        name: "install".into(),
        command: "x".into(),
        on_new_worktree: true,
    };
    chat.setup = Some(SetupJob {
        queue: vec![script],
        current: 0,
        text: "hi".into(),
        attachments: vec![],
        failed: None,
    });
    assert_eq!(child_status(&chat), "starting");
    chat.setup.as_mut().unwrap().failed = Some(SetupFailure::Code(1));
    assert_eq!(child_status(&chat), "idle");
}

#[gpui_kit::test]
fn setup_failure_is_delivered_to_lead(cx: &mut TestAppContext) {
    use crate::app::worktree::{SetupFailure, SetupJob};
    use aether_terminal::view::TerminalEvent;
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            let a = with_child(app, 10);
            let ix = child_ix(app, a);
            let script = aether_core::ProjectScript {
                name: "install".into(),
                command: "x".into(),
                on_new_worktree: true,
            };
            app.chats[ix].setup = Some(SetupJob {
                queue: vec![script],
                current: 0,
                text: "hi".into(),
                attachments: vec![],
                failed: None,
            });
            let ended = TerminalEvent::CommandEnded {
                key: "install".to_string().into(),
                code: Some(1),
                stopped: false,
                failed_to_start: false,
            };
            app.setup_ended(a, &ended, window, cx);
            assert_eq!(
                app.chats[ix].setup.as_ref().unwrap().failed,
                Some(SetupFailure::Code(1))
            );
            let queue = &app.chats[1].queue;
            assert_eq!(queue.len(), 1);
            assert_eq!(
                queue[0].text,
                format!(
                    "[Aether child result] Agent {a} \"kid\" (codex/gpt-5) failed.\n\nIts worktree setup failed: install exited with code 1"
                )
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn message_agent_refuses_a_child_without_its_worktree(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let a = with_child(app, 10);
            let ix = child_ix(app, a);
            app.chats[ix].messages.clear();
            app.chats[ix].provider = Provider::OpenRouter;
            app.chats[ix].worktree_error = Some("Canceled.".into());
            let err =
                app.message_agent(1, &json!({"id": a, "text": "go"}), window, cx).unwrap_err();
            assert_eq!(err, format!("agent {a} has no worktree; spawn a new one"));
        })
        .unwrap();
}

#[gpui_kit::test]
fn spawn_worktree_without_a_folder_gets_the_folder_error(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            app.chats[1].cwd.clear();
            app.settings.workspace_root.clear();
            let mut args = spawn_args("codex", "gpt-5");
            args["worktree"] = true.into();
            let err = app.spawn_agent(1, &args, window, cx).unwrap_err();
            assert!(err.starts_with("this lead has no project folder"), "{err}");
        })
        .unwrap();
}

#[gpui_kit::test]
fn deleting_a_child_mid_creation_cancels_its_worktree(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let a = with_child(app, 10);
            let ix = child_ix(app, a);
            app.chats[ix].worktree_job = creating();
            let Some(crate::app::WorktreeJob::Creating(_, cancel)) = &app.chats[ix].worktree_job
            else {
                unreachable!()
            };
            let cancel = cancel.clone();
            app.delete_chat(ix, window, cx);
            assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
        })
        .unwrap();
}

fn finished_child(app: &mut ChatApp, lead: i64) -> i64 {
    let id = with_child(app, lead);
    let ix = child_ix(app, id);
    let replies = app.chats[ix].messages.iter_mut();
    replies.filter(|m| m.role == aether_core::Role::Assistant).for_each(Message::finish);
    id
}

#[gpui_kit::test]
fn read_then_finish_drops_queued_delivery(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            let a = finished_child(app, 10);
            app.read_agent(1, &json!({"id": a})).unwrap();
            deliver(app, a, window, cx);
            app.drop_read_deliveries(1);
            assert!(app.chats[1].queue.is_empty());
        })
        .unwrap();
}

#[gpui_kit::test]
fn read_agent_drops_queued_delivery_at_once(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            let a = finished_child(app, 10);
            deliver(app, a, window, cx);
            assert_eq!(app.chats[1].queue.len(), 1);
            app.read_agent(1, &json!({"id": a})).unwrap();
            assert!(app.chats[1].queue.is_empty());
        })
        .unwrap();
}

#[gpui_kit::test]
fn unfinished_reply_is_not_marked_read(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            let a = with_child(app, 10);
            let ix = child_ix(app, a);
            app.chats[ix].messages[1].finish();
            app.read_agent(1, &json!({"id": a})).unwrap();
            assert_eq!(app.chats[ix].lead_seen, Some(1));
            deliver(app, a, window, cx);
            app.drop_read_deliveries(1);
            assert_eq!(app.chats[1].queue.len(), 1);
        })
        .unwrap();
}

#[gpui_kit::test]
fn unread_delivery_is_kept(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            let a = finished_child(app, 10);
            app.read_agent(1, &json!({"id": a})).unwrap();
            let ix = child_ix(app, a);
            assert_eq!(app.chats[ix].lead_seen, Some(3));
            app.chats[ix].messages.extend(chat_with("", &["q3", "third"]).messages);
            app.chats[ix].messages[5].finish();
            deliver(app, a, window, cx);
            app.drop_read_deliveries(1);
            assert_eq!(app.chats[1].queue.len(), 1);
            assert!(app.chats[1].queue[0].text.contains("third"));
        })
        .unwrap();
}

#[gpui_kit::test]
fn coalesced_item_keeps_only_unread(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            let a = finished_child(app, 10);
            let b = finished_child(app, 10);
            deliver(app, a, window, cx);
            deliver(app, b, window, cx);
            app.read_agent(1, &json!({"id": a})).unwrap();
            let c = &app.chats[child_ix(app, b)];
            let want = delivery_text(c, c.messages.last().unwrap(), "finished");
            let queue = &app.chats[1].queue;
            assert_eq!(queue.len(), 1);
            assert_eq!(queue[0].text, want);
            assert_eq!(queue[0].deliveries.len(), 1);
        })
        .unwrap();
}

#[gpui_kit::test]
fn failure_delivery_never_dropped(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            busy_lead(app);
            let a = finished_child(app, 10);
            let ix = child_ix(app, a);
            app.deliver_failure_to_lead(ix, "Couldn't create its worktree: boom", window, cx);
            app.read_agent(1, &json!({"id": a})).unwrap();
            assert_eq!(app.chats[ix].lead_seen, Some(3));
            let failure = Delivery { child: a, reply_ix: None, text: "x".into() };
            assert_eq!(unread(vec![failure.clone()], &app.chats), vec![failure]);
            assert_eq!(app.chats[1].queue.len(), 1);
        })
        .unwrap();
}

#[gpui_kit::test]
fn read_agent_on_running_child_adds_note(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let a = finished_child(app, 10);
            let ix = child_ix(app, a);
            let v = app.read_agent(1, &json!({"id": a})).unwrap();
            assert!(v.get("note").is_none());
            app.chats[ix].lead_seen = None;
            app.chats[ix].cancel = Some(aether_agent::Cancel::inert());
            let v = app.read_agent(1, &json!({"id": a})).unwrap();
            assert_eq!(v["status"], "running");
            assert!(
                v["note"].as_str().unwrap().starts_with("still running; this is partial output.")
            );
            let ask = aether_core::Ask::Unknown;
            let status = aether_core::RequestStatus::Pending;
            let request = aether_core::Request { id: 1, at: 0, ask, status, asked_ms: None };
            app.chats[ix].messages.last_mut().unwrap().requests.push(request);
            let v = app.read_agent(1, &json!({"id": a})).unwrap();
            assert_eq!(v["status"], "needs_you");
            assert!(v["note"].is_string());
            assert_eq!(app.chats[ix].lead_seen, None);
        })
        .unwrap();
}

#[test]
fn stranded_texts_drops_prompts_but_keeps_deliveries() {
    let mut chat = chat_with("kid", &[]);
    chat.queue = vec![
        Queued {
            text: "fix it".into(),
            attachments: vec![],
            delivering: false,
            from_children: false,
            deliveries: vec![],
        },
        Queued {
            text: "result".into(),
            attachments: vec![],
            delivering: false,
            from_children: true,
            deliveries: vec![Delivery { child: 3, reply_ix: Some(1), text: "result".into() }],
        },
        Queued {
            text: "again".into(),
            attachments: vec![],
            delivering: false,
            from_children: false,
            deliveries: vec![],
        },
    ];
    assert_eq!(stranded_texts(&mut chat), ["fix it", "again"]);
    assert_eq!(chat.queue.len(), 1);
    assert!(chat.queue[0].from_children);
}

#[test]
fn delivery_head_carries_child_result_provenance() {
    let mut child = chat_with("kid", &["q", "done"]);
    child.id = 7;
    child.provider = Provider::Codex;
    child.provider_model = "gpt-5".into();
    let text = delivery_text(&child, &child.messages[1], "finished");
    assert!(
        text.starts_with("[Aether child result] Agent 7 \"kid\" (codex/gpt-5) finished."),
        "{text}"
    );
}

#[gpui_kit::test]
fn spawn_retry_with_same_nonce_returns_existing_child(cx: &mut TestAppContext) {
    // a successful spawn starts a real CLI run whose worker thread sends updates
    cx.background_executor.allow_parking();
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let mut args = spawn_args("codex", "gpt-5");
            args["nonce"] = "retry-1".into();
            let first = app.spawn_agent(1, &args, window, cx).unwrap();
            let n = app.chats.len();
            let second = app.spawn_agent(1, &args, window, cx).unwrap();
            assert_eq!(first, second);
            assert_eq!(app.chats.len(), n, "retry must not create a second child");
        })
        .unwrap();
}

#[gpui_kit::test]
fn card_tools_stay_in_the_leads_project(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            app.chats[1].cwd = "/tmp/proj-wt/feat".into();
            let other = app.board_add(
                "Other".into(),
                String::new(),
                Col::Todo,
                "/tmp/x".into(),
                None,
                false,
            );
            let shipped = app.board_add(
                "Shipped".into(),
                String::new(),
                Col::Done,
                "/tmp/proj".into(),
                None,
                false,
            );
            let mut call = |tool: &str, args: Value| {
                let (reply, answer) = std::sync::mpsc::channel();
                app.orchestrate(
                    OrchCall {
                        deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
                        lead: "10".into(),
                        tool: tool.into(),
                        args,
                        reply,
                    },
                    window,
                    cx,
                );
                answer.try_recv().unwrap()
            };
            assert_eq!(
                call("add_card", json!({"title": "Mine"})),
                Ok(Value::String("Created #3 in todo: Mine".into()))
            );
            assert_eq!(call("list_cards", json!({})), Ok(Value::String("#3 [todo] Mine".into())));
            assert_eq!(
                call("list_cards", json!({"filter": {"col": "done", "search": "ship"}})),
                Ok(Value::String(format!("#{shipped} [done] Shipped")))
            );
            assert_eq!(
                call("move_card", json!({"id": other, "col": "review"})),
                Err(format!("card #{other} not found"))
            );
            assert_eq!(
                call("update_card", json!({"id": other, "body": "x"})),
                Err(format!("card #{other} not found"))
            );
            assert_eq!(
                call("update_card", json!({"id": 3, "body": "Details", "col": "doing"})),
                Ok(Value::String("Updated #3 doing (description, column): Mine".into()))
            );
            assert_eq!(
                call("list_cards", json!({"bodies": true})),
                Ok(Value::String("#3 [doing] Mine\n    Details".into()))
            );
            assert_eq!(
                call("list_cards", json!({"filter": {"project": "all", "col": "todo"}})),
                Ok(Value::String(format!("#{other} [todo] Other [project x]")))
            );
            assert_eq!(
                call("add_card", json!({"title": "First"})),
                Ok(Value::String("Created #4 in todo: First".into()))
            );
            assert_eq!(
                call("update_card", json!({"id": 4, "col": "doing", "before": 3, "chat": 10})),
                Ok(Value::String("Updated #4 doing (column, position, chat): First".into()))
            );
            assert_eq!(
                call("update_card", json!({"id": 4, "chat": 999})),
                Err("chat 999 not found".into())
            );
            assert_eq!(
                call("list_cards", json!({})),
                Ok(Value::String("#4 [doing] First (agent 10)\n#3 [doing] Mine".into()))
            );
            let mine = app.cards.iter().find(|c| c.id == 3).unwrap();
            assert_eq!((mine.project.as_str(), mine.by_agent), ("/tmp/proj", true));
        })
        .unwrap();
}

#[gpui_kit::test]
fn non_lead_chats_only_read_their_own_projects_cards(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let ix = app.chats.iter().position(|c| c.id == 10).unwrap();
            app.chats[ix].lead = false;
            let home = app.lead_project(ix);
            let other = app.board_add(
                "Other".into(),
                String::new(),
                Col::Todo,
                "/tmp/x".into(),
                None,
                false,
            );
            let mine = app.board_add("Mine".into(), String::new(), Col::Todo, home, None, false);
            let mut call = |tool: &str, args: Value| {
                let (reply, answer) = std::sync::mpsc::channel();
                app.orchestrate(
                    OrchCall {
                        deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
                        lead: "10".into(),
                        tool: tool.into(),
                        args,
                        reply,
                    },
                    window,
                    cx,
                );
                answer.try_recv().unwrap()
            };
            assert_eq!(
                call("list_cards", json!({"filter": {"project": "all"}})),
                Ok(Value::String(format!("#{mine} [todo] Mine")))
            );
            assert_eq!(
                call("list_cards", json!({"filter": {"ids": [other]}})),
                Ok(Value::String("No matching cards.".into()))
            );
            assert_eq!(
                call("read_card", json!({"id": other})),
                Err(format!("card #{other} not found"))
            );
            assert!(call("read_card", json!({"id": mine})).is_ok());
        })
        .unwrap();
}

#[gpui_kit::test]
fn deleting_a_lead_unlinks_its_children_cards_and_handoffs(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let kid = with_child(app, 10);
            let kid_ix = child_ix(app, kid);
            app.chats[kid_ix].lead_seen = Some(1);
            app.chats[0].handoff_from = Some(10);
            let card = app.board_add(
                "Lead work".into(),
                String::new(),
                Col::Doing,
                "/tmp/proj".into(),
                Some(10),
                false,
            );
            app.delete_chat(1, window, cx);
            let kid = &app.chats[child_ix(app, kid)];
            assert_eq!((kid.parent, kid.lead_seen), (None, None));
            assert_eq!(app.chats[0].handoff_from, None);
            let card = app.cards.iter().find(|c| c.id == card).unwrap();
            assert_eq!((card.chats.as_slice(), card.col), (&[][..], Col::Doing));
        })
        .unwrap();
}

#[gpui_kit::test]
fn cards_take_labels_blockers_notes_and_read_back_in_full(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let mut call = |tool: &str, args: Value| {
                let (reply, answer) = std::sync::mpsc::channel();
                app.orchestrate(
                    OrchCall { deadline: std::time::Instant::now() + std::time::Duration::from_secs(30), lead: "10".into(), tool: tool.into(), args, reply },
                    window,
                    cx,
                );
                answer.try_recv().unwrap()
            };
            let text = |v: Result<Value, String>| v.unwrap().as_str().unwrap().to_string();
            assert_eq!(text(call("add_card", json!({"title": "Design"}))), "Created #1 in todo: Design");
            assert_eq!(
                call("add_card", json!({"title": "Build", "blocked_by": [99]})),
                Err("blocker card #99 not found".into())
            );
            assert_eq!(
                text(call("add_card", json!({"title": "Build", "labels": ["UI"], "blocked_by": [1]}))),
                "Created #2 in todo: Build"
            );
            assert_eq!(
                call("spawn_agent", json!({"provider": "codex", "model": "gpt-5", "title": "b", "task": "t", "card": 2})),
                Err("card #2 is blocked by #1; finish those first or clear blocked_by with update_card".into())
            );
            assert_eq!(
                text(call("list_cards", json!({"filter": {"label": "ui"}}))),
                "#2 [todo] Build {ui} blocked by #1"
            );
            assert_eq!(
                text(call("update_card", json!({"id": 2, "note": "waiting on design"}))),
                "Updated #2 todo (note): Build"
            );
            assert_eq!(
                text(call("update_card", json!({"id": 2, "labels": ["UI", "Backend"]}))),
                "Updated #2 todo (labels: ui, backend): Build"
            );
            assert_eq!(
                call("update_card", json!({"id": 2, "notes": "x"})),
                Err(r#"unknown argument "notes"; expected one of: append, before, blocked_by, body, chat, col, id, labels, note, title"#.into())
            );
            let card = call("read_card", json!({"id": "#2"})).unwrap();
            assert_eq!(card["labels"], json!(["ui", "backend"]));
            assert_eq!(card["blocked_by"], json!([{"id": 1, "title": "Design", "col": "todo"}]));
            assert_eq!(card["notes"][0]["by"], "lead 10");
            assert_eq!(card["notes"][0]["text"], "waiting on design");
            assert_eq!((card["added_by"].as_str(), card["body"].as_str()), (Some("lead 10"), Some("")));
            let brief = call("read_card", json!({"id": 2, "brief": true})).unwrap();
            assert!(brief.get("body").is_none(), "brief leaves out the description");
        })
        .unwrap();
    window
        .update(cx, |app, _, _| {
            app.board_move(1, Col::Done, None);
            let build = app.cards.iter().find(|c| c.id == 2).unwrap();
            assert!(build.blocked_by.is_empty(), "a done blocker unblocks its dependents");
        })
        .unwrap();
}

#[gpui_kit::test]
fn finished_agents_file_their_reply_on_linked_cards(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let (a, b) = (with_child(app, 10), with_child(app, 10));
            let card = app.board_add(
                "Pair".into(),
                String::new(),
                Col::Todo,
                "/tmp/proj".into(),
                Some(a),
                false,
            );
            app.board_link(card, b);
            app.board_on_turn(b, crate::board::model::Turn::Started);
            let ix = child_ix(app, a);
            let mut reply = aether_core::Message::assistant();
            reply.text = "done: parser fixed".into();
            app.chats[ix].messages.push(reply);
            let b_ix = child_ix(app, b);
            app.chats[b_ix].cancel = Some(aether_agent::Cancel::inert());
            app.board_on_turn(a, crate::board::model::Turn::Finished { failed: false });
            let card = app.cards.iter().find(|c| c.id == card).unwrap();
            assert_eq!(card.col, Col::Doing, "stays in Doing while another linked agent runs");
            assert_eq!(card.notes.len(), 1);
            assert_eq!(
                (card.notes[0].by.as_str(), card.notes[0].text.as_str()),
                (&*format!("agent {a}"), "done: parser fixed")
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn deleted_card_ids_are_not_reused(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let add = |app: &mut ChatApp| {
                app.board_add("c".into(), String::new(), Col::Todo, "/tmp/proj".into(), None, false)
            };
            let first = add(app);
            let second = add(app);
            app.board_delete(second);
            assert_eq!(add(app), second + 1, "#{second} stays retired");
            app.board_delete(first);
            assert_eq!(add(app), second + 2);
        })
        .unwrap();
}

#[gpui_kit::test]
fn crew_ids_cover_children_and_grandchildren(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, _, _| {
            let kid = with_child(app, 10);
            let grandkid = with_child(app, kid);
            let other = with_child(app, 99);
            let crew = app.crew_ids(10);
            assert_eq!(crew, [kid, grandkid]);
            assert!(!crew.contains(&other));
            assert!(app.crew_ids(grandkid).is_empty());
        })
        .unwrap();
}

#[gpui_kit::test]
fn expired_call_is_skipped_before_mutating_chat(cx: &mut TestAppContext) {
    let window = lead_app(cx);
    window
        .update(cx, |app, window, cx| {
            let title = app.chats[1].title.clone();
            let (reply, answer) = std::sync::mpsc::channel();
            let call = OrchCall {
                lead: "10".into(),
                tool: "update_chat".into(),
                args: json!({"id": 10, "title": "late rename"}),
                reply,
                deadline: std::time::Instant::now() - std::time::Duration::from_secs(1),
            };
            app.orchestrate(call, window, cx);
            assert_eq!(app.chats[1].title, title);
            assert_eq!(answer.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected));
        })
        .unwrap();
}
