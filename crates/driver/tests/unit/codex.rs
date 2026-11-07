use std::path::PathBuf;

use aether_core::{
    ApprovalKind, ApprovalOption, Attachment, Choice, FieldKind, PlanItem, PlanStatus, Question,
};

use super::*;
use crate::record::{fixtures, last_calls};

fn ev(s: &mut State, line: &str) -> Vec<Update> {
    map_event(s, &serde_json::from_str(line).unwrap())
}

fn replay(scenario: &str) -> Vec<Update> {
    let lines = crate::record::fixture("codex", scenario);
    let values: Vec<Value> = lines.iter().map(|l| serde_json::from_str(l).unwrap()).collect();
    let thread = values
        .iter()
        .find_map(|v| v["result"]["thread"]["id"].as_str())
        .unwrap_or_default()
        .to_string();
    let mut s = State { active: true, thread, ..State::default() };
    let mut out = Vec::new();
    for (i, v) in values.iter().enumerate() {
        // our requests reuse the recorded id, so the recorded reply finds its call
        let reply_id = values[i..]
            .iter()
            .find(|r| r.get("method").is_none() && r.get("id").is_some())
            .and_then(|r| r["id"].as_u64());
        match v["aether"].as_str() {
            Some("prompt") => {
                s.next_id = reply_id.unwrap_or(s.next_id);
                start_turn(&mut s, v["text"].as_str().unwrap(), &[]);
            }
            Some("steer") => {
                let text = v["text"].as_str().unwrap().to_string();
                if s.turn_id.is_some() {
                    s.next_id = reply_id.unwrap_or(s.next_id);
                    steer_turn(&mut s, text, vec![]);
                } else {
                    s.queued.push((text, vec![]));
                }
            }
            Some("cancel") => {
                on_cancel(&mut s);
            }
            Some("answer") => {
                let answer = serde_json::from_value(v["answer"].clone()).unwrap();
                reply(&mut s, v["id"].as_u64().unwrap(), &answer);
            }
            _ => out.extend(map_event(&mut s, v)),
        }
        s.outbox.clear();
    }
    out
}

fn tool(updates: &[Update]) -> &ToolCall {
    match updates {
        [Update::Tool(call)] => call,
        other => panic!("expected one tool update, got {other:?}"),
    }
}

fn opts(access: AccessMode, model: &str, resume: &str) -> DriverStartOptions {
    DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: PathBuf::from("/tmp/codex-probe"),
        model: model.into(),
        effort: String::new(),
        access,
        resume: resume.into(),
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    }
}

const COMMENTARY_DELTA: &str = r#"{"method":"item/agentMessage/delta","params":{"threadId":"01a15157-b7e2-7e01-ad4c-0dcb40b02092","turnId":"01a15157-d0ec-70f3-9473-d9702bd26670","itemId":"msg_0d731dae99f9109c016ad555fac7f08191973308eee6338c0a","delta":"I"}}"#;
const COMMENTARY_DONE: &str = r#"{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"msg_0d731dae99f9109c016ad555fac7f08191973308eee6338c0a","text":"I’ll run `touch a.txt`, then use `apply_patch` to create `b.txt`.\n","phase":"commentary","memoryCitation":null,"delivery":null,"questions":null},"threadId":"01a15157-b7e2-7e01-ad4c-0dcb40b02092","turnId":"01a15157-d0ec-70f3-9473-d9702bd26670","completedAtMs":1792366075356}}"#;
const TOUCH_STARTED: &str = r#"{"method":"item/started","params":{"item":{"type":"commandExecution","id":"exec-51287e8a-b0d5-40b1-9cac-d01224dcd678","pluginId":null,"scriptPath":null,"command":"/bin/zsh -lc 'touch a.txt'","cwd":"/tmp/codex-probe","processId":null,"source":"agent","status":"inProgress","commandActions":[{"type":"unknown","command":"touch a.txt"}],"aggregatedOutput":null,"exitCode":null,"durationMs":null}}}"#;
const TURN_DONE: &str = r#"{"method":"turn/completed","params":{"threadId":"01a15157-b7e2-7e01-ad4c-0dcb40b02092","turn":{"id":"01a15157-d0ec-70f3-9473-d9702bd26670","items":[],"itemsView":"summary","status":"completed","error":null,"durationMs":7047}}}"#;
const TURN_INTERRUPTED: &str = r#"{"method":"turn/completed","params":{"threadId":"01a15157-b7e2-7e01-ad4c-0dcb40b02092","turn":{"id":"01a15157-ecc5-79a3-920b-ab8582353f09","items":[],"itemsView":"notLoaded","status":"interrupted","error":null,"durationMs":3835}}}"#;
const USAGE_1: &str = r#"{"method":"thread/tokenUsage/updated","params":{"threadId":"01a15159-370f-7950-8fb0-36b8564f477f","turnId":"01a15159-37a0-7e91-8b17-66a8e86975a5","tokenUsage":{"total":{"totalTokens":21894,"inputTokens":21836,"cachedInputTokens":12288,"cacheWriteInputTokens":0,"outputTokens":58,"reasoningOutputTokens":0},"last":{"totalTokens":21894,"inputTokens":21836,"cachedInputTokens":12288,"cacheWriteInputTokens":0,"outputTokens":58,"reasoningOutputTokens":0},"modelContextWindow":475000}}}"#;
const USAGE_2: &str = r#"{"method":"thread/tokenUsage/updated","params":{"threadId":"01a15159-370f-7950-8fb0-36b8564f477f","turnId":"01a15159-37a0-7e91-8b17-66a8e86975a5","tokenUsage":{"total":{"totalTokens":43876,"inputTokens":43754,"cachedInputTokens":34048,"cacheWriteInputTokens":0,"outputTokens":122,"reasoningOutputTokens":29},"last":{"totalTokens":21982,"inputTokens":21918,"cachedInputTokens":21760,"cacheWriteInputTokens":0,"outputTokens":64,"reasoningOutputTokens":29},"modelContextWindow":475000}}}"#;
const USAGE_3: &str = r#"{"method":"thread/tokenUsage/updated","params":{"threadId":"01a15159-370f-7950-8fb0-36b8564f477f","turnId":"01a15159-37a0-7e91-8b17-66a8e86975a5","tokenUsage":{"total":{"totalTokens":65923,"inputTokens":65796,"cachedInputTokens":55808,"cacheWriteInputTokens":0,"outputTokens":127,"reasoningOutputTokens":29},"last":{"totalTokens":22047,"inputTokens":22042,"cachedInputTokens":21760,"cacheWriteInputTokens":0,"outputTokens":5,"reasoningOutputTokens":0},"modelContextWindow":475000}}}"#;
const FILE_ADD_STARTED: &str = r#"{"method":"item/started","params":{"item":{"type":"fileChange","id":"exec-58e8de41-3f98-4054-833c-3ec79adacea9","changes":[{"path":"/tmp/codex-probe/b.txt","kind":{"type":"add"},"diff":"hi\n"}],"status":"inProgress"}}}"#;
const FILE_ADD_DONE: &str = r#"{"method":"item/completed","params":{"item":{"type":"fileChange","id":"exec-58e8de41-3f98-4054-833c-3ec79adacea9","changes":[{"path":"/tmp/codex-probe/b.txt","kind":{"type":"add"},"diff":"hi\n"}],"status":"completed"}}}"#;
const FILE_APPROVAL: &str = r#"{"method":"item/fileChange/requestApproval","id":1,"params":{"threadId":"01a15157-b7e2-7e01-ad4c-0dcb40b02092","turnId":"01a15157-d0ec-70f3-9473-d9702bd26670","itemId":"exec-58e8de41-3f98-4054-833c-3ec79adacea9","startedAtMs":1792366077033,"reason":null,"grantRoot":null}}"#;
const MODEL_ERROR: &str = r#"{"method":"error","params":{"error":{"message":"{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'nonexistent-model-xyz' model is not supported when using Codex with a ChatGPT account.\"}}","codexErrorInfo":"other","additionalDetails":null,"misalignment":null},"willRetry":false,"threadId":"01a15157-b7e2-7e01-ad4c-0dcb40b02092","turnId":"01a15157-fbcf-7280-ad8f-9fc33a9247a4"}}"#;
const TURN_FAILED: &str = r#"{"method":"turn/completed","params":{"threadId":"01a15157-b7e2-7e01-ad4c-0dcb40b02092","turn":{"id":"01a15157-fbcf-7280-ad8f-9fc33a9247a4","items":[],"itemsView":"notLoaded","status":"failed","error":{"message":"{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'nonexistent-model-xyz' model is not supported when using Codex with a ChatGPT account.\"}}","codexErrorInfo":"other","additionalDetails":null,"misalignment":null},"durationMs":1192}}}"#;
const MODEL_REJECTED: &str =
    "The 'nonexistent-model-xyz' model is not supported when using Codex with a ChatGPT account.";

#[test]
fn access_modes_map_to_codex_policies() {
    let cases = [
        (AccessMode::Ask, "untrusted", "read-only", "readOnly", "user"),
        (AccessMode::ReadOnly, "never", "read-only", "readOnly", "user"),
        (AccessMode::AutoEdit, "untrusted", "workspace-write", "workspaceWrite", "user"),
        (AccessMode::Auto, "on-request", "workspace-write", "workspaceWrite", "auto_review"),
        (AccessMode::Full, "never", "danger-full-access", "dangerFullAccess", "user"),
    ];
    for (mode, policy, sandbox, turn_sandbox, reviewer) in cases {
        let expected = json!({
            "cwd": "/tmp/codex-probe",
            "approvalPolicy": policy,
            "sandbox": sandbox,
            "approvalsReviewer": reviewer,
        });
        assert_eq!(thread_params(&opts(mode, "", "")), expected, "{mode:?}");
        assert_eq!(access(mode), [policy, sandbox, turn_sandbox, reviewer], "{mode:?}");
    }
}

#[test]
fn thread_start_and_resume_requests() {
    let start = thread_params(&opts(AccessMode::Ask, "gpt-6.1-sol", ""));
    assert_eq!(start["model"], "gpt-6.1-sol");
    assert!(start.get("ephemeral").is_none());
    let side = DriverStartOptions { ephemeral: true, ..opts(AccessMode::ReadOnly, "", "") };
    assert_eq!(thread_params(&side)["ephemeral"], true);
    let resume =
        resume_params(&opts(AccessMode::ReadOnly, "", "01a15157-b7e2-7e01-ad4c-0dcb40b02092"));
    assert_eq!(
        resume,
        json!({
            "cwd": "/tmp/codex-probe",
            "approvalPolicy": "never",
            "sandbox": "read-only",
            "approvalsReviewer": "user",
            "threadId": "01a15157-b7e2-7e01-ad4c-0dcb40b02092",
            "excludeTurns": true,
        })
    );
}

#[test]
fn turn_start_request_and_reply() {
    let mut s = State {
        thread: "01a15159-370f-7950-8fb0-36b8564f477f".into(),
        model: "gpt-6.1-sol".into(),
        effort: "high".into(),
        access: AccessMode::Auto,
        next_id: 2,
        ..State::default()
    };
    assert_eq!(
        start_turn(&mut s, "Reply with the single word OK.", &[]),
        json!({"method": "turn/start", "id": 2, "params": {
            "threadId": "01a15159-370f-7950-8fb0-36b8564f477f",
            "input": [{"type": "text", "text": "Reply with the single word OK."}],
            "model": "gpt-6.1-sol",
            "effort": "high",
            "summary": "auto",
        }})
    );
    assert!(s.active);
    let plain = start_turn(&mut State::default(), "x", &[]);
    assert!(plain["params"].get("model").is_none() && plain["params"].get("effort").is_none());
    let reply = r#"{"id":2,"result":{"turn":{"id":"01a15159-37a0-7e91-8b17-66a8e86975a5","items":[],"itemsView":"notLoaded","status":"inProgress","error":null}}}"#;
    assert_eq!(ev(&mut s, reply), []);
    assert_eq!(s.turn_id.as_deref(), Some("01a15159-37a0-7e91-8b17-66a8e86975a5"));

    start_turn(&mut s, "Reply OK.", &[]);
    let rejected = r#"{"error":{"code":-32600,"message":"thread not found"},"id":3}"#;
    assert_eq!(
        ev(&mut s, rejected),
        [Update::Error("thread not found".into()), Update::Final(String::new())]
    );
    assert!(!s.active);
}

#[test]
fn turn_input_adds_local_images() {
    let img = Attachment {
        name: "a.png".into(),
        path: "/c/a.png".into(),
        mime: "image/png".into(),
        size: 3,
    };
    let v = input("hi", &[img]);
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[1], json!({"type": "localImage", "path": "/c/a.png"}));
}

#[test]
fn turn_streams_text_with_gap_fill_and_message_breaks() {
    let mut s = State { active: true, ..State::default() };
    assert_eq!(ev(&mut s, COMMENTARY_DELTA), [Update::Delta("I".into())]);
    assert_eq!(
        ev(&mut s, COMMENTARY_DONE),
        [Update::Thinking(
            "’ll run `touch a.txt`, then use `apply_patch` to create `b.txt`.\n".into()
        )]
    );
    ev(&mut s, TOUCH_STARTED);
    let reply = |s: &mut State, id: &str, text: &str| {
        map_event(
            s,
            &json!({"method": "item/agentMessage/delta", "params": {"itemId": id, "delta": text}}),
        )
    };
    assert_eq!(reply(&mut s, "msg_b", "DONE"), [Update::Delta("DONE".into())]);
    assert_eq!(
        reply(&mut s, "msg_c", "Next"),
        [Update::Delta("\n\n".into()), Update::Delta("Next".into())]
    );
    let out = ev(&mut s, TURN_DONE);
    assert_eq!(out.last(), Some(&Update::Final(String::new())));
    assert!(matches!(&out[0], Update::Tool(call) if call.status == ToolStatus::Stopped));
    assert_eq!(ev(&mut s, TURN_DONE), []);
}

const SEARCH_DONE: &str = r#"{"method":"item/completed","params":{"item":{"type":"webSearch","id":"exec-e9120a0b-5d44-4665-b796-3b0d494ca881","query":"site.rust-lang.org latest stable Rust release October 2026","action":{"type":"search","query":"site.rust-lang.org latest stable Rust release October 2026","queries":null},"results":[{"type":"text_result","domain":"blog.rust-lang.org","ref_id":"turn0search0","snippet":"The release is scheduled for October 15.","title":"1.99.1 pre-release testing | Inside Rust Blog","url":"https://blog.rust-lang.org/inside-rust/2026/10/13/1.99.1-prerelease/"},{"type":"text_result","domain":"forge.rust-lang.org","ref_id":"turn0search2","snippet":"Stable | 1.99 | October 01 2026 UTC","title":"Overview - Rust Forge","url":"https://forge.rust-lang.org/"}]},"threadId":"01a151bc-b311-7932-8fdc-ed47f9db0d66","turnId":"01a151bc-b36d-72e0-92ab-77b3b89161a3"}}"#;
const PAGE_DONE: &str = r#"{"method":"item/completed","params":{"item":{"type":"webSearch","id":"exec-46e6d996-c656-45c1-95f7-2552f35a628f","query":"https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/","action":{"type":"openPage","url":"https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/"},"results":[{"type":"text_result","domain":"blog.rust-lang.org","ref_id":"turn1view0","snippet":"Total lines: 56","title":"Announcing Rust 1.99.1 | Rust Blog","url":"https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/"}]},"threadId":"01a151bc-b311-7932-8fdc-ed47f9db0d66","turnId":"01a151bc-b36d-72e0-92ab-77b3b89161a3"}}"#;

#[test]
fn citations_link_to_the_turns_search_results() {
    let mut s = State { active: true, ..State::default() };
    ev(&mut s, SEARCH_DONE);
    ev(&mut s, PAGE_DONE);
    let delta = |d: &str| json!({"method": "item/agentMessage/delta", "params": {"itemId": "m", "delta": d}});
    assert_eq!(
        map_event(&mut s, &delta("Rust 1.99.1.\u{e200}ci")),
        [Update::Delta("Rust 1.99.1.".into())]
    );
    assert_eq!(
        map_event(&mut s, &delta("te\u{e202}turn1view0\u{e202}turn0search2\u{e201} Forge")),
        [Update::Delta(
            " [1](https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/) [2](https://forge.rust-lang.org/) Forge"
                .into()
        )]
    );
    assert_eq!(
        map_event(
            &mut s,
            &delta(" agrees \u{e200}cite\u{e202}turn0search2\u{e202}turn9search0\u{e201}")
        ),
        [Update::Delta(" agrees [2](https://forge.rust-lang.org/)".into())]
    );
    assert_eq!(
        map_event(
            &mut s,
            &delta(".\u{e200}cite\u{e202}turn9search0\u{e201}\u{e200}navlist\u{e202}x\u{e201}")
        ),
        [Update::Delta(".".into())]
    );
    let done = json!({"method": "item/completed", "params": {"item": {"type": "agentMessage", "id": "m2", "text": "Pre-release notes.\u{e200}cite\u{e202}turn0search0\u{e201}"}}});
    assert_eq!(
        map_event(&mut s, &done),
        [
            Update::Delta("\n\n".into()),
            Update::Delta(
                "Pre-release notes. [3](https://blog.rust-lang.org/inside-rust/2026/10/13/1.99.1-prerelease/)"
                    .into()
            )
        ]
    );
    let mut cites = Cites::default();
    cites.refs.insert("turn0view0".into(), "https://en.wikipedia.org/wiki/Rust_(language)".into());
    assert_eq!(
        cites.rewrite("Rust\u{e200}cite\u{e202}turn0view0\u{e201}"),
        "Rust [1](https://en.wikipedia.org/wiki/Rust_%28language%29)"
    );
    start_turn(&mut s, "next", &[]);
    assert_eq!(
        map_event(&mut s, &delta("A\u{e200}cite\u{e202}turn0search2\u{e201}")),
        [Update::Delta("A".into())]
    );
}

#[test]
fn reasoning_summary_streams_as_thinking() {
    let mut s = State { active: true, ..State::default() };
    let delta = r#"{"method":"item/reasoning/summaryTextDelta","params":{"threadId":"01a1515a-45cf-7983-ae64-032fb3722297","turnId":"01a1515a-466f-7ed1-b4fb-456ce31230fd","itemId":"rs_075cd58f2679efdd016ad5569ca12c8191a512a8862dbbb41c","delta":"**Verifying primality of 391**","summaryIndex":0}}"#;
    assert_eq!(ev(&mut s, delta), [Update::Thinking("**Verifying primality of 391**".into())]);
    let next = delta.replace(r#""summaryIndex":0"#, r#""summaryIndex":1"#);
    assert_eq!(
        ev(&mut s, &next),
        [
            Update::Thinking("\n\n".into()),
            Update::Thinking("**Verifying primality of 391**".into())
        ]
    );
    let done = r#"{"method":"item/completed","params":{"item":{"type":"reasoning","id":"rs_075cd58f2679efdd016ad5569ca12c8191a512a8862dbbb41c","summary":["**Verifying primality of 391**"],"content":[]}}}"#;
    assert_eq!(ev(&mut s, done), []);
    let unstreamed = done.replace("rs_075", "rs_999");
    assert_eq!(
        ev(&mut s, &unstreamed),
        [
            Update::Thinking("\n\n".into()),
            Update::Thinking("**Verifying primality of 391**".into())
        ]
    );
}

#[test]
fn token_usage_counts_deltas_and_skips_the_resume_replay() {
    let window = Update::Window(475_000);
    let mut s = State { totals: Some([0; 3]), ..State::default() };
    assert_eq!(
        ev(&mut s, USAGE_1),
        [
            Update::Usage(21894),
            window.clone(),
            Update::Count { prompt: 21836, completion: 58, cached: 12288 }
        ]
    );
    assert_eq!(
        ev(&mut s, USAGE_2),
        [
            Update::Usage(21982),
            window.clone(),
            Update::Count { prompt: 21918, completion: 64, cached: 21760 }
        ]
    );
    assert_eq!(ev(&mut s, USAGE_2), [Update::Usage(21982), window.clone()]);
    let mut resumed = State::default();
    assert_eq!(ev(&mut resumed, USAGE_2), [Update::Usage(21982), window.clone()]);
    assert_eq!(
        ev(&mut resumed, USAGE_3),
        [
            Update::Usage(22047),
            window,
            Update::Count { prompt: 22042, completion: 5, cached: 21760 }
        ]
    );
}

#[test]
fn command_streams_output_then_settles() {
    let mut s = State { active: true, ..State::default() };
    let started = r#"{"method":"item/started","params":{"item":{"type":"commandExecution","id":"exec-896636ea-6657-410f-8005-6af6f96a7179","command":"/bin/zsh -lc \"printf 'a\\\\nb\\\\n'\"","cwd":"/tmp/codex-probe","status":"inProgress","commandActions":[{"type":"unknown","command":"printf 'a\\nb\\n'"}],"aggregatedOutput":null,"exitCode":null,"durationMs":null}}}"#;
    let command = |output: &str, exit_code| ToolDetail::Command {
        command: r"printf 'a\nb\n'".into(),
        cwd: Some("/tmp/codex-probe".into()),
        output: output.into(),
        exit_code,
    };
    let call = tool(&ev(&mut s, started)).clone();
    assert_eq!((call.title.as_str(), call.status), ("Run command", ToolStatus::Running));
    assert_eq!(call.detail, command("", None));
    let delta = |d: &str| {
        let params = json!({"itemId": "exec-896636ea-6657-410f-8005-6af6f96a7179", "delta": d});
        json!({"method": "item/commandExecution/outputDelta", "params": params})
    };
    assert_eq!(tool(&map_event(&mut s, &delta("a\n"))).detail, command("a\n", None));
    assert_eq!(tool(&map_event(&mut s, &delta("b\n"))).detail, command("a\nb\n", None));
    let done = r#"{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"exec-896636ea-6657-410f-8005-6af6f96a7179","command":"/bin/zsh -lc \"printf 'a\\\\nb\\\\n'\"","cwd":"/tmp/codex-probe","status":"completed","commandActions":[{"type":"unknown","command":"printf 'a\\nb\\n'"}],"aggregatedOutput":"a\nb\n","exitCode":0,"durationMs":0}}}"#;
    let call = tool(&ev(&mut s, done)).clone();
    assert_eq!((call.status, call.ms), (ToolStatus::Done, Some(0)));
    assert_eq!(call.detail, command("a\nb\n", Some(0)));
    assert!(s.tools.is_empty() && s.live.is_empty());

    let curl = r#"{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"exec-c5ed934c-67ae-4629-bb83-fb3dd4a9a002","command":"/bin/zsh -lc 'curl -sI https://example.com'","cwd":"/tmp/codex-probe","status":"failed","commandActions":[{"type":"unknown","command":"curl -sI https://example.com"}],"aggregatedOutput":null,"exitCode":6,"durationMs":0}}}"#;
    let call = tool(&ev(&mut s, curl)).clone();
    assert_eq!(call.status, ToolStatus::Failed);
    assert!(matches!(call.detail, ToolDetail::Command { exit_code: Some(6), .. }));
}

#[test]
fn approval_answers_and_declined_command() {
    let mut s = State { active: true, ..State::default() };
    let request = r#"{"method":"item/commandExecution/requestApproval","id":0,"params":{"kind":"command","threadId":"01a15159-370f-7950-8fb0-36b8564f477f","turnId":"01a15159-657d-75e3-9f8e-13d65c1c78bc","itemId":"exec-d02a7480-7d96-4398-a4ed-52ab739b28e0","startedAtMs":1792366178249,"environmentId":"local","command":"/bin/zsh -lc 'touch c.txt'","cwd":"/tmp/codex-probe","commandActions":[{"type":"unknown","command":"touch c.txt"}],"proposedExecpolicyAmendment":["touch","c.txt"],"availableDecisions":["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["touch","c.txt"]}},"cancel"]}}"#;
    let ask = Ask::Approval(Approval {
        tool: "Bash".into(),
        title: "Run command".into(),
        input: "touch c.txt".into(),
        kind: ApprovalKind::Command,
        tool_id: Some("exec-d02a7480-7d96-4398-a4ed-52ab739b28e0".into()),
        cwd: Some("/tmp/codex-probe".into()),
        options: vec![ApprovalOption {
            id: "execpolicy".into(),
            label: "Always allow `touch c.txt`".into(),
        }],
        ..Approval::default()
    });
    assert_eq!(ev(&mut s, request), [Update::Ask { id: 1, ask }]);
    assert_eq!(
        reply(&mut s, 1, &Answer::Deny),
        Some(json!({"id": 0, "result": {"decision": "decline"}}))
    );
    assert_eq!(reply(&mut s, 1, &Answer::Deny), None);
    ev(&mut s, request);
    let approve = Answer::Approve { input: "touch c.txt".into(), always: false };
    assert_eq!(
        reply(&mut s, 2, &approve),
        Some(json!({"id": 0, "result": {"decision": "accept"}}))
    );

    let declined = r#"{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"exec-d02a7480-7d96-4398-a4ed-52ab739b28e0","command":"/bin/zsh -lc 'touch c.txt'","cwd":"/tmp/codex-probe","status":"declined","commandActions":[{"type":"unknown","command":"touch c.txt"}],"aggregatedOutput":null,"exitCode":null,"durationMs":null}}}"#;
    assert_eq!(tool(&ev(&mut s, declined)).status, ToolStatus::Denied);
}

#[test]
fn interrupted_turn_stops_open_tools() {
    let mut s = State { active: true, cancelled: true, ..State::default() };
    let sleep = r#"{"method":"item/started","params":{"item":{"type":"commandExecution","id":"exec-fab23fa7-437f-48f3-8d7c-0ceda2a0f553","command":"/bin/zsh -lc 'sleep 6 && echo slept'","cwd":"/tmp/codex-probe","status":"inProgress","commandActions":[{"type":"unknown","command":"sleep 6 && echo slept"}],"aggregatedOutput":null,"exitCode":null,"durationMs":null}}}"#;
    ev(&mut s, sleep);
    let out = ev(&mut s, TURN_INTERRUPTED);
    assert!(
        matches!(&out[..], [Update::Tool(call), Update::Final(text)]
            if call.status == ToolStatus::Stopped && text.is_empty()),
        "{out:?}"
    );
    assert!(!s.active && s.tools.is_empty());
}

#[test]
fn file_changes_become_edit_calls() {
    let mut s = State { active: true, ..State::default() };
    let call = tool(&ev(&mut s, FILE_ADD_STARTED)).clone();
    assert_eq!(call.id, "exec-58e8de41-3f98-4054-833c-3ec79adacea9#0");
    assert_eq!(call.title, "Write file");
    assert!(matches!(&call.detail, ToolDetail::Edit { path, added: 1, removed: 0, .. }
        if path == "/tmp/codex-probe/b.txt"));
    let ask = Ask::Approval(Approval {
        tool: "Edit".into(),
        title: "Edit file".into(),
        input: "/tmp/codex-probe/b.txt".into(),
        kind: ApprovalKind::Edit,
        tool_id: Some("exec-58e8de41-3f98-4054-833c-3ec79adacea9#0".into()),
        diff: Some("@@ -0,0 +1,1 @@\n+hi\n".into()),
        options: vec![ApprovalOption {
            id: "session".into(),
            label: "Allow edits for the session".into(),
        }],
        ..Approval::default()
    });
    assert_eq!(ev(&mut s, FILE_APPROVAL), [Update::Ask { id: 1, ask }]);
    assert_eq!(tool(&ev(&mut s, FILE_ADD_DONE)).status, ToolStatus::Done);

    let mut auto = State { access: AccessMode::AutoEdit, ..State::default() };
    assert_eq!(ev(&mut auto, FILE_APPROVAL), []);
    assert_eq!(auto.outbox, [json!({"id": 1, "result": {"decision": "accept"}})]);

    let update = json!({"method": "item/completed", "params": {"item": {
        "type": "fileChange",
        "id": "patch-1",
        "changes": [
            {"path": "src/app.rs", "kind": {"type": "update", "move_path": null}, "diff": "@@ -1 +1,2 @@\n-old\n+new\n+more"},
            {"path": "old.rs", "kind": {"type": "delete"}, "diff": "gone\n"},
        ],
        "status": "declined",
    }}});
    let out = map_event(&mut s, &update);
    let [Update::Tool(patch), Update::Tool(delete)] = &out[..] else { panic!("{out:?}") };
    assert_eq!((patch.id.as_str(), patch.title.as_str()), ("patch-1#0", "Apply patch"));
    assert!(matches!(&patch.detail, ToolDetail::Edit { added: 2, removed: 1, .. }));
    assert_eq!((delete.title.as_str(), delete.status), ("Delete file", ToolStatus::Denied));
}

#[test]
fn plan_updates_one_done_row_per_turn() {
    let mut s = State::default();
    let update = r#"{"method":"turn/plan/updated","params":{"threadId":"t","turnId":"01a15159-37a0-7e91-8b17-66a8e86975a5","explanation":null,"plan":[{"step":"Run printf","status":"completed"},{"step":"Reply DONE","status":"inProgress"},{"step":"Wrap up","status":"pending"}]}}"#;
    let call = tool(&ev(&mut s, update)).clone();
    assert_eq!(call.id, "plan-01a15159-37a0-7e91-8b17-66a8e86975a5");
    assert_eq!(call.status, ToolStatus::Done);
    let item = |text: &str, status| PlanItem { text: text.into(), status };
    assert_eq!(
        call.detail,
        ToolDetail::Plan {
            items: vec![
                item("Run printf", PlanStatus::Done),
                item("Reply DONE", PlanStatus::Active),
                item("Wrap up", PlanStatus::Pending),
            ]
        }
    );
}

#[test]
fn questions_and_unsupported_requests_are_answered() {
    let mut s = State::default();
    let request = r#"{"method":"item/tool/requestUserInput","id":5,"params":{"threadId":"t","turnId":"u","itemId":"i","isBlocking":true,"autoResolutionMs":null,"questions":[{"id":"q1","header":"Deploy","question":"Ship it?","isOther":false,"isSecret":false,"options":[{"label":"Yes","description":"Do it"},{"label":"No","description":""}]}]}}"#;
    let choice = |label: &str, description: &str| Choice {
        label: label.into(),
        description: description.into(),
        ..Choice::default()
    };
    let ask = Ask::Questions(vec![Question {
        header: "Deploy".into(),
        question: "Ship it?".into(),
        options: vec![choice("Yes", "Do it"), choice("No", "")],
        ..Question::default()
    }]);
    assert_eq!(ev(&mut s, request), [Update::Ask { id: 1, ask }]);
    assert_eq!(
        reply(&mut s, 1, &Answer::Answers(vec![vec!["Yes".into()]])),
        Some(json!({"id": 5, "result": {"answers": {"q1": {"answers": ["Yes"]}}}}))
    );
    ev(&mut s, request);
    assert_eq!(reply(&mut s, 2, &Answer::Deny), Some(json!({"id": 5, "result": {"answers": {}}})));

    let unknown = r#"{"method":"item/tool/call","id":7,"params":{"tool":"x"}}"#;
    assert_eq!(
        ev(&mut s, unknown),
        [Update::Warning("Codex asked for item/tool/call, which Aether doesn't support".into())]
    );
    let elicit = r#"{"method":"mcpServer/elicitation/request","id":8,"params":{}}"#;
    assert!(matches!(&ev(&mut s, elicit)[..], [Update::Warning(_)]));
    assert_eq!(
        s.outbox,
        [
            json!({"id": 7, "error": {"code": -32601, "message": "Codex asked for item/tool/call, which Aether doesn't support"}}),
            json!({"id": 8, "result": {"action": "decline", "content": null, "_meta": null}}),
        ]
    );
}

#[test]
fn errors_report_once_and_retries_are_skipped() {
    let mut s = State { active: true, ..State::default() };
    let retrying = MODEL_ERROR.replace(r#""willRetry":false"#, r#""willRetry":true"#);
    assert!(
        matches!(&ev(&mut s, &retrying)[..], [Update::Notice(n)] if n.kind == NoticeKind::Retry)
    );
    let out = ev(&mut s, MODEL_ERROR);
    assert!(
        matches!(&out[..], [Update::Notice(n), Update::Error(e)] if n.state == NoticeState::Failed && e == MODEL_REJECTED),
        "{out:?}"
    );
    assert_eq!(ev(&mut s, TURN_FAILED), [Update::Final(String::new())]);
    let mut s = State { active: true, ..State::default() };
    assert_eq!(
        ev(&mut s, TURN_FAILED),
        [Update::Error(MODEL_REJECTED.into()), Update::Final(String::new())]
    );
}

#[test]
fn steers_queued_before_the_turn_id_are_sent_at_turn_start() {
    let mut s = State { thread: "t".into(), ..State::default() };
    start_turn(&mut s, "first", &[]);
    let img = Attachment {
        name: "a.png".into(),
        path: "/c/a.png".into(),
        mime: "image/png".into(),
        size: 3,
    };
    s.queued.push(("also".into(), vec![img]));
    let also =
        json!([{"type": "text", "text": "also"}, {"type": "localImage", "path": "/c/a.png"}]);
    ev(&mut s, r#"{"id":0,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}"#);
    let steer = s.outbox.pop().unwrap();
    assert_eq!(steer["method"], "turn/steer");
    assert_eq!(
        steer["params"],
        json!({"threadId": "t", "expectedTurnId": "turn-1", "input": also})
    );

    start_turn(&mut s, "again", &[]);
    s.cancelled = true;
    s.queued.push(("dropped".into(), vec![]));
    ev(&mut s, r#"{"id":2,"result":{"turn":{"id":"turn-2","status":"inProgress"}}}"#);
    let [interrupt] = &s.outbox[..] else { panic!("{:?}", s.outbox) };
    assert_eq!(interrupt["method"], "turn/interrupt");
    assert_eq!(interrupt["params"], json!({"threadId": "t", "turnId": "turn-2"}));
}

#[test]
fn commands_show_their_parsed_action() {
    let call = |actions: Value| {
        let item = json!({"id": "c", "command": "/bin/zsh -lc 'x'", "cwd": "/w", "commandActions": actions});
        command_call(&item).detail
    };
    assert_eq!(
        call(json!([{"type": "read", "command": "cat a.rs", "name": "a.rs", "path": "/w/a.rs"}])),
        ToolDetail::Read { path: "/w/a.rs".into(), range: None }
    );
    assert!(matches!(
        call(json!([{"type": "search", "command": "rg foo", "query": "foo", "path": "src"}])),
        ToolDetail::Search { pattern, path: Some(path), .. } if pattern == "foo" && path == "src"
    ));
    let wrapped =
        json!([{"type": "unknown", "command": "/bin/zsh -lc 'curl -sI https://example.com'"}]);
    assert!(matches!(call(wrapped), ToolDetail::Command { command, .. }
        if command == "curl -sI https://example.com"));
    let piped =
        json!([{"type": "unknown", "command": "cd src"}, {"type": "unknown", "command": "ls"}]);
    assert!(
        matches!(call(piped), ToolDetail::Command { command, .. } if command == "cd src && ls")
    );
    assert!(matches!(call(json!([])), ToolDetail::Command { command, .. } if command == "x"));
    assert_eq!(unwrap_shell(r#"/bin/zsh -lc "printf 'a\\nb\\n'""#), r"printf 'a\nb\n'");
    assert_eq!(unwrap_shell(r"/bin/bash -lc 'echo '\''hi'\'''"), "echo 'hi'");
    assert_eq!(unwrap_shell("ls -lc foo"), "ls -lc foo");
}

#[test]
fn stderr_lines_lose_their_colors() {
    let line = "\u{1b}[2m2026-10-18T23:28:04.237955Z\u{1b}[0m \u{1b}[31mERROR\u{1b}[0m \u{1b}[2mcodex_core::session\u{1b}[0m\u{1b}[2m:\u{1b}[0m failed to record rollout items";
    assert_eq!(
        clean(line),
        "2026-10-18T23:28:04.237955Z ERROR codex_core::session: failed to record rollout items"
    );
}

#[test]
fn compact_and_review_use_native_requests() {
    let mut s = State { thread: "t".into(), ..State::default() };
    assert_eq!(
        start_turn(&mut s, " /compact ", &[]),
        json!({"method": "thread/compact/start", "id": 0, "params": {"threadId": "t"}})
    );
    assert_eq!(
        start_turn(&mut s, "/review", &[])["params"],
        json!({"threadId": "t", "target": {"type": "uncommittedChanges"}, "delivery": "inline"})
    );
    assert_eq!(
        start_turn(&mut s, "/review  focus on errors", &[])["params"]["target"],
        json!({"type": "custom", "instructions": "focus on errors"})
    );
    for text in ["/compact now", "/reviewer", "please /review"] {
        let line = start_turn(&mut s, text, &[]);
        assert_eq!(line["method"], "turn/start", "{text}");
        assert_eq!(line["params"]["input"][0]["text"], text);
    }
    assert!(matches!(s.calls.get(&0), Some(Call::Turn)));
}

#[test]
fn compaction_turn_takes_its_id_from_turn_started() {
    let mut s = State { thread: "01a1516e-4d32-72a1-9898-521541c20fc7".into(), ..State::default() };
    start_turn(&mut s, "/compact", &[]);
    assert_eq!(ev(&mut s, r#"{"id":0,"result":{}}"#), []);
    assert_eq!(s.turn_id, None);
    let started = r#"{"method":"turn/started","params":{"threadId":"01a1516e-4d32-72a1-9898-521541c20fc7","turn":{"id":"01a1516e-6adf-7212-9030-5572adaf7da0","rootTurnId":"01a1516e-6adf-7212-9030-5572adaf7da0","items":[],"itemsView":"notLoaded","status":"inProgress","error":null,"startedAt":1792367553,"completedAt":null,"durationMs":null}}}"#;
    assert_eq!(ev(&mut s, started), []);
    assert_eq!(s.turn_id.as_deref(), Some("01a1516e-6adf-7212-9030-5572adaf7da0"));
    let item = r#"{"method":"item/started","params":{"item":{"type":"contextCompaction","id":"01a1516e-6b66-7a30-865e-ffa3b8954b6b"},"threadId":"01a1516e-4d32-72a1-9898-521541c20fc7","turnId":"01a1516e-6adf-7212-9030-5572adaf7da0","startedAtMs":1792367553382}}"#;
    assert_eq!(tool(&ev(&mut s, item)).title, "Compact context");
    let done = item.replace("item/started", "item/completed");
    assert_eq!(tool(&ev(&mut s, &done)).status, ToolStatus::Done);
    let turn_done = r#"{"method":"turn/completed","params":{"threadId":"01a1516e-4d32-72a1-9898-521541c20fc7","turn":{"id":"01a1516e-6adf-7212-9030-5572adaf7da0","items":[],"itemsView":"notLoaded","status":"completed","error":null,"durationMs":2894}}}"#;
    assert_eq!(ev(&mut s, turn_done), [Update::Final(String::new())]);
}

#[test]
fn always_approve_accepts_for_the_session() {
    let mut s = State { active: true, ..State::default() };
    ev(&mut s, FILE_APPROVAL);
    assert_eq!(
        reply(&mut s, 1, &Answer::Pick("session".into())),
        Some(json!({"id": 1, "result": {"decision": "acceptForSession"}}))
    );
    ev(&mut s, FILE_APPROVAL);
    let always = Answer::Approve { input: String::new(), always: true };
    assert_eq!(reply(&mut s, 2, &always), Some(json!({"id": 1, "result": {"decision": "accept"}})));
}

fn side_state() -> State {
    State {
        active: true,
        thread: "main".into(),
        naming: Naming::Side("side".into()),
        ..State::default()
    }
}

#[test]
fn other_threads_never_reach_the_main_turn() {
    let mut s = side_state();
    let approval = json!({"method": "item/commandExecution/requestApproval", "id": 4, "params": {"threadId": "side", "command": "ls"}});
    assert_eq!(map_event(&mut s, &approval), []);
    let question = json!({"method": "item/tool/requestUserInput", "id": 5, "params": {"threadId": "side", "questions": []}});
    assert_eq!(map_event(&mut s, &question), []);
    for line in [COMMENTARY_DELTA, USAGE_1, MODEL_ERROR, TURN_DONE] {
        for thread in ["side", "subagent"] {
            let mut v: Value = serde_json::from_str(line).unwrap();
            v["params"]["threadId"] = thread.into();
            assert_eq!(map_event(&mut s, &v), [], "{thread} {line}");
        }
    }
    assert!(s.active && s.totals.is_none());
    let [decline, answers, unsubscribe] = &s.outbox[..] else { panic!("{:?}", s.outbox) };
    assert_eq!(*decline, json!({"id": 4, "result": {"decision": "decline"}}));
    assert_eq!(*answers, json!({"id": 5, "result": {"answers": {}}}));
    assert_eq!(unsubscribe["params"], json!({"threadId": "side"}));
    let main = COMMENTARY_DELTA.replace("01a15157-b7e2-7e01-ad4c-0dcb40b02092", "main");
    assert_eq!(ev(&mut s, &main), [Update::Delta("I".into())]);
}

const MAIN: &str = "01a151bc-4ffd-7f31-b512-9a2784983d8c";
const SPAWNED: &str = r#"{"method":"item/started","params":{"item":{"type":"subAgentActivity","model":"gpt-6.1-sol","reasoningEffort":"medium","id":"call_cb112290082546d6996819381506ca7a","kind":"started","agentThreadId":"01a151bc-65df-7bd2-a720-d40312abf5d7","agentPath":"/root/echo_hi"},"threadId":"01a151bc-4ffd-7f31-b512-9a2784983d8c","turnId":"01a151bc-50da-7363-baca-81d45eefc2f5","startedAtMs":1792372663838},"emittedAtMs":1792372663839}"#;
const SUB_ECHO_STARTED: &str = r#"{"method":"item/started","params":{"item":{"type":"commandExecution","id":"exec-9d627cde-7995-43ff-aefb-c53f87ece230","pluginId":null,"scriptPath":null,"command":"/bin/zsh -lc 'echo hi'","cwd":"/tmp/codex-agent-probe-a","processId":"33257","source":"unifiedExecStartup","status":"inProgress","commandActions":[{"type":"unknown","command":"echo hi"}],"aggregatedOutput":null,"exitCode":null,"durationMs":null},"threadId":"01a151bc-65df-7bd2-a720-d40312abf5d7","turnId":"01a151bc-661e-73a0-a01d-add76ecfad5d","startedAtMs":1792372669008},"emittedAtMs":1792372669013}"#;
const CHILD: &str = "01a151bc-65df-7bd2-a720-d40312abf5d7";
const SUB_ECHO_STARTED_ON_CHILD: &str = SUB_ECHO_STARTED;
const SUB_ECHO_DONE: &str = r#"{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"exec-9d627cde-7995-43ff-aefb-c53f87ece230","pluginId":null,"scriptPath":null,"command":"/bin/zsh -lc 'echo hi'","cwd":"/tmp/codex-agent-probe-a","processId":"33257","source":"unifiedExecStartup","status":"completed","commandActions":[{"type":"unknown","command":"echo hi"}],"aggregatedOutput":"hi\n","exitCode":0,"durationMs":1},"threadId":"01a151bc-65df-7bd2-a720-d40312abf5d7","turnId":"01a151bc-661e-73a0-a01d-add76ecfad5d","completedAtMs":1792372669010},"emittedAtMs":1792372669056}"#;
const SUB_REPLY: &str = r#"{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"msg_0ab1aeaa2c60c4dd016ad56fbf89e88191894f1c7407d574ef","text":"hi","phase":"final_answer","memoryCitation":null,"delivery":null,"questions":null},"threadId":"01a151bc-65df-7bd2-a720-d40312abf5d7","turnId":"01a151bc-661e-73a0-a01d-add76ecfad5d","completedAtMs":1792372671498},"emittedAtMs":1792372671505}"#;
const SUB_FINISHED: &str = r#"{"method":"item/started","params":{"item":{"type":"subAgentActivity","model":null,"reasoningEffort":null,"id":"subagent-completed-01a151bc-661e-73a0-a01d-add76ecfad5d","kind":"completed","agentThreadId":"01a151bc-65df-7bd2-a720-d40312abf5d7","agentPath":"/root/echo_hi"},"threadId":"01a151bc-4ffd-7f31-b512-9a2784983d8c","turnId":"01a151bc-50da-7363-baca-81d45eefc2f5","startedAtMs":1792372671782},"emittedAtMs":1792372671782}"#;
const SUB_TURN_DONE: &str = r#"{"method":"turn/completed","params":{"threadId":"01a151bc-65df-7bd2-a720-d40312abf5d7","turn":{"id":"01a151bc-661e-73a0-a01d-add76ecfad5d","rootTurnId":"01a151bc-50da-7363-baca-81d45eefc2f5","items":[],"itemsView":"summary","status":"completed","error":null,"startedAt":1792372663,"completedAt":1792372671,"durationMs":7934}},"emittedAtMs":1792372671783}"#;

fn on_thread(line: &str, thread: &str) -> Value {
    let mut v: Value = serde_json::from_str(line).unwrap();
    v["params"]["threadId"] = thread.into();
    v
}

#[test]
fn sub_agent_calls_nest_and_outlive_the_parent_turn() {
    let mut s =
        State { active: true, thread: MAIN.into(), totals: Some([0; 3]), ..State::default() };
    let row = tool(&ev(&mut s, SPAWNED)).clone();
    assert_eq!(row.title, "Run agent");
    assert_eq!(ev(&mut s, &SPAWNED.replace("item/started", "item/completed")), []);
    let out = ev(&mut s, SUB_ECHO_STARTED);
    let calls = last_calls(&out);
    assert!(
        calls.iter().any(|c| c.parent.as_deref() == Some(row.id.as_str())
            && matches!(c.detail, ToolDetail::Command { .. })),
        "{out:?}"
    );
    assert!(
        calls
            .iter()
            .any(|c| c.id == row.id && matches!(c.detail, ToolDetail::Task { tools: 1, .. }))
    );
    let sub = |line: &str| on_thread(line, "01a151bc-65df-7bd2-a720-d40312abf5d7");
    let out = map_event(&mut s, &sub(SUB_ECHO_DONE));
    assert!(
        last_calls(&out)
            .iter()
            .any(|c| c.parent.as_deref() == Some(row.id.as_str()) && c.status == ToolStatus::Done),
        "{out:?}"
    );
    let usage = r#"{"method":"thread/tokenUsage/updated","params":{"threadId":"01a151bc-65df-7bd2-a720-d40312abf5d7","tokenUsage":{"total":{"totalTokens":321,"inputTokens":300,"outputTokens":21,"cachedInputTokens":0},"last":{"totalTokens":321}}}}"#;
    assert!(matches!(tool(&ev(&mut s, usage)).detail, ToolDetail::Task { tokens: 321, .. }));
    let reply = tool(&ev(&mut s, SUB_REPLY)).clone();
    assert!(matches!(&reply.detail, ToolDetail::Task { output, .. } if output == "hi"));
    let err = r#"{"method":"error","params":{"threadId":"01a151bc-65df-7bd2-a720-d40312abf5d7","willRetry":false,"error":{"message":"boom"}}}"#;
    assert_eq!(tool(&ev(&mut s, err)).error.as_deref(), Some("boom"));
    assert!(s.error.is_none());

    let main_done = TURN_DONE.replace("01a15157-b7e2-7e01-ad4c-0dcb40b02092", MAIN);
    s.turn_id = Some("01a15157-d0ec-70f3-9473-d9702bd26670".into());
    let end = ev(&mut s, &main_done);
    assert!(
        end.iter().any(
            |u| matches!(u, Update::Tool(c) if c.id == row.id && c.status == ToolStatus::Background)
        ),
        "{end:?}"
    );
    let done = tool(&ev(&mut s, SUB_TURN_DONE)).clone();
    assert_eq!((done.id, done.status), (row.id, ToolStatus::Done));
}

#[test]
fn sub_agent_activity_kinds_update_the_spawn_row() {
    let mut s = State { active: true, thread: MAIN.into(), ..State::default() };
    ev(&mut s, SPAWNED);
    let kind = |k: &str| SUB_FINISHED.replace(r#""kind":"completed""#, &format!(r#""kind":"{k}""#));
    let row = tool(&ev(&mut s, &kind("interacted"))).clone();
    assert!(matches!(&row.detail, ToolDetail::Task { activity, .. } if activity == "Working"));
    assert_eq!(tool(&ev(&mut s, &kind("interrupted"))).status, ToolStatus::Stopped);
    assert_eq!(tool(&ev(&mut s, &kind("completed"))).status, ToolStatus::Done);
}

#[test]
fn grandchildren_nest_under_the_child_row() {
    let mut s = State { active: true, thread: MAIN.into(), ..State::default() };
    let row = tool(&ev(&mut s, SPAWNED)).clone();
    let child = "01a151bc-65df-7bd2-a720-d40312abf5d7";
    let mut nested = on_thread(SPAWNED, child);
    nested["params"]["item"]["id"] = "call_grand".into();
    nested["params"]["item"]["agentThreadId"] = "grandchild".into();
    let out = map_event(&mut s, &nested);
    let calls = last_calls(&out);
    let grand = calls.iter().find(|c| c.id == "call_grand").expect("grandchild row");
    assert_eq!(grand.parent.as_deref(), Some(row.id.as_str()));
    assert_eq!(s.agents.get("grandchild").map(String::as_str), Some("call_grand"));
    let run = on_thread(SUB_ECHO_STARTED, "grandchild").to_string();
    let calls = last_calls(&ev(&mut s, &run));
    assert!(calls.iter().any(|c| c.parent.as_deref() == Some("call_grand")
        && matches!(c.detail, ToolDetail::Command { .. })));
}

#[test]
fn background_terminals_survive_turn_end_and_settle_later() {
    let mut s =
        State { active: true, thread: "t".into(), turn_id: Some("u".into()), ..State::default() };
    let started = r#"{"method":"item/started","params":{"threadId":"t","item":{"type":"commandExecution","id":"exec-1","command":"/bin/zsh -lc 'pnpm dev'","cwd":"/w","processId":"pty-7","source":"agent","status":"inProgress","commandActions":[{"type":"unknown","command":"pnpm dev"}],"aggregatedOutput":null,"exitCode":null,"durationMs":null}}}"#;
    ev(&mut s, started);
    let end = ev(
        &mut s,
        r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"u","items":[],"status":"completed","error":null}}}"#,
    );
    assert!(end.iter().any(
        |u| matches!(u, Update::Tool(c) if c.id == "exec-1" && c.status == ToolStatus::Background)
    ));
    assert_eq!(s.terminals.get("exec-1").map(String::as_str), Some("pty-7"));
    assert_eq!(
        terminate(&mut s, "exec-1").unwrap()["params"],
        json!({"threadId": "t", "processId": "pty-7"})
    );
    assert!(terminate(&mut s, "nope").is_none());
    let late = started
        .replace("item/started", "item/completed")
        .replace("\"inProgress\"", "\"completed\"")
        .replace("\"exitCode\":null", "\"exitCode\":0");
    assert_eq!(tool(&ev(&mut s, &late)).status, ToolStatus::Done);
    assert!(s.terminals.is_empty());
}

#[test]
fn listed_background_terminals_become_rows() {
    let rows = background_rows(
        &json!({"data": [{"itemId": "exec-1", "processId": "pty-7", "command": "pnpm dev", "cwd": "/w", "osPid": 4242}]}),
    );
    assert!(
        matches!(&rows[..], [c] if c.id == "exec-1" && c.status == ToolStatus::Background && c.detail.summary() == "pnpm dev")
    );
}

#[test]
fn collab_actions_are_rows() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let wait = r#"{"method":"item/started","params":{"threadId":"t","item":{"type":"collabAgentToolCall","id":"call_w","tool":"wait","status":"inProgress","senderThreadId":"t","receiverThreadIds":["a","b"],"prompt":null,"model":null,"reasoningEffort":null,"agentsStates":{}}}}"#;
    assert_eq!(tool(&ev(&mut s, wait)).title, "Wait for agents");
    s.agents.insert("a".into(), "spawn-a".into());
    let one = wait
        .replace("call_w", "call_m")
        .replace(r#"["a","b"]"#, r#"["a"]"#)
        .replace(r#""tool":"wait""#, r#""tool":"sendMessage""#)
        .replace(r#""prompt":null"#, r#""prompt":"hello""#);
    let row = tool(&ev(&mut s, &one)).clone();
    assert_eq!((row.title.as_str(), row.parent.as_deref()), ("Message agent", Some("spawn-a")));
    assert!(
        matches!(&row.detail, ToolDetail::Other { name, input, .. } if name == "sendMessage" && input == "hello")
    );
    let done = one.replace("item/started", "item/completed").replace("inProgress", "completed");
    assert_eq!(tool(&ev(&mut s, &done)).status, ToolStatus::Done);
    assert_eq!(s.agents.get("a").map(String::as_str), Some("spawn-a"));
}

#[test]
fn subagent_fixture_nests_and_counts() {
    let calls = last_calls(&replay("subagent"));
    let task =
        calls.iter().find(|c| matches!(c.detail, ToolDetail::Task { .. })).expect("task row");
    assert!(calls.iter().any(|c| c.parent.as_deref() == Some(task.id.as_str())));
    assert!(matches!(task.detail, ToolDetail::Task { tools, .. } if tools > 0));
}

#[test]
fn unknown_methods_are_logged() {
    let mut s = State { thread: "t".into(), ..State::default() };
    assert_eq!(ev(&mut s, r#"{"method":"zz/never-seen","params":{"threadId":"t"}}"#), []);
    assert!(!log_unknown("codex", "zz/never-seen"));
}

#[test]
fn live_output_without_newlines_stays_capped() {
    let mut live = LiveOutput::default();
    for _ in 0..100 {
        live.push(&"x".repeat(4096));
    }
    assert!(live.partial.len() <= LIVE_OUTPUT_CAP);
}

#[test]
fn a_prompt_during_cancel_starts_even_when_the_interrupted_turn_fails() {
    let mut s = State { thread: "t".into(), active: true, cancelled: true, ..State::default() };
    assert_eq!(queue_next(&mut s, "first".into(), vec![]), []);
    let second = queue_next(&mut s, "second".into(), vec![]);
    assert!(matches!(&second[..], [Update::Warning(_)]), "{second:?}");
    let died = refuse_next(&mut s);
    assert_eq!(
        died,
        [Update::Warning("Codex stopped before your message was sent. Send it again.".into())]
    );
    assert!(s.next.is_none());

    let turn = start_turn(&mut s, "go", &[]);
    s.next = Some(("queued".into(), vec![]));
    let out = map_event(&mut s, &json!({"id": turn["id"], "error": {"message": "nope"}}));
    assert!(out.iter().all(|u| !matches!(u, Update::SteerRefused(_))), "{out:?}");
    assert!(s.active && s.next.is_none());
    let sent = s.outbox.last().expect("turn/start");
    assert_eq!(sent["method"], "turn/start");
    assert_eq!(sent["params"]["input"][0]["text"], "queued");
}

const BG_STARTED: &str = r#"{"method":"item/started","params":{"threadId":"t","item":{"type":"commandExecution","id":"exec-1","command":"/bin/zsh -lc 'pnpm dev'","cwd":"/w","processId":"pty-7","source":"agent","status":"inProgress","commandActions":[{"type":"unknown","command":"pnpm dev"}],"aggregatedOutput":null,"exitCode":null,"durationMs":null}}}"#;
const BG_DELTA: &str = r#"{"method":"item/commandExecution/outputDelta","params":{"threadId":"t","itemId":"exec-1","delta":"DELTA\n"}}"#;
const T_DONE: &str = r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"u","items":[],"status":"completed","error":null}}}"#;

fn bg_state() -> State {
    State { active: true, thread: "t".into(), turn_id: Some("u".into()), ..State::default() }
}

#[test]
fn background_rows_keep_their_output_across_turn_end() {
    let mut s = bg_state();
    ev(&mut s, BG_STARTED);
    ev(&mut s, &BG_DELTA.replace("DELTA", "early"));
    ev(&mut s, T_DONE);
    let row = tool(&ev(&mut s, &BG_DELTA.replace("DELTA", "late"))).clone();
    let ToolDetail::Command { output, .. } = &row.detail else { panic!("{row:?}") };
    assert!(output.contains("early") && output.contains("late"), "{output:?}");
}

#[test]
fn a_failed_terminate_warns_with_the_command() {
    let mut s = bg_state();
    ev(&mut s, BG_STARTED);
    ev(&mut s, T_DONE);
    let line = terminate(&mut s, "exec-1").unwrap();
    let out =
        map_event(&mut s, &json!({"id": line["id"], "error": {"message": "no such process"}}));
    assert!(
        matches!(&out[..], [Update::Warning(w)] if w.contains("pnpm dev") && w.contains("no such process")),
        "{out:?}"
    );
    let line = terminate(&mut s, "exec-1").unwrap();
    assert_eq!(map_event(&mut s, &json!({"id": line["id"], "result": {}})), []);
}

#[test]
fn cancelled_ends_with_its_turn() {
    let mut s = bg_state();
    s.cancelled = true;
    ev(&mut s, BG_STARTED);
    ev(&mut s, T_DONE);
    assert!(!s.cancelled);
    let late = BG_STARTED
        .replace("item/started", "item/completed")
        .replace("\"inProgress\"", "\"failed\"")
        .replace("\"exitCode\":null", "\"exitCode\":2");
    assert_eq!(tool(&ev(&mut s, &late)).status, ToolStatus::Failed);
}

#[test]
fn collab_rows_stop_when_the_turn_ends() {
    let mut s =
        State { active: true, thread: MAIN.into(), turn_id: Some("u".into()), ..State::default() };
    ev(&mut s, SPAWNED);
    let msg = format!(
        r#"{{"method":"item/started","params":{{"threadId":"{MAIN}","item":{{"type":"collabAgentToolCall","id":"call_m","tool":"sendMessage","status":"inProgress","senderThreadId":"{MAIN}","receiverThreadIds":["01a151bc-65df-7bd2-a720-d40312abf5d7"],"prompt":"hi","agentsStates":{{}}}}}}}}"#
    );
    let started = last_calls(&ev(&mut s, &msg));
    assert!(started.iter().any(|c| c.id == "call_m" && c.parent.is_some()));
    let done = r#"{"method":"turn/completed","params":{"threadId":"MAIN","turn":{"id":"u","items":[],"status":"interrupted","error":null}}}"#
        .replace("MAIN", MAIN);
    let end = ev(&mut s, &done);
    let calls = last_calls(&end);
    let row = calls.iter().find(|c| c.id == "call_m").expect("collab row");
    assert_eq!(row.status, ToolStatus::Stopped, "{end:?}");
}

#[test]
fn child_events_nest_under_the_latest_collab_row_when_the_spawn_row_is_gone() {
    let mut s = State { active: true, thread: MAIN.into(), ..State::default() };
    let spawn = tool(&ev(&mut s, SPAWNED)).clone();
    s.tools.remove(&spawn.id);
    let out = ev(&mut s, SUB_ECHO_STARTED_ON_CHILD);
    let calls = last_calls(&out);
    let cmd =
        calls.iter().find(|c| matches!(c.detail, ToolDetail::Command { .. })).expect("{out:?}");
    assert_eq!(cmd.parent, None);
    let msg = format!(
        r#"{{"method":"item/started","params":{{"threadId":"{MAIN}","item":{{"type":"collabAgentToolCall","id":"call_f","tool":"followupTask","status":"inProgress","senderThreadId":"{MAIN}","receiverThreadIds":["{CHILD}"],"prompt":"again","agentsStates":{{}}}}}}}}"#
    );
    ev(&mut s, &msg);
    s.tools.remove(&spawn.id);
    let other = SUB_ECHO_STARTED_ON_CHILD.replace("exec-9d627cde", "exec-other");
    let calls = last_calls(&ev(&mut s, &other));
    let cmd = calls.iter().find(|c| c.id.starts_with("exec-other")).expect("row");
    assert_eq!(cmd.parent.as_deref(), Some("call_f"));
}

#[test]
fn child_command_output_streams_and_unknown_child_events_are_logged() {
    let mut s = State { active: true, thread: MAIN.into(), ..State::default() };
    ev(&mut s, SPAWNED);
    ev(&mut s, SUB_ECHO_STARTED);
    let delta = format!(
        r#"{{"method":"item/commandExecution/outputDelta","params":{{"threadId":"{CHILD}","itemId":"exec-9d627cde-7995-43ff-aefb-c53f87ece230","delta":"streamed\n"}}}}"#
    );
    let row = tool(&ev(&mut s, &delta)).clone();
    assert!(
        matches!(&row.detail, ToolDetail::Command { output, .. } if output.contains("streamed")),
        "{row:?}"
    );
    let odd = format!(r#"{{"method":"zz/child-never-seen","params":{{"threadId":"{CHILD}"}}}}"#);
    assert_eq!(ev(&mut s, &odd), []);
    assert!(!log_unknown("codex", "zz/child-never-seen"));
}

#[test]
fn sub_agent_approvals_reach_the_user() {
    let out = replay("subagent-approval");
    assert!(out.iter().any(|u| matches!(u, Update::Ask { .. })), "{out:?}");
}

#[test]
fn child_file_change_approval_lists_the_childs_paths() {
    let mut s = State { active: true, thread: MAIN.into(), ..State::default() };
    ev(&mut s, SPAWNED);
    let edit = format!(
        r#"{{"method":"item/started","params":{{"threadId":"{CHILD}","item":{{"type":"fileChange","id":"fc-1","changes":[{{"path":"/w/a.txt","kind":{{"type":"add"}},"diff":"x\n"}}],"status":"inProgress"}}}}}}"#
    );
    ev(&mut s, &edit);
    let ask = format!(
        r#"{{"method":"item/fileChange/requestApproval","id":9,"params":{{"threadId":"{CHILD}","turnId":"x","itemId":"fc-1","reason":null,"grantRoot":null}}}}"#
    );
    let out = ev(&mut s, &ask);
    assert!(
        matches!(&out[..], [Update::Ask { ask: Ask::Approval(a), .. }] if a.input.contains("/w/a.txt")),
        "{out:?}"
    );
}

#[test]
fn interrupted_commands_with_a_process_settle_from_the_terminal_list() {
    let mut s = bg_state();
    ev(&mut s, BG_STARTED);
    let done = T_DONE.replace(r#""status":"completed""#, r#""status":"interrupted""#);
    let end = ev(&mut s, &done);
    assert!(end.iter().any(|u| matches!(u, Update::Tool(c) if c.id == "exec-1" && c.status == ToolStatus::Background)), "{end:?}");
    let list = s.outbox.pop().expect("terminal list request");
    assert_eq!(list["method"], "thread/backgroundTerminals/list");
    let gone = map_event(&mut s, &json!({"id": list["id"], "result": {"data": []}}));
    assert!(
        matches!(&gone[..], [Update::Tool(c)] if c.id == "exec-1" && c.status == ToolStatus::Stopped),
        "{gone:?}"
    );
    assert!(s.terminals.is_empty() && s.tools.is_empty());
}

#[test]
fn interrupted_commands_still_listed_stay_background() {
    let mut s = bg_state();
    ev(&mut s, BG_STARTED);
    ev(&mut s, &T_DONE.replace(r#""status":"completed""#, r#""status":"interrupted""#));
    let list = s.outbox.pop().unwrap();
    let live = json!({"data": [{"itemId": "exec-1", "processId": "pty-7", "command": "pnpm dev", "cwd": "/w"}]});
    assert_eq!(map_event(&mut s, &json!({"id": list["id"], "result": live})), []);
    assert!(s.terminals.contains_key("exec-1"));
}

#[test]
fn cancel_running_fixture_settles_the_interrupted_command() {
    let lines = crate::record::fixture("codex", "cancel-running");
    let values: Vec<Value> = lines.iter().map(|l| serde_json::from_str(l).unwrap()).collect();
    let thread =
        values.iter().find_map(|v| v["result"]["thread"]["id"].as_str()).unwrap_or_default();
    let mut s = State { active: true, thread: thread.into(), ..State::default() };
    let mut out = vec![];
    for v in values.iter().filter(|v| v["aether"].is_null()) {
        out.extend(map_event(&mut s, v));
    }
    let calls = last_calls(&out);
    let cmd =
        calls.iter().find(|c| matches!(c.detail, ToolDetail::Command { .. })).expect("command row");
    assert_eq!(cmd.status, ToolStatus::Background);
    let list = s.outbox.pop().expect("terminal list request");
    assert_eq!(list["method"], "thread/backgroundTerminals/list");
    let gone = map_event(&mut s, &json!({"id": list["id"], "result": {"data": []}}));
    assert!(matches!(&gone[..], [Update::Tool(c)] if c.status == ToolStatus::Stopped), "{gone:?}");
}

#[test]
fn spawn_agent_call_routes_its_new_thread_and_fails_alone() {
    let mut s = State { active: true, thread: "main".into(), ..State::default() };
    let spawn = |status: &str, receivers: Value| {
        json!({"method": "item/completed", "params": {"threadId": "main", "item": {
            "type": "collabAgentToolCall", "id": "call_1", "tool": "spawnAgent", "status": status,
            "senderThreadId": "main", "receiverThreadIds": receivers,
            "prompt": "Run `echo hi` and report the output", "model": null, "reasoningEffort": null, "agentsStates": {},
        }}})
    };
    let mut started = spawn("inProgress", json!([]));
    started["method"] = "item/started".into();
    let row = tool(&map_event(&mut s, &started)).clone();
    assert_eq!(row.title, "Run agent");
    assert_eq!(row.detail.summary(), "Run `echo hi` and report the output");
    assert_eq!(map_event(&mut s, &spawn("completed", json!(["sub"]))), []);
    let reply = on_thread(SUB_REPLY, "sub");
    assert!(
        matches!(&tool(&map_event(&mut s, &reply)).detail, ToolDetail::Task { output, .. } if output == "hi")
    );

    map_event(&mut s, &started);
    let failed = tool(&map_event(&mut s, &spawn("failed", json!([])))).clone();
    assert_eq!((failed.id.as_str(), failed.status), ("call_1", ToolStatus::Failed));
    assert!(s.active);
}

#[test]
fn mcp_startup_failure_is_a_failed_row_not_an_error() {
    let mut s = State { active: true, thread: MAIN.into(), ..State::default() };
    let status = |status: &str, error: Value| json!({"method": "mcpServer/startupStatus/updated", "params": {"threadId": MAIN, "name": "codex_apps", "status": status, "error": error, "failureReason": null}});
    assert_eq!(map_event(&mut s, &status("starting", Value::Null)), []);
    let why = "handshake failed: connection closed\nstderr: npx not found";
    let call = tool(&map_event(&mut s, &status("failed", why.into()))).clone();
    assert_eq!(
        (call.id.as_str(), call.title.as_str()),
        ("mcp-codex_apps", "Start MCP server codex_apps")
    );
    assert_eq!(call.status, ToolStatus::Failed);
    assert_eq!(call.error.as_deref(), Some("handshake failed: connection closed"));
    assert!(matches!(&call.detail, ToolDetail::Other { output, .. } if output == why));
    assert!(s.active && s.error.is_none());
}

#[test]
fn side_requests_ask_a_cheap_model_for_a_structured_title() {
    assert_eq!(
        side_thread_params("/w", "gpt-5.6-luna"),
        json!({"cwd": "/w", "approvalPolicy": "never", "sandbox": "read-only", "ephemeral": true, "model": "gpt-5.6-luna"})
    );
    assert!(side_thread_params("/w", "").get("model").is_none());
    let turn = side_turn_params("side", "fix the login bug");
    assert_eq!(turn["threadId"], "side");
    assert_eq!((turn["effort"].as_str(), turn["summary"].as_str()), (Some("low"), Some("none")));
    assert_eq!(
        turn["outputSchema"],
        json!({"type": "object", "properties": {"title": {"type": "string"}}, "required": ["title"], "additionalProperties": false})
    );
    let text = turn["input"][0]["text"].as_str().unwrap();
    assert!(text.starts_with(TITLE_PROMPT) && text.ends_with("User prompt:\nfix the login bug"));
}

#[test]
fn side_title_reads_the_structured_agent_message() {
    let message = |text: &str| json!({"type": "agentMessage", "id": "m", "text": text});
    assert_eq!(
        side_title(&message(r#"{"title":" Fix login bug "}"#)),
        Some("Fix login bug".into())
    );
    assert_eq!(side_title(&message(r#"{"title":""}"#)), None);
    assert_eq!(side_title(&message("Fix login bug")), None);
    assert_eq!(side_title(&json!({"type": "userMessage", "text": r#"{"title":"x"}"#})), None);
}

#[test]
fn first_turn_of_a_fresh_thread_names_it_through_a_side_thread() {
    let mut s = State {
        thread: "main".into(),
        cwd: "/w".into(),
        model: "gpt-6.1-sol".into(),
        naming: Naming::Wanted,
        ..State::default()
    };
    start_turn(&mut s, "Fix the login bug", &[]);
    assert_eq!(s.naming, Naming::Prompt("Fix the login bug".into()));
    ev(&mut s, r#"{"id":0,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}"#);
    let [list] = &std::mem::take(&mut s.outbox)[..] else { panic!("{:?}", s.outbox) };
    assert_eq!((list["method"].as_str(), list["id"].as_u64()), (Some("model/list"), Some(1)));
    assert_eq!(s.naming, Naming::Off);
    start_turn(&mut s, "second", &[]);
    assert_eq!(s.naming, Naming::Off);

    let models = r#"{"id":1,"result":{"data":[{"id":"gpt-6.1-sol","model":"gpt-6.1-sol"},{"id":"gpt-5.6-luna","model":"gpt-5.6-luna"}],"nextCursor":null}}"#;
    assert_eq!(ev(&mut s, models), []);
    let [start] = &std::mem::take(&mut s.outbox)[..] else { panic!("{:?}", s.outbox) };
    assert_eq!(start["method"], "thread/start");
    assert_eq!(start["params"], side_thread_params("/w", "gpt-5.6-luna"));
    let started = json!({"id": start["id"], "result": {"thread": {"id": "side", "name": null}}});
    assert_eq!(map_event(&mut s, &started), []);
    assert_eq!(s.naming, Naming::Side("side".into()));
    let [turn] = &std::mem::take(&mut s.outbox)[..] else { panic!("{:?}", s.outbox) };
    assert_eq!(turn["params"], side_turn_params("side", "Fix the login bug"));

    let done = json!({"method": "item/completed", "params": {"threadId": "side", "item": {"type": "agentMessage", "id": "m", "text": r#"{"title":"Fix login bug"}"#}}});
    assert_eq!(map_event(&mut s, &done), []);
    let [set, unsubscribe] = &std::mem::take(&mut s.outbox)[..] else { panic!("{:?}", s.outbox) };
    assert_eq!(set["method"], "thread/name/set");
    assert_eq!(set["params"], json!({"threadId": "main", "name": "Fix login bug"}));
    assert_eq!(unsubscribe["method"], "thread/unsubscribe");
    assert_eq!(unsubscribe["params"], json!({"threadId": "side"}));
    assert_eq!(s.naming, Naming::Off);
    let side_done = TURN_DONE.replace("01a15157-b7e2-7e01-ad4c-0dcb40b02092", "side");
    assert_eq!(ev(&mut s, &side_done), []);
    assert!(s.outbox.is_empty());

    let renamed = r#"{"method":"thread/name/updated","params":{"threadId":"main","threadName":"Fix login bug"}}"#;
    assert_eq!(ev(&mut s, renamed), [Update::Title("Fix login bug".into())]);
}

#[test]
fn title_falls_back_to_the_session_model_and_gives_up_quietly() {
    let mut s = State { model: "gpt-6.1-sol".into(), ..State::default() };
    s.calls.insert(7, Call::Models("hi".into()));
    ev(&mut s, r#"{"id":7,"error":{"code":-32601,"message":"unknown method"}}"#);
    let [start] = &std::mem::take(&mut s.outbox)[..] else { panic!("{:?}", s.outbox) };
    assert_eq!(start["params"]["model"], "gpt-6.1-sol");
    let failed = json!({"id": start["id"], "error": {"code": -32600, "message": "bad model"}});
    assert_eq!(map_event(&mut s, &failed), []);
    assert!(s.outbox.is_empty() && s.naming == Naming::Off);

    let mut s = side_state();
    assert_eq!(ev(&mut s, &TURN_DONE.replace("01a15157-b7e2-7e01-ad4c-0dcb40b02092", "side")), []);
    let [unsubscribe] = &s.outbox[..] else { panic!("{:?}", s.outbox) };
    assert_eq!(unsubscribe["method"], "thread/unsubscribe");
    assert!(s.active && s.naming == Naming::Off);
}

const MCP_APPROVAL: &str = r#"{"method":"mcpServer/elicitation/request","id":11,"params":{"serverName":"figma","threadId":"t","mode":"form","message":"Approve app tool call?","requestedSchema":{"type":"object","properties":{}},"_meta":{"codex_approval_kind":"mcp_tool_call","tool_params":{"file":"abc"}}}}"#;
const PERMS: &str = r#"{"method":"item/permissions/requestApproval","id":12,"params":{"threadId":"t","turnId":"u","itemId":"i","cwd":"/p","startedAtMs":1,"reason":"Needs network","permissions":{"network":{"enabled":true}}}}"#;

#[test]
fn mcp_tool_approvals_ask_and_accept() {
    let mut s = State { active: true, ..State::default() };
    let [Update::Ask { id, ask: Ask::Approval(Approval { tool, title, input, .. }) }] =
        &ev(&mut s, MCP_APPROVAL)[..]
    else {
        panic!()
    };
    assert_eq!(
        (tool.as_str(), title.as_str(), input.as_str()),
        ("mcp", "Approve app tool call?", "figma\n{\n  \"file\": \"abc\"\n}")
    );
    assert_eq!(
        reply(&mut s, *id, &Answer::Approve { input: String::new(), always: false }),
        Some(json!({"id": 11, "result": {"action": "accept", "content": {}, "_meta": null}}))
    );
    ev(&mut s, MCP_APPROVAL);
    assert_eq!(
        reply(&mut s, 2, &Answer::Deny),
        Some(json!({"id": 11, "result": {"action": "decline", "content": null, "_meta": null}}))
    );
}

#[test]
fn url_elicitation_shows_the_link() {
    let mut s = State { active: true, ..State::default() };
    let url = r#"{"method":"mcpServer/elicitation/request","id":13,"params":{"serverName":"linear","threadId":"t","mode":"url","elicitationId":"e","message":"Sign in to Linear","url":"https://linear.app/oauth"}}"#;
    let [Update::Ask { id, ask: Ask::Form(f) }] = &ev(&mut s, url)[..] else { panic!() };
    assert_eq!(
        (f.title.as_str(), f.message.as_str(), f.url.as_deref(), f.fields.len()),
        (
            "linear needs you to open a link",
            "Sign in to Linear",
            Some("https://linear.app/oauth"),
            0
        )
    );
    assert_eq!(
        reply(&mut s, *id, &Answer::Form(vec![])),
        Some(json!({"id": 13, "result": {"action": "accept", "content": null, "_meta": null}}))
    );
    ev(&mut s, url);
    assert_eq!(
        reply(&mut s, 2, &Answer::Deny),
        Some(json!({"id": 13, "result": {"action": "decline", "content": null, "_meta": null}}))
    );
}

#[test]
fn permission_requests_grant_for_turn_or_session() {
    let mut s = State { active: true, ..State::default() };
    let [Update::Ask { id, ask: Ask::Approval(Approval { tool, reason, options, .. }) }] =
        &ev(&mut s, PERMS)[..]
    else {
        panic!()
    };
    assert_eq!((tool.as_str(), reason.as_str()), ("permissions", "Needs network"));
    assert_eq!(options[0].id, "session");
    assert_eq!(
        reply(&mut s, *id, &Answer::Pick("session".into())),
        Some(
            json!({"id": 12, "result": {"permissions": {"network": {"enabled": true}}, "scope": "session"}})
        )
    );
    ev(&mut s, PERMS);
    let once = Answer::Approve { input: String::new(), always: true };
    assert_eq!(
        reply(&mut s, 2, &once),
        Some(
            json!({"id": 12, "result": {"permissions": {"network": {"enabled": true}}, "scope": "turn"}})
        )
    );
    ev(&mut s, PERMS);
    assert_eq!(
        reply(&mut s, 3, &Answer::Deny),
        Some(json!({"id": 12, "result": {"permissions": {}, "scope": "turn"}}))
    );
}

#[test]
fn other_elicitations_still_decline() {
    let mut s = State { active: true, ..State::default() };
    let verify = r#"{"method":"mcpServer/elicitation/request","id":14,"params":{"mode":"openai/userVerification","challenge":"c","title":"t","description":"d"}}"#;
    assert!(
        matches!(&ev(&mut s, verify)[..], [Update::Warning(w)] if w.contains("user verification"))
    );
    assert_eq!(
        s.outbox.last(),
        Some(&json!({"id": 14, "result": {"action": "decline", "content": null, "_meta": null}}))
    );
}

#[test]
fn every_fixture_replays_without_panicking() {
    let all = fixtures("codex");
    assert!(!all.is_empty());
    for (name, _) in all {
        let out = replay(&name);
        assert!(!out.is_empty(), "{name} produced no updates");
    }
}

#[test]
fn a_stale_completion_never_ends_the_next_turn() {
    let mut s = State { thread: "t".into(), ..State::default() };
    let started = |s: &mut State, line: Value, turn: &str| {
        map_event(
            s,
            &json!({"id": line["id"], "result": {"turn": {"id": turn, "status": "inProgress"}}}),
        )
    };
    let first = start_turn(&mut s, "first", &[]);
    started(&mut s, first, "turn-1");
    on_cancel(&mut s);
    let second = start_turn(&mut s, "second", &[]);
    started(&mut s, second, "turn-2");
    let old = r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"turn-1","items":[],"status":"interrupted","error":null}}}"#;
    assert_eq!(ev(&mut s, old), []);
    assert!(s.active && s.turn_id.as_deref() == Some("turn-2"));
    assert!(s.stale.is_empty());
}

#[test]
fn a_prompt_during_cancel_waits_for_the_interrupted_turn() {
    let mut s = State { thread: "t".into(), ..State::default() };
    let first = start_turn(&mut s, "first", &[]);
    map_event(&mut s, &json!({"id": first["id"], "result": {"turn": {"id": "turn-1"}}}));
    on_cancel(&mut s);
    s.next = Some(("second".into(), vec![]));
    let done = r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"turn-1","items":[],"status":"interrupted","error":null}}}"#;
    let out = ev(&mut s, done);
    assert_eq!(out, [Update::Final(String::new())]);
    assert!(s.active && s.next.is_none());
    assert_eq!(s.outbox.pop().unwrap()["params"]["input"][0]["text"], "second");
}

#[test]
fn other_turns_completing_are_logged_not_applied() {
    let mut s = State {
        thread: "t".into(),
        active: true,
        turn_id: Some("turn-1".into()),
        ..State::default()
    };
    let other = r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"turn-7","items":[],"status":"completed","error":null}}}"#;
    assert_eq!(ev(&mut s, other), []);
    assert!(s.active);
}

#[test]
fn server_started_turns_wake_with_their_reason() {
    let mut s = State { thread: "t".into(), ..State::default() };
    ev(&mut s, r#"{"method":"thread/queue/changed","params":{"threadId":"t"}}"#);
    let out = ev(
        &mut s,
        r#"{"method":"turn/started","params":{"threadId":"t","turn":{"id":"turn-9","items":[],"status":"inProgress","error":null}}}"#,
    );
    assert!(
        matches!(&out[..], [Update::Wake, Update::Notice(n)] if n.kind == NoticeKind::Wake && n.text.contains("a queued message")),
        "{out:?}"
    );
    assert!(s.active && s.turn_id.as_deref() == Some("turn-9"));
}

#[test]
fn goal_updates_name_the_wake_reason() {
    let mut s = State { thread: "t".into(), ..State::default() };
    ev(
        &mut s,
        r#"{"method":"thread/goal/updated","params":{"threadId":"t","goal":{"objective":"ship it"}}}"#,
    );
    let out =
        ev(&mut s, r#"{"method":"turn/started","params":{"threadId":"t","turn":{"id":"turn-9"}}}"#);
    assert!(
        matches!(&out[..], [Update::Wake, Update::Notice(n)] if n.text.ends_with("goal: ship it")),
        "{out:?}"
    );
    let out = ev(
        &mut s,
        r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"turn-9","status":"completed"}}}"#,
    );
    assert_eq!(out, [Update::Final(String::new())]);
    let out = ev(
        &mut s,
        r#"{"method":"turn/started","params":{"threadId":"t","turn":{"id":"turn-10"}}}"#,
    );
    assert!(
        matches!(&out[..], [Update::Wake, Update::Notice(n)] if n.text == "Codex started a turn on its own"),
        "{out:?}"
    );
}

#[test]
fn unsteerable_turn_refuses_the_steer() {
    let mut s = State {
        thread: "t".into(),
        active: true,
        turn_id: Some("turn-1".into()),
        ..State::default()
    };
    let line = steer_turn(&mut s, "also this".into(), vec![]);
    let id = line["id"].as_u64().unwrap();
    let err = json!({"id": id, "error": {"code": -32600, "message": "cannot steer a review turn",
        "data": {"message": "x", "codexErrorInfo": {"activeTurnNotSteerable": {"turnKind": "review"}}}}});
    let out = map_event(&mut s, &err);
    assert!(out.contains(&Update::SteerRefused("also this".into())), "{out:?}");
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.id == "info-steer" && n.text == "Codex can't take messages during /review. Yours stays queued.")), "{out:?}");
    assert!(!out.contains(&Update::Wake) && s.outbox.iter().all(|l| l["method"] != "turn/start"));
}

#[test]
fn steer_errors_are_classified_by_message_not_code() {
    let kind =
        |k: &str| json!({"data": {"codexErrorInfo": {"activeTurnNotSteerable": {"turnKind": k}}}});
    assert_eq!(steer_refused(&kind("compact")), Some("compact"));
    assert_eq!(steer_refused(&kind("regular")), None);
    let msg = |m: &str| json!({"code": 1, "message": m});
    assert!(steer_refused(&msg("no active turn to steer")).is_some());
    assert!(steer_refused(&msg("expected active turn id `a` but found `b`")).is_some());
    assert!(steer_refused(&msg("cannot steer a compact turn")).is_some());
    assert!(steer_refused(&msg("boom")).is_none());
}

#[test]
fn other_steer_errors_refuse_and_warn() {
    let mut s = State {
        thread: "t".into(),
        active: true,
        turn_id: Some("turn-1".into()),
        ..State::default()
    };
    let line = steer_turn(&mut s, "x".into(), vec![]);
    let out =
        map_event(&mut s, &json!({"id": line["id"], "error": {"code": -1, "message": "boom"}}));
    assert_eq!(out, [Update::SteerRefused("x".into()), Update::Warning("boom".into())]);
}

#[test]
fn accepted_steers_are_reported() {
    let mut s = State {
        thread: "t".into(),
        active: true,
        turn_id: Some("turn-1".into()),
        ..State::default()
    };
    let line = steer_turn(&mut s, "also this".into(), vec![]);
    let ok = json!({"id": line["id"], "result": {"turnId": "turn-1"}});
    assert_eq!(map_event(&mut s, &ok), [Update::Steered("also this".into())]);
}

#[test]
fn turns_no_longer_override_the_thread_sandbox() {
    let mut s = State { thread: "t".into(), access: AccessMode::AutoEdit, ..State::default() };
    let line = start_turn(&mut s, "hi", &[]);
    for key in ["sandboxPolicy", "approvalPolicy", "approvalsReviewer"] {
        assert!(line["params"].get(key).is_none(), "{key}");
    }
}

#[test]
fn live_output_cleans_split_escapes_and_stays_bounded() {
    let mut out = LiveOutput::default();
    out.push("a\x1b[3");
    assert_eq!(out.push("1mred\x1b[0m\nload 10%\rload 99%"), "ared\nload 99%");
    let big = "x".repeat(1000) + "\n";
    for _ in 0..100 {
        out.push(&big);
    }
    assert!(out.push("").len() <= LIVE_OUTPUT_CAP + 1100);
}

#[test]
fn thread_modes_invert_access() {
    for mode in [
        AccessMode::Ask,
        AccessMode::ReadOnly,
        AccessMode::AutoEdit,
        AccessMode::Auto,
        AccessMode::Full,
    ] {
        let [policy, _, sandbox, reviewer] = access(mode);
        assert_eq!(thread_mode(policy, &json!({"type": sandbox}), reviewer), Some(mode));
    }
    assert_eq!(thread_mode("on-failure", &json!({"type": "readOnly"}), "user"), None);
}

#[test]
fn thread_replies_report_model_and_mode() {
    let result = json!({"model": "gpt-6.1-sol", "approvalPolicy": "never", "approvalsReviewer": "user",
        "sandbox": {"type": "dangerFullAccess"}});
    let out = thread_updates(&result, AccessMode::Ask);
    assert!(
        matches!(&out[..], [Update::Model(m), Update::Mode(AccessMode::Full), Update::Notice(n)]
        if m == "gpt-6.1-sol" && n.kind == NoticeKind::ModeChange),
        "{out:?}"
    );
    assert_eq!(thread_updates(&result, AccessMode::Full).len(), 2);
}

#[test]
fn resume_failures_explain_themselves() {
    assert_eq!(
        resume_notice("thread writer lock held").text,
        "Couldn't resume the Codex thread because it's open in another Codex window. Started a new one."
    );
    assert_eq!(
        resume_notice("gone").text,
        "Couldn't resume the Codex thread (gone). Started a new one."
    );
}

#[test]
fn stdout_end_error_carries_the_stderr_tail() {
    let mut tail = StderrTail::default();
    tail.push("\x1b[31mboom\x1b[0m");
    assert_eq!(tail.fatal("codex exited"), "codex exited\nboom");
}

#[test]
fn cancel_and_steer_fixtures_end_cleanly() {
    assert!(replay("steer").iter().any(|u| matches!(u, Update::Steered(_))));
    let cancel = replay("cancel");
    assert_eq!(cancel.iter().filter(|u| matches!(u, Update::Final(_))).count(), 1);
    assert!(last_calls(&cancel).iter().all(|c| c.status != ToolStatus::Running));
}

#[test]
fn server_death_stops_background_rows_and_stop_tool_refuses() {
    let mut s = bg_state();
    ev(&mut s, BG_STARTED);
    ev(&mut s, T_DONE);
    s.dead = Some("codex exited".into());
    assert!(terminate(&mut s, "exec-1").is_none());
    s.dead = None;
    let out = settle_background(&mut s);
    assert!(
        matches!(&out[..], [Update::Tool(c)] if c.id == "exec-1" && c.status == ToolStatus::Stopped),
        "{out:?}"
    );
    assert!(s.tools.is_empty() && s.terminals.is_empty());
}

#[test]
fn only_main_thread_commands_register_terminals() {
    let mut s = State { active: true, thread: MAIN.into(), ..State::default() };
    ev(&mut s, SPAWNED);
    s.tools.remove("call_cb112290082546d6996819381506ca7a");
    ev(&mut s, SUB_ECHO_STARTED);
    assert!(s.terminals.is_empty(), "{:?}", s.terminals);
}

#[test]
fn sub_agent_collab_rows_stay_under_a_live_task_at_turn_end() {
    let mut s =
        State { active: true, thread: MAIN.into(), turn_id: Some("u".into()), ..State::default() };
    ev(&mut s, SPAWNED);
    let wait = format!(
        r#"{{"method":"item/started","params":{{"threadId":"{CHILD}","item":{{"type":"collabAgentToolCall","id":"call_w","tool":"wait","status":"inProgress","senderThreadId":"{CHILD}","receiverThreadIds":["g"],"prompt":null,"agentsStates":{{}}}}}}}}"#
    );
    ev(&mut s, &wait);
    let done = r#"{"method":"turn/completed","params":{"threadId":"MAIN","turn":{"id":"u","items":[],"status":"completed","error":null}}}"#.replace("MAIN", MAIN);
    let end = ev(&mut s, &done);
    assert!(last_calls(&end).iter().all(|c| c.id != "call_w"), "{end:?}");
    assert_eq!(s.tools["call_w"].status, ToolStatus::Running);
}

#[test]
fn reconcile_settles_only_the_snapshot() {
    let mut s = bg_state();
    ev(&mut s, BG_STARTED);
    ev(&mut s, &T_DONE.replace(r#""status":"completed""#, r#""status":"interrupted""#));
    let list = s.outbox.pop().unwrap();
    ev(&mut s, &BG_STARTED.replace("exec-1", "exec-2").replace("pty-7", "pty-8"));
    let out = map_event(&mut s, &json!({"id": list["id"], "result": {"data": []}}));
    assert!(matches!(&out[..], [Update::Tool(c)] if c.id == "exec-1"), "{out:?}");
    assert!(s.terminals.contains_key("exec-2"));
}

#[test]
fn cancelling_twice_drops_the_held_prompt() {
    let mut s = bg_state();
    on_cancel(&mut s);
    queue_next(&mut s, "B".into(), vec![]);
    on_cancel(&mut s);
    assert!(s.next.is_none());
    let out = ev(&mut s, T_DONE);
    assert!(out.iter().all(|u| !matches!(u, Update::Warning(_))), "{out:?}");
    assert!(!s.active && s.outbox.is_empty());
}

#[test]
fn steers_dropped_by_a_cancel_return_their_chip() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    s.queued.push(("more".into(), vec![]));
    on_cancel(&mut s);
    let out =
        ev(&mut s, r#"{"method":"turn/started","params":{"threadId":"t","turn":{"id":"u"}}}"#);
    assert!(out.iter().any(|u| matches!(u, Update::SteerRefused(t) if t == "more")), "{out:?}");
}

fn note(method: &str, params: Value) -> Value {
    json!({"method": method, "params": params})
}

fn live() -> State {
    State { active: true, thread: "t".into(), ..State::default() }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aether-codex-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn item_event(method: &str, item: Value) -> Value {
    note(method, json!({"threadId": "t", "item": item}))
}

#[test]
fn retries_rate_limits_reroutes_and_warnings_are_notices() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let retrying = MODEL_ERROR.replace(r#""willRetry":false"#, r#""willRetry":true"#);
    let retrying = retrying.replace("01a15157-b7e2-7e01-ad4c-0dcb40b02092", "t");
    assert!(
        matches!(&ev(&mut s, &retrying)[..], [Update::Notice(n)] if n.kind == NoticeKind::Retry && n.state == NoticeState::Running)
    );
    let limits = note(
        "account/rateLimits/updated",
        json!({"rateLimits": {"limitId": "codex", "primary": {"usedPercent": 91, "windowDurationMins": 300, "resetsAt": 1800000000}, "rateLimitReachedType": null}}),
    );
    assert!(
        matches!(&map_event(&mut s, &limits)[..], [Update::Notice(n)] if n.kind == NoticeKind::RateLimit && n.text.contains("91") && n.until_ms == Some(1_800_000_000_000))
    );
    let rerouted = note(
        "model/rerouted",
        json!({"threadId": "t", "turnId": "u", "fromModel": "gpt-a", "toModel": "gpt-b", "reason": "highRiskCyberActivity"}),
    );
    let out = map_event(&mut s, &rerouted);
    assert!(
        out.contains(&Update::Model("gpt-b".into()))
            && out
                .iter()
                .any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::ModelChange))
    );
    assert_eq!(
        map_event(&mut s, &note("warning", json!({"threadId": null, "message": "slow disk"}))),
        [Update::Warning("slow disk".into())]
    );
}

#[test]
fn quiet_rate_limits_and_failed_ones_differ() {
    let mut s = live();
    let snap = |used: u64, reached: Value| {
        note(
            "account/rateLimits/updated",
            json!({"rateLimits": {"limitId": null, "primary": {"usedPercent": used}, "rateLimitReachedType": reached}}),
        )
    };
    assert_eq!(map_event(&mut s, &snap(40, Value::Null)), []);
    let out = map_event(&mut s, &snap(40, "rate_limit_reached".into()));
    assert!(
        matches!(&out[..], [Update::Notice(n)] if n.id == "ratelimit-default" && n.state == NoticeState::Failed && n.text.contains("rate_limit_reached")),
        "{out:?}"
    );
}

#[test]
fn retry_closes_on_the_next_delta_and_limit_errors_add_a_notice() {
    let mut s = live();
    let on_t = |line: &str| line.replace("01a15157-b7e2-7e01-ad4c-0dcb40b02092", "t");
    let retrying = on_t(&MODEL_ERROR.replace(r#""willRetry":false"#, r#""willRetry":true"#));
    let opened = ev(&mut s, &retrying);
    let Update::Notice(open) = &opened[0] else { panic!("{opened:?}") };
    assert_eq!(open.id, "retry-01a15157-fbcf-7280-ad8f-9fc33a9247a4");
    assert!(open.text.starts_with("Retrying: "));
    let delta =
        note("item/agentMessage/delta", json!({"threadId": "t", "itemId": "m", "delta": "hi"}));
    let out = map_event(&mut s, &delta);
    assert!(
        matches!(&out[..], [Update::Notice(n), Update::Delta(_)] if n.id == open.id && n.state == NoticeState::Done),
        "{out:?}"
    );
    assert!(map_event(&mut s, &delta).iter().all(|u| !matches!(u, Update::Notice(_))));
    let limited = MODEL_ERROR
        .replace(r#""codexErrorInfo":"other""#, r#""codexErrorInfo":"usageLimitExceeded""#);
    let out = ev(&mut s, &on_t(&limited));
    assert!(
        matches!(&out[..], [Update::Notice(n), Update::Error(_)] if n.id == "ratelimit-default" && n.kind == NoticeKind::RateLimit),
        "{out:?}"
    );
}

#[test]
fn hooks_run_as_notices() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let run = |status: &str| json!({"id": "h1", "eventName": "preToolUse", "handlerType": "command", "status": status, "statusMessage": null, "entries": []});
    let started = map_event(
        &mut s,
        &note("hook/started", json!({"threadId": "t", "turnId": "u", "run": run("running")})),
    );
    let done = map_event(
        &mut s,
        &note("hook/completed", json!({"threadId": "t", "turnId": "u", "run": run("failed")})),
    );
    assert!(
        matches!((&started[..], &done[..]), ([Update::Notice(a)], [Update::Notice(b)]) if a.id == "hook-h1" && b.id == "hook-h1" && b.state == NoticeState::Failed)
    );
}

#[test]
fn hook_text_carries_status_message_and_entries() {
    let mut s = live();
    let run = json!({"id": "h2", "eventName": "stop", "status": "completed", "statusMessage": "linted", "entries": [{"kind": "feedback", "text": "a"}, {"kind": "warning", "text": "b"}]});
    let out = map_event(&mut s, &note("hook/completed", json!({"threadId": "t", "run": run})));
    let [Update::Notice(n)] = &out[..] else { panic!("{out:?}") };
    assert_eq!((n.text.as_str(), n.state), ("stop hook: linted", NoticeState::Done));
    assert_eq!((n.kind, n.detail.as_deref()), (NoticeKind::Hook, Some("a\nb")));
}

#[test]
fn review_images_sleep_and_commentary_items_show() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let review = note(
        "item/completed",
        json!({"threadId": "t", "item": {"type": "exitedReviewMode", "id": "r1", "review": "1 issue: x"}}),
    );
    assert_eq!(map_event(&mut s, &review), [Update::Delta("\n\n1 issue: x".into())]);
    let sleep = note(
        "item/started",
        json!({"threadId": "t", "item": {"type": "sleep", "id": "s1", "durationMs": 5000}}),
    );
    assert_eq!(tool(&map_event(&mut s, &sleep)).title, "Wait 5 s");
    let commentary = note(
        "item/started",
        json!({"threadId": "t", "item": {"type": "agentMessage", "id": "m1", "text": "", "phase": "commentary"}}),
    );
    map_event(&mut s, &commentary);
    let delta = note(
        "item/agentMessage/delta",
        json!({"threadId": "t", "itemId": "m1", "delta": "I'll run ls"}),
    );
    assert_eq!(map_event(&mut s, &delta), [Update::Thinking("I'll run ls".into())]);
}

#[test]
fn sleep_settles_done_and_commentary_gap_fill_stays_thinking() {
    let mut s = live();
    let sleep = json!({"type": "sleep", "id": "s1", "durationMs": 1200, "status": "completed"});
    assert_eq!(
        tool(&map_event(&mut s, &item_event("item/started", sleep.clone()))).status,
        ToolStatus::Running
    );
    let done = map_event(&mut s, &item_event("item/completed", sleep));
    assert_eq!(tool(&done).status, ToolStatus::Done);
    let m = |text: &str| json!({"type": "agentMessage", "id": "m1", "text": text, "phase": "commentary"});
    map_event(&mut s, &item_event("item/started", m("")));
    let delta =
        note("item/agentMessage/delta", json!({"threadId": "t", "itemId": "m1", "delta": "ab"}));
    map_event(&mut s, &delta);
    assert_eq!(
        map_event(&mut s, &item_event("item/completed", m("abcd"))),
        [Update::Thinking("cd".into())]
    );
    assert!(s.phases.is_empty());
}

#[test]
fn agent_message_questions_become_prose() {
    let mut s = live();
    let item = json!({"type": "agentMessage", "id": "m1", "text": "Pick one.", "phase": null,
        "questions": [{"title": "Which db?", "options": ["pg", "sqlite"]}, {"title": "Why?", "options": null}]});
    let out = map_event(&mut s, &item_event("item/completed", item));
    assert_eq!(
        out,
        [Update::Delta("Pick one.\n\n**Which db?**\n- pg\n- sqlite\n\n**Why?**".into())]
    );
}

#[test]
fn plan_items_stream_like_messages() {
    let mut s = live();
    let delta = note("item/plan/delta", json!({"threadId": "t", "itemId": "p1", "delta": "1. a"}));
    assert_eq!(map_event(&mut s, &delta), [Update::Delta("1. a".into())]);
    let done =
        item_event("item/completed", json!({"type": "plan", "id": "p1", "text": "1. a\n2. b"}));
    assert_eq!(map_event(&mut s, &done), [Update::Delta("\n2. b".into())]);
}

#[test]
fn raw_reasoning_shows_only_without_a_summary() {
    let mut s = live();
    let raw = |id: &str| {
        note(
            "item/reasoning/textDelta",
            json!({"threadId": "t", "itemId": id, "delta": "raw", "contentIndex": 0}),
        )
    };
    let summary = |id: &str| {
        note(
            "item/reasoning/summaryTextDelta",
            json!({"threadId": "t", "itemId": id, "delta": "sum", "summaryIndex": 0}),
        )
    };
    assert_eq!(map_event(&mut s, &raw("r1")), [Update::Thinking("raw".into())]);
    assert_eq!(map_event(&mut s, &summary("r1")), [Update::Thinking("sum".into())]);
    assert_eq!(map_event(&mut s, &raw("r1")), []);
    assert_eq!(
        map_event(&mut s, &summary("r2")),
        [Update::Thinking("\n\n".into()), Update::Thinking("sum".into())]
    );
    assert_eq!(map_event(&mut s, &raw("r2")), []);
}

#[test]
fn review_and_hook_prompt_items_are_notices() {
    let mut s = live();
    let entered = item_event(
        "item/started",
        json!({"type": "enteredReviewMode", "id": "e1", "review": "the diff"}),
    );
    let out = map_event(&mut s, &entered);
    assert!(
        matches!(&out[..], [Update::Notice(n)] if n.id == "info-review-e1" && n.text == "Reviewing the diff" && n.kind == NoticeKind::Info),
        "{out:?}"
    );
    let hook = item_event(
        "item/completed",
        json!({"type": "hookPrompt", "id": "k1", "fragments": [{"hookRunId": "h", "text": "one"}, {"hookRunId": "h", "text": "two"}]}),
    );
    let out = map_event(&mut s, &hook);
    assert!(
        matches!(&out[..], [Update::Notice(n)] if n.id == "hook-prompt-k1" && n.kind == NoticeKind::Hook && n.text == "one\ntwo"),
        "{out:?}"
    );
}

#[test]
fn image_items_copy_into_the_chat_folder() {
    let dir = scratch("images");
    let src = dir.join("shot.png");
    std::fs::write(&src, b"png").unwrap();
    let attachments = dir.join("chat");
    let mut s = State { attachments: Some(attachments.clone()), ..live() };
    let view = item_event("item/completed", json!({"type": "imageView", "id": "v1", "path": src}));
    let call = tool(&map_event(&mut s, &view)).clone();
    assert_eq!((call.title.as_str(), call.status), ("View image", ToolStatus::Done));
    assert!(matches!(&call.detail, ToolDetail::Read { path, .. } if path.ends_with("shot.png")));
    assert!(
        call.images.len() == 1
            && call.images[0].path.starts_with(&attachments)
            && call.images[0].path.exists()
    );
    assert_eq!((call.images[0].mime.as_str(), call.images[0].size), ("image/png", 3));
    let generated = json!({"type": "imageGeneration", "id": "g1", "status": "completed", "revisedPrompt": "a red cat", "savedPath": src, "result": "", "failure": null});
    let call = tool(&map_event(&mut s, &item_event("item/completed", generated))).clone();
    assert_eq!(call.title, "Generate image");
    assert!(matches!(&call.detail, ToolDetail::Other { input, .. } if input == "a red cat"));
    assert_eq!((call.status, call.images.len()), (ToolStatus::Done, 1));
    let failed = json!({"type": "imageGeneration", "id": "g2", "status": "failed", "revisedPrompt": "x", "savedPath": null, "result": "", "failure": {"type": "usageLimitExceeded", "limitId": "codex", "resetsAt": null}});
    let call = tool(&map_event(&mut s, &item_event("item/completed", failed))).clone();
    assert_eq!((call.status, call.images.len()), (ToolStatus::Failed, 0));
    assert_eq!(copy_image("/no/such/file.png", Some(&attachments)), None);
    assert_eq!(copy_image(src.to_str().unwrap(), None), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dynamic_tool_calls_show_text_images_and_failure() {
    let dir = scratch("dynamic");
    let mut s = State { attachments: Some(dir.clone()), ..live() };
    let started = json!({"type": "dynamicToolCall", "id": "d1", "tool": "bash", "arguments": {"command": "ls"}, "status": "inProgress", "contentItems": null, "success": null});
    let call = tool(&map_event(&mut s, &item_event("item/started", started))).clone();
    assert!(matches!(&call.detail, ToolDetail::Command { command, .. } if command == "ls"));
    let png = "iVBORw0KGgo=";
    let done = json!({"type": "dynamicToolCall", "id": "d1", "tool": "bash", "arguments": {"command": "ls"}, "status": "failed", "success": false,
        "contentItems": [{"type": "inputText", "text": "boom"}, {"type": "inputImage", "imageUrl": format!("data:image/png;base64,{png}")}]});
    let call = tool(&map_event(&mut s, &item_event("item/completed", done))).clone();
    assert_eq!((call.status, call.images.len()), (ToolStatus::Failed, 1));
    assert!(matches!(&call.detail, ToolDetail::Command { output, .. } if output == "boom"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mcp_results_keep_images_and_structured_content() {
    let dir = scratch("mcp");
    let mut s = State { attachments: Some(dir.clone()), ..live() };
    let item = |result: Value| json!({"type": "mcpToolCall", "id": "c1", "server": "srv", "tool": "t", "arguments": {}, "status": "completed", "result": result});
    let structured = item(json!({"content": [], "structuredContent": {"n": 1}}));
    let call = tool(&map_event(&mut s, &item_event("item/completed", structured))).clone();
    assert!(
        matches!(&call.detail, ToolDetail::Other { output, .. } if output == "{\n  \"n\": 1\n}"),
        "{call:?}"
    );
    let pic = item(
        json!({"content": [{"type": "text", "text": "ok"}, {"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png"}]}),
    );
    let call = tool(&map_event(&mut s, &item_event("item/completed", pic))).clone();
    assert_eq!(call.images.len(), 1);
    assert!(matches!(&call.detail, ToolDetail::Other { output, .. } if output == "ok"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn renames_show_both_paths() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let mv = note(
        "item/started",
        json!({"threadId": "t", "item": {"type": "fileChange", "id": "f1", "status": "inProgress",
        "changes": [{"path": "/w/a.rs", "kind": {"type": "update", "move_path": "/w/b.rs"}, "diff": ""}]}}),
    );
    let call = tool(&map_event(&mut s, &mv)).clone();
    assert_eq!(
        (call.title.as_str(), call.detail.summary().as_str()),
        ("Rename file", "/w/a.rs → /w/b.rs")
    );
}

#[test]
fn turn_diff_fills_rows_that_finished_without_one() {
    let mut s = State { turn_id: Some("u".into()), ..live() };
    let change = |kind: Value| json!({"type": "fileChange", "id": "f1", "status": "completed", "changes": [{"path": "/w/src/a.rs", "kind": kind, "diff": ""}]});
    map_event(
        &mut s,
        &item_event("item/started", change(json!({"type": "update", "move_path": null}))),
    );
    let done = map_event(
        &mut s,
        &item_event("item/completed", change(json!({"type": "update", "move_path": null}))),
    );
    assert!(matches!(tool(&done).detail, ToolDetail::Edit { ref diff, .. } if diff.is_empty()));
    let diff = "diff --git a/src/b.rs b/src/b.rs\n--- a/src/b.rs\n+++ b/src/b.rs\n@@ -1 +1 @@\n-no\n+nope\ndiff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(
        map_event(
            &mut s,
            &note("turn/diff/updated", json!({"threadId": "t", "turnId": "u", "diff": diff}))
        ),
        []
    );
    let end = map_event(
        &mut s,
        &note(
            "turn/completed",
            json!({"threadId": "t", "turn": {"id": "u", "status": "completed", "error": null}}),
        ),
    );
    let rows = last_calls(&end);
    let [row] = &rows[..] else { panic!("{end:?}") };
    assert!(
        matches!(&row.detail, ToolDetail::Edit { diff, added: 1, removed: 1, .. } if diff.contains("+new") && !diff.contains("nope")),
        "{row:?}"
    );
    assert!(s.turn_diff.is_empty());
}

#[test]
fn auto_review_and_strict_review_notices() {
    let mut s = live();
    let review = |method: &str, status: &str| {
        note(
            method,
            json!({"threadId": "t", "turnId": "u", "reviewId": "rv1", "targetItemId": null, "startedAtMs": 1,
        "review": {"status": status, "rationale": "safe enough"}, "action": {"type": "command", "command": "rm -rf build", "cwd": "/w", "source": "shell"}}),
        )
    };
    let out = map_event(&mut s, &review("item/autoApprovalReview/started", "inProgress"));
    assert!(
        matches!(&out[..], [Update::Notice(n)] if n.id == "info-review-rv1" && n.state == NoticeState::Running && n.text == "Auto-review inProgress: rm -rf build"),
        "{out:?}"
    );
    let out = map_event(&mut s, &review("item/autoApprovalReview/completed", "approved"));
    assert!(
        matches!(&out[..], [Update::Notice(n)] if n.state == NoticeState::Done && n.detail.as_deref() == Some("safe enough") && n.kind == NoticeKind::Info),
        "{out:?}"
    );
    let out = map_event(
        &mut s,
        &note("autoApprovalReview/strictReviewRequired", json!({"threadId": "t"})),
    );
    assert!(matches!(&out[..], [Update::Notice(n)] if n.id == "info-strict-review"), "{out:?}");
}

#[test]
fn live_row_notifications_update_their_rows() {
    let mut s = live();
    let cmd = json!({"type": "commandExecution", "id": "c1", "command": "cat", "cwd": "/w", "processId": "p", "status": "inProgress", "commandActions": []});
    map_event(&mut s, &item_event("item/started", cmd));
    let out = map_event(
        &mut s,
        &note(
            "item/commandExecution/terminalInteraction",
            json!({"threadId": "t", "turnId": "u", "itemId": "c1", "processId": "p", "stdin": "yes"}),
        ),
    );
    assert!(
        matches!(&tool(&out).detail, ToolDetail::Command { output, .. } if output.ends_with("\n> yes")),
        "{out:?}"
    );
    let patch = note(
        "item/fileChange/patchUpdated",
        json!({"threadId": "t", "turnId": "u", "itemId": "f9", "changes": [
        {"path": "/w/a", "kind": {"type": "add"}, "diff": "x\n"}, {"path": "/w/b", "kind": {"type": "delete"}, "diff": ""}]}),
    );
    let out = map_event(&mut s, &patch);
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(s.tools.contains_key("f9#0") && s.tools.contains_key("f9#1"));
    let call = json!({"type": "mcpToolCall", "id": "m1", "server": "s", "tool": "t", "arguments": {}, "status": "inProgress"});
    map_event(&mut s, &item_event("item/started", call));
    let out = map_event(
        &mut s,
        &note(
            "item/mcpToolCall/progress",
            json!({"threadId": "t", "turnId": "u", "itemId": "m1", "message": "50%"}),
        ),
    );
    assert!(
        matches!(&tool(&out).detail, ToolDetail::Other { output, .. } if output == "50%"),
        "{out:?}"
    );
    assert_eq!(
        map_event(
            &mut s,
            &note(
                "item/mcpToolCall/progress",
                json!({"threadId": "t", "itemId": "nope", "message": "x"})
            )
        ),
        []
    );
}

#[test]
fn oauth_and_config_warnings() {
    let mut s = live();
    let out = map_event(
        &mut s,
        &note(
            "mcpServer/oauthLogin/completed",
            json!({"name": "linear", "success": true, "error": null}),
        ),
    );
    assert!(
        matches!(&out[..], [Update::Notice(n)] if n.id == "mcp-linear" && n.kind == NoticeKind::McpAuth && n.text == "Signed in to linear" && n.state == NoticeState::Done),
        "{out:?}"
    );
    let out = map_event(
        &mut s,
        &note("configWarning", json!({"summary": "bad key", "details": "line 3"})),
    );
    assert!(
        matches!(&out[..], [Update::Warning(w)] if w.contains("bad key") && w.contains("line 3")),
        "{out:?}"
    );
    let out = map_event(
        &mut s,
        &note("deprecationNotice", json!({"summary": "old flag", "details": null})),
    );
    assert_eq!(out, [Update::Warning("old flag".into())]);
    assert_eq!(
        map_event(&mut s, &note("guardianWarning", json!({"threadId": "t", "message": "careful"}))),
        [Update::Warning("careful".into())]
    );
}

#[test]
fn unknown_notifications_are_logged_not_mapped() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    assert_eq!(map_event(&mut s, &note("thread/teleported", json!({"threadId": "t"}))), []);
    assert!(!log_unknown("codex", "thread/teleported"), "already logged by map_event");
}

#[test]
fn ignored_notifications_are_not_logged() {
    let mut s = live();
    for method in [
        "thread/status/changed",
        "thread/compacted",
        "skills/changed",
        "rawResponseItem/completed",
        "model/verification",
        "turn/moderationMetadata",
        "account/updated",
    ] {
        assert_eq!(map_event(&mut s, &note(method, json!({"threadId": "t"}))), [], "{method}");
        assert!(log_unknown("codex", method), "{method} was logged");
    }
}

#[test]
fn plan_command_starts_a_plan_mode_turn() {
    let mut s = State {
        thread: "t".into(),
        model: "gpt-x".into(),
        effort: "high".into(),
        ..State::default()
    };
    let line = start_turn(&mut s, "/plan  add a cache", &[]);
    let p = &line["params"];
    assert_eq!(line["method"], "turn/start");
    assert_eq!(p["input"][0]["text"], "add a cache");
    assert_eq!(
        p["collaborationMode"],
        json!({"mode": "plan", "settings": {"model": "gpt-x", "reasoning_effort": "high", "developer_instructions": null}})
    );
    s.effort.clear();
    let line = start_turn(&mut s, "/plan x", &[]);
    assert_eq!(line["params"]["collaborationMode"]["settings"]["reasoning_effort"], Value::Null);
    let plain = start_turn(&mut s, "/plan", &[]);
    assert_eq!(plain["params"]["collaborationMode"]["mode"], "default");
    assert_eq!(plain["params"]["input"][0]["text"], "/plan");
}

#[test]
fn only_main_thread_collab_rows_count_as_main() {
    let mut s = State { active: true, thread: MAIN.into(), ..State::default() };
    ev(&mut s, SPAWNED);
    s.tools.remove("call_cb112290082546d6996819381506ca7a");
    let wait = |thread: &str, id: &str| {
        item_event_on(
            thread,
            json!({"type": "collabAgentToolCall", "id": id, "tool": "wait", "status": "inProgress", "senderThreadId": thread, "receiverThreadIds": ["g"], "prompt": null, "agentsStates": {}}),
        )
    };
    map_event(&mut s, &wait(CHILD, "w_child"));
    assert!(!s.main_collab.contains("w_child"), "{:?}", s.main_collab);
    map_event(&mut s, &wait(MAIN, "w_main"));
    assert!(s.main_collab.contains("w_main"));
}

fn item_event_on(thread: &str, item: Value) -> Value {
    note("item/started", json!({"threadId": thread, "item": item}))
}

#[test]
fn queued_steers_are_returned_when_the_turn_never_starts() {
    let mut s = live();
    start_turn(&mut s, "go", &[]);
    s.queued.push(("also this".into(), vec![]));
    let id = s.calls.keys().next().copied().unwrap();
    let out = map_event(&mut s, &json!({"id": id, "error": {"message": "nope"}}));
    assert!(out.contains(&Update::SteerRefused("also this".into())), "{out:?}");
    assert!(s.queued.is_empty());
}

#[test]
fn death_returns_queued_steers_and_stops_background_rows() {
    let mut s = bg_state();
    ev(&mut s, BG_STARTED);
    ev(&mut s, T_DONE);
    s.active = true;
    s.queued.push(("more".into(), vec![]));
    s.next = Some(("held".into(), vec![]));
    let out = on_death(&mut s, "codex exited".into());
    assert!(
        out.iter().any(|u| matches!(u, Update::Tool(c) if c.status == ToolStatus::Stopped)),
        "{out:?}"
    );
    assert!(out.contains(&Update::SteerRefused("more".into())), "{out:?}");
    assert!(out.contains(&Update::Error("codex exited".into())));
    assert_eq!(out.last(), Some(&Update::Final(String::new())));
    assert!(s.queued.is_empty() && s.next.is_none() && !s.active);
    assert_eq!(s.dead.as_deref(), Some("codex exited"));
    let mut idle = State::default();
    assert_eq!(on_death(&mut idle, "x".into()), [Update::Warning("x".into())]);
    assert_eq!(idle.dead.as_deref(), Some("x"));
}

#[test]
fn tools_and_compaction_fixtures_settle_every_row() {
    for scenario in ["tools", "tool-error", "compaction"] {
        let calls = last_calls(&replay(scenario));
        assert!(calls.iter().all(|c| c.status != ToolStatus::Running), "{scenario}: {calls:?}");
    }
}

fn done_on(thread: &str, item: Value) -> Value {
    note("item/completed", json!({"threadId": thread, "item": item}))
}

#[test]
fn child_sleep_and_bare_file_changes_finish_without_the_turn_diff() {
    let mut s =
        State { active: true, thread: MAIN.into(), turn_id: Some("u".into()), ..State::default() };
    ev(&mut s, SPAWNED);
    let sleep = json!({"type": "sleep", "id": "cs", "durationMs": 1000, "status": "completed"});
    map_event(&mut s, &item_event_on(CHILD, sleep.clone()));
    let out = map_event(&mut s, &done_on(CHILD, sleep));
    assert!(
        last_calls(&out).iter().any(|c| c.id == "cs" && c.status == ToolStatus::Done),
        "{out:?}"
    );
    let change = json!({"type": "fileChange", "id": "cf", "status": "completed", "changes": [{"path": "/w/src/a.rs", "kind": {"type": "update", "move_path": null}, "diff": ""}]});
    map_event(&mut s, &item_event_on(CHILD, change.clone()));
    let out = map_event(&mut s, &done_on(CHILD, change));
    assert!(
        last_calls(&out).iter().any(|c| c.id == "cf#0" && c.status == ToolStatus::Done),
        "{out:?}"
    );
    assert!(!s.tools.contains_key("cf#0"));
    let diff = "diff --git a/src/a.rs b/src/a.rs\n@@ -1 +1 @@\n-old\n+new\n";
    map_event(
        &mut s,
        &note("turn/diff/updated", json!({"threadId": MAIN, "turnId": "u", "diff": diff})),
    );
    let end = map_event(
        &mut s,
        &note(
            "turn/completed",
            json!({"threadId": MAIN, "turn": {"id": "u", "status": "completed", "error": null}}),
        ),
    );
    assert!(last_calls(&end).iter().all(|c| c.id != "cf#0"), "{end:?}");
}

#[test]
fn plan_mode_is_left_once_and_models_are_never_empty() {
    let mut s = State { thread: "t".into(), resolved: "gpt-res".into(), ..State::default() };
    let plan = start_turn(&mut s, "/plan do x", &[]);
    assert_eq!(plan["params"]["collaborationMode"]["settings"]["model"], "gpt-res");
    let next = start_turn(&mut s, "now build it", &[]);
    assert_eq!(next["params"]["collaborationMode"]["mode"], "default");
    assert_eq!(next["params"]["collaborationMode"]["settings"]["model"], "gpt-res");
    let after = start_turn(&mut s, "again", &[]);
    assert!(after["params"].get("collaborationMode").is_none());
    s.model = "gpt-set".into();
    let again = start_turn(&mut s, "/plan y", &[]);
    assert_eq!(again["params"]["collaborationMode"]["settings"]["model"], "gpt-set");
}

#[test]
fn leaving_plan_mode_without_any_model_omits_the_mode() {
    let mut s = State { thread: "t".into(), ..State::default() };
    start_turn(&mut s, "/plan do x", &[]);
    let next = start_turn(&mut s, "now build it", &[]);
    assert!(next["params"].get("collaborationMode").is_none(), "{next}");
    s.resolved = "gpt-res".into();
    let retry = start_turn(&mut s, "again", &[]);
    assert_eq!(retry["params"]["collaborationMode"]["mode"], "default");
}

#[test]
fn thread_results_remember_the_resolved_model() {
    let mut s = State::default();
    note_thread(&mut s, &json!({"model": "gpt-9"}));
    assert_eq!(s.resolved, "gpt-9");
}

#[test]
fn death_and_final_errors_fail_the_retry_notice() {
    let mut s = live();
    let retrying = MODEL_ERROR
        .replace(r#""willRetry":false"#, r#""willRetry":true"#)
        .replace("01a15157-b7e2-7e01-ad4c-0dcb40b02092", "t");
    ev(&mut s, &retrying);
    let out = on_death(&mut s, "gone".into());
    assert!(
        matches!(&out[0], Update::Notice(n) if n.state == NoticeState::Failed && n.kind == NoticeKind::Retry),
        "{out:?}"
    );
    let mut s = live();
    ev(&mut s, &retrying);
    let out = ev(&mut s, &MODEL_ERROR.replace("01a15157-b7e2-7e01-ad4c-0dcb40b02092", "t"));
    assert!(matches!(&out[0], Update::Notice(n) if n.state == NoticeState::Failed), "{out:?}");
}

#[test]
fn empty_terminal_stdin_is_skipped() {
    let mut s = live();
    let cmd = json!({"type": "commandExecution", "id": "c1", "command": "cat", "cwd": "/w", "processId": "p", "status": "inProgress", "commandActions": []});
    map_event(&mut s, &item_event("item/started", cmd));
    let out = map_event(
        &mut s,
        &note(
            "item/commandExecution/terminalInteraction",
            json!({"threadId": "t", "itemId": "c1", "processId": "p", "stdin": ""}),
        ),
    );
    assert_eq!(out, []);
}

#[test]
fn turn_diff_sections_split_on_line_starts_and_path_boundaries() {
    let call = |path: &str| {
        let mut c = start("f", "apply_patch", &json!({"file_path": path, "diff": ""}));
        c.status = ToolStatus::Done;
        c
    };
    let diff = "diff --git a/src/a.rs b/src/a.rs\n@@ -1 +1 @@\n-x\n+ diff --git a/b.rs b/b.rs\ndiff --git a/a.rs b/a.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let mut c = call("/w/a.rs");
    assert!(fill_diff(&mut c, diff));
    assert!(matches!(&c.detail, ToolDetail::Edit { diff, .. } if diff.contains("+new")), "{c:?}");
    let mut c = call("/w/data.rs");
    assert!(!fill_diff(&mut c, diff), "suffix without a / boundary must not match");
}

#[test]
fn phases_and_reasoning_reset_each_turn() {
    let mut s = live();
    s.phases.insert("m".into(), "commentary".into());
    s.reasoning.insert("r".into(), true);
    start_turn(&mut s, "go", &[]);
    assert!(s.phases.is_empty() && s.reasoning.is_empty());
    s.phases.insert("m".into(), "commentary".into());
    s.reasoning.insert("r".into(), true);
    s.turn_id = Some("u".into());
    map_event(
        &mut s,
        &note(
            "turn/completed",
            json!({"threadId": "t", "turn": {"id": "u", "status": "completed", "error": null}}),
        ),
    );
    assert!(s.phases.is_empty() && s.reasoning.is_empty());
}

#[test]
fn auto_review_names_network_and_mcp_actions() {
    let mut s = live();
    let review = |action: Value| {
        note(
            "item/autoApprovalReview/started",
            json!({"threadId": "t", "reviewId": "r", "review": {"status": "inProgress"}, "action": action}),
        )
    };
    let text = |s: &mut State, action: Value| match &map_event(s, &review(action))[..] {
        [Update::Notice(n)] => n.text.clone(),
        other => panic!("{other:?}"),
    };
    assert!(text(&mut s, json!({"type": "networkAccess", "host": "example.com", "port": 443, "protocol": "https", "target": "https://example.com"})).ends_with("example.com"));
    assert!(
        text(&mut s, json!({"type": "mcpToolCall", "server": "linear", "toolName": "search"}))
            .ends_with("linear.search")
    );
}

#[test]
fn copied_images_never_collide_and_dynamic_calls_keep_their_time() {
    let dir = scratch("seq");
    let src = dir.join("a.png");
    std::fs::write(&src, b"x").unwrap();
    let out = dir.join("out");
    let one = copy_image(src.to_str().unwrap(), Some(&out)).unwrap();
    let two = copy_image(src.to_str().unwrap(), Some(&out)).unwrap();
    assert_ne!(one.path, two.path);
    let mut s = live();
    let done = json!({"type": "dynamicToolCall", "id": "d", "tool": "t", "arguments": {}, "status": "completed", "success": true, "durationMs": 42, "contentItems": []});
    let call = tool(&map_event(&mut s, &item_event("item/completed", done))).clone();
    assert_eq!(call.ms, Some(42));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn completed_commentary_without_a_started_item_is_thinking() {
    let mut s = live();
    let item =
        json!({"type": "agentMessage", "id": "m9", "text": "plan it", "phase": "commentary"});
    assert_eq!(
        map_event(&mut s, &item_event("item/completed", item)),
        [Update::Thinking("plan it".into())]
    );
}

const CMD_APPROVAL: &str = r#"{"method":"item/commandExecution/requestApproval","id":21,"params":{"kind":"command","threadId":"t","turnId":"u","itemId":"exec-1","startedAtMs":1,"command":"/bin/zsh -lc 'pnpm add motion'","cwd":"/w","reason":"Needs network","networkApprovalContext":{"host":"registry.npmjs.org","protocol":"https"},"availableDecisions":["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["pnpm","add"]}},{"applyNetworkPolicyAmendment":{"network_policy_amendment":{"host":"registry.npmjs.org","action":"allow"}}},"decline"]}}"#;

#[test]
fn command_approvals_offer_the_clis_own_decisions() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let [Update::Ask { id, ask: Ask::Approval(a) }] = &ev(&mut s, CMD_APPROVAL)[..] else {
        panic!()
    };
    assert_eq!(
        (a.kind, a.tool_id.as_deref(), a.cwd.as_deref(), a.input.as_str()),
        (ApprovalKind::Command, Some("exec-1"), Some("/w"), "pnpm add motion")
    );
    assert!(a.reason.contains("registry.npmjs.org"));
    let ids: Vec<&str> = a.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["execpolicy", "network-allow"]);
    assert_eq!(
        reply(&mut s, *id, &Answer::Pick("execpolicy".into())),
        Some(
            json!({"id": 21, "result": {"decision": {"acceptWithExecpolicyAmendment": {"execpolicy_amendment": ["pnpm", "add"]}}}})
        )
    );
    ev(&mut s, CMD_APPROVAL);
    let network = json!({"host": "registry.npmjs.org", "action": "allow"});
    assert_eq!(
        reply(&mut s, 2, &Answer::Pick("network-allow".into())),
        Some(
            json!({"id": 21, "result": {"decision": {"applyNetworkPolicyAmendment": {"network_policy_amendment": network}}}})
        )
    );
}

#[test]
fn commands_without_listed_decisions_offer_the_session() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let bare = r#"{"method":"item/commandExecution/requestApproval","id":3,"params":{"threadId":"t","itemId":"e","command":"ls"}}"#;
    let [Update::Ask { id, ask: Ask::Approval(a) }] = &ev(&mut s, bare)[..] else { panic!() };
    assert_eq!(a.options[0].id, "session");
    assert_eq!(
        reply(&mut s, *id, &Answer::Pick("session".into())),
        Some(json!({"id": 3, "result": {"decision": "acceptForSession"}}))
    );
}

#[test]
fn denying_with_a_note_steers_the_live_turn() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    s.turn_id = Some("u".into());
    let [Update::Ask { id, .. }] = &ev(&mut s, CMD_APPROVAL)[..] else { panic!() };
    assert_eq!(
        reply(&mut s, *id, &Answer::DenyWith("use yarn".into())),
        Some(json!({"id": 21, "result": {"decision": "decline"}}))
    );
    let [steer] = &s.outbox[..] else { panic!("{:?}", s.outbox) };
    assert_eq!(steer["method"], "turn/steer");
    assert_eq!(steer["params"]["input"][0]["text"], "use yarn");
}

#[test]
fn every_question_reaches_the_user_and_answers_map_by_id() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let req = r#"{"method":"item/tool/requestUserInput","id":22,"params":{"threadId":"t","turnId":"u","itemId":"i","isBlocking":true,"questions":[{"id":"q1","header":"DB","question":"Which DB?","isOther":true,"isSecret":false,"options":[{"label":"pg","description":"Postgres"}]},{"id":"q2","header":"Key","question":"API key?","isOther":false,"isSecret":true,"options":null}]}}"#;
    let [Update::Ask { id, ask: Ask::Questions(qs) }] = &ev(&mut s, req)[..] else { panic!() };
    assert_eq!((qs.len(), qs[0].other, qs[1].secret), (2, true, true));
    assert_eq!(
        reply(&mut s, *id, &Answer::Answers(vec![vec!["pg".into()], vec!["sk-1".into()]])),
        Some(
            json!({"id": 22, "result": {"answers": {"q1": {"answers": ["pg"]}, "q2": {"answers": ["sk-1"]}}}})
        )
    );
}

#[test]
fn mcp_form_elicitations_become_forms() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let req = r#"{"method":"mcpServer/elicitation/request","id":23,"params":{"threadId":"t","turnId":"u","serverName":"jira","mode":"form","message":"Pick a project","requestedSchema":{"type":"object","properties":{"project":{"type":"string","enum":["WEB","API"],"title":"Project"},"notify":{"type":"boolean","default":true}},"required":["project"]},"_meta":null}}"#;
    let [Update::Ask { id, ask: Ask::Form(f) }] = &ev(&mut s, req)[..] else { panic!() };
    assert_eq!(
        (f.fields[0].kind, f.fields[0].required, f.fields[1].kind),
        (FieldKind::Choice, true, FieldKind::Bool)
    );
    assert_eq!(f.source.as_deref(), Some("jira"));
    let values =
        vec![("project".into(), vec!["API".into()]), ("notify".into(), vec!["false".into()])];
    assert_eq!(
        reply(&mut s, *id, &Answer::Form(values)),
        Some(
            json!({"id": 23, "result": {"action": "accept", "content": {"project": "API", "notify": false}, "_meta": null}})
        )
    );
    ev(&mut s, req);
    assert_eq!(
        reply(&mut s, 2, &Answer::Deny),
        Some(json!({"id": 23, "result": {"action": "decline", "content": null, "_meta": null}}))
    );
}

#[test]
fn schema_forms_map_every_field_type() {
    let schema = json!({"properties": {
        "name": {"type": "string", "title": "Name", "description": "Who", "default": "bob"},
        "size": {"type": "string", "oneOf": [{"const": "S", "title": "Small"}, {"const": "L"}]},
        "n": {"type": "integer"},
        "x": {"type": "number"},
        "tags": {"type": "array", "items": {"enum": ["a", "b"]}},
    }, "required": ["n"]});
    let (form, keys) = schema_form("hi", &schema);
    let kinds: Vec<FieldKind> = form.fields.iter().map(|f| f.kind).collect();
    assert_eq!(
        kinds,
        [
            FieldKind::Text,
            FieldKind::Choice,
            FieldKind::Number,
            FieldKind::Number,
            FieldKind::Multi
        ]
    );
    assert_eq!(form.message, "hi");
    let f = &form.fields;
    assert_eq!(
        (f[0].label.as_str(), f[0].description.as_str(), f[0].default.clone()),
        ("Name", "Who", vec!["bob".to_string()])
    );
    assert_eq!(
        (f[1].options.len(), f[1].options[0].label.as_str(), f[1].options[0].value.as_str()),
        (2, "Small", "S")
    );
    assert_eq!(f[4].options.len(), 2);
    assert!(f[2].required && !f[3].required);
    assert_eq!(keys[2], ("n".to_string(), "integer".to_string()));
}

#[test]
fn typed_form_values_follow_the_schema() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let req = r#"{"method":"mcpServer/elicitation/request","id":5,"params":{"threadId":"t","serverName":"m","mode":"form","message":"x","requestedSchema":{"properties":{"n":{"type":"integer"},"tags":{"type":"array","items":{"enum":["a","b"]}},"s":{"type":"string"}}}}}"#;
    let [Update::Ask { id, .. }] = &ev(&mut s, req)[..] else { panic!() };
    let values = vec![
        ("n".into(), vec!["3".into()]),
        ("tags".into(), vec!["a".into(), "b".into()]),
        ("s".into(), vec!["hi".into()]),
    ];
    assert_eq!(
        reply(&mut s, *id, &Answer::Form(values)),
        Some(
            json!({"id": 5, "result": {"action": "accept", "content": {"n": 3, "tags": ["a", "b"], "s": "hi"}, "_meta": null}})
        )
    );
}

#[test]
fn asks_resolved_elsewhere_are_withdrawn() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let [Update::Ask { id, .. }] = &ev(&mut s, CMD_APPROVAL)[..] else { panic!() };
    let resolved =
        r#"{"method":"serverRequest/resolved","params":{"threadId":"t","requestId":21}}"#;
    assert_eq!(ev(&mut s, resolved), [Update::AskResolved(*id)]);
    assert!(s.asks.is_empty());
    assert_eq!(ev(&mut s, resolved), []);
}

#[test]
fn an_idle_ask_resolved_elsewhere_ends_its_side_turn() {
    let mut s = State { thread: "t".into(), ..State::default() };
    let [Update::Ask { id, .. }] = &ev(&mut s, CMD_APPROVAL)[..] else { panic!() };
    let resolved =
        r#"{"method":"serverRequest/resolved","params":{"threadId":"t","requestId":21}}"#;
    assert_eq!(ev(&mut s, resolved), [Update::AskResolved(*id), Update::Final(String::new())]);
}

#[test]
fn unknown_requests_get_an_error_and_a_warning() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let out = ev(
        &mut s,
        r#"{"method":"item/tool/call","id":24,"params":{"threadId":"t","turnId":"u","callId":"c","namespace":null,"tool":"x","arguments":{}}}"#,
    );
    assert!(matches!(&out[..], [Update::Warning(w)] if w.contains("item/tool/call")));
    assert_eq!(s.outbox.last().unwrap()["error"]["code"], -32601);
}

#[test]
fn current_time_is_answered() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let out = ev(&mut s, r#"{"method":"currentTime/read","id":30,"params":{}}"#);
    assert_eq!(out, []);
    assert!(s.outbox[0]["result"]["currentTimeAt"].as_u64().unwrap() > 1_700_000_000);
}

#[test]
fn child_thread_asks_name_their_agent() {
    let mut s = State { active: true, thread: MAIN.into(), ..State::default() };
    ev(&mut s, SPAWNED);
    let line = CMD_APPROVAL.replace(r#""threadId":"t""#, &format!(r#""threadId":"{CHILD}""#));
    let [Update::Ask { ask: Ask::Approval(a), .. }] = &ev(&mut s, &line)[..] else { panic!() };
    assert_eq!(a.source.as_deref(), Some("echo_hi"));
}

#[test]
fn approval_fixtures_link_cards_to_rows() {
    for scenario in ["approval", "subagent-approval"] {
        let out = replay(scenario);
        let a = out
            .iter()
            .find_map(|u| match u {
                Update::Ask { ask: Ask::Approval(a), .. } => Some(a.clone()),
                _ => None,
            })
            .expect(scenario);
        assert!(a.tool_id.is_some(), "{scenario}");
        assert!(last_calls(&out).iter().any(|c| Some(&c.id) == a.tool_id.as_ref()), "{scenario}");
    }
}

#[test]
fn the_recorded_question_fixture_asks_every_question() {
    let out = replay("question");
    let qs = out
        .iter()
        .find_map(|u| match u {
            Update::Ask { ask: Ask::Questions(qs), .. } => Some(qs.clone()),
            _ => None,
        })
        .expect("a Questions ask");
    assert!(!qs.is_empty() && qs.iter().all(|q| !q.question.is_empty()));
}

#[test]
fn network_amendments_keep_their_own_action() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let amend = |action: &str| {
        format!(
            r#"{{"applyNetworkPolicyAmendment":{{"network_policy_amendment":{{"host":"h.io","action":"{action}"}}}}}}"#
        )
    };
    let line = format!(
        r#"{{"method":"item/commandExecution/requestApproval","id":40,"params":{{"threadId":"t","itemId":"e","command":"curl h.io","networkApprovalContext":{{"host":"h.io","protocol":"https"}},"availableDecisions":["accept","acceptForSession",{},{},"decline"]}}}}"#,
        amend("allow"),
        amend("deny")
    );
    let [Update::Ask { id, ask: Ask::Approval(a) }] = &ev(&mut s, &line)[..] else { panic!() };
    let ids: Vec<&str> = a.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["session", "network-allow", "network-deny"]);
    assert_eq!(a.options[0].label, "Allow h.io for this conversation");
    assert_eq!(a.options[2].label, "Always block network access to h.io");
    let allow = json!({"host": "h.io", "action": "allow"});
    assert_eq!(
        reply(&mut s, *id, &Answer::Pick("network-allow".into())),
        Some(
            json!({"id": 40, "result": {"decision": {"applyNetworkPolicyAmendment": {"network_policy_amendment": allow}}}})
        )
    );
}

#[test]
fn multi_selects_read_any_of_and_enum_names() {
    let schema = json!({"properties": {
        "tags": {"type": "array", "items": {"anyOf": [{"const": "a", "title": "Alpha"}, {"const": "b"}]}},
        "old": {"type": "string", "enum": ["x", "y"], "enumNames": ["Ex", "Why"]},
    }, "required": ["tags"]});
    let (form, _) = schema_form("m", &schema);
    let f = &form.fields;
    assert_eq!((f[0].kind, f[0].required, f[0].options.len()), (FieldKind::Multi, true, 2));
    assert_eq!((f[0].options[0].label.as_str(), f[0].options[0].value.as_str()), ("Alpha", "a"));
    assert_eq!((f[1].options[0].label.as_str(), f[1].options[0].value.as_str()), ("Ex", "x"));
}

#[test]
fn notes_on_any_denied_approval_steer_the_turn() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    s.turn_id = Some("u".into());
    ev(&mut s, PERMS);
    reply(&mut s, 1, &Answer::DenyWith("no net".into()));
    assert_eq!(s.outbox.last().unwrap()["method"], "turn/steer");
}

#[test]
fn multi_file_diffs_keep_their_paths() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let started = r#"{"method":"item/started","params":{"item":{"type":"fileChange","id":"fc","changes":[{"path":"/w/a.txt","kind":{"type":"add"},"diff":"x\n"},{"path":"/w/b.txt","kind":{"type":"add"},"diff":"y\n"}],"status":"inProgress"}}}"#;
    ev(&mut s, started);
    let ask = r#"{"method":"item/fileChange/requestApproval","id":9,"params":{"threadId":"t","itemId":"fc","reason":null}}"#;
    let [Update::Ask { ask: Ask::Approval(a), .. }] = &ev(&mut s, ask)[..] else { panic!() };
    let diff = a.diff.as_deref().unwrap();
    assert!(diff.contains("/w/a.txt") && diff.contains("/w/b.txt"), "{diff}");
}

#[test]
fn a_deny_note_never_reports_a_steer() {
    for (reply_line, expect_warning) in [
        (r#"{"id":ID,"result":{}}"#, false),
        (r#"{"id":ID,"error":{"code":-1,"message":"no turn"}}"#, true),
    ] {
        let mut s = State { active: true, thread: "t".into(), ..State::default() };
        s.turn_id = Some("u".into());
        let [Update::Ask { id, .. }] = &ev(&mut s, CMD_APPROVAL)[..] else { panic!() };
        reply(&mut s, *id, &Answer::DenyWith("use yarn".into()));
        let rpc = s.outbox.last().unwrap()["id"].as_u64().unwrap();
        let out = ev(&mut s, &reply_line.replace("ID", &rpc.to_string()));
        assert!(
            !out.iter().any(|u| matches!(u, Update::Steered(_) | Update::SteerRefused(_))),
            "{out:?}"
        );
        assert_eq!(out.iter().any(|u| matches!(u, Update::Warning(_))), expect_warning, "{out:?}");
    }
}

#[test]
fn answering_the_last_idle_ask_closes_its_side_turn() {
    let mut s = State { thread: "t".into(), ..State::default() };
    let [Update::Ask { id: first, .. }] = &ev(&mut s, CMD_APPROVAL)[..] else { panic!() };
    let second = CMD_APPROVAL.replace(r#""id":21"#, r#""id":22"#);
    let [Update::Ask { id: next, .. }] = &ev(&mut s, &second)[..] else { panic!() };
    reply(&mut s, *first, &Answer::Deny);
    assert_eq!(side_turn_end(&s), None, "another ask still waits");
    reply(&mut s, *next, &Answer::Deny);
    assert_eq!(side_turn_end(&s), Some(Update::Final(String::new())));
    s.active = true;
    assert_eq!(side_turn_end(&s), None, "a running turn ends on its own");
}

#[test]
fn a_late_failure_of_an_earlier_turns_row_is_a_failure_after_a_stop() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    s.turn_id = Some("u1".into());
    ev(&mut s, TOUCH_STARTED);
    s.turn_rows.clear();
    s.turn_id = Some("u2".into());
    on_cancel(&mut s);
    let failed = TOUCH_STARTED
        .replace("item/started", "item/completed")
        .replace(r#""status":"inProgress""#, r#""status":"failed""#)
        .replace(r#""exitCode":null"#, r#""exitCode":1"#);
    assert_eq!(tool(&ev(&mut s, &failed)).status, ToolStatus::Failed);
}

#[test]
fn a_subagent_activity_row_names_its_agent() {
    let item = json!({"type":"subAgentActivity","id":"c1","kind":"started","agentThreadId":"t2","agentPath":"/root/read_first_line"});
    let [call] = &item_calls(&item)[..] else { panic!() };
    assert!(
        matches!(&call.detail, ToolDetail::Task { agent, .. } if agent == "read_first_line"),
        "{call:?}"
    );
}

#[test]
fn death_stops_the_children_of_a_background_agent_before_the_agent() {
    let mut s = State::default();
    let mut task = start("call_a", "agent", &json!({"prompt": "scan"}));
    task.status = ToolStatus::Background;
    let mut kid = start("exec-c", "bash", &json!({"command": "ls"}));
    kid.parent = Some("call_a".into());
    let mut done = start("exec-d", "bash", &json!({"command": "pwd"}));
    done.parent = Some("call_a".into());
    done.status = ToolStatus::Done;
    for c in [task, kid, done] {
        s.tools.insert(c.id.clone(), c);
    }
    let out = on_death(&mut s, "codex exited".into());
    let stopped: Vec<&str> = out
        .iter()
        .filter_map(|u| match u {
            Update::Tool(c) if c.status == ToolStatus::Stopped => Some(c.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(stopped, ["exec-c", "call_a"]);
}

#[test]
fn closing_the_session_stops_background_rows() {
    let mut s = bg_state();
    ev(&mut s, BG_STARTED);
    ev(&mut s, T_DONE);
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let driver = CodexDriver {
        child: Arc::default(),
        stdin: Arc::default(),
        tx: tx.clone(),
        throttle: Throttle::new(tx),
        state: Arc::new(Mutex::new(s)),
        resumed: false,
        rec: None,
    };
    drop(driver);
    let out: Vec<Update> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(
        matches!(&out[..], [Update::Tool(c)] if c.id == "exec-1" && c.status == ToolStatus::Stopped),
        "{out:?}"
    );
}

fn lead_opts() -> DriverStartOptions {
    let lead = crate::LeadTools {
        exe: "/x/aether".into(),
        sock: "/s/o.sock".into(),
        token: "tok123".into(),
        specs: Value::Null,
    };
    DriverStartOptions { lead: Some(lead), ..opts(AccessMode::Full, "m", "") }
}

#[test]
fn lead_spawn_adds_aether_mcp_config() {
    let o = lead_opts();
    let args = launch_args(&o);
    assert_eq!(args[0], "app-server");
    for want in [
        r#"mcp_servers.aether.command="/x/aether""#,
        r#"mcp_servers.aether.args=["mcp"]"#,
        r#"mcp_servers.aether.default_tools_approval_mode="approve""#,
        r#"mcp_servers.aether.env_vars=["AETHER_ORCH_SOCK","AETHER_ORCH_TOKEN"]"#,
    ] {
        let at = args.iter().position(|a| a == want).unwrap_or_else(|| panic!("missing {want}"));
        assert_eq!(args[at - 1], "-c");
    }
    assert!(!args.iter().any(|a| a.contains("tok123")), "token stays out of argv");
    assert_eq!(
        spawn_env(&o),
        vec![
            ("AETHER_ORCH_SOCK".to_string(), "/s/o.sock".to_string()),
            ("AETHER_ORCH_TOKEN".to_string(), "tok123".to_string()),
        ]
    );
}

#[test]
fn non_lead_spawn_is_unchanged() {
    let o = opts(AccessMode::Full, "m", "");
    assert_eq!(launch_args(&o), vec!["app-server".to_string()]);
    assert!(spawn_env(&o).is_empty());
}

#[test]
fn lead_exe_with_space_and_quote_is_a_valid_toml_string() {
    let mut o = lead_opts();
    o.lead.as_mut().unwrap().exe = PathBuf::from(r#"/A B/"q"/aether"#);
    assert!(
        launch_args(&o).contains(&r#"mcp_servers.aether.command="/A B/\"q\"/aether""#.to_string())
    );
}

#[test]
fn network_deny_pick_sends_exact_deny_amendment() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let req = json!({"method": "item/commandExecution/requestApproval", "id": 41, "params": {"threadId": "t", "itemId": "e", "command": "curl h.io", "networkApprovalContext": {"host": "h.io", "protocol": "https"}, "availableDecisions": [{"applyNetworkPolicyAmendment": {"network_policy_amendment": {"host": "h.io", "action": "deny"}}}]}});
    let [Update::Ask { id, .. }] = &map_event(&mut s, &req)[..] else { panic!() };
    assert_eq!(
        reply(&mut s, *id, &Answer::Pick("network-deny".into())),
        Some(
            json!({"id": 41, "result": {"decision": {"applyNetworkPolicyAmendment": {"network_policy_amendment": {"host": "h.io", "action": "deny"}}}}})
        )
    );
}

#[test]
fn decision_reply_answer_sends_exact_accept() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let [Update::Ask { id, .. }] = &ev(&mut s, CMD_APPROVAL)[..] else { panic!() };
    assert_eq!(
        reply(&mut s, *id, &Answer::Reply("go".into())),
        Some(json!({"id": 21, "result": {"decision": "accept"}}))
    );
}

#[test]
fn decision_session_pick_without_session_decision_sends_exact_decline() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let [Update::Ask { id, .. }] = &ev(&mut s, CMD_APPROVAL)[..] else { panic!() };
    assert_eq!(
        reply(&mut s, *id, &Answer::Pick("session".into())),
        Some(json!({"id": 21, "result": {"decision": "decline"}}))
    );
}

#[test]
fn decision_execpolicy_pick_without_amendment_sends_exact_decline() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let bare = r#"{"method":"item/commandExecution/requestApproval","id":3,"params":{"threadId":"t","itemId":"e","command":"ls"}}"#;
    let [Update::Ask { id, .. }] = &ev(&mut s, bare)[..] else { panic!() };
    assert_eq!(
        reply(&mut s, *id, &Answer::Pick("execpolicy".into())),
        Some(json!({"id": 3, "result": {"decision": "decline"}}))
    );
}

#[test]
fn decision_network_allow_pick_without_amendment_sends_exact_decline() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let bare = r#"{"method":"item/commandExecution/requestApproval","id":3,"params":{"threadId":"t","itemId":"e","command":"ls"}}"#;
    let [Update::Ask { id, .. }] = &ev(&mut s, bare)[..] else { panic!() };
    assert_eq!(
        reply(&mut s, *id, &Answer::Pick("network-allow".into())),
        Some(json!({"id": 3, "result": {"decision": "decline"}}))
    );
}

#[test]
fn decision_network_deny_pick_without_amendment_sends_exact_decline() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let bare = r#"{"method":"item/commandExecution/requestApproval","id":3,"params":{"threadId":"t","itemId":"e","command":"ls"}}"#;
    let [Update::Ask { id, .. }] = &ev(&mut s, bare)[..] else { panic!() };
    assert_eq!(
        reply(&mut s, *id, &Answer::Pick("network-deny".into())),
        Some(json!({"id": 3, "result": {"decision": "decline"}}))
    );
}

#[test]
fn permissions_reply_answer_sends_exact_turn_grant() {
    let mut s = State { active: true, ..State::default() };
    let [Update::Ask { id, .. }] = &ev(&mut s, PERMS)[..] else { panic!() };
    assert_eq!(
        reply(&mut s, *id, &Answer::Reply("grant".into())),
        Some(
            json!({"id": 12, "result": {"permissions": {"network": {"enabled": true}}, "scope": "turn"}})
        )
    );
}

#[test]
fn permissions_unknown_pick_sends_exact_empty_turn_grant() {
    let mut s = State { active: true, ..State::default() };
    let [Update::Ask { id, .. }] = &ev(&mut s, PERMS)[..] else { panic!() };
    assert_eq!(
        reply(&mut s, *id, &Answer::Pick("bogus".into())),
        Some(json!({"id": 12, "result": {"permissions": {}, "scope": "turn"}}))
    );
}

fn fork_state() -> State {
    State {
        active: true,
        turn: 1,
        thread: "main".into(),
        model: "gpt-6.1-sol".into(),
        ..State::default()
    }
}

#[test]
fn fork_requests_pin_model_read_only_and_boundary() {
    let p = fork_params("main", "gpt-6.1-sol");
    assert_eq!(p["threadId"], "main");
    assert_eq!((p["ephemeral"].as_bool(), p["excludeTurns"].as_bool()), (Some(true), Some(true)));
    assert_eq!(
        (p["model"].as_str(), p["sandbox"].as_str()),
        (Some("gpt-6.1-sol"), Some("read-only"))
    );
    assert_eq!(p["approvalPolicy"], "never");
    assert!(p["developerInstructions"].as_str().unwrap().contains("boundary"));
    let turn = fork_turn_params("side", "why?");
    assert_eq!(turn["threadId"], "side");
    assert_eq!(turn["input"][0]["text"], "why?");
}

#[test]
fn side_question_needs_a_prior_turn_and_a_model() {
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    let mut s = State { turn: 0, ..fork_state() };
    assert!(fork_line(&mut s, false, "q", tx.clone()).is_none());
    assert!(fork_line(&mut s, true, "q", tx.clone()).is_some());
    let mut s = State { model: String::new(), ..fork_state() };
    assert!(fork_line(&mut s, false, "q", tx.clone()).is_none());
    s.resolved = "gpt-6.1-sol".into();
    assert!(fork_line(&mut s, false, "q", tx.clone()).is_some());
    let mut s = State { dead: Some("gone".into()), ..fork_state() };
    assert!(fork_line(&mut s, false, "q", tx).is_none());
}

#[test]
fn fork_answer_goes_to_the_reply_and_never_the_main_stream() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut s = fork_state();
    let line = fork_line(&mut s, false, "why?", tx).unwrap();
    assert_eq!(line["method"], "thread/fork");
    let forked = json!({"id": line["id"], "result": {"thread": {"id": "fork"}}});
    assert_eq!(map_event(&mut s, &forked), []);
    let [turn] = &std::mem::take(&mut s.outbox)[..] else { panic!("{:?}", s.outbox) };
    assert_eq!(
        (turn["method"].as_str(), turn["params"]["threadId"].as_str()),
        (Some("turn/start"), Some("fork"))
    );
    let approval = json!({"method": "item/commandExecution/requestApproval", "id": 9, "params": {"threadId": "fork", "command": "ls"}});
    assert_eq!(map_event(&mut s, &approval), []);
    assert_eq!(std::mem::take(&mut s.outbox)[0]["result"]["decision"], "decline");
    let say = |text: &str| json!({"method": "item/completed", "params": {"threadId": "fork", "item": {"type": "agentMessage", "id": "m", "text": text}}});
    for v in [say("because"), say("really")] {
        assert_eq!(map_event(&mut s, &v), []);
    }
    for line in [COMMENTARY_DELTA, USAGE_1, MODEL_ERROR] {
        let mut v: Value = serde_json::from_str(line).unwrap();
        v["params"]["threadId"] = "fork".into();
        assert_eq!(map_event(&mut s, &v), []);
    }
    assert!(rx.try_recv().is_err());
    let done = json!({"method": "turn/completed", "params": {"threadId": "fork", "turn": {"id": "t", "status": "completed"}}});
    assert_eq!(map_event(&mut s, &done), []);
    assert_eq!(rx.try_recv().unwrap(), Update::Final("because\n\nreally".into()));
    let [unsub] = &std::mem::take(&mut s.outbox)[..] else { panic!("{:?}", s.outbox) };
    assert_eq!(unsub["method"], "thread/unsubscribe");
    assert!(s.sides.is_empty() && s.active && s.turn == 1);
}

#[test]
fn fork_failures_are_errors_on_the_reply() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut s = fork_state();
    let line = fork_line(&mut s, false, "q", tx).unwrap();
    let bad = json!({"id": line["id"], "error": {"message": "no fork"}});
    assert_eq!(map_event(&mut s, &bad), []);
    assert_eq!(rx.try_recv().unwrap(), Update::Error("no fork".into()));

    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let line = fork_line(&mut s, false, "q", tx).unwrap();
    map_event(&mut s, &json!({"id": line["id"], "result": {"thread": {"id": "fork"}}}));
    let failed = json!({"method": "turn/completed", "params": {"threadId": "fork", "turn": {"id": "t", "status": "failed", "error": {"message": "boom"}}}});
    assert_eq!(map_event(&mut s, &failed), []);
    assert_eq!(rx.try_recv().unwrap(), Update::Error("boom".into()));

    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let line = fork_line(&mut s, false, "q", tx).unwrap();
    map_event(&mut s, &json!({"id": line["id"], "result": {"thread": {"id": "fork"}}}));
    on_death(&mut s, "gone".into());
    assert_eq!(rx.try_recv().unwrap(), Update::Error("gone".into()));
}
