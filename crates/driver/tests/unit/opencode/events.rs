use super::*;
use crate::opencode::{on_cancel, on_prompt, on_steer};
use crate::record::{fixtures, last_calls};
use aether_core::{
    ApprovalKind, ApprovalOption, FieldKind, NoticeKind, NoticeState, ToolCall, When,
};

fn ev(s: &mut State, line: &str) -> Vec<Update> {
    let v: Value = serde_json::from_str(line).unwrap();
    map_event(s, v["type"].as_str().unwrap(), &v["data"])
}

fn replay(scenario: &str) -> Vec<Update> {
    let values: Vec<Value> = crate::record::fixture("opencode", scenario)
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let root =
        values.iter().find_map(|v| v["data"]["sessionID"].as_str()).unwrap_or_default().to_string();
    let mut s = State { active: true, root, ..State::default() };
    let mut out = Vec::new();
    for v in &values {
        match v["aether"].as_str() {
            Some("prompt") => on_prompt(&mut s),
            Some("steer") => {
                on_steer(&s);
            }
            Some("cancel") => {
                on_cancel(&mut s);
            }
            Some("answer") => {}
            _ => out.extend(map_event(&mut s, v["type"].as_str().unwrap(), &v["data"])),
        }
        s.auto.clear();
    }
    out
}

#[test]
fn text_delta_then_succeeded_settles_once() {
    let mut s = State { active: true, root: "ses_1".into(), ..State::default() };
    let delta = r#"{"type":"session.text.delta","data":{"sessionID":"ses_1","delta":"hi"}}"#;
    assert_eq!(ev(&mut s, delta), [Update::Delta("hi".into())]);
    let ok = r#"{"type":"session.execution.succeeded","data":{"sessionID":"ses_1"}}"#;
    assert_eq!(ev(&mut s, ok), [Update::Final(String::new())], "the delta already said it");
    assert_eq!(ev(&mut s, ok), []);
    s.active = true;
    let shutdown = r#"{"type":"session.execution.interrupted","data":{"sessionID":"ses_1","reason":"shutdown"}}"#;
    assert!(matches!(&ev(&mut s, shutdown)[..], [Update::Notice(n)] if n.id == "info-service"));
    assert!(s.active, "a service stop suspends the run");
    ev(&mut s, r#"{"type":"form.cancelled","data":{"sessionID":"ses_1","id":"frm_1"}}"#);
    assert_eq!(ev(&mut s, shutdown), [Update::Final(String::new())], "a dismissed form ends it");
    assert!(!s.dismissed);
    s.active = true;
    let failed = r#"{"type":"session.execution.failed","data":{"sessionID":"ses_1","error":{"type":"x","message":"boom"}}}"#;
    assert_eq!(ev(&mut s, failed), [Update::Error("boom".into()), Update::Final(String::new())]);
}

#[test]
fn interrupts_by_reason() {
    let mut s = State { active: true, root: "ses_1".into(), ..State::default() };
    let at = |r: &str| {
        format!(
            r#"{{"type":"session.execution.interrupted","data":{{"sessionID":"ses_1","reason":"{r}"}}}}"#
        )
    };
    assert!(
        matches!(&ev(&mut s, &at("inactivity"))[..], [Update::Warning(w), Update::Final(_)] if w == "OpenCode stopped this turn (inactivity)")
    );
    s.active = true;
    assert!(matches!(&ev(&mut s, &at("superseded"))[..], [Update::Warning(_), Update::Final(_)]));
    s.active = true;
    assert_eq!(ev(&mut s, &at("user")), [Update::Final(String::new())]);
}

#[test]
fn inactivity_while_idle_stays_quiet() {
    let mut s = State { root: "ses_1".into(), ..State::default() };
    let line = r#"{"type":"session.execution.interrupted","data":{"sessionID":"ses_1","reason":"inactivity"}}"#;
    assert_eq!(ev(&mut s, line), []);
}

#[test]
fn an_idle_disconnect_marks_lost_without_an_error() {
    let mut s = State { root: "ses_1".into(), ..State::default() };
    let mut read = false;
    assert_eq!(
        disconnected(&mut s, || {
            read = true;
            "x".into()
        }),
        []
    );
    assert!(!read, "the log is read only for an active turn");
    assert!(s.lost && !s.usable() && s.dead.is_none());
    s.lost = false;
    assert!(s.usable());
    s.active = true;
    assert!(matches!(
        &disconnected(&mut s, || "t".into())[..],
        [Update::Error(_), Update::Final(_)]
    ));
    assert!(!s.usable());
}

#[test]
fn a_lost_service_fails_the_turn_with_its_log() {
    let mut s = State { active: true, root: "ses_1".into(), ..State::default() };
    let out = lost_service(&mut s, "panic: db locked");
    assert!(
        matches!(&out[..], [Update::Error(e), Update::Final(_)] if e == "OpenCode service stopped responding\npanic: db locked")
    );
    assert!(s.dead.is_some());
    assert_eq!(lost_service(&mut s, ""), []);
}

fn calls(updates: Vec<Update>) -> Vec<ToolCall> {
    updates
        .into_iter()
        .map(|u| match u {
            Update::Tool(call) => ToolCall { ms: None, ..call },
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn tool_events_upsert_one_step_and_reset_final_text() {
    let mut s = State { active: true, ..State::default() };
    ev(&mut s, r#"{"type":"session.text.delta","data":{"sessionID":"ses_1","delta":"before"}}"#);
    let bash = |command: &str, output: &str| ToolDetail::Command {
        command: command.into(),
        cwd: None,
        output: output.into(),
        exit_code: None,
    };
    let tool =
        |detail, status| ToolCall { status, ..ToolCall::new("call_1", "Run command", detail) };
    let started = r#"{"type":"session.tool.input.started","data":{"sessionID":"ses_1","id":"call_1","name":"bash"}}"#;
    assert_eq!(calls(ev(&mut s, started)), [tool(bash("", ""), ToolStatus::Running)]);
    let called = r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"call_1","input":{"command":"ls"},"executed":true}}"#;
    assert_eq!(calls(ev(&mut s, called)), [tool(bash("ls", ""), ToolStatus::Running)]);
    let progress = r#"{"type":"session.tool.progress","data":{"sessionID":"ses_1","id":"call_1","metadata":{"output":"a.rs\n"}}}"#;
    assert_eq!(calls(ev(&mut s, progress)), [tool(bash("ls", "a.rs\n"), ToolStatus::Running)]);
    let failed = r#"{"type":"session.tool.failed","data":{"sessionID":"ses_1","id":"call_1","error":{"message":"nope"}}}"#;
    let failed = calls(ev(&mut s, failed));
    assert_eq!(
        failed,
        [ToolCall { error: Some("nope".into()), ..tool(bash("ls", "nope"), ToolStatus::Failed) }]
    );
    assert!(s.tools.is_empty());
    let ok = r#"{"type":"session.execution.succeeded","data":{"sessionID":"ses_1"}}"#;
    assert_eq!(ev(&mut s, ok), [Update::Final(String::new())]);
}

#[test]
fn rejected_permissions_and_cancels_settle_tools_as_denied_or_stopped() {
    let mut s = State { active: true, ..State::default() };
    ev(
        &mut s,
        r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"call_1","name":"bash","input":{"command":"rm x"}}}"#,
    );
    ev(
        &mut s,
        r#"{"type":"permission.asked","data":{"id":"per_1","sessionID":"ses_1","action":"bash","resources":["rm x"],"source":{"type":"tool","messageID":"m","id":"call_1"}}}"#,
    );
    assert_eq!(
        ev(
            &mut s,
            r#"{"type":"permission.replied","data":{"sessionID":"ses_1","requestID":"per_1","reply":"reject"}}"#
        ),
        [Update::AskResolved(1)]
    );
    assert!(s.asks.is_empty());
    let failed = r#"{"type":"session.tool.failed","data":{"sessionID":"ses_1","id":"call_1","error":{"message":"rejected"}}}"#;
    assert_eq!(calls(ev(&mut s, failed))[0].status, ToolStatus::Denied);
    ev(
        &mut s,
        r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"call_2","name":"bash","input":{"command":"sleep 9"}}}"#,
    );
    s.cancelled = true;
    let cut = r#"{"type":"session.tool.failed","data":{"sessionID":"ses_1","id":"call_2","error":{"message":"aborted"}}}"#;
    assert_eq!(calls(ev(&mut s, cut))[0].status, ToolStatus::Stopped);
}

#[test]
fn edit_success_prefers_the_metadata_diff_and_bash_reads_exit() {
    let mut s = State { active: true, ..State::default() };
    let called = r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"e1","name":"edit","input":{"filePath":"/p/a.rs","oldString":"a","newString":"b"}}}"#;
    ev(&mut s, called);
    let diff = "Index: /p/a.rs\n===\n--- /p/a.rs\n+++ /p/a.rs\n@@ -3,1 +3,1 @@\n-a\n+b\n";
    let success = serde_json::json!({"type": "session.tool.success", "data": {"sessionID": "ses_1", "id": "e1", "content": [{"type": "text", "text": "Edit applied"}], "metadata": {"diff": diff}}});
    let [call] = calls(ev(&mut s, &success.to_string())).try_into().unwrap();
    assert_eq!(
        call.detail,
        ToolDetail::Edit { path: "/p/a.rs".into(), diff: diff.into(), added: 1, removed: 1 }
    );
    assert_eq!((call.title.as_str(), call.status), ("Edit file", ToolStatus::Done));
    let called = r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"b1","name":"bash","input":{"command":"false"}}}"#;
    ev(&mut s, called);
    let done = r#"{"type":"session.tool.success","data":{"sessionID":"ses_1","id":"b1","content":[{"type":"text","text":""}],"metadata":{"exit":1}}}"#;
    let [call] = calls(ev(&mut s, done)).try_into().unwrap();
    assert!(matches!(call.detail, ToolDetail::Command { exit_code: Some(1), .. }), "{call:?}");
}

#[test]
fn permission_auto_replies_or_asks_by_access() {
    let asked = r#"{"type":"permission.asked","data":{"id":"per_1","sessionID":"ses_1","action":"edit","resources":["src/a.rs"]}}"#;
    let mut s = State { access: AccessMode::AutoEdit, root: "ses_1".into(), ..State::default() };
    assert_eq!(ev(&mut s, asked), []);
    assert_eq!(s.auto, [("per_1".to_string(), "ses_1".to_string())]);
    assert!(auto_approves(AccessMode::AutoEdit, "edit"));
    assert!(
        !auto_approves(AccessMode::AutoEdit, "bash") && auto_approves(AccessMode::Full, "bash")
    );
    let mut s = State { root: "ses_1".into(), ..State::default() };
    let ask = Ask::Approval(Approval {
        tool: "edit".into(),
        title: "src/a.rs".into(),
        input: "src/a.rs".into(),
        kind: ApprovalKind::Edit,
        ..Approval::default()
    });
    assert_eq!(ev(&mut s, asked), [Update::Ask { id: 1, ask }]);
    assert!(
        matches!(s.asks.get(&1), Some(OpencodeAsk::Permission { request, .. }) if request == "per_1")
    );
    assert!(s.auto.is_empty());
}

#[test]
fn asks_answered_elsewhere_are_dropped_and_outcomes_always_settle() {
    let mut s = State { active: true, root: "ses_1".into(), ..State::default() };
    let asked = r#"{"type":"permission.asked","data":{"id":"per_1","sessionID":"ses_1","action":"shell","resources":["ls"]}}"#;
    assert_eq!(ev(&mut s, asked).len(), 1);
    assert_eq!(ev(&mut s, asked), [], "a re-surfaced request asks once");
    let replied = r#"{"type":"permission.replied","data":{"sessionID":"ses_1","requestID":"per_1","reply":"once"}}"#;
    ev(&mut s, replied);
    assert!(s.asks.is_empty() && s.denied.is_empty());
    ev(&mut s, FORM);
    let cancelled = r#"{"type":"form.cancelled","data":{"id":"frm_1","sessionID":"ses_1"}}"#;
    ev(&mut s, cancelled);
    assert!(s.asks.is_empty());
    ev(&mut s, asked);
    let stopped =
        r#"{"type":"session.execution.interrupted","data":{"sessionID":"ses_1","reason":"user"}}"#;
    assert_eq!(ev(&mut s, stopped), [Update::Final(String::new())]);
    assert!(s.asks.is_empty());
}

const FORM: &str = r#"{"type":"form.created","location":{"directory":"/p"},"data":{"form":{"id":"frm_1","sessionID":"ses_1","title":"Questions","metadata":{"kind":"question","tool":{"messageID":"msg_1","id":"call_q"}},"fields":[{"key":"q0","title":"Database","description":"Which database?","type":"multiselect","options":[{"value":"Postgres","label":"Postgres","description":"Production"},{"value":"SQLite","label":"SQLite"}],"custom":true},{"key":"q1","title":"Name","description":"Project name?","type":"string","options":[],"custom":true}]}}}"#;

#[test]
fn question_forms_are_one_card() {
    let mut s = State::default();
    let [Update::Ask { id, ask: Ask::Form(form) }] = &ev(&mut s, FORM)[..] else { panic!() };
    assert_eq!((form.title.as_str(), form.fields.len()), ("Questions", 2));
    assert_eq!((form.fields[0].kind, form.fields[0].other), (FieldKind::Multi, true));
    assert_eq!(form.fields[0].options[0].description, "Production");
    assert_eq!(form.fields[1].kind, FieldKind::Text);
    assert_eq!(ev(&mut s, FORM), [], "a repeated form asks once");
    let Some(OpencodeAsk::Form { id: form_id, fields, .. }) = s.asks.get(id) else { panic!() };
    assert_eq!((form_id.as_str(), fields.len()), ("frm_1", 2));
    let replied =
        r#"{"type":"form.replied","data":{"id":"frm_1","sessionID":"ses_1","answer":{}}}"#;
    let end = Update::Final(String::new());
    assert_eq!(ev(&mut s, replied), [Update::AskResolved(*id), end], "idle: its side turn ends");
    assert!(s.asks.is_empty());
}

#[test]
fn an_ask_resolved_elsewhere_ends_only_an_idle_side_turn() {
    let asked = r#"{"type":"permission.asked","data":{"id":"per_1","sessionID":"ses_1","action":"shell","resources":["ls"]}}"#;
    let replied = r#"{"type":"permission.replied","data":{"sessionID":"ses_1","requestID":"per_1","reply":"once"}}"#;
    let mut s = State { active: false, ..root() };
    let [Update::Ask { id, .. }] = &ev(&mut s, asked)[..] else { panic!() };
    assert_eq!(ev(&mut s, replied), [Update::AskResolved(*id), Update::Final(String::new())]);
    s.active = true;
    let [Update::Ask { id, .. }] = &ev(&mut s, asked)[..] else { panic!() };
    assert_eq!(ev(&mut s, replied), [Update::AskResolved(*id)], "the live turn goes on");
}

#[test]
fn dismissed_question_forms_end_their_tool_denied() {
    let mut s = State { active: true, ..State::default() };
    let called = r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","assistantMessageID":"msg_1","id":"call_q","name":"question","input":{"questions":[{"question":"Which database?","options":[{"label":"Postgres"}]}]},"executed":false}}"#;
    ev(&mut s, called);
    ev(&mut s, FORM);
    deny(&mut s, "frm_1");
    let failed = r#"{"type":"session.tool.failed","data":{"sessionID":"ses_1","assistantMessageID":"msg_1","id":"call_q","error":{"type":"tool.execution","message":"The user dismissed this question"},"executed":false}}"#;
    assert_eq!(calls(ev(&mut s, failed))[0].status, ToolStatus::Denied);
    assert!(s.denied.is_empty() && s.calls.is_empty());
}

#[test]
fn question_tools_are_plain_tool_rows() {
    let mut s = State::default();
    let question = r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"call_q","name":"question","input":{"questions":[{"header":"About","question":"What?","options":[{"label":"A"}]}]}}}"#;
    let [call] = calls(ev(&mut s, question)).try_into().unwrap();
    assert_eq!(call.title, "Ask question");
    let mcp = r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"call_m","name":"ask_question","input":{"question":"Proceed?"}}}"#;
    assert_eq!(calls(ev(&mut s, mcp)).len(), 1);
    assert!(s.asks.is_empty());
}

#[test]
fn step_tokens_count_cache_as_prompt_and_reasoning_as_completion() {
    let mut s = State::default();
    let ended = r#"{"type":"session.step.ended","data":{"sessionID":"ses_1","tokens":{"input":3923,"output":10,"reasoning":12,"cache":{"read":17344,"write":5}}}}"#;
    assert_eq!(
        ev(&mut s, ended),
        [Update::Usage(21294), Update::Count { prompt: 21272, completion: 22, cached: 17344 }]
    );
}

#[test]
fn step_failures_report_once_when_the_run_ends() {
    let failed = r#"{"type":"session.step.failed","data":{"sessionID":"ses_1","assistantMessageID":"msg_1","error":{"type":"provider.auth","message":"Insufficient balance."}}}"#;
    let mut s = State { active: true, ..State::default() };
    assert_eq!(ev(&mut s, failed), []);
    let ok = r#"{"type":"session.execution.succeeded","data":{"sessionID":"ses_1"}}"#;
    let error = Update::Error("Insufficient balance.".into());
    assert_eq!(ev(&mut s, ok), [error.clone(), Update::Final(String::new())]);
    s.active = true;
    ev(&mut s, failed);
    let run_failed = r#"{"type":"session.execution.failed","data":{"sessionID":"ses_1","error":{"type":"provider.auth","message":"Insufficient balance."}}}"#;
    assert_eq!(ev(&mut s, run_failed), [error, Update::Final(String::new())]);
    s.active = true;
    ev(&mut s, failed);
    ev(&mut s, r#"{"type":"session.step.started","data":{"sessionID":"ses_1"}}"#);
    assert_eq!(ev(&mut s, ok), [Update::Final(String::new())], "a retried step recovered");
    s.active = true;
    let aborted = r#"{"type":"session.step.failed","data":{"sessionID":"ses_1","error":{"type":"aborted","message":"x"}}}"#;
    ev(&mut s, aborted);
    assert_eq!(ev(&mut s, ok), [Update::Final(String::new())]);
}

#[test]
fn deleted_session_ends_the_turn_and_the_driver() {
    let mut s = State { active: true, ..State::default() };
    let why = "OpenCode session was deleted";
    let deleted = r#"{"type":"session.deleted","data":{"sessionID":"ses_1"}}"#;
    assert_eq!(ev(&mut s, deleted), [Update::Error(why.into()), Update::Final(String::new())]);
    assert_eq!(s.dead.as_deref(), Some(why));
}

#[test]
fn prompt_cancelled_from_the_inbox_settles_the_turn() {
    let mut s = State { active: true, prompt: "msg_1".into(), ..State::default() };
    let cancelled =
        r#"{"type":"session.inbox.cancelled","data":{"sessionID":"ses_1","inboxID":"msg_1"}}"#;
    let why = Update::Error("OpenCode cancelled the prompt".into());
    assert_eq!(ev(&mut s, cancelled), [why, Update::Final(String::new())]);
    assert_eq!(ev(&mut s, cancelled), []);
}

#[test]
fn compaction_notice_carries_the_summary_or_the_failure() {
    let mut s = root();
    let notice = |out: Vec<Update>| match &out[..] {
        [Update::Notice(n)] => n.clone(),
        other => panic!("{other:?}"),
    };
    let started = r#"{"type":"session.compaction.started","data":{"sessionID":"ses_1","reason":"manual","recent":""}}"#;
    let n = notice(ev(&mut s, started));
    assert_eq!((n.kind, n.state), (NoticeKind::Compaction, NoticeState::Running));
    let delta =
        json!({"type": "session.compaction.delta", "data": {"sessionID": "ses_1", "text": "##"}});
    assert_eq!(ev(&mut s, &delta.to_string()), []);
    let ended = json!({"type": "session.compaction.ended", "data": {"sessionID": "ses_1", "text": "## Objective", "recent": ""}});
    let done = notice(ev(&mut s, &ended.to_string()));
    assert_eq!((done.id.as_str(), done.state), (n.id.as_str(), NoticeState::Done));
    assert_eq!(done.detail.as_deref(), Some("## Objective"));
    ev(&mut s, started);
    let failed = r#"{"type":"session.compaction.failed","data":{"sessionID":"ses_1","error":{"type":"provider.auth","message":"Insufficient balance."}}}"#;
    let n = notice(ev(&mut s, failed));
    assert_eq!((n.state, n.text.as_str()), (NoticeState::Failed, "Insufficient balance."));
    assert!(s.tools.is_empty());
}

#[test]
fn shell_approval_lists_each_command_and_what_always_covers() {
    let mut s = State { active: true, ..State::default() };
    let asked = r#"{"type":"permission.asked","data":{"id":"per_1","sessionID":"ses_1","action":"shell","resources":["ls src","cat package.json"],"save":["ls *","cat *"],"message":"Inspect the project"}}"#;
    let [Update::Ask { ask: Ask::Approval(Approval { input, reason, options, .. }), .. }] =
        &ev(&mut s, asked)[..]
    else {
        panic!()
    };
    assert_eq!(input, "ls src\ncat package.json");
    assert_eq!(
        reason,
        &format!(
            "Inspect the project\n{}ls *, cat *",
            "Always allows for this whole project, in every chat, until removed in OpenCode: "
        )
    );
    assert_eq!(options[0].label, "Always allow ls *, cat *");
}

#[test]
fn edit_approval_shows_the_patch() {
    let mut s = State { active: true, ..State::default() };
    let asked = r#"{"type":"permission.asked","data":{"id":"per_2","sessionID":"ses_1","action":"edit","resources":["src/a.ts"],"metadata":{"files":[{"file":"src/a.ts","patch":"@@ -1 +1 @@\n-a\n+b"}]}}}"#;
    let [Update::Ask { ask: Ask::Approval(Approval { input, diff, .. }), .. }] =
        &ev(&mut s, asked)[..]
    else {
        panic!()
    };
    assert_eq!(input, "src/a.ts");
    assert_eq!(diff.as_deref(), Some("@@ -1 +1 @@\n-a\n+b"));
}

#[test]
fn decisions_map_answers_to_server_replies() {
    assert_eq!(decision(&Answer::Pick("always".into())), ("always", None));
    assert_eq!(decision(&Answer::Pick("other".into())), ("once", None));
    assert_eq!(decision(&Answer::Approve { input: String::new(), always: true }), ("once", None));
    assert_eq!(decision(&Answer::Reply("x".into())), ("once", None));
    assert_eq!(decision(&Answer::Deny), ("reject", None));
    assert_eq!(decision(&Answer::DenyWith("no".into())), ("reject", Some("no".into())));
}

#[test]
fn apply_patch_and_mcp_asks_show_their_content() {
    let mut s = State { active: true, ..State::default() };
    let patch = r#"{"type":"permission.asked","data":{"id":"per_3","sessionID":"ses_1","action":"apply_patch","resources":["a.ts"],"metadata":{"diff":"-a\n+b"}}}"#;
    let [Update::Ask { ask: Ask::Approval(Approval { input, diff, .. }), .. }] =
        &ev(&mut s, patch)[..]
    else {
        panic!()
    };
    assert_eq!((input.as_str(), diff.as_deref()), ("a.ts", Some("-a\n+b")));
    let mcp = r#"{"type":"permission.asked","data":{"id":"per_4","sessionID":"ses_1","action":"mcp","resources":["srv_tool"],"save":["*"],"metadata":{"input":{"q":"x"}}}}"#;
    let [Update::Ask { ask: Ask::Approval(Approval { input, reason, .. }), .. }] =
        &ev(&mut s, mcp)[..]
    else {
        panic!()
    };
    assert_eq!(input, "srv_tool\n{\n  \"q\": \"x\"\n}");
    assert_eq!(reason, "");
}

#[test]
fn child_session_asks_are_labelled_and_keep_their_session() {
    let mut s = State { active: true, root: "ses_1".into(), ..State::default() };
    let asked = r#"{"type":"permission.asked","data":{"id":"per_9","sessionID":"ses_child","action":"shell","resources":["pnpm add motion"]}}"#;
    let [Update::Ask { id, ask: Ask::Approval(Approval { title, source, .. }) }] =
        &ev(&mut s, asked)[..]
    else {
        panic!()
    };
    assert_eq!((title.as_str(), source.as_deref()), ("pnpm add motion", Some("Subagent")));
    assert!(
        matches!(&s.asks[id], OpencodeAsk::Permission { session, .. } if session == "ses_child")
    );
    let form = r#"{"type":"form.created","data":{"form":{"id":"frm_2","sessionID":"ses_child","fields":[{"key":"k","title":"Pick"}]}}}"#;
    let [Update::Ask { id, .. }] = &ev(&mut s, form)[..] else { panic!() };
    assert!(matches!(&s.asks[id], OpencodeAsk::Form { session, .. } if session == "ses_child"));
}

#[test]
fn child_auto_approval_keeps_the_child_session() {
    let mut s = State { access: AccessMode::AutoEdit, root: "ses_1".into(), ..State::default() };
    let asked = r#"{"type":"permission.asked","data":{"id":"per_3","sessionID":"ses_child","action":"edit","resources":["a.rs"]}}"#;
    assert_eq!(ev(&mut s, asked), []);
    assert_eq!(s.auto, [("per_3".to_string(), "ses_child".to_string())]);
}

#[test]
fn child_form_cancel_does_not_dismiss_the_root_turn() {
    let mut s = State { active: true, root: "ses_1".into(), ..State::default() };
    ev(&mut s, r#"{"type":"form.cancelled","data":{"sessionID":"ses_child","id":"frm_1"}}"#);
    assert!(!s.dismissed);
    ev(&mut s, r#"{"type":"form.cancelled","data":{"sessionID":"ses_1","id":"frm_2"}}"#);
    assert!(s.dismissed);
}

#[test]
fn every_fixture_replays_without_panicking() {
    let all = fixtures("opencode");
    assert!(!all.is_empty());
    for (name, _) in all {
        let out = replay(&name);
        assert!(!out.is_empty(), "{name} produced no updates");
    }
}

const SUB_START: &str = r#"{"type":"session.tool.input.started","data":{"sessionID":"ses_1","assistantMessageID":"m","id":"call_t","name":"subagent"}}"#;
const SUB_CALLED: &str = r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"call_t","input":{"agent":"explore","description":"Find styles","prompt":"look in src"},"executed":true}}"#;
const SUB_PROGRESS: &str = r#"{"type":"session.tool.progress","data":{"sessionID":"ses_1","id":"call_t","metadata":{"sessionID":"ses_c","status":"running"}}}"#;
const CHILD_READ: &str = r#"{"type":"session.tool.input.started","data":{"sessionID":"ses_c","assistantMessageID":"m2","id":"call_r","name":"read"}}"#;

fn root() -> State {
    State { active: true, root: "ses_1".into(), ..State::default() }
}

fn tool_status(out: &[Update], id: &str) -> Option<ToolStatus> {
    out.iter()
        .filter_map(|u| match u {
            Update::Tool(c) if c.id == id => Some(c.status),
            _ => None,
        })
        .next_back()
}

#[test]
fn child_session_tools_nest_under_their_subagent_row() {
    let mut s = root();
    for line in [SUB_START, SUB_CALLED, SUB_PROGRESS] {
        ev(&mut s, line);
    }
    let calls = last_calls(&ev(&mut s, CHILD_READ));
    assert!(
        calls.iter().any(|c| c.id == "call_r" && c.parent.as_deref() == Some("call_t")),
        "{calls:?}"
    );
    assert!(calls.iter().any(|c| c.id == "call_t"
        && matches!(&c.detail, ToolDetail::Task { tools: 1, agent, .. } if agent == "explore")));
    let child_text = r#"{"type":"session.text.delta","data":{"sessionID":"ses_c","assistantMessageID":"m2","ordinal":0,"delta":"hi"}}"#;
    assert!(!ev(&mut s, child_text).iter().any(|u| matches!(u, Update::Delta(_))));
}

#[test]
fn child_step_text_and_failure_land_on_the_task_row() {
    let mut s = root();
    for line in [SUB_START, SUB_CALLED, SUB_PROGRESS] {
        ev(&mut s, line);
    }
    let text =
        r#"{"type":"session.text.ended","data":{"sessionID":"ses_c","text":"Found it.\nMore"}}"#;
    let calls = last_calls(&ev(&mut s, text));
    assert!(
        matches!(&calls[0].detail, ToolDetail::Task { activity, .. } if activity == "Found it.")
    );
    let step = r#"{"type":"session.step.ended","data":{"sessionID":"ses_c","cost":0.5,"tokens":{"input":10,"output":5,"reasoning":1,"cache":{"read":9,"write":0}}}}"#;
    let out = ev(&mut s, step);
    assert!(out.contains(&Update::Cost(500_000)), "{out:?}");
    let calls = last_calls(&out);
    assert!(matches!(&calls[0].detail, ToolDetail::Task { tokens: 16, .. }), "{calls:?}");
    let failed = r#"{"type":"session.execution.failed","data":{"sessionID":"ses_c","error":{"message":"boom"}}}"#;
    let calls = last_calls(&ev(&mut s, failed));
    assert_eq!((calls[0].status, calls[0].error.as_deref()), (ToolStatus::Failed, Some("boom")));
}

#[test]
fn background_subagent_stays_live_until_its_session_ends() {
    let mut s = root();
    for line in [SUB_START, SUB_CALLED, SUB_PROGRESS] {
        ev(&mut s, line);
    }
    let ok = r#"{"type":"session.tool.success","data":{"sessionID":"ses_1","id":"call_t","content":[{"type":"text","text":"started"}],"metadata":{"sessionID":"ses_c","status":"running"},"executed":true}}"#;
    assert_eq!(tool_status(&ev(&mut s, ok), "call_t"), Some(ToolStatus::Background));
    let done = r#"{"type":"session.execution.succeeded","data":{"sessionID":"ses_c"}}"#;
    assert_eq!(tool_status(&ev(&mut s, done), "call_t"), Some(ToolStatus::Done));
    assert!(s.background.is_empty());
}

#[test]
fn execute_tool_calls_become_nested_rows() {
    let mut s = root();
    let ok = r#"{"type":"session.tool.success","data":{"sessionID":"ses_1","id":"call_x","content":[],"executed":true,"metadata":{"toolCalls":[{"tool":"opencode.read","status":"completed","input":{"filePath":"a.ts"}},{"tool":"opencode.bash","status":"error","input":{"command":"ls"}}]}}}"#;
    ev(
        &mut s,
        r#"{"type":"session.tool.input.started","data":{"sessionID":"ses_1","id":"call_x","name":"execute"}}"#,
    );
    let calls = last_calls(&ev(&mut s, ok));
    let nested: Vec<(&str, ToolStatus)> = calls
        .iter()
        .filter(|c| c.parent.as_deref() == Some("call_x"))
        .map(|c| (c.id.as_str(), c.status))
        .collect();
    assert_eq!(nested, [("call_x#0", ToolStatus::Done), ("call_x#1", ToolStatus::Failed)]);
}

#[test]
fn skill_tool_is_a_skill_step_not_a_row() {
    let mut s = root();
    let start = r#"{"type":"session.tool.input.started","data":{"sessionID":"ses_1","id":"call_s","name":"skill"}}"#;
    let ok = r#"{"type":"session.tool.success","data":{"sessionID":"ses_1","id":"call_s","content":[],"executed":true,"metadata":{"name":"pdf"}}}"#;
    let out = [ev(&mut s, start), ev(&mut s, ok)].concat();
    assert_eq!(out, [Update::Skill("pdf".into())]);
    let activated =
        r#"{"type":"session.skill.activated","data":{"sessionID":"ses_1","name":"docx"}}"#;
    assert_eq!(ev(&mut s, activated), [Update::Skill("docx".into())]);
}

#[test]
fn execution_started_without_a_turn_wakes_once() {
    let mut s = State { root: "ses_1".into(), ..State::default() };
    let started = r#"{"type":"session.execution.started","data":{"sessionID":"ses_1"}}"#;
    assert!(
        matches!(&ev(&mut s, started)[..], [Update::Wake, Update::Notice(n)] if n.kind == NoticeKind::Wake)
    );
    assert!(s.active);
    assert_eq!(ev(&mut s, started), []);
}

#[test]
fn subagent_fixture_nests_child_rows() {
    let calls = last_calls(&replay("subagent-children.hand"));
    let task =
        calls.iter().find(|c| matches!(c.detail, ToolDetail::Task { .. })).expect("a task row");
    assert!(calls.iter().any(|c| c.parent.as_deref() == Some(task.id.as_str())), "{calls:?}");
    assert_eq!(task.status, ToolStatus::Done);
    let real = last_calls(&replay("subagent"));
    assert!(real.iter().any(|c| matches!(c.detail, ToolDetail::Task { .. })));
}

#[test]
fn background_fixture_ends_with_a_settled_row() {
    let calls = last_calls(&replay("background"));
    assert!(calls.iter().all(|c| c.status != ToolStatus::Running), "{calls:?}");
}

#[test]
fn retry_is_a_countdown_notice_that_closes_on_progress() {
    let mut s = root();
    let retry = r#"{"type":"session.retry.scheduled","data":{"sessionID":"ses_1","assistantMessageID":"m","attempt":2,"at":1800000000000,"error":{"type":"provider.rate-limit","message":"overloaded"}}}"#;
    let [Update::Notice(n)] = &ev(&mut s, retry)[..] else { panic!() };
    assert_eq!(
        (n.kind, n.state, n.until_ms),
        (NoticeKind::Retry, NoticeState::Running, Some(1_800_000_000_000))
    );
    assert!(n.text.contains("attempt 2") && n.text.contains("overloaded"));
    let step = r#"{"type":"session.step.started","data":{"sessionID":"ses_1","assistantMessageID":"m","agent":"build","model":{"id":"x","providerID":"p"},"started":1}}"#;
    assert!(
        ev(&mut s, step).iter().any(
            |u| matches!(u, Update::Notice(d) if d.id == n.id && d.state == NoticeState::Done)
        )
    );
    assert_eq!(s.model, Some(json!({"providerID": "p", "id": "x"})));
}

#[test]
fn step_cost_length_and_content_filter_are_not_errors() {
    let mut s = root();
    let ended = r#"{"type":"session.step.ended","data":{"sessionID":"ses_1","assistantMessageID":"m","finish":"length","cost":0.0123,"files":["a.rs"],"tokens":{"input":1,"output":1,"reasoning":0,"cache":{"read":0,"write":0}}}}"#;
    let out = ev(&mut s, ended);
    assert!(out.contains(&Update::Cost(12_300)));
    assert!(out.iter().any(|u| matches!(u, Update::Warning(_))));
    assert_eq!(s.changed, ["a.rs"]);
    let filtered = r#"{"type":"session.step.failed","data":{"sessionID":"ses_1","assistantMessageID":"m","finish":"content-filter","error":{"type":"provider.content-filter","message":"blocked"}}}"#;
    assert!(ev(&mut s, filtered).iter().any(|u| matches!(u, Update::Warning(_))));
    let ok = r#"{"type":"session.execution.succeeded","data":{"sessionID":"ses_1"}}"#;
    assert!(!ev(&mut s, ok).iter().any(|u| matches!(u, Update::Error(_))));
}

#[test]
fn text_ended_repairs_missed_deltas() {
    let mut s = root();
    ev(
        &mut s,
        r#"{"type":"session.text.delta","data":{"sessionID":"ses_1","assistantMessageID":"m","ordinal":0,"delta":"Hel"}}"#,
    );
    let ended = r#"{"type":"session.text.ended","data":{"sessionID":"ses_1","assistantMessageID":"m","ordinal":0,"text":"Hello"}}"#;
    assert_eq!(ev(&mut s, ended), [Update::Delta("lo".into())]);
    assert_eq!(ev(&mut s, ended), [], "a repeated end emits nothing");
    let whole = r#"{"type":"session.text.ended","data":{"sessionID":"ses_1","assistantMessageID":"m2","ordinal":0,"text":""}}"#;
    assert_eq!(ev(&mut s, whole), []);
}

#[test]
fn text_ended_never_garbles_gaps() {
    let mut s = root();
    let delta = |m: &str, d: &str| {
        format!(
            r#"{{"type":"session.text.delta","data":{{"sessionID":"ses_1","assistantMessageID":"{m}","ordinal":0,"delta":"{d}"}}}}"#
        )
    };
    let ended = |m: &str, t: &str| {
        format!(
            r#"{{"type":"session.text.ended","data":{{"sessionID":"ses_1","assistantMessageID":"{m}","ordinal":0,"text":"{t}"}}}}"#
        )
    };
    ev(&mut s, &delta("mid", "Hel"));
    ev(&mut s, &delta("mid", "o w"));
    assert_eq!(ev(&mut s, &ended("mid", "Hello world")), [], "middle gap");
    ev(&mut s, &delta("pre", "llo"));
    assert_eq!(ev(&mut s, &ended("pre", "Hello")), [], "prefix gap");
    assert_eq!(ev(&mut s, &ended("none", "all")), [Update::Delta("all".into())]);
}

#[test]
fn compactions_get_unique_notice_ids_and_a_summary() {
    let mut s = root();
    let started = r#"{"type":"session.compaction.started","data":{"sessionID":"ses_1","reason":"manual","recent":1}}"#;
    let ended = r#"{"type":"session.compaction.ended","data":{"sessionID":"ses_1","reason":"manual","text":"Summary.","recent":1}}"#;
    let ids: Vec<String> = [started, ended, started, ended]
        .iter()
        .flat_map(|l| ev(&mut s, l))
        .filter_map(|u| match u {
            Update::Notice(n) => Some(n.id),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[0], ids[2]);
}

#[test]
fn approvals_offer_the_save_patterns_and_resolve_elsewhere() {
    let mut s = root();
    let asked = r#"{"type":"permission.asked","data":{"id":"per_1","sessionID":"ses_1","action":"shell","resources":["ls src"],"save":["ls *"],"message":"Inspect","source":{"type":"tool","messageID":"m","id":"call_b"}}}"#;
    let [Update::Ask { id, ask: Ask::Approval(a) }] = &ev(&mut s, asked)[..] else { panic!() };
    assert_eq!(
        (a.kind, a.tool_id.as_deref(), a.reason.as_str()),
        (
            ApprovalKind::Command,
            Some("call_b"),
            "Inspect\nAlways allows for this whole project, in every chat, until removed in OpenCode: ls *"
        )
    );
    assert_eq!(
        a.options,
        [ApprovalOption { id: "always".into(), label: "Always allow ls *".into() }]
    );
    let replied = r#"{"type":"permission.replied","data":{"sessionID":"ses_1","requestID":"per_1","reply":"once"}}"#;
    assert_eq!(ev(&mut s, replied), [Update::AskResolved(*id)]);
    let write = r#"{"type":"permission.asked","data":{"id":"per_2","sessionID":"ses_1","action":"edit","resources":["a.txt"],"save":["*"]}}"#;
    assert!(
        matches!(&ev(&mut s, write)[..], [Update::Ask { ask: Ask::Approval(a), .. }] if a.options.is_empty())
    );
    let mixed = r#"{"type":"permission.asked","data":{"id":"per_4","sessionID":"ses_1","action":"edit","resources":["a.txt"],"save":["src/*","*"]}}"#;
    assert!(
        matches!(&ev(&mut s, mixed)[..], [Update::Ask { ask: Ask::Approval(a), .. }] if a.options.is_empty())
    );
    let none = r#"{"type":"permission.asked","data":{"id":"per_3","sessionID":"ses_1","action":"edit","resources":["a.txt"],"save":[]}}"#;
    assert!(
        matches!(&ev(&mut s, none)[..], [Update::Ask { ask: Ask::Approval(a), .. }] if a.options.is_empty())
    );
}

#[test]
fn hidden_fields_reply_with_their_defaults() {
    let form = json!({"id":"frm_1","sessionID":"ses_1","title":"Deploy","fields":[
        {"key":"env","type":"string","title":"Env","options":[{"value":"prod","label":"Production"}],"required":true},
        {"key":"token","type":"string","hidden":true,"default":"abc"},
        {"key":"why","type":"string","title":"Why","when":[{"key":"env","op":"eq","value":"prod"}]},
        {"key":"n","type":"integer","default":2},
        {"key":"tags","type":"multiselect","options":[{"value":"a","label":"A"}],"custom":true},
        {"key":"docs","type":"external","url":"https://docs.test"}]});
    let (ask, raw) = form_ask(&form, None);
    assert_eq!(ask.url.as_deref(), Some("https://docs.test"));
    let keys: Vec<&str> = ask.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["env", "why", "n", "tags"]);
    assert_eq!(
        (ask.fields[0].kind, ask.fields[3].kind, ask.fields[3].other),
        (FieldKind::Choice, FieldKind::Multi, true)
    );
    assert_eq!(ask.fields[1].when, [When { key: "env".into(), not: false, value: "prod".into() }]);
    let answer = Answer::Form(vec![
        ("env".into(), vec!["prod".into()]),
        ("why".into(), vec!["hotfix".into()]),
        ("n".into(), vec!["3".into()]),
        ("tags".into(), vec!["a".into(), "b".into()]),
    ]);
    assert_eq!(
        form_reply(&raw, &answer),
        json!({"answer": {"env": "prod", "token": "abc", "why": "hotfix", "n": 3, "tags": ["a", "b"]}})
    );
    let dev = Answer::Form(vec![
        ("env".into(), vec!["dev".into()]),
        ("why".into(), vec!["ignored".into()]),
    ]);
    assert_eq!(form_reply(&raw, &dev), json!({"answer": {"env": "dev", "token": "abc"}}));
}

#[test]
fn steers_report_delivery_and_refusal() {
    let mut s = root();
    s.steers.insert("inb_1".into(), "also dark mode".into());
    s.steers.insert("inb_2".into(), "never mind".into());
    let delivered =
        r#"{"type":"session.inbox.delivered","data":{"sessionID":"ses_1","inboxID":"inb_1"}}"#;
    assert_eq!(ev(&mut s, delivered), [Update::Steered("also dark mode".into())]);
    let cancelled =
        r#"{"type":"session.inbox.cancelled","data":{"sessionID":"ses_1","inboxID":"inb_2"}}"#;
    assert_eq!(ev(&mut s, cancelled), [Update::SteerRefused("never mind".into())]);
    assert_eq!(ev(&mut s, cancelled), [], "an unknown inbox id is not a steer");
}

#[test]
fn model_selection_and_synthetic_messages_become_updates() {
    let mut s = root();
    let selected = r#"{"type":"session.model.selected","data":{"sessionID":"ses_1","model":{"providerID":"p","id":"m2"},"previous":{"providerID":"p","id":"m1"}}}"#;
    let out = ev(&mut s, selected);
    assert_eq!(out[0], Update::Model("p/m2".into()));
    assert!(
        matches!(&out[1], Update::Notice(n) if n.kind == NoticeKind::ModelChange && n.text == "Model changed to m2")
    );
    let synthetic =
        r#"{"type":"session.synthetic","data":{"sessionID":"ses_1","text":"Hook said hi\nmore"}}"#;
    assert!(
        matches!(&ev(&mut s, synthetic)[..], [Update::Notice(n)] if n.text == "Hook said hi" && n.kind == NoticeKind::Info)
    );
}

#[test]
fn multi_file_edits_get_one_row_each_and_overwrites_are_titled() {
    let mut s = root();
    let called = r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"p1","name":"patch","input":{}}}"#;
    ev(&mut s, called);
    let patch = |f: &str| format!("--- {f}\n+++ {f}\n@@ -1 +1 @@\n-a\n+b\n");
    let ok = json!({"type": "session.tool.success", "data": {"sessionID": "ses_1", "id": "p1", "content": [], "metadata": {"files": [
        {"file": "a.rs", "patch": patch("a.rs"), "status": "modified"},
        {"file": "b.rs", "patch": patch("b.rs"), "status": "modified"}]}}});
    let calls = last_calls(&ev(&mut s, &ok.to_string()));
    let ids: Vec<&str> = calls.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["p1", "p1#1"]);
    assert!(
        matches!(&calls[1].detail, ToolDetail::Edit { path, added: 1, removed: 1, .. } if path == "b.rs")
    );
    assert_eq!(s.edited.len(), 2);
    let called = r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"w1","name":"write","input":{"filePath":"c.rs","content":"x"}}}"#;
    ev(&mut s, called);
    let ok = json!({"type": "session.tool.success", "data": {"sessionID": "ses_1", "id": "w1", "content": [], "metadata": {"files": [{"file": "c.rs", "patch": patch("c.rs"), "status": "modified"}]}}});
    assert_eq!(last_calls(&ev(&mut s, &ok.to_string()))[0].title, "Overwrite file");
}

#[test]
fn command_changes_become_edit_rows_unless_an_edit_covered_them() {
    let mut s = root();
    s.turn = 3;
    s.edited.insert("/p/a.rs".into());
    s.changed = vec!["a.rs".into(), "gen.txt".into()];
    let patch = "--- x\n+++ x\n@@ -1 +1 @@\n-a\n+b\n";
    let diff =
        json!({"files": [{"file": "a.rs", "patch": patch}, {"file": "gen.txt", "patch": patch}]});
    let calls = last_calls(&map_event(&mut s, "aether.diff", &diff));
    let [row] = &calls[..] else { panic!("{calls:?}") };
    assert_eq!(
        (row.id.as_str(), row.title.as_str(), row.status),
        ("diff-3-gen.txt", "Changed by command", ToolStatus::Done)
    );
    assert!(matches!(&row.detail, ToolDetail::Edit { added: 1, removed: 1, .. }));
}

#[test]
fn a_parent_success_does_not_undo_a_child_failure() {
    let mut s = root();
    for line in [SUB_START, SUB_CALLED, SUB_PROGRESS] {
        ev(&mut s, line);
    }
    let failed = r#"{"type":"session.execution.failed","data":{"sessionID":"ses_c","error":{"message":"boom"}}}"#;
    ev(&mut s, failed);
    let ok = r#"{"type":"session.tool.success","data":{"sessionID":"ses_1","id":"call_t","content":[{"type":"text","text":"done"}],"metadata":{"sessionID":"ses_c"},"executed":true}}"#;
    let calls = last_calls(&ev(&mut s, ok));
    assert_eq!((calls[0].status, calls[0].error.as_deref()), (ToolStatus::Failed, Some("boom")));
}

#[test]
fn a_failed_background_subagent_shows_failed() {
    let mut s = root();
    for line in [SUB_START, SUB_CALLED, SUB_PROGRESS] {
        ev(&mut s, line);
    }
    let ok = r#"{"type":"session.tool.success","data":{"sessionID":"ses_1","id":"call_t","content":[],"metadata":{"sessionID":"ses_c","status":"running"},"executed":true}}"#;
    ev(&mut s, ok);
    let failed = r#"{"type":"session.execution.failed","data":{"sessionID":"ses_c","error":{"message":"boom"}}}"#;
    assert_eq!(tool_status(&ev(&mut s, failed), "call_t"), Some(ToolStatus::Failed));
    assert!(s.background.is_empty());
}

#[test]
fn a_failed_child_step_still_counts_tokens_and_cost() {
    let mut s = root();
    for line in [SUB_START, SUB_CALLED, SUB_PROGRESS] {
        ev(&mut s, line);
    }
    let step = r#"{"type":"session.step.failed","data":{"sessionID":"ses_c","cost":0.25,"error":{"message":"x"},"tokens":{"input":4,"output":1,"reasoning":0,"cache":{"read":0,"write":0}}}}"#;
    let out = ev(&mut s, step);
    assert!(out.contains(&Update::Cost(250_000)), "{out:?}");
    assert!(matches!(&last_calls(&out)[0].detail, ToolDetail::Task { tokens: 5, .. }));
}

#[test]
fn rebuilt_children_adopt_their_call_once_it_shows_up() {
    let mut s = root();
    s.children.insert("ses_c".into(), Child { task: "ses_c".into(), agent: "explore".into() });
    for line in [SUB_START, SUB_CALLED, SUB_PROGRESS] {
        ev(&mut s, line);
    }
    assert_eq!(s.children["ses_c"].task, "call_t");
    let calls = last_calls(&ev(&mut s, CHILD_READ));
    assert!(calls.iter().any(|c| c.id == "call_r" && c.parent.as_deref() == Some("call_t")));
}

#[test]
fn approval_fixtures_link_cards_to_rows() {
    for scenario in ["approval", "subagent-approval"] {
        let out = replay(scenario);
        let ask = out
            .iter()
            .find_map(|u| match u {
                Update::Ask { ask: Ask::Approval(a), .. } => Some(a.clone()),
                _ => None,
            })
            .expect(scenario);
        assert!(ask.tool_id.is_some(), "{scenario}");
        if scenario == "subagent-approval" {
            assert!(ask.source.is_some());
        }
    }
}

#[test]
fn question_fixture_is_a_form_card() {
    let out = replay("question");
    assert!(
        out.iter()
            .any(|u| matches!(u, Update::Ask { ask: Ask::Form(f), .. } if !f.fields.is_empty()))
    );
    assert!(out.iter().any(|u| matches!(u, Update::AskResolved(_))));
}

#[test]
fn write_without_file_metadata_still_covers_its_changed_file() {
    let mut s = State { active: true, root: "ses_x".into(), ..State::default() };
    let mut out = Vec::new();
    for v in crate::record::fixture("opencode", "approval") {
        let v: Value = serde_json::from_str(&v).unwrap();
        match v["type"].as_str() {
            Some("session.execution.succeeded") => break,
            Some(ty) => out.extend(map_event(&mut s, ty, &v["data"])),
            None => {}
        }
    }
    assert!(s.changed.iter().any(|f| f == "notes.txt"), "{:?}", s.changed);
    let patch = "--- x\n+++ x\n@@ -0,0 +1 @@\n+a\n";
    let diff = json!({"files": [{"file": "notes.txt", "patch": patch}, {"file": "other", "patch": patch}]});
    let rows = last_calls(&map_event(&mut s, "aether.diff", &diff));
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn early_inbox_acks_land_once_the_steer_is_registered() {
    let mut s = root();
    ev(
        &mut s,
        r#"{"type":"session.inbox.delivered","data":{"sessionID":"ses_1","inboxID":"inb_1"}}"#,
    );
    ev(
        &mut s,
        r#"{"type":"session.inbox.cancelled","data":{"sessionID":"ses_1","inboxID":"inb_2"}}"#,
    );
    assert_eq!(register_steer(&mut s, "inb_1", "more".into()), [Update::Steered("more".into())]);
    assert_eq!(register_steer(&mut s, "inb_2", "no".into()), [Update::SteerRefused("no".into())]);
    assert_eq!(register_steer(&mut s, "inb_3", "later".into()), []);
    assert!(s.steers.contains_key("inb_3") && !s.steers.contains_key("inb_1"));
}

#[test]
fn a_failed_turn_fails_its_retry_notice() {
    let mut s = root();
    s.active = true;
    ev(
        &mut s,
        r#"{"type":"session.retry.scheduled","data":{"sessionID":"ses_1","attempt":1,"at":5,"error":{"type":"x","message":"busy"}}}"#,
    );
    let out = ev(
        &mut s,
        r#"{"type":"session.execution.failed","data":{"sessionID":"ses_1","error":{"type":"x","message":"boom"}}}"#,
    );
    assert!(
        out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::Retry && n.state == NoticeState::Failed && n.until_ms.is_none())),
        "{out:?}"
    );
}

#[test]
fn retry_fixture_counts_down_then_closes() {
    let out = replay("retry.hand");
    let notices: Vec<_> = out
        .iter()
        .filter_map(|u| match u {
            Update::Notice(n) if n.kind == NoticeKind::Retry => Some(n),
            _ => None,
        })
        .collect();
    assert_eq!(notices.len(), 2, "{notices:?}");
    assert_eq!((notices[0].state, notices[0].until_ms.is_some()), (NoticeState::Running, true));
    assert!(notices[0].text.contains("overloaded"), "{}", notices[0].text);
    assert_eq!(notices[1].state, NoticeState::Done);
}

#[test]
fn compaction_fixture_ends_with_a_summary() {
    let out = replay("compaction");
    let notices: Vec<_> = out
        .iter()
        .filter_map(|u| match u {
            Update::Notice(n) if n.kind == NoticeKind::Compaction => Some(n),
            _ => None,
        })
        .collect();
    assert_eq!(notices.first().map(|n| n.state), Some(NoticeState::Running));
    let last = notices.last().unwrap();
    assert_eq!(last.state, NoticeState::Done);
    assert!(last.detail.is_some());
    assert!(notices.iter().all(|n| n.id == notices[0].id));
}

#[test]
fn steer_fixture_reports_the_steer_delivered() {
    let values: Vec<Value> = crate::record::fixture("opencode", "steer")
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let root = values.iter().find_map(|v| v["data"]["sessionID"].as_str()).unwrap().to_string();
    let mut s = State { active: true, root, ..State::default() };
    let steer_inbox = values
        .iter()
        .filter(|v| v["type"] == "session.inbox.enqueued")
        .nth(1)
        .and_then(|v| v["data"]["inboxID"].as_str())
        .unwrap()
        .to_string();
    s.steers.insert(steer_inbox, "more".into());
    let mut out = Vec::new();
    for v in &values {
        if let Some(ty) = v["type"].as_str() {
            out.extend(map_event(&mut s, ty, &v["data"]));
        }
    }
    assert!(out.contains(&Update::Steered("more".into())), "{out:?}");
}

fn with_background_child(s: &mut State) {
    for line in [SUB_START, SUB_CALLED, SUB_PROGRESS] {
        ev(s, line);
    }
    let ok = r#"{"type":"session.tool.success","data":{"sessionID":"ses_1","id":"call_t","content":[{"type":"text","text":"started"}],"metadata":{"sessionID":"ses_c","status":"running"},"executed":true}}"#;
    ev(s, ok);
}

const CHILD_ASK: &str = r#"{"type":"permission.asked","data":{"id":"per_c","sessionID":"ses_c","action":"shell","resources":["touch x"]}}"#;
const ROOT_OK: &str = r#"{"type":"session.execution.succeeded","data":{"sessionID":"ses_1"}}"#;

#[test]
fn a_background_childs_ask_outlives_the_turn_as_a_new_live_card() {
    let mut s = root();
    with_background_child(&mut s);
    let [Update::Ask { id: first, ask }] = &ev(&mut s, CHILD_ASK)[..] else { panic!() };
    let root_ask = r#"{"type":"permission.asked","data":{"id":"per_r","sessionID":"ses_1","action":"shell","resources":["ls"]}}"#;
    ev(&mut s, root_ask);
    let out = ev(&mut s, ROOT_OK);
    let [Update::Final(_), Update::Ask { id: again, ask: shown }] = &out[..] else {
        panic!("{out:?}")
    };
    assert!(again > first && shown == ask);
    assert_eq!(s.asks.len(), 1, "the root's own ask ended with its turn");
    assert!(
        matches!(&s.asks[again], OpencodeAsk::Permission { request, session } if request == "per_c" && session == "ses_c")
    );
}

#[test]
fn steered_text_is_never_repeated_by_the_final() {
    let mut s = root();
    s.root = "ses_1".into();
    let delta = |t: &str| {
        json!({"type":"session.text.delta","data":{"sessionID":"ses_1","delta":t}}).to_string()
    };
    let mut m = aether_core::Message::assistant();
    let mut feed = |out: Vec<Update>| out.into_iter().for_each(|u| m.apply(u));
    feed(ev(&mut s, &delta("a")));
    assert!(crate::opencode::on_steer(&s));
    feed(register_steer(&mut s, "msg_s", "more".into()));
    feed(ev(&mut s, &delta("b")));
    feed(ev(
        &mut s,
        r#"{"type":"session.inbox.delivered","data":{"sessionID":"ses_1","inboxID":"msg_s"}}"#,
    ));
    feed(ev(&mut s, &delta("c")));
    feed(ev(&mut s, ROOT_OK));
    assert_eq!(m.text, "ab\n\nc");
}

#[test]
fn a_childs_form_reply_resolves_its_card() {
    let mut s = root();
    with_background_child(&mut s);
    let form = r#"{"type":"form.created","data":{"form":{"id":"frm_c","sessionID":"ses_c","fields":[{"key":"k","title":"Pick"}]}}}"#;
    let [Update::Ask { id, .. }] = &ev(&mut s, form)[..] else { panic!() };
    let replied = r#"{"type":"form.replied","data":{"sessionID":"ses_c","id":"frm_c"}}"#;
    assert_eq!(ev(&mut s, replied), [Update::AskResolved(*id)]);
}

#[test]
fn an_interrupted_background_child_stops_its_row() {
    let mut s = root();
    with_background_child(&mut s);
    let shutdown = r#"{"type":"session.execution.interrupted","data":{"sessionID":"ses_c","reason":"shutdown"}}"#;
    assert_eq!(ev(&mut s, shutdown), []);
    let user =
        r#"{"type":"session.execution.interrupted","data":{"sessionID":"ses_c","reason":"user"}}"#;
    assert_eq!(tool_status(&ev(&mut s, user), "call_t"), Some(ToolStatus::Stopped));
    assert!(s.background.is_empty());
}

#[test]
fn a_lost_or_deleted_session_refuses_steers_and_stops_background_rows() {
    let deleted = r#"{"type":"session.deleted","data":{"sessionID":"ses_1"}}"#;
    for lost in [true, false] {
        let mut s = root();
        with_background_child(&mut s);
        ev(&mut s, CHILD_READ);
        register_steer(&mut s, "msg_s", "more".into());
        let out = if lost { lost_service(&mut s, "") } else { ev(&mut s, deleted) };
        assert!(out.contains(&Update::SteerRefused("more".into())), "{out:?}");
        let stopped: Vec<&str> = out
            .iter()
            .filter_map(|u| match u {
                Update::Tool(c) if c.status == ToolStatus::Stopped => Some(c.id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(stopped, ["call_r", "call_t"], "children first");
        assert!(s.background.is_empty() && s.steers.is_empty());
    }
}

#[test]
fn a_background_shell_settles_from_its_inbox_notice_and_names_it_on_wake() {
    let mut s = root();
    let out = replay("background");
    let shell: Vec<ToolStatus> = out
        .iter()
        .filter_map(|u| match u {
            Update::Tool(c) if matches!(c.detail, ToolDetail::Command { .. }) => Some(c.status),
            _ => None,
        })
        .collect();
    assert!(shell.contains(&ToolStatus::Background), "{shell:?}");
    assert_eq!(shell.last(), Some(&ToolStatus::Done));
    let wake = out.iter().find_map(|u| match u {
        Update::Notice(n) if n.kind == NoticeKind::Wake => Some(n.text.clone()),
        _ => None,
    });
    let wake = wake.expect("a wake notice");
    assert!(wake.contains("sleep 8 && echo bg-done") && wake.contains("exit 0"), "{wake}");

    s.shells.insert("sh_1".into(), "call_b".into());
    let mut row = crate::tool::start("call_b", "shell", &json!({"command": "false"}));
    row.status = ToolStatus::Background;
    s.background.insert("call_b".into(), row);
    let failed = r#"{"type":"session.inbox.enqueued","data":{"sessionID":"ses_1","inboxID":"m","item":{"type":"synthetic","payload":{"description":"false","metadata":{"exit":1,"shellID":"sh_1","state":"completed"},"text":"<shell id=\"sh_1\" state=\"completed\" command=\"false\">\noops\n</shell>"}}}}"#;
    let [Update::Tool(c)] = &ev(&mut s, failed)[..] else { panic!() };
    assert_eq!(c.status, ToolStatus::Failed);
    assert!(
        matches!(&c.detail, ToolDetail::Command { output, exit_code: Some(1), .. } if output == "oops"),
        "{c:?}"
    );
}

#[test]
fn settle_prunes_what_only_the_ended_turn_used() {
    let mut s = root();
    s.calls.insert("per_x".into(), "call_x".into());
    s.denied.insert("call_gone".into());
    s.children.insert("ses_old".into(), Child { task: "call_old".into(), agent: String::new() });
    with_background_child(&mut s);
    ev(&mut s, CHILD_ASK);
    s.calls.insert("per_c".into(), "call_c".into());
    ev(&mut s, ROOT_OK);
    assert_eq!(s.calls.keys().collect::<Vec<_>>(), ["per_c"]);
    assert!(s.denied.is_empty());
    assert!(s.children.contains_key("ses_c") && !s.children.contains_key("ses_old"));
}

#[test]
fn answering_the_last_ask_while_idle_ends_its_side_turn() {
    let mut s = State { root: "ses_1".into(), ..State::default() };
    assert_eq!(crate::opencode::side_turn_end(&s), Some(Update::Final(String::new())));
    s.asks.insert(1, OpencodeAsk::Permission { request: "p".into(), session: "ses_c".into() });
    assert_eq!(crate::opencode::side_turn_end(&s), None);
    s.asks.clear();
    s.active = true;
    assert_eq!(crate::opencode::side_turn_end(&s), None);
}
