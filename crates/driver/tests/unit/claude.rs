use super::*;
use crate::record::fixtures;
use aether_core::{
    AccessMode, ApprovalKind, Attachment, FieldKind, NoticeKind, NoticeState, PlanStatus,
    ToolStatus,
};

const CAPTURE: &str = include_str!("../fixtures/claude_tools.jsonl");

const BASH_USE: &str = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}"#;

#[test]
fn text_delta_becomes_delta() {
    let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}}"#;
    let mut s = State { active: true, ..State::default() };
    assert_eq!(map_line(&mut s, line), [Update::Delta("hi".into())]);
}

fn replay(scenario: &str) -> Vec<Update> {
    let mut s = State { active: true, ..State::default() };
    let mut out = Vec::new();
    for line in crate::record::fixture("claude", scenario) {
        let v: Value = serde_json::from_str(&line).unwrap();
        match v["aether"].as_str() {
            Some("prompt") => on_prompt(&mut s, v["text"].as_str().unwrap()),
            Some("steer") => {
                on_steer(&mut s, v["text"].as_str().unwrap());
                if let (Some(uuid), Some(last)) = (v["uuid"].as_str(), s.steer_ids.last_mut()) {
                    last.0 = uuid.to_string();
                }
            }
            Some("cancel") => {
                on_cancel(&mut s);
            }
            Some("answer") => {
                let answer = serde_json::from_value(v["answer"].clone()).unwrap();
                reply(&mut s, v["id"].as_u64().unwrap(), &answer);
            }
            _ => out.extend(map_line(&mut s, &line)),
        }
    }
    out
}

fn calls(updates: Vec<Update>) -> Vec<ToolCall> {
    updates
        .into_iter()
        .filter_map(|u| match u {
            Update::Tool(call) => Some(ToolCall { ms: None, ..call }),
            _ => None,
        })
        .collect()
}

#[test]
fn tool_use_then_failed_result() {
    let mut s = State::default();
    let running = calls(map_line(&mut s, BASH_USE));
    assert_eq!(running.len(), 1);
    assert_eq!(
        (running[0].id.as_str(), running[0].title.as_str(), running[0].status),
        ("toolu_1", "Run command", ToolStatus::Running)
    );
    let result = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"hi","is_error":true}]}}"#;
    let failed = calls(map_line(&mut s, result));
    assert_eq!((failed[0].status, failed[0].error.as_deref()), (ToolStatus::Failed, Some("hi")));
    assert!(s.tools.is_empty());
}

#[test]
fn live_capture_maps_every_tool_to_a_typed_row() {
    let mut s = State { active: true, ..State::default() };
    let mut out = Vec::new();
    for line in CAPTURE.lines() {
        out.extend(map_line(&mut s, line));
    }
    let mut rows: Vec<ToolCall> = Vec::new();
    for call in calls(out.clone()) {
        match rows.iter_mut().find(|r| r.id == call.id) {
            Some(row) => *row = call,
            None => rows.push(call),
        }
    }
    let dir = "/private/tmp/aether-s2-capture.cadHdt";
    let path = format!("{dir}/notes.txt");
    let edit = |diff: &str, added, removed| ToolDetail::Edit {
        path: path.clone(),
        diff: diff.into(),
        added,
        removed,
    };
    let command = |command: &str, output: &str, exit_code| ToolDetail::Command {
        command: command.into(),
        cwd: None,
        output: output.into(),
        exit_code,
    };
    let row = |title: &str, detail, status, error: Option<&str>| ToolCall {
        status,
        error: error.map(str::to_string),
        ..ToolCall::new("", title, detail)
    };
    // key order of pretty JSON follows serde_json's `preserve_order`, which the workspace may unify on
    let input: Value = match &rows[5].detail {
        ToolDetail::Other { input, .. } => serde_json::from_str(input).unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(input, json!({"query": "select:Grep,Glob", "max_results": 5}));
    let strip = |r: &ToolCall| {
        let mut r = ToolCall { id: String::new(), ..r.clone() };
        if let ToolDetail::Other { input, .. } = &mut r.detail {
            input.clear();
        }
        r
    };
    let got: Vec<ToolCall> = rows.iter().map(strip).collect();
    assert_eq!(
        got,
        [
            row("Write file", edit("@@ -0,0 +1,1 @@\n+a\n", 1, 0), ToolStatus::Done, None),
            row("Edit file", edit("@@ -1,1 +1,1 @@\n-a\n+b\n", 1, 1), ToolStatus::Done, None),
            row(
                "Read file",
                ToolDetail::Read { path: path.clone(), range: None },
                ToolStatus::Done,
                None
            ),
            row("List files in current directory", command("ls", "", None), ToolStatus::Done, None),
            row(
                "List nonexistent directory, expected to fail",
                command(
                    "ls missing-dir",
                    "\"missing-dir\": No such file or directory (os error 2)",
                    Some(2)
                ),
                ToolStatus::Failed,
                Some("\"missing-dir\": No such file or directory (os error 2)"),
            ),
            row(
                "Tool search",
                ToolDetail::Other {
                    name: "ToolSearch".into(),
                    input: String::new(),
                    output: "No matching deferred tools found".into(),
                },
                ToolStatus::Done,
                None,
            ),
        ]
    );
    assert!(rows.iter().all(|r| r.id.starts_with("toolu_")));
    assert!(s.tools.is_empty());
    let finals: Vec<&Update> = out.iter().filter(|u| matches!(u, Update::Final(_))).collect();
    assert!(matches!(finals.as_slice(), [Update::Final(t)] if t.contains("notes.txt now says")));
    let tail: Vec<&Update> = out.iter().rev().skip(1).take(4).collect();
    assert_eq!(
        tail,
        [
            &Update::Count { prompt: 14 + 27985 + 243970, completion: 1256, cached: 243970 },
            &Update::Cost(297_850),
            &Update::Window(1_000_000),
            &Update::Usage(2 + 175 + 39858),
        ]
    );
}

#[test]
fn denied_and_interrupted_calls_are_not_failures() {
    let mut s = State::default();
    map_line(&mut s, BASH_USE);
    let denied = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"Denied in Aether","is_error":true}]}}"#;
    assert_eq!(calls(map_line(&mut s, denied))[0].status, ToolStatus::Denied);
    on_prompt(&mut s, "go");
    map_line(&mut s, BASH_USE);
    on_cancel(&mut s);
    let cut = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"[Request interrupted by user for tool use]","is_error":true}]}}"#;
    assert_eq!(calls(map_line(&mut s, cut))[0].status, ToolStatus::Stopped);
}

const AGENT_USE: &str = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_a","name":"Agent","input":{"description":"Find styles","prompt":"look in src","subagent_type":"Explore"}}]}}"#;

fn last_calls(updates: &[Update]) -> Vec<ToolCall> {
    let mut rows: Vec<ToolCall> = Vec::new();
    for call in calls(updates.to_vec()) {
        match rows.iter_mut().find(|r| r.id == call.id) {
            Some(row) => *row = call,
            None => rows.push(call),
        }
    }
    rows
}

#[test]
fn subagent_calls_nest_under_their_agent_row() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, AGENT_USE);
    let child =
        BASH_USE.replace(r#""parent_tool_use_id":null"#, r#""parent_tool_use_id":"toolu_a""#);
    let out = calls(map_line(&mut s, &child));
    assert!(out.iter().any(|c| c.id == "toolu_1" && c.parent.as_deref() == Some("toolu_a")));
    assert!(
        out.iter()
            .any(|c| c.id == "toolu_a" && matches!(c.detail, ToolDetail::Task { tools: 1, .. }))
    );
    let replay = r#"{"type":"user","isReplay":true,"parent_tool_use_id":null,"message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#;
    assert_eq!(map_line(&mut s, replay), []);
}

#[test]
fn subagent_text_becomes_activity_not_reply() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, AGENT_USE);
    let text = r#"{"type":"assistant","parent_tool_use_id":"toolu_a","message":{"content":[{"type":"text","text":"Reading the css\nmore"}]}}"#;
    let out = map_line(&mut s, text);
    assert!(!out.iter().any(|u| matches!(u, Update::Delta(_))));
    assert!(
        matches!(&calls(out)[0].detail, ToolDetail::Task { activity, .. } if activity == "Reading the css")
    );
    let delta = r#"{"type":"stream_event","parent_tool_use_id":"toolu_a","event":{"delta":{"type":"text_delta","text":"Now "}}}"#;
    let out = map_line(&mut s, delta);
    assert!(!out.iter().any(|u| matches!(u, Update::Delta(_))));
    assert!(
        matches!(&calls(out)[0].detail, ToolDetail::Task { activity, .. } if activity == "Now")
    );
}

#[test]
fn agent_results_lose_the_internal_trailer() {
    let raw = "Found it in src/a.css.\nagentId: a1b2 (internal ID - do not mention to user. Use SendMessage with to: 'a1b2' to continue this agent.)\n<usage>total_tokens: 1200\ntool_uses: 3\nduration_ms: 900</usage>";
    assert_eq!(strip_agent_trailer(raw), ("Found it in src/a.css.".into(), Some((1200, 3))));
    let live = "[Subagent hand-back] note. The report follows:\n  line one\n  line two\nagentId: a1 (use SendMessage with to: 'a1', summary: '<5-10 word recap>' to continue this agent)\n<usage>subagent_tokens: 11070\ntool_uses: 1\nduration_ms: 7839</usage>";
    assert_eq!(strip_agent_trailer(live), ("line one\nline two".into(), Some((11070, 1))));
}

#[test]
fn agent_result_fills_task_tokens_and_output() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, AGENT_USE);
    let result = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_a","content":"Done.\nagentId: a1 (internal ID - x)\n<usage>total_tokens: 50\ntool_uses: 2</usage>"}]}}"#;
    let row = &calls(map_line(&mut s, result))[0];
    assert!(
        matches!(&row.detail, ToolDetail::Task { output, tokens: 50, tools: 2, .. } if output == "Done."),
        "{row:?}"
    );
}

#[test]
fn task_lines_update_the_row_and_stop_finds_it() {
    let mut s = State { active: true, ..State::default() };
    map_line(
        &mut s,
        AGENT_USE.replace(r#""subagent_type":"Explore""#, r#""subagent_type":"""#).as_str(),
    );
    let started = r#"{"type":"system","subtype":"task_started","task_id":"t1","tool_use_id":"toolu_a","description":"Find styles","subagent_type":"Explore","prompt":"look"}"#;
    let row = &calls(map_line(&mut s, started))[0];
    assert!(matches!(&row.detail, ToolDetail::Task { agent, .. } if agent == "Explore"));
    assert_eq!(s.tasks["t1"], ("toolu_a".to_string(), "Find styles".to_string()));
    let progress = r#"{"type":"system","subtype":"task_progress","task_id":"t1","tool_use_id":"toolu_a","description":"Reading","summary":"Reading css","usage":{"total_tokens":900,"tool_uses":4}}"#;
    let row = &calls(map_line(&mut s, progress))[0];
    assert!(
        matches!(&row.detail, ToolDetail::Task { tokens: 900, tools: 4, activity, .. } if activity == "Reading css")
    );
    let upd = |p: &str| {
        format!(r#"{{"type":"system","subtype":"task_updated","task_id":"t1","patch":{p}}}"#)
    };
    assert_eq!(
        calls(map_line(&mut s, &upd(r#"{"status":"running","is_backgrounded":true}"#)))[0].status,
        ToolStatus::Background
    );
    let failed = &calls(map_line(&mut s, &upd(r#"{"status":"failed","error":"boom"}"#)))[0];
    assert_eq!((failed.status, failed.error.as_deref()), (ToolStatus::Failed, Some("boom")));
    assert!(s.tasks.is_empty());
    map_line(&mut s, started);
    assert_eq!(
        calls(map_line(&mut s, &upd(r#"{"status":"killed"}"#)))[0].status,
        ToolStatus::Stopped
    );
    assert!(
        map_line(&mut s, r#"{"type":"system","subtype":"background_tasks_changed","tasks":[]}"#)
            .is_empty()
    );
}

#[test]
fn background_tasks_settle_their_original_row() {
    let mut s = State { active: true, ..State::default() };
    map_line(
        &mut s,
        &BASH_USE
            .replace(r#""command":"echo hi""#, r#""command":"sleep 8","run_in_background":true"#),
    );
    let started = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"Command running in background with ID: b1"}]}}"#;
    assert_eq!(calls(map_line(&mut s, started))[0].status, ToolStatus::Background);
    map_line(
        &mut s,
        r#"{"type":"system","subtype":"task_started","task_id":"b1","tool_use_id":"toolu_1","description":"sleep 8","task_type":"local_bash"}"#,
    );
    s.active = false;
    let done = r#"{"type":"system","subtype":"task_notification","task_id":"b1","tool_use_id":"toolu_1","status":"completed","output_file":"/tmp/b1.out","summary":"exit 0"}"#;
    let row = &calls(map_line(&mut s, done))[0];
    assert_eq!((row.id.as_str(), row.status), ("toolu_1", ToolStatus::Done));
    assert!(
        matches!(&row.detail, ToolDetail::Command { output, .. } if output.ends_with("exit 0\n\nFull output: /tmp/b1.out"))
    );
    let woke = map_line(
        &mut s,
        r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"delta":{"type":"text_delta","text":"It finished."}}}"#,
    );
    assert!(
        matches!(&woke[..], [Update::Wake, Update::Notice(n), ..] if n.text.contains("sleep 8")),
        "{woke:?}"
    );
}

#[test]
fn wake_without_a_reason_still_says_so() {
    let mut s = State::default();
    let woke = map_line(
        &mut s,
        r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"delta":{"type":"text_delta","text":"hi"}}}"#,
    );
    assert!(
        matches!(&woke[..], [Update::Wake, Update::Notice(n), ..] if n.text == "Claude started a turn on its own" && n.id == "wake-1"),
        "{woke:?}"
    );
}

#[test]
fn stop_tool_names_the_task_of_a_row() {
    let mut s = State::default();
    s.tasks.insert("t1".into(), ("toolu_a".into(), "d".into()));
    assert_eq!(stop_request(&s, "toolu_a", 3).unwrap()["request"]["task_id"], "t1");
    assert_eq!(stop_request(&s, "toolu_a", 3).unwrap()["request_id"], "aether-stop-3");
    assert!(stop_request(&s, "nope", 4).is_none());
}

#[test]
fn subagent_and_background_fixtures_nest_and_settle() {
    let calls = last_calls(&replay("subagent"));
    let task =
        calls.iter().find(|c| matches!(c.detail, ToolDetail::Task { .. })).expect("task row");
    assert!(calls.iter().any(|c| c.parent.as_deref() == Some(task.id.as_str())));
    assert!(
        matches!(&task.detail, ToolDetail::Task { output, .. } if !output.contains("agentId:"))
    );
    assert!(
        last_calls(&replay("background"))
            .iter()
            .all(|c| !matches!(c.status, ToolStatus::Running | ToolStatus::Background))
    );
}

#[test]
fn can_use_tool_asks_and_deny_replies_with_request_id() {
    let mut s = State { active: true, ..State::default() };
    let line = r#"{"type":"control_request","request_id":"req-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"rm x"}}}"#;
    let out = map_line(&mut s, line);
    let [Update::Ask { id, ask: Ask::Approval(Approval { tool, input, .. }) }] = out.as_slice()
    else {
        panic!("expected one approval");
    };
    assert_eq!((tool.as_str(), input.as_str()), ("Bash", "rm x"));
    let reply = reply(&mut s, *id, &Answer::Deny).unwrap().to_string();
    assert!(
        reply.contains(r#""request_id":"req-1""#) && reply.contains(r#""behavior":"deny""#),
        "{reply}"
    );
}

// live 2.1.303 lines, trimmed to the fields the driver reads
const ASK_USE: &str = r#"{"type":"assistant","message":{"model":"claude-haiku-5-5","id":"msg_011CgAad5AzxAqAjqd4F6WEe","type":"message","role":"assistant","content":[{"type":"tool_use","id":"toolu_01Q5zvhA2uQwRFiRS8EqYnWi","name":"AskUserQuestion","input":{"questions":[{"question":"Which color do you prefer?","header":"Color","multiSelect":false,"options":[{"label":"Red","description":"Pick red"},{"label":"Blue","description":"Pick blue"}]}]},"caller":{"type":"direct"}}],"usage":{"input_tokens":2,"cache_creation_input_tokens":20273,"cache_read_input_tokens":15259,"output_tokens":157}},"parent_tool_use_id":null}"#;
const ASK_REQUEST: &str = r#"{"type":"control_request","request_id":"b915b282-7cd4-43ee-806d-2ba964c791b2","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","display_name":"AskUserQuestion","input":{"questions":[{"question":"Which color do you prefer?","header":"Color","options":[{"label":"Red","description":"Pick red"},{"label":"Blue","description":"Pick blue"}],"multiSelect":false},{"question":"Which size?","header":"Size","options":[{"label":"S"},{"label":"L"}],"multiSelect":false}]},"tool_use_id":"toolu_01Q5zvhA2uQwRFiRS8EqYnWi","requires_user_interaction":true}}"#;
const WRITE_REQUEST: &str = r#"{"type":"control_request","request_id":"984a7c5a-0adf-43be-bffc-1ee49a7ca0fd","request":{"subtype":"can_use_tool","tool_name":"Write","display_name":"Write","input":{"file_path":"/private/tmp/claude-probe/probe1.txt","content":"hi"},"description":"probe1.txt","permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"}],"tool_use_id":"toolu_01Ly3im2TnrYv8cB2Gd5FJBg"}}"#;

fn asks(updates: Vec<Update>) -> Vec<(u64, Ask)> {
    updates
        .into_iter()
        .filter_map(|u| match u {
            Update::Ask { id, ask } => Some((id, ask)),
            _ => None,
        })
        .collect()
}

#[test]
fn ask_user_question_tool_use_is_a_plain_row_with_live_context() {
    let mut s = State { active: true, ..State::default() };
    let out = map_line(&mut s, ASK_USE);
    assert_eq!(out[0], Update::Usage(2 + 20273 + 15259));
    assert!(asks(out.clone()).is_empty(), "{out:?}");
    assert_eq!(calls(out)[0].id, "toolu_01Q5zvhA2uQwRFiRS8EqYnWi");
    let sub = ASK_USE.replace(r#""parent_tool_use_id":null"#, r#""parent_tool_use_id":"toolu_9""#);
    let out = map_line(&mut s, &sub);
    assert!(asks(out.clone()).is_empty() && !out.contains(&Update::Wake), "{out:?}");
    assert_eq!(calls(out)[0].parent.as_deref(), Some("toolu_9"));
}

#[test]
fn questions_are_one_ask_and_multi_select_joins_labels() {
    let mut s = State::default();
    let [(id, Ask::Questions(qs))] = &asks(map_line(&mut s, ASK_REQUEST))[..] else { panic!() };
    assert_eq!(qs.len(), 2);
    assert_eq!(
        (qs[0].header.as_str(), qs[0].options[1].description.as_str()),
        ("Color", "Pick blue")
    );
    assert!(qs[0].other && !qs[0].multi);
    let rep = reply(
        &mut s,
        *id,
        &Answer::Answers(vec![vec!["Red".into(), "Blue".into()], vec!["S".into()]]),
    )
    .unwrap();
    assert_eq!(
        rep["response"]["response"]["updatedInput"]["answers"],
        json!({"Which color do you prefer?": "Red, Blue", "Which size?": "S"})
    );
    assert_eq!(rep["response"]["request_id"], "b915b282-7cd4-43ee-806d-2ba964c791b2");
}

#[test]
fn denying_the_questions_denies_the_request() {
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, ASK_REQUEST))[..] else { panic!() };
    let rep = reply(&mut s, id, &Answer::Deny).unwrap();
    assert_eq!(rep["response"]["response"], json!({"behavior": "deny", "message": DENIED}));
}

#[test]
fn approval_options_keep_set_mode_separate() {
    let opts = approval_options(&[
        json!({"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"pnpm add:*"}],"behavior":"allow","destination":"session"}),
        json!({"type":"setMode","mode":"acceptEdits","destination":"session"}),
        json!({"type":"removeRules","rules":[],"behavior":"allow","destination":"session"}),
        json!({"type":"addRules","rules":[{"toolName":"Read"}],"behavior":"allow","destination":"session"}),
        json!({"type":"addDirectories","directories":["/a","/b"],"destination":"session"}),
        json!({"type":"setMode","mode":"plan","destination":"session"}),
    ]);
    let got: Vec<(&str, &str)> = opts.iter().map(|o| (o.id.as_str(), o.label.as_str())).collect();
    assert_eq!(
        got,
        [
            ("s-0", "Allow `pnpm add *` for this chat"),
            ("s-1", "Allow and switch this chat to Accept edits"),
            ("s-3", "Allow Read for this chat"),
            ("s-4", "Allow edits in /a, /b for this chat"),
            ("s-5", "Allow and switch this chat to plan"),
        ]
    );
}

#[test]
fn write_approval_links_its_row_and_picks_one_suggestion() {
    let mut s = State::default();
    let [(id, Ask::Approval(a))] = &asks(map_line(&mut s, WRITE_REQUEST))[..] else { panic!() };
    assert_eq!(
        (a.kind, a.tool_id.as_deref(), a.input.as_str()),
        (
            ApprovalKind::Edit,
            Some("toolu_01Ly3im2TnrYv8cB2Gd5FJBg"),
            "/private/tmp/claude-probe/probe1.txt"
        )
    );
    assert!(a.diff.as_deref().is_some_and(|d| d.lines().any(|l| l == "+hi")));
    let rep = reply(&mut s, *id, &Answer::Pick("s-0".into())).unwrap();
    assert_eq!(
        rep["response"]["response"]["updatedPermissions"],
        json!([{"type":"setMode","mode":"acceptEdits","destination":"session"}])
    );
    assert_eq!(rep["response"]["response"]["updatedInput"]["content"], "hi");
}

#[test]
fn deny_with_note_passes_the_note_to_the_model() {
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, WRITE_REQUEST))[..] else { panic!() };
    let rep = reply(&mut s, id, &Answer::DenyWith("use /tmp".into())).unwrap();
    assert_eq!(rep["response"]["response"], json!({"behavior": "deny", "message": "use /tmp"}));
}

#[test]
fn steers_are_reported_when_claude_echoes_them() {
    let mut s = State { active: true, ..State::default() };
    on_prompt(&mut s, "count");
    on_steer(&mut s, "also say done");
    let echo = |t: &str| {
        json!({"type":"user","isReplay":true,"parent_tool_use_id":null,"message":{"role":"user","content":[{"type":"text","text":t}]}}).to_string()
    };
    assert_eq!(map_line(&mut s, &echo("count")), [Update::Accepted]);
    assert_eq!(
        map_line(&mut s, &echo("also say done")),
        [Update::Accepted, Update::Steered("also say done".into())]
    );
}

#[test]
fn elicitations_are_forms_and_dialogs_are_cancelled() {
    let mut s = State { active: true, ..State::default() };
    let el = r#"{"type":"control_request","request_id":"e1","request":{"subtype":"elicitation","mcp_server_name":"mcp-1","message":"Pick a project","requested_schema":{"type":"object","properties":{"p":{"type":"string","enum":["WEB","API"]},"n":{"type":"integer"},"ok":{"type":"boolean"}},"required":["p"]}}}"#;
    let [Update::Ask { id, ask: Ask::Form(f) }] = &map_line(&mut s, el)[..] else { panic!() };
    assert_eq!(
        (f.fields.iter().find(|x| x.key == "p").unwrap().kind, f.source.as_deref()),
        (FieldKind::Choice, Some("mcp-1"))
    );
    let form = vec![
        ("p".into(), vec!["API".into()]),
        ("n".into(), vec!["3".into()]),
        ("ok".into(), vec!["true".into()]),
    ];
    let rep = reply(&mut s, *id, &Answer::Form(form)).unwrap();
    assert_eq!(
        rep["response"]["response"],
        json!({"action":"accept","content":{"p":"API","n":3,"ok":true}})
    );
    let [Update::Ask { id, .. }] = &map_line(&mut s, el)[..] else { panic!() };
    let rep = reply(&mut s, *id, &Answer::Deny).unwrap();
    assert_eq!(rep["response"]["response"], json!({"action":"decline"}));
    let dialog = r#"{"type":"control_request","request_id":"d1","request":{"subtype":"request_user_dialog","dialog_kind":"refusal_fallback_prompt","payload":{}}}"#;
    assert!(matches!(&map_line(&mut s, dialog)[..], [Update::Warning(_)]));
    assert_eq!(s.outbox.last().unwrap()["response"]["response"], json!({"behavior":"cancelled"}));
}

#[test]
fn approval_and_question_fixtures_are_answerable() {
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
        if scenario == "subagent-approval" {
            assert!(a.source.is_some());
        }
    }
    assert!(
        replay("question")
            .iter()
            .any(|u| matches!(u, Update::Ask { ask: Ask::Questions(q), .. } if q.len() == 2))
    );
    assert!(replay("steer").iter().any(|u| matches!(u, Update::Steered(_))));
}

#[test]
fn question_shaped_tools_still_need_approval() {
    let line = r#"{"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"mcp__forms__ask_question","input":{"question":"Ship it?"}}}"#;
    let got = asks(map_line(&mut State::default(), line));
    assert!(matches!(got.as_slice(), [(_, Ask::Approval(_))]), "{got:?}");
}

#[test]
fn approval_card_shows_why_and_allows_with_original_input() {
    let mut s = State::default();
    let got = asks(map_line(&mut s, WRITE_REQUEST));
    let [(id, ask)] = got.as_slice() else { panic!("{got:?}") };
    let Ask::Approval(a) = ask else { panic!("{ask:?}") };
    let input = &a.input;
    assert_eq!((a.tool.as_str(), a.title.as_str()), ("Write", "Write probe1.txt"));
    assert_eq!(input, "/private/tmp/claude-probe/probe1.txt");
    let rep = reply(&mut s, *id, &Answer::Approve { input: "edited".into(), always: false });
    assert_eq!(
        rep.unwrap()["response"]["response"],
        json!({"behavior": "allow", "updatedInput": {"file_path": "/private/tmp/claude-probe/probe1.txt", "content": "hi"}})
    );

    let blocked = r#"{"type":"control_request","request_id":"b","request":{"subtype":"can_use_tool","tool_name":"Bash","display_name":"Bash","input":{"command":"cat ../x","description":"Show x"},"decision_reason":"Reads outside the project","blocked_path":"/private/tmp/x"}}"#;
    let got = asks(map_line(&mut s, blocked));
    let [(_, Ask::Approval(Approval { input, reason, .. }))] = got.as_slice() else {
        panic!("{got:?}")
    };
    assert_eq!(input, "cat ../x");
    assert_eq!(reason, "Reads outside the project (/private/tmp/x)");
}

#[test]
fn approve_sends_edited_bash_command_and_picks_session_rules() {
    let line = r#"{"type":"control_request","request_id":"req-b","request":{"subtype":"can_use_tool","tool_name":"Bash","display_name":"Bash","input":{"command":"rm -rf build","description":"Clean"},"decision_reason":"Deletes files","permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"rm -rf build"}],"behavior":"allow","destination":"localSettings"}]}}"#;
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, line))[..] else { panic!() };
    let edited = Answer::Approve { input: "rm -rf dist".into(), always: true };
    let rep = reply(&mut s, id, &edited).unwrap();
    assert_eq!(
        rep["response"]["response"],
        json!({"behavior": "allow", "updatedInput": {"command": "rm -rf dist", "description": "Clean"}})
    );
    let [(id, _)] = asks(map_line(&mut s, line))[..] else { panic!() };
    let rep = reply(&mut s, id, &Answer::Pick("s-0".into())).unwrap();
    assert_eq!(
        rep["response"]["response"]["updatedPermissions"],
        json!([{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "rm -rf build"}], "behavior": "allow", "destination": "session"}])
    );
    let [(id, _)] = asks(map_line(&mut s, line))[..] else { panic!() };
    let same = Answer::Approve { input: "rm -rf build".into(), always: false };
    let rep = reply(&mut s, id, &same).unwrap();
    assert_eq!(
        rep["response"]["response"],
        json!({"behavior": "allow", "updatedInput": {"command": "rm -rf build", "description": "Clean"}})
    );
}

#[test]
fn bash_approval_shows_every_line_and_edits_keep_them() {
    let cmd = "cat <<'EOF' > a.txt\nline one\nline two\nEOF";
    let line = json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command": cmd}}}).to_string();
    let mut s = State::default();
    let [(id, Ask::Approval(Approval { input, .. }))] = &asks(map_line(&mut s, &line))[..] else {
        panic!()
    };
    assert_eq!(input, cmd);
    let edited = "cat <<'EOF' > b.txt\nline one\nEOF";
    let rep = reply(&mut s, *id, &Answer::Approve { input: edited.into(), always: false }).unwrap();
    assert_eq!(rep["response"]["response"]["updatedInput"]["command"], edited);
}

#[test]
fn empty_bash_edit_keeps_the_original_command() {
    let line = r#"{"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, line))[..] else { panic!() };
    let rep = reply(&mut s, id, &Answer::Approve { input: "  ".into(), always: false }).unwrap();
    assert_eq!(rep["response"]["response"]["updatedInput"]["command"], "ls");
}

#[test]
fn write_approval_shows_path_and_diff_and_always_skips_set_mode() {
    let mut s = State::default();
    let [(id, Ask::Approval(Approval { input, diff, .. }))] =
        &asks(map_line(&mut s, WRITE_REQUEST))[..]
    else {
        panic!()
    };
    assert_eq!(input, "/private/tmp/claude-probe/probe1.txt");
    assert!(diff.as_deref().is_some_and(|d| d.lines().any(|l| l == "+hi")), "{diff:?}");
    let rep = reply(&mut s, *id, &Answer::Approve { input: input.clone(), always: true }).unwrap();
    assert!(rep["response"]["response"].get("updatedPermissions").is_none(), "{rep}");
}

#[test]
fn long_bash_command_is_shown_in_full_and_sent_unedited() {
    let cmd = format!("echo {}", "x".repeat(9000));
    let line = json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command": cmd}}}).to_string();
    let mut s = State::default();
    let [(id, Ask::Approval(Approval { input, .. }))] = &asks(map_line(&mut s, &line))[..] else {
        panic!()
    };
    assert_eq!(input, &cmd);
    let rep = reply(&mut s, *id, &Answer::Approve { input: input.clone(), always: false }).unwrap();
    assert_eq!(rep["response"]["response"]["updatedInput"]["command"], cmd.as_str());
}

#[test]
fn edit_approval_shows_every_old_and_new_line() {
    let line = json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Edit","input":{"file_path":"/a.txt","old_string":"one\ntwo","new_string":"uno\ndos\ntres"}}}).to_string();
    let mut s = State::default();
    let [(_, Ask::Approval(Approval { input, diff, .. }))] = &asks(map_line(&mut s, &line))[..]
    else {
        panic!()
    };
    assert_eq!(input, "/a.txt");
    assert_eq!(diff.as_deref(), Some("-one\n-two\n+uno\n+dos\n+tres\n"));
}

#[test]
fn other_tool_approval_shows_pretty_json_of_every_key() {
    let line = r#"{"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"WebFetch","input":{"url":"https://x.dev","prompt":"summarize"}}}"#;
    let mut s = State::default();
    let [(_, Ask::Approval(Approval { input, .. }))] = &asks(map_line(&mut s, line))[..] else {
        panic!()
    };
    assert!(
        input.contains("\"url\": \"https://x.dev\"") && input.contains("\"prompt\": \"summarize\""),
        "{input}"
    );
}

#[test]
fn exit_plan_mode_asks_for_review() {
    let line = r##"{"type":"control_request","request_id":"p","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"# Plan\n\n1. Read notes.txt\n2. Fix the typo\n"}}}"##;
    let mut s = State::default();
    let got = asks(map_line(&mut s, line));
    assert_eq!(
        got[0].1,
        Ask::Plan(Plan {
            markdown: "# Plan\n\n1. Read notes.txt\n2. Fix the typo\n".into(),
            ..Plan::default()
        })
    );
    let rep = reply(&mut s, got[0].0, &Answer::Plan { approved: true, text: "x".into() }).unwrap();
    assert_eq!(rep["response"]["response"]["behavior"], "allow");
    for (text, message) in [("Add tests first", "Add tests first"), ("", DENIED)] {
        let [(id, _)] = asks(map_line(&mut s, line))[..] else { panic!() };
        let rep = reply(&mut s, id, &Answer::Plan { approved: false, text: text.into() }).unwrap();
        assert_eq!(rep["response"]["response"], json!({"behavior": "deny", "message": message}));
    }
}

#[test]
fn cancelled_requests_resolve_their_asks() {
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, WRITE_REQUEST))[..] else { panic!() };
    let cancel =
        r#"{"type":"control_cancel_request","request_id":"984a7c5a-0adf-43be-bffc-1ee49a7ca0fd"}"#;
    let resolved = Update::AskResolved(id);
    assert_eq!(
        map_line(&mut s, cancel),
        [resolved, Update::Final(String::new())],
        "its side turn ends"
    );
    assert!(s.asks.is_empty() && !s.active);
}

#[test]
fn unknown_control_request_gets_an_error_reply_and_a_warning() {
    let mut s = State::default();
    let line = r#"{"type":"control_request","request_id":"e1","request":{"subtype":"mystery"}}"#;
    let out = map_line(&mut s, line);
    assert!(matches!(&out[..], [Update::Warning(w)] if w.contains("mystery")), "{out:?}");
    assert_eq!(
        s.outbox,
        [
            json!({"type": "control_response", "response": {"subtype": "error", "request_id": "e1", "error": "unsupported in aether"}})
        ]
    );
}

#[test]
fn title_response_becomes_title_and_reply_echoes_are_ignored() {
    let mut s = State::default();
    let title = r#"{"type":"control_response","response":{"subtype":"success","request_id":"title","response":{"title":"Echo hi command"}}}"#;
    assert_eq!(map_line(&mut s, title), [Update::Title("Echo hi command".into())]);
    let echo = r#"{"type":"control_response","response":{"subtype":"success","request_id":"61dee81e-f2cd-49c5-a619-b6ff07c7e0e7","response":{"behavior":"allow"}}}"#;
    assert_eq!(map_line(&mut s, echo), []);
}

#[test]
fn init_tracks_model_and_reports_a_new_session() {
    let mut s =
        State { session: "1120d783-66df-4e34-8178-3a3495a37644".into(), ..State::default() };
    let init = r#"{"type":"system","subtype":"init","cwd":"/private/tmp/claude-probe","session_id":"1120d783-66df-4e34-8178-3a3495a37644","model":"claude-opus-5-5[1m]","permissionMode":"default"}"#;
    assert_eq!(
        map_line(&mut s, init),
        [Update::Model("claude-opus-5-5[1m]".into()), Update::Mode(AccessMode::Ask)]
    );
    assert_eq!(s.model, "claude-opus-5-5[1m]");
    let cleared = init.replace("1120d783-66df-4e34-8178-3a3495a37644", "new-id");
    assert_eq!(map_line(&mut s, &cleared)[0], Update::Session("new-id".into()));
}

const HAIKU_RESULT: &str = r#"{"type":"result","subtype":"success","is_error":false,"num_turns":2,"result":"WARDEN - hi","usage":{"input_tokens":4,"cache_creation_input_tokens":27437,"cache_read_input_tokens":48200,"output_tokens":352,"iterations":[{"input_tokens":2,"cache_creation_input_tokens":4623,"cache_read_input_tokens":35507,"output_tokens":278}]},"modelUsage":{"claude-haiku-5-5":{"inputTokens":4,"outputTokens":352,"cacheReadInputTokens":48200,"cacheCreationInputTokens":27437,"costUSD":0.0061458,"contextWindow":1000000,"maxOutputTokens":128000,"canonicalModel":"claude-haiku-5-5"}}}"#;

#[test]
fn result_reports_last_call_context_window_and_turn_counts() {
    let mut s = State { active: true, model: "claude-haiku-5-5".into(), ..State::default() };
    assert_eq!(
        map_line(&mut s, HAIKU_RESULT),
        [
            Update::Usage(2 + 4623 + 35507),
            Update::Window(1_000_000),
            Update::Count { prompt: 4 + 27437 + 48200, completion: 352, cached: 48200 },
            Update::Final("WARDEN - hi".into()),
        ]
    );
}

#[test]
fn window_prefers_the_init_model_then_the_largest() {
    let models = json!({
        "claude-opus-5-5[1m]": {"contextWindow": 1_000_000},
        "claude-haiku-5-5": {"contextWindow": 200_000},
    });
    assert_eq!(window(&models, "claude-haiku-5-5"), Some(200_000));
    assert_eq!(window(&models, "claude-opus-5-5[1m]"), Some(1_000_000));
    assert_eq!(window(&models, ""), Some(1_000_000));
    assert_eq!(window(&json!({}), "claude-haiku-5-5"), None);
}

#[test]
fn zero_usage_results_leave_the_meter_alone() {
    let bogus = r#"{"type":"result","subtype":"success","is_error":true,"num_turns":1,"result":"There's an issue with the selected model (bogus-model-xyz). It may not exist or you may not have access to it. Run --model to pick a different model.","usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0,"iterations":[]},"modelUsage":{},"terminal_reason":"api_error","api_error_status":404}"#;
    let out = map_line(&mut State { active: true, ..State::default() }, bogus);
    assert!(
        matches!(out.as_slice(), [Update::Error(e), Update::Final(f)] if e.contains("bogus-model-xyz") && f.is_empty()),
        "{out:?}"
    );

    // slash command: synthetic message, no deltas, no model call
    let mut s = State { active: true, model: "claude-haiku-5-5".into(), ..State::default() };
    s.usage_seen = [2 + 15259 + 20234, 10, 15259];
    let synthetic = r#"{"type":"assistant","message":{"id":"40c93a73-2bac-4dbe-b3f2-5f70d4387209","model":"<synthetic>","role":"assistant","stop_reason":"end_turn","type":"message","usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"iterations":null},"content":[{"type":"text","text":"/bug isn't available in this environment."}]},"parent_tool_use_id":null}"#;
    assert_eq!(map_line(&mut s, synthetic), []);
    let result = r#"{"type":"result","subtype":"success","is_error":false,"num_turns":0,"result":"/bug isn't available in this environment.","usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0,"iterations":[]},"modelUsage":{"claude-haiku-5-5":{"inputTokens":2,"outputTokens":10,"cacheReadInputTokens":15259,"cacheCreationInputTokens":20234,"contextWindow":1000000}}}"#;
    assert_eq!(
        map_line(&mut s, result),
        [
            Update::Window(1_000_000),
            Update::Final("/bug isn't available in this environment.".into())
        ]
    );
}

#[test]
fn compact_boundary_sets_context_to_post_tokens() {
    let line = r#"{"type":"system","subtype":"compact_boundary","session_id":"3990319a-5941-42f1-a565-7ee29d8a38b0","compact_metadata":{"trigger":"manual","pre_tokens":35593,"post_tokens":6874,"cumulative_dropped_tokens":28719,"duration_ms":4902}}"#;
    let out = map_line(&mut State::default(), line);
    let [Update::Notice(n), Update::Usage(6874)] = &out[..] else { panic!("{out:?}") };
    assert_eq!((n.kind, n.state), (NoticeKind::Compaction, NoticeState::Done));
    assert_eq!(n.text, "Compacted 35593 → 6874 tokens");
}

#[test]
fn steer_missed_by_the_last_call_keeps_the_turn_open_for_its_own_result() {
    // prompt and steer written, prompt echoed; from a live probe
    let mut s = State {
        active: true,
        echoes: VecDeque::from([
            None,
            Some("Also, after the numbers, write the word BANANA on its own line.".into()),
        ]),
        ..State::default()
    };
    let echo = |text: &str| {
        json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}, "session_id": "cb257c98-3278-4612-a7d9-b321bba01641", "parent_tool_use_id": null, "isReplay": true}).to_string()
    };
    assert_eq!(
        map_line(&mut s, &echo("Count from 1 to 40, one number per line, no other text.")),
        [Update::Accepted]
    );
    let first = r#"{"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"1\n2\n40","usage":{"input_tokens":2,"cache_creation_input_tokens":20243,"cache_read_input_tokens":15259,"output_tokens":88,"iterations":[{"input_tokens":2,"output_tokens":88,"cache_read_input_tokens":15259,"cache_creation_input_tokens":20243}]},"modelUsage":{"claude-haiku-5-5":{"inputTokens":2,"outputTokens":88,"cacheReadInputTokens":15259,"cacheCreationInputTokens":20243,"contextWindow":1000000}},"result_index":0}"#;
    let out = map_line(&mut s, first);
    assert!(out.contains(&Update::Count { prompt: 35504, completion: 88, cached: 15259 }));
    assert!(!out.iter().any(|u| matches!(u, Update::Final(_))), "{out:?}");
    assert!(s.active);
    assert_eq!(
        map_line(&mut s, &echo("Also, after the numbers, write the word BANANA on its own line.")),
        [
            Update::Accepted,
            Update::Steered(
                "Also, after the numbers, write the word BANANA on its own line.".into()
            )
        ]
    );
    let second = first.replace(r#""result":"1\n2\n40""#, r#""result":"1\n2\n40\nBANANA""#);
    let out = map_line(&mut s, &second);
    assert_eq!(out.last(), Some(&Update::Final("1\n2\n40\nBANANA".into())));
    assert!(!s.active);
}

#[cfg(unix)]
#[test]
fn exit_error_carries_status_and_stderr_tail() {
    use std::os::unix::process::ExitStatusExt;
    let mut tail = StderrTail::default();
    tail.push(
        r#"[claude-code:unrecognized_model] {"model":"bogus-model-xyz","query_source":"sdk"}"#,
    );
    assert_eq!(
        exit_error(Some(ExitStatus::from_raw(256)), &tail),
        "claude exited (exit status: 1)\n[claude-code:unrecognized_model] {\"model\":\"bogus-model-xyz\",\"query_source\":\"sdk\"}"
    );
    assert_eq!(exit_error(None, &StderrTail::default()), "claude exited");
}

#[test]
fn result_ends_with_final() {
    let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"done","usage":{"input_tokens":3,"output_tokens":2},"modelUsage":{"m":{"inputTokens":3,"outputTokens":2}}}"#;
    let out = map_line(&mut State { active: true, ..State::default() }, line);
    assert!(out.contains(&Update::Count { prompt: 3, completion: 2, cached: 0 }));
    assert_eq!(out.last(), Some(&Update::Final("done".into())));
}

#[test]
fn result_without_active_turn_is_dropped() {
    let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"woke","usage":{"input_tokens":3,"output_tokens":2}}"#;
    assert_eq!(map_line(&mut State::default(), line), []);
}

#[test]
fn cancelled_error_result_only_settles() {
    let mut s = State { active: true, cancelled: Some(0), ..State::default() };
    let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"usage":{"input_tokens":0,"output_tokens":0}}"#;
    assert_eq!(map_line(&mut s, line), [Update::Final(String::new())]);
}

#[test]
fn full_access_args_start_fresh_session() {
    let opts = DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: "/tmp".into(),
        model: "opus".into(),
        effort: "high".into(),
        access: AccessMode::Full,
        resume: "no-such-session-aether-test".into(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let (args, id, resumed, _) = launch_args(&opts);
    let has = |pair: [&str; 2]| args.windows(2).any(|w| w == pair);
    assert!(has(["--permission-mode", "bypassPermissions"]));
    assert!(args.iter().any(|a| a == "--dangerously-skip-permissions"));
    assert!(has(["--thinking-display", "summarized"]));
    assert!(has(["--effort", "high"]) && has(["--model", "opus"]));
    assert!(has(["--session-id", id.as_str()]) && !args.iter().any(|a| a == "--resume"));
    assert!(!resumed);
    assert_eq!((id.len(), &id[14..15], id.split('-').count()), (36, "4", 5));
    assert!("89ab".contains(&id[19..20]));
}

#[test]
fn launch_args_let_claude_read_attachments() {
    let opts = DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: "/tmp".into(),
        model: "opus".into(),
        effort: "high".into(),
        access: AccessMode::Full,
        resume: "no-such-session-aether-test".into(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let (args, ..) = launch_args(&opts);
    assert!(args.windows(2).any(|w| w[0] == "--add-dir" && w[1].ends_with("attachments")));
}

#[test]
fn user_message_puts_images_after_the_text() {
    let dir = std::env::temp_dir().join(format!("aether-claude-prompt-img-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.png");
    std::fs::write(&path, [1u8, 2, 3]).unwrap();
    let img = Attachment { name: "a.png".into(), path, mime: "image/png".into(), size: 3 };
    let gone = Attachment { path: dir.join("gone.png"), ..img.clone() };
    let v = user_message("look", &[img.clone(), gone]);
    let content = &v["message"]["content"];
    assert_eq!(content[0], json!({"type": "text", "text": "look"}));
    let image =
        json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"AQID"}});
    assert_eq!(content[1], image);
    assert_eq!(content.as_array().unwrap().len(), 2);
    assert_eq!(user_message("", &[img])["message"]["content"], json!([image]));
    assert_eq!(user_message("", &[])["message"]["content"], json!([{"type": "text", "text": ""}]));
}

#[test]
fn auto_access_uses_auto_permission_mode_without_bypass() {
    let opts = DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: "/tmp".into(),
        model: String::new(),
        effort: String::new(),
        access: AccessMode::Auto,
        resume: String::new(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let (args, ..) = launch_args(&opts);
    assert!(args.windows(2).any(|w| w == ["--permission-mode", "auto"]));
    assert!(!args.iter().any(|a| a == "--dangerously-skip-permissions"));
}

fn turn(
    d: &mut ClaudeDriver,
    rx: &mut futures::channel::mpsc::UnboundedReceiver<Update>,
    cancel: bool,
) -> Vec<Update> {
    let start = std::time::Instant::now();
    let mut deadline = start + Duration::from_secs(120);
    let mut out = Vec::new();
    loop {
        assert!(std::time::Instant::now() < deadline, "timed out: {out:?}");
        let u = match rx.try_recv() {
            Ok(u) => u,
            Err(futures::channel::mpsc::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(_) => panic!("channel closed: {out:?}"),
        };
        println!("{:>6}ms {u:?}", start.elapsed().as_millis());
        match &u {
            Update::Ask { id, .. } => d.answer(*id, Answer::Deny),
            Update::Delta(_) if cancel && !out.iter().any(|u| matches!(u, Update::Delta(_))) => {
                println!("-- cancel");
                d.cancel();
                deadline = std::time::Instant::now() + Duration::from_secs(15);
            }
            _ => {}
        }
        let done = matches!(u, Update::Final(_));
        out.push(u);
        if done {
            return out;
        }
    }
}

fn text(updates: &[Update]) -> String {
    updates
        .iter()
        .filter_map(|u| match u {
            Update::Delta(t) | Update::Final(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<String>()
        .to_lowercase()
}

#[test]
#[ignore]
fn live_claude_smoke() {
    let dir = std::env::temp_dir().join(format!("aether-claude-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let opts = |resume: &str| DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: dir.clone(),
        model: String::new(),
        effort: String::new(),
        access: AccessMode::Ask,
        resume: resume.into(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut d = ClaudeDriver::spawn(opts(""), tx);
    println!("-- turn 1");
    d.prompt("Reply with exactly: ok".into(), vec![]);
    let first = turn(&mut d, &mut rx, false);
    let id = first
        .iter()
        .find_map(|u| match u {
            Update::Session(id) => Some(id.clone()),
            _ => None,
        })
        .expect("session id");
    assert!(text(&first).contains("ok"));
    drop(d);

    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut d = ClaudeDriver::spawn(opts(&id), tx);
    assert!(d.resumed());
    println!("-- turn 2 (resumed {id})");
    d.prompt("Reply with exactly: again".into(), vec![]);
    assert!(text(&turn(&mut d, &mut rx, false)).contains("again"));
    println!("-- turn 3 (cancel)");
    d.prompt("Count from 1 to 300, one number per line.".into(), vec![]);
    turn(&mut d, &mut rx, true);
    println!("-- turn 4");
    d.prompt("Reply with exactly: ok".into(), vec![]);
    assert!(text(&turn(&mut d, &mut rx, false)).contains("ok"));
    drop(d);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn model_output_while_idle_wakes_a_turn_that_its_result_settles() {
    let mut s = State { turn: 3, ..State::default() };
    let start =
        r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_start"}}"#;
    let out = map_line(&mut s, start);
    assert!(
        matches!(out.as_slice(), [Update::Wake, Update::Notice(n)] if n.kind == NoticeKind::Wake),
        "{out:?}"
    );
    assert!(s.active && s.turn == 4);
    let delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"done"}}}"#;
    assert_eq!(map_line(&mut s, delta), [Update::Delta("done".into())]);
    let result = r#"{"type":"result","is_error":false,"result":"done","usage":{"iterations":[]}}"#;
    assert_eq!(map_line(&mut s, result), [Update::Final("done".into())]);
    assert!(!s.active);
}

#[test]
fn a_permission_ask_while_idle_wakes_but_subagent_output_and_dead_sessions_dont() {
    let mut s = State::default();
    let ask = r#"{"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
    let out = map_line(&mut s, ask);
    assert!(
        matches!(out.as_slice(), [Update::Wake, Update::Notice(_), Update::Ask { .. }]),
        "{out:?}"
    );

    let mut s = State::default();
    let sub = BASH_USE.replace(r#""parent_tool_use_id":null"#, r#""parent_tool_use_id":"toolu_9""#);
    assert!(!map_line(&mut s, &sub).contains(&Update::Wake));
    assert!(!s.active);

    let mut s = State { dead: Some("gone".into()), ..State::default() };
    assert!(!map_line(&mut s, BASH_USE).contains(&Update::Wake));
}

#[test]
#[ignore]
fn live_wake_smoke() {
    let dir = std::env::temp_dir().join("claude-wake-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let opts = DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: dir,
        model: "haiku".into(),
        effort: String::new(),
        access: AccessMode::Full,
        resume: String::new(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let mut d = ClaudeDriver::spawn(opts, tx);
    d.prompt("Use the Bash tool with run_in_background set to true to run `sleep 8; echo finished`. Then reply with only the word started. When you are told the background command finished, reply with only the word finished.".into(), vec![]);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(90) {
        match rx.try_recv() {
            Ok(u) => println!("{:>6}ms {u:?}", start.elapsed().as_millis()),
            Err(futures::channel::mpsc::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(_) => break,
        }
    }
}

#[test]
fn a_failed_model_switch_is_reported_and_its_confirmation_is_not_a_prompt_echo() {
    let mut s = State { active: true, echoes: VecDeque::from([None]), ..State::default() };
    let failed = r#"{"type":"control_response","response":{"subtype":"error","request_id":"aether-model-1","error":"Model 'bogus' not found"}}"#;
    assert_eq!(map_line(&mut s, failed), [Update::Warning("Model 'bogus' not found".into())]);
    let confirmed = r#"{"type":"user","isReplay":true,"parent_tool_use_id":null,"message":{"role":"user","content":"<local-command-stdout>Set model to haiku</local-command-stdout>"}}"#;
    assert_eq!(map_line(&mut s, confirmed), []);
    assert_eq!(s.echoes.len(), 1);
}

#[test]
fn approval_notes_flags_and_offers_each_rule_as_an_option() {
    let line = json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls","dangerouslyDisableSandbox":true},"permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"ls"},{"toolName":"Read"}],"behavior":"allow","destination":"localSettings"}]}}).to_string();
    let mut s = State::default();
    let [(id, Ask::Approval(Approval { input, reason, options: opts, .. }))] =
        &asks(map_line(&mut s, &line))[..]
    else {
        panic!()
    };
    assert_eq!(input, "ls\n(runs outside the sandbox)");
    assert_eq!(reason, "");
    let labels: Vec<&str> = opts.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["Allow `ls`, Read for this chat"]);
    let same = Answer::Approve { input: input.clone(), always: false };
    let rep = reply(&mut s, *id, &same).unwrap();
    assert_eq!(rep["response"]["response"]["updatedInput"]["command"], "ls");
}

#[test]
fn edit_approval_notes_replace_all() {
    let line = json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Edit","input":{"file_path":"/a","old_string":"a","new_string":"b","replace_all":true}}}).to_string();
    let mut s = State::default();
    let [(_, Ask::Approval(Approval { input, diff, .. }))] = &asks(map_line(&mut s, &line))[..]
    else {
        panic!()
    };
    assert_eq!(input, "/a\n(replaces every occurrence)");
    assert_eq!(diff.as_deref(), Some("-a\n+b\n"));
}

#[test]
fn every_fixture_replays_without_panicking() {
    let all = fixtures("claude");
    assert!(!all.is_empty());
    for (name, _) in all {
        let out = replay(&name);
        assert!(!out.is_empty(), "{name} produced no updates");
    }
}

fn ask_opts() -> DriverStartOptions {
    DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: "/tmp".into(),
        model: String::new(),
        effort: String::new(),
        access: AccessMode::Ask,
        resume: String::new(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    }
}

#[test]
fn launch_uses_public_flags_and_hook_events() {
    let (args, ..) = launch_args(&ask_opts());
    let pair = |a: &str, b: &str| args.windows(2).any(|w| w[0] == a && w[1] == b);
    assert!(pair("--permission-prompts", "host") && pair("--permission-mode", "manual"));
    assert!(args.iter().any(|a| a == "--include-hook-events"));
    assert!(!args.iter().any(|a| a == "--permission-prompt-tool"));
}

#[test]
fn initialize_asks_for_nested_calls_and_progress() {
    let r = &initialize_request()["request"];
    assert_eq!(
        (
            r["subtype"].as_str(),
            r["sdkForwardNestedSubagentToolCalls"].as_bool(),
            r["agentProgressSummaries"].as_bool()
        ),
        (Some("initialize"), Some(true), Some(true))
    );
}

#[test]
fn init_reports_model_mode_and_mcp_trouble() {
    let mut s = State { active: true, requested: AccessMode::Auto, ..State::default() };
    let init = r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-sonnet-5-5","permissionMode":"default","claude_code_version":"2.1.304","mcp_servers":[{"name":"mcp-1","status":"failed"},{"name":"mcp-2","status":"needs-auth"},{"name":"mcp-3","status":"connected"}]}"#;
    let out = map_line(&mut s, init);
    assert!(
        out.contains(&Update::Model("claude-sonnet-5-5".into()))
            && out.contains(&Update::Mode(AccessMode::Ask))
    );
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::ModeChange)));
    assert!(out.iter().any(|u| matches!(u, Update::Warning(w) if w.contains("mcp-1"))));
    assert!(out.iter().any(
        |u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::McpAuth && n.id == "mcp-mcp-2")
    ));
}

#[test]
fn status_mode_change_updates_mode() {
    let mut s = State { requested: AccessMode::Ask, ..State::default() };
    let status =
        r#"{"type":"system","subtype":"status","status":null,"permissionMode":"acceptEdits"}"#;
    let out = map_line(&mut s, status);
    assert!(out.contains(&Update::Mode(AccessMode::AutoEdit)));
}

#[test]
fn modes_map_and_unknown_is_none() {
    let m = |x| claude_mode(x);
    assert_eq!(
        [m("default"), m("manual"), m("acceptEdits"), m("auto"), m("bypassPermissions")],
        [
            Some(AccessMode::Ask),
            Some(AccessMode::Ask),
            Some(AccessMode::AutoEdit),
            Some(AccessMode::Auto),
            Some(AccessMode::Full)
        ]
    );
    assert_eq!(
        (m("plan"), m("dontAsk"), m("nonsense")),
        (Some(AccessMode::ReadOnly), Some(AccessMode::ReadOnly), None)
    );
}

#[test]
fn lost_resume_notice_only_when_a_resume_was_requested_but_not_possible() {
    let notice = lost_resume("no-such-session-aether-test", false).unwrap();
    assert_eq!(notice.id, "info-resume");
    assert_eq!(notice.kind, NoticeKind::Info);
    assert_eq!(notice.text, "Couldn't find the earlier Claude session, so this is a new one.");
    assert!(lost_resume("", false).is_none());
    assert!(lost_resume("abc", true).is_none());
    let opts = DriverStartOptions { resume: "no-such-session-aether-test".into(), ..ask_opts() };
    let (_, _, resumed, note) = launch_args(&opts);
    assert!(!resumed);
    assert_eq!(note.unwrap().text, notice.text);
    let fresh = DriverStartOptions { resume: String::new(), ..ask_opts() };
    assert!(launch_args(&fresh).3.is_none());
}

#[test]
fn resume_failures_pick_fresh_or_fork() {
    let mut tail = StderrTail::default();
    tail.push("No conversation found with session ID: 1234");
    assert!(matches!(resume_fix(&tail), Some(ResumeFix::Fresh(_))));
    let mut tail = StderrTail::default();
    tail.push("Session 1234 is running in the background. Add --fork-session to continue it here.");
    assert!(matches!(resume_fix(&tail), Some(ResumeFix::Fork)));
    assert!(resume_fix(&StderrTail::default()).is_none());
}

#[test]
fn fatal_fixture_fails_and_resume_fixture_does_not() {
    assert!(replay("fatal").iter().any(|u| matches!(u, Update::Error(_))));
    assert!(!replay("resume").iter().any(|u| matches!(u, Update::Error(_))));
}

#[test]
fn read_only_launches_plan_and_plan_init_is_no_mismatch() {
    let (args, ..) =
        launch_args(&DriverStartOptions { access: AccessMode::ReadOnly, ..ask_opts() });
    assert!(args.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "plan"));
    let mut s = State { requested: AccessMode::ReadOnly, ..State::default() };
    let init = r#"{"type":"system","subtype":"init","session_id":"s1","model":"m","permissionMode":"plan"}"#;
    let out = map_line(&mut s, init);
    assert!(out.contains(&Update::Mode(AccessMode::ReadOnly)));
    assert!(!out.iter().any(|u| matches!(u, Update::Notice(_))));
}

#[test]
fn retries_rate_limits_and_compaction_are_notices() {
    let mut s = State { active: true, ..State::default() };
    let retry = r#"{"type":"system","subtype":"api_retry","attempt":2,"max_retries":10,"retry_delay_ms":4000,"error_status":529,"error":"overloaded"}"#;
    assert!(
        matches!(&map_line(&mut s, retry)[..], [Update::Notice(n)] if n.kind == NoticeKind::Retry && n.until_ms.is_some() && n.text == "Retrying (2/10): 529 overloaded")
    );
    let next = r#"{"type":"stream_event","event":{"delta":{"type":"text_delta","text":"hi"}}}"#;
    assert!(
        matches!(&map_line(&mut s, next)[..], [Update::Notice(n), Update::Delta(_)] if n.id == "retry-0" && n.state == NoticeState::Done)
    );
    let warn = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","resetsAt":1800000000,"rateLimitType":"five_hour","utilization":0.9}}"#;
    assert!(
        matches!(&map_line(&mut s, warn)[..], [Update::Notice(n)] if n.kind == NoticeKind::RateLimit && n.text.contains("90") && n.until_ms == Some(1_800_000_000_000))
    );
    let rejected = warn.replace("allowed_warning", "rejected");
    assert!(
        matches!(&map_line(&mut s, &rejected)[..], [Update::Notice(n)] if n.state == NoticeState::Failed)
    );
    let compacting = r#"{"type":"system","subtype":"status","status":"compacting"}"#;
    let boundary = r#"{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"manual","pre_tokens":90000,"post_tokens":12000}}"#;
    let ids: Vec<String> = [compacting, boundary, compacting, boundary]
        .iter()
        .flat_map(|l| map_line(&mut s, l))
        .filter_map(|u| match u {
            Update::Notice(n) => Some(n.id),
            _ => None,
        })
        .collect();
    assert_eq!((ids.len(), ids[0] == ids[1], ids[1] == ids[2]), (4, true, false));
}

#[test]
fn an_allowed_rate_limit_stays_quiet_unless_it_cleared_a_warning() {
    let mut s = State { active: true, ..State::default() };
    let ok = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1800000000,"rateLimitType":"five_hour"}}"#;
    assert_eq!(map_line(&mut s, ok), []);
    let warn = ok.replace("\"allowed\"", "\"allowed_warning\",\"utilization\":0.8");
    map_line(&mut s, &warn);
    assert!(
        matches!(&map_line(&mut s, ok)[..], [Update::Notice(n)] if n.state == NoticeState::Done)
    );
    assert_eq!(map_line(&mut s, ok), []);
}

#[test]
fn failed_compaction_fails_the_open_notice() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, r#"{"type":"system","subtype":"status","status":"compacting"}"#);
    let failed = r#"{"type":"system","subtype":"status","status":null,"compact_result":"failed","compact_error":"too big"}"#;
    assert!(
        matches!(&map_line(&mut s, failed)[..], [Update::Notice(n)] if n.id == "compact-1" && n.state == NoticeState::Failed && n.text.contains("too big"))
    );
}

#[test]
fn a_repeated_compacting_status_stays_one_notice() {
    let mut s = State { active: true, ..State::default() };
    let compacting = r#"{"type":"system","subtype":"status","status":"compacting"}"#;
    let boundary = r#"{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"auto","pre_tokens":90000,"post_tokens":12000}}"#;
    let ids: Vec<String> = [compacting, compacting, compacting, boundary]
        .iter()
        .flat_map(|l| map_line(&mut s, l))
        .filter_map(|u| match u {
            Update::Notice(n) => Some(n.id),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["compact-1"; 4]);
}

#[test]
fn hooks_denials_and_model_fallbacks_show() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, BASH_USE);
    let denied = r#"{"type":"system","subtype":"permission_denied","tool_name":"Bash","tool_use_id":"toolu_1","decision_reason":"blocked by policy","message":"no"}"#;
    assert!(
        matches!(&calls(map_line(&mut s, denied))[..], [c] if c.status == ToolStatus::Denied && c.error.as_deref() == Some("blocked by policy"))
    );
    let hook = r#"{"type":"system","subtype":"hook_started","hook_id":"h1","hook_name":"lint","hook_event":"PreToolUse"}"#;
    assert!(
        matches!(&map_line(&mut s, hook)[..], [Update::Notice(n)] if n.id == "hook-h1" && n.state == NoticeState::Running && n.text == "PreToolUse hook: lint")
    );
    let progress = r#"{"type":"system","subtype":"hook_progress","hook_id":"h1"}"#;
    assert_eq!(map_line(&mut s, progress), []);
    let failed = r#"{"type":"system","subtype":"hook_response","hook_id":"h1","hook_name":"lint","hook_event":"PreToolUse","outcome":"error","exit_code":2,"stderr":"bad"}"#;
    assert!(
        matches!(&map_line(&mut s, failed)[..], [Update::Notice(n)] if n.id == "hook-h1" && n.state == NoticeState::Failed && n.detail.as_deref() == Some("bad"))
    );
    let fallback = r#"{"type":"system","subtype":"model_refusal_fallback","trigger":"refusal","direction":"retry","original_model":"a","fallback_model":"b","request_id":null,"retracted_message_uuids":[],"content":"x"}"#;
    let out = map_line(&mut s, fallback);
    assert!(out.contains(&Update::Model("b".into())));
    assert!(
        matches!(&out[0], Update::Notice(n) if n.kind == NoticeKind::ModelChange && n.text == "Switched from a to b")
    );
    let info = r#"{"type":"system","subtype":"informational","content":"careful","level":"warning","uuid":"u1"}"#;
    assert_eq!(map_line(&mut s, info), [Update::Warning("careful".into())]);
    let note =
        r#"{"type":"system","subtype":"notification","key":"k","text":"fyi","priority":"low"}"#;
    assert!(matches!(&map_line(&mut s, note)[..], [Update::Notice(n)] if n.id == "info-k"));
    let no_fallback = r#"{"type":"system","subtype":"model_refusal_no_fallback","content":"nope"}"#;
    assert_eq!(map_line(&mut s, no_fallback), [Update::Warning("nope".into())]);
}

#[test]
fn a_refusal_fallback_retracts_the_refused_text() {
    let mut s = State { active: true, ..State::default() };
    let say = |id: &str, text: &str| {
        format!(
            r#"{{"type":"assistant","uuid":"{id}","parent_tool_use_id":null,"message":{{"id":"m-{id}","content":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    };
    map_line(&mut s, &say("u1", "Fine part."));
    map_line(&mut s, &say("u2", "Refused part."));
    let fallback = r#"{"type":"system","subtype":"model_refusal_fallback","original_model":"a","fallback_model":"b","retracted_message_uuids":["u2"],"content":"x"}"#;
    assert!(map_line(&mut s, fallback).contains(&Update::Replace("Fine part.".into())));
    assert!(!map_line(&mut s, fallback).iter().any(|u| matches!(u, Update::Final(_))));
}

#[test]
fn unstreamed_assistant_text_is_kept() {
    let mut s = State { active: true, ..State::default() };
    let msg = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"id":"msg_1","content":[{"type":"text","text":"Before the tool."}]}}"#;
    assert_eq!(map_line(&mut s, msg), [Update::Delta("Before the tool.".into())]);
}

#[test]
fn streamed_assistant_text_is_not_repeated() {
    let mut s = State { active: true, ..State::default() };
    map_line(
        &mut s,
        r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_start","message":{"id":"msg_1"}}}"#,
    );
    map_line(
        &mut s,
        r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}}"#,
    );
    let msg = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"id":"msg_1","content":[{"type":"text","text":"hi"}]}}"#;
    assert_eq!(map_line(&mut s, msg), []);
}

#[test]
fn task_tools_fold_into_one_plan_row() {
    let mut s = State { active: true, ..State::default() };
    let create = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_c","name":"TaskCreate","input":{"subject":"Write tests","activeForm":"Writing tests"}}]}}"#;
    assert_eq!(map_line(&mut s, create), []);
    let created = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_c","content":"Task #3 created successfully: Write tests"}]}}"#;
    let update = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_u","name":"TaskUpdate","input":{"taskId":"3","status":"in_progress"}}]}}"#;
    let made = calls(map_line(&mut s, created));
    assert!(
        matches!(&made[..], [c] if c.id == TODOS_ID && matches!(&c.detail, ToolDetail::Plan { items } if items[0].status == PlanStatus::Pending))
    );
    let rows = calls(map_line(&mut s, update));
    assert!(
        matches!(&rows[..], [c] if c.id == TODOS_ID && matches!(&c.detail, ToolDetail::Plan { items } if items[0].status == PlanStatus::Active))
    );
    let ack = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_u","content":"Updated task #3 status"}]}}"#;
    assert_eq!(map_line(&mut s, ack), []);
    let delete = update.replace("in_progress", "deleted");
    assert!(
        matches!(&calls(map_line(&mut s, &delete))[..], [c] if matches!(&c.detail, ToolDetail::Plan { items } if items.is_empty()))
    );
}

#[test]
fn skill_tool_use_is_a_step_not_a_row() {
    let mut s = State { active: true, ..State::default() };
    let skill = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_s","name":"Skill","input":{"skill":"tdd"}}]}}"#;
    assert_eq!(map_line(&mut s, skill), [Update::Skill("tdd".into())]);
}

#[test]
fn tool_result_images_are_saved() {
    let dir = std::env::temp_dir().join(format!("aether-claude-result-img-{}", std::process::id()));
    let mut s = State { active: true, attachments: Some(dir.clone()), ..State::default() };
    map_line(&mut s, BASH_USE);
    let result = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo="}}]}]}}"#;
    let row = &calls(map_line(&mut s, result))[0];
    assert_eq!(row.images.len(), 1);
    assert!(row.images[0].path.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn results_report_errors_cost_and_subagent_tokens() {
    let mut s = State { active: true, ..State::default() };
    let failed = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"errors":["Reached max turns (5)"],"total_cost_usd":0.25,"usage":{},"modelUsage":{"claude-sonnet-5-5":{"inputTokens":100,"outputTokens":10,"cacheReadInputTokens":50,"cacheCreationInputTokens":0},"claude-haiku-5-5":{"inputTokens":40,"outputTokens":5,"cacheReadInputTokens":0,"cacheCreationInputTokens":0}}}"#;
    let out = map_line(&mut s, failed);
    assert!(out.contains(&Update::Error("Reached max turns (5)".into())));
    assert!(out.contains(&Update::Cost(250_000)));
    assert!(out.contains(&Update::Count { prompt: 190, completion: 15, cached: 50 }));
    s.active = true;
    let again =
        failed.replace("0.25", "0.30").replace("\"inputTokens\":100", "\"inputTokens\":120");
    let out = map_line(&mut s, &again);
    assert!(out.contains(&Update::Cost(50_000)));
    assert!(out.contains(&Update::Count { prompt: 20, completion: 0, cached: 0 }));
}

#[test]
fn error_results_without_errors_say_why() {
    let mut s = State { active: true, ..State::default() };
    let line = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"usage":{},"modelUsage":{}}"#;
    assert!(
        map_line(&mut s, line).contains(&Update::Error("Claude stopped: max turns reached".into()))
    );
}

#[test]
fn compaction_fixture_is_a_notice_not_a_row() {
    let out = replay("compaction");
    assert!(
        out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::Compaction && n.state == NoticeState::Done))
    );
    assert!(!last_calls(&out).iter().any(|c| c.title == "Compact context"));
}

#[test]
fn agent_trailer_needs_usage_at_a_line_start() {
    let raw = "The <usage> tag counts tokens.\nSecond line.\nagentId: a1 (internal ID - x)\n<usage>total_tokens: 9\ntool_uses: 1</usage>";
    assert_eq!(
        strip_agent_trailer(raw),
        ("The <usage> tag counts tokens.\nSecond line.".into(), Some((9, 1)))
    );
}

fn background_agent(s: &mut State) {
    map_line(s, AGENT_USE);
    map_line(
        s,
        r#"{"type":"system","subtype":"task_started","task_id":"a1","tool_use_id":"toolu_a","description":"Find styles","subagent_type":"Explore"}"#,
    );
}

#[test]
fn a_foreground_agents_notification_changes_nothing() {
    let mut s = State { active: true, ..State::default() };
    background_agent(&mut s);
    let done = r#"{"type":"system","subtype":"task_notification","task_id":"a1","tool_use_id":"toolu_a","status":"completed","summary":"x"}"#;
    assert_eq!(map_line(&mut s, done), []);
    assert!(s.wake_reason.is_none());
    assert!(s.tools.contains_key("toolu_a"));
}

#[test]
fn background_notifications_map_failure_and_usage_and_drop_the_task() {
    let mut s = State { active: true, ..State::default() };
    background_agent(&mut s);
    let started = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_a","content":"Async agent launched successfully."}]}}"#;
    map_line(&mut s, started);
    s.active = false;
    let done = r#"{"type":"system","subtype":"task_notification","task_id":"a1","tool_use_id":"toolu_a","status":"failed","summary":"boom","usage":{"total_tokens":77,"tool_uses":4,"duration_ms":10}}"#;
    let row = &calls(map_line(&mut s, done))[0];
    assert_eq!(row.status, ToolStatus::Failed);
    assert!(matches!(&row.detail, ToolDetail::Task { tokens: 77, tools: 4, .. }), "{row:?}");
    assert!(s.tasks.is_empty());
}

#[test]
fn a_resumed_agent_runs_in_the_background_until_its_notification() {
    let mut s = State { active: true, ..State::default() };
    let send = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_s","name":"SendMessage","input":{"to":"a1","message":"fix it"}}]}}"#;
    map_line(&mut s, send);
    let resumed = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_s","content":[{"type":"text","text":"{\"success\":true,\"message\":\"Resuming agent a1\",\"resumedAgentId\":\"a1\"}"}]}]}}"#;
    assert_eq!(calls(map_line(&mut s, resumed))[0].status, ToolStatus::Background);
    let done = r#"{"type":"system","subtype":"task_notification","task_id":"a1","tool_use_id":"toolu_s","status":"completed","summary":"fixed"}"#;
    assert_eq!(calls(map_line(&mut s, done))[0].status, ToolStatus::Done);
}

#[test]
fn settled_rows_cannot_be_stopped() {
    let mut s = State { active: true, ..State::default() };
    background_agent(&mut s);
    assert!(stop_request(&s, "toolu_a", 1).is_some());
    let result = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_a","content":"Done."}]}}"#;
    map_line(&mut s, result);
    assert!(stop_request(&s, "toolu_a", 2).is_none());
}

const DENIAL: &str = r#"{"type":"system","subtype":"permission_denied","tool_name":"Bash","tool_use_id":"toolu_1","decision_reason":"blocked by policy","message":"no"}"#;
const ERR_RESULT: &str = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"Permission denied by hook","is_error":true}]}}"#;

#[test]
fn a_denial_survives_the_error_result_that_follows() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, BASH_USE);
    map_line(&mut s, DENIAL);
    let row = &calls(map_line(&mut s, ERR_RESULT))[0];
    assert_eq!((row.status, row.error.as_deref()), (ToolStatus::Denied, Some("blocked by policy")));
}

#[test]
fn a_denial_after_the_result_leaves_the_settled_row_alone() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, BASH_USE);
    let row = &calls(map_line(&mut s, ERR_RESULT))[0];
    assert_eq!(row.status, ToolStatus::Failed);
    assert_eq!(map_line(&mut s, DENIAL), []);
}

#[test]
fn only_the_last_usage_block_is_the_trailer() {
    let raw = "Intro\n<usage> is a tag\nmore body\nagentId: a1 (internal ID - x)\n<usage>total_tokens: 9\ntool_uses: 1</usage>";
    assert_eq!(
        strip_agent_trailer(raw),
        ("Intro\n<usage> is a tag\nmore body".into(), Some((9, 1)))
    );
    let raw =
        "Body\n<usage>total_tokens: 5</usage>\nTail\n<usage>total_tokens: 9\ntool_uses: 2</usage>";
    assert_eq!(
        strip_agent_trailer(raw),
        ("Body\n<usage>total_tokens: 5</usage>\nTail".into(), Some((9, 2)))
    );
}

#[test]
fn retry_closes_at_the_result() {
    let retry = r#"{"type":"system","subtype":"api_retry","attempt":1,"max_retries":3,"retry_delay_ms":500,"error_status":529,"error":"overloaded"}"#;
    let ok = r#"{"type":"result","subtype":"success","is_error":false,"result":"x","usage":{},"modelUsage":{}}"#;
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, retry);
    let out = map_line(&mut s, ok);
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::Retry && n.state == NoticeState::Done && n.until_ms.is_none())));
    assert!(s.retry.is_none());
    let bad = ok.replace(r#""is_error":false"#, r#""is_error":true"#);
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, retry);
    let out = map_line(&mut s, &bad);
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.state == NoticeState::Failed)));
}

#[test]
fn a_task_create_result_without_an_id_adds_no_todo() {
    let mut s = State { active: true, ..State::default() };
    let create = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_c","name":"TaskCreate","input":{"subject":"A"}}]}}"#;
    map_line(&mut s, create);
    let odd = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_c","content":"Task # created"}]}}"#;
    assert_eq!(map_line(&mut s, odd), []);
    assert!(s.todos.is_empty() && s.todo_uses.is_empty());
}

#[test]
fn texted_resets_at_the_result() {
    let mut s = State { active: true, ..State::default() };
    s.texted.insert("m1".into());
    map_line(
        &mut s,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"x","usage":{},"modelUsage":{}}"#,
    );
    assert!(s.texted.is_empty());
}

#[test]
fn a_model_fallback_moves_the_window_to_the_new_model() {
    let mut s = State { active: true, model: "a".into(), ..State::default() };
    map_line(
        &mut s,
        r#"{"type":"system","subtype":"model_fallback","original_model":"a","fallback_model":"b"}"#,
    );
    assert_eq!(s.model, "b");
    let result = r#"{"type":"result","subtype":"success","is_error":false,"result":"x","usage":{},"modelUsage":{"a":{"contextWindow":200000},"b":{"contextWindow":1000000}}}"#;
    assert!(map_line(&mut s, result).contains(&Update::Window(1_000_000)));
}

#[test]
fn slash_steer_is_reported_at_once_and_queues_no_echo() {
    let mut s = State { active: true, ..State::default() };
    let now = on_steer(&mut s, "/compact\n\nAttached files:\n- a.png");
    assert!(
        matches!(&now, Some(u) if matches!(&u[..], [Update::Steered(t)] if t.starts_with("/compact"))),
        "{now:?}"
    );
    assert!(s.echoes.is_empty());
    assert_eq!(on_steer(&mut s, "more"), Some(vec![]));
}

#[test]
fn picking_set_mode_is_not_a_surprise_mode_change() {
    let status =
        r#"{"type":"system","subtype":"status","status":null,"permissionMode":"acceptEdits"}"#;
    let mut s = State { requested: AccessMode::Ask, ..State::default() };
    let [(id, _)] = asks(map_line(&mut s, WRITE_REQUEST))[..] else { panic!() };
    reply(&mut s, id, &Answer::Pick("s-0".into())).unwrap();
    let out = map_line(&mut s, status);
    assert!(out.iter().all(|u| !matches!(u, Update::Notice(_))), "{out:?}");
    assert!(out.contains(&Update::Mode(AccessMode::AutoEdit)));
}

#[test]
fn picking_set_mode_on_a_plan_is_not_a_surprise_mode_change() {
    let line = r##"{"type":"control_request","request_id":"p","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"# P"},"permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"}]}}"##;
    let status =
        r#"{"type":"system","subtype":"status","status":null,"permissionMode":"acceptEdits"}"#;
    let mut s = State { requested: AccessMode::ReadOnly, ..State::default() };
    let [(id, Ask::Plan(p))] = &asks(map_line(&mut s, line))[..] else { panic!() };
    assert_eq!(p.options[0].id, "s-0");
    let rep = reply(&mut s, *id, &Answer::Pick("s-0".into())).unwrap();
    assert_eq!(rep["response"]["response"]["updatedPermissions"][0]["mode"], "acceptEdits");
    assert_eq!(rep["response"]["response"]["updatedPermissions"][0]["destination"], "session");
    let out = map_line(&mut s, status);
    assert!(out.iter().all(|u| !matches!(u, Update::Notice(_))), "{out:?}");
}

fn echo(t: &str) -> String {
    json!({"type":"user","isReplay":true,"parent_tool_use_id":null,"message":{"role":"user","content":[{"type":"text","text":t}]}}).to_string()
}

#[test]
fn a_mismatched_echo_still_pops_so_the_turn_can_end() {
    let mut s = State { active: true, ..State::default() };
    on_prompt(&mut s, "count");
    on_steer(&mut s, "also say done\n\nAttached files:\n- a.png");
    assert_eq!(map_line(&mut s, &echo("count")), [Update::Accepted]);
    assert_eq!(
        map_line(&mut s, &echo("[Image #1] also say done")),
        [Update::Accepted, Update::Steered("also say done".into())]
    );
    assert!(s.echoes.is_empty());
    let result = r#"{"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"ok","usage":{},"modelUsage":{}}"#;
    assert_eq!(map_line(&mut s, result).last(), Some(&Update::Final("ok".into())));
}

#[test]
fn out_of_order_echoes_mark_their_own_steers() {
    let mut s = State { active: true, ..State::default() };
    on_prompt(&mut s, "count");
    on_steer(&mut s, "first");
    on_steer(&mut s, "second");
    assert_eq!(
        map_line(&mut s, &echo("second")),
        [Update::Accepted, Update::Steered("second".into())]
    );
    assert_eq!(
        map_line(&mut s, &echo("first")),
        [Update::Accepted, Update::Steered("first".into())]
    );
    assert_eq!(map_line(&mut s, &echo("count")), [Update::Accepted]);
    assert!(s.echoes.is_empty());
}

#[test]
fn a_trimmed_echo_matches_its_steer() {
    let mut s = State { active: true, ..State::default() };
    on_prompt(&mut s, "count");
    on_steer(&mut s, "one");
    on_steer(&mut s, "two");
    assert_eq!(map_line(&mut s, &echo("two\n")), [Update::Accepted, Update::Steered("two".into())]);
    assert_eq!(s.echoes.len(), 2);
}

#[test]
fn an_idle_elicitation_wakes_the_turn() {
    let mut s = State::default();
    let el = r#"{"type":"control_request","request_id":"e1","request":{"subtype":"elicitation","mcp_server_name":"m","message":"hi","requested_schema":{"type":"object","properties":{}}}}"#;
    let out = map_line(&mut s, el);
    assert!(out.contains(&Update::Wake) && s.active, "{out:?}");
}

#[test]
fn deny_with_a_note_reaches_questions_and_plans() {
    let plan = r##"{"type":"control_request","request_id":"p","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"# P"}}}"##;
    for line in [ASK_REQUEST, plan] {
        let mut s = State::default();
        let [(id, _)] = asks(map_line(&mut s, line))[..] else { panic!() };
        let rep = reply(&mut s, id, &Answer::DenyWith("not now".into())).unwrap();
        assert_eq!(rep["response"]["response"], json!({"behavior": "deny", "message": "not now"}));
    }
}

#[test]
fn a_subtypeless_control_request_is_named_in_its_warning() {
    let mut s = State::default();
    let out = map_line(&mut s, r#"{"type":"control_request","request_id":"x","request":{}}"#);
    assert!(matches!(&out[..], [Update::Warning(w)] if !w.contains("  ")), "{out:?}");
}

const BG_RESULT: &str = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_a","content":"Async agent launched successfully."}]}}"#;

fn with_background_child(s: &mut State) {
    background_agent(s);
    map_line(s, BG_RESULT);
    let child =
        BASH_USE.replace(r#""parent_tool_use_id":null"#, r#""parent_tool_use_id":"toolu_a""#);
    map_line(s, &child);
}

#[test]
fn an_exit_mid_turn_refuses_waiting_steers_and_stops_background_work() {
    let mut s = State { active: true, ..State::default() };
    with_background_child(&mut s);
    on_prompt(&mut s, "count");
    on_steer(&mut s, "more\n\nAttached files:\n- a.png");
    let out = on_exit(&mut s, "claude exited (1)".into());
    let stopped: Vec<(String, ToolStatus)> =
        calls(out.clone()).into_iter().map(|c| (c.id, c.status)).collect();
    assert_eq!(
        stopped,
        [("toolu_1".into(), ToolStatus::Stopped), ("toolu_a".into(), ToolStatus::Stopped)],
        "children before their agent, so they still route to its reply"
    );
    let tail: Vec<&Update> = out.iter().filter(|u| !matches!(u, Update::Tool(_))).collect();
    assert_eq!(
        tail,
        [
            &Update::SteerRefused("more".into()),
            &Update::Error("claude exited (1)".into()),
            &Update::Final(String::new())
        ]
    );
    assert!(s.dead.is_some() && !s.active && s.background.is_empty());
}

#[test]
fn an_idle_exit_warns_and_settles_background_rows() {
    let mut s = State { active: true, ..State::default() };
    with_background_child(&mut s);
    s.active = false;
    let out = on_exit(&mut s, "claude exited (9)".into());
    assert_eq!(calls(out.clone()).len(), 2);
    assert_eq!(out.last(), Some(&Update::Warning("claude exited (9)".into())));
    assert!(!out.iter().any(|u| matches!(u, Update::Final(_) | Update::Error(_))));
}

#[test]
fn settling_without_background_work_sends_nothing() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, BASH_USE);
    assert_eq!(settle_background(&mut s), []);
}

fn interrupt_reply(n: u64, kept: &[&str]) -> String {
    json!({"type":"control_response","response":{"subtype":"success","request_id":format!("{INTERRUPT_REQUEST}{n}"),"response":{"still_queued":kept}}}).to_string()
}

const CANCELLED_RESULT: &str = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["[ede_diagnostic] result_type=user"],"usage":{"input_tokens":0,"output_tokens":0}}"#;

#[test]
fn a_stop_keeps_steers_the_cli_still_holds_and_refuses_the_rest() {
    let mut s = State { active: true, ..State::default() };
    on_prompt(&mut s, "count");
    on_steer(&mut s, "kept");
    on_steer(&mut s, "dropped");
    map_line(&mut s, &echo("count"));
    let kept = s.steer_ids[0].0.clone();
    assert!(matches!(on_cancel(&mut s), Stop::Interrupt(_)));
    assert_eq!(
        map_line(&mut s, &interrupt_reply(1, &[&kept])),
        [Update::SteerRefused("dropped".into())]
    );
    assert!(
        !map_line(&mut s, CANCELLED_RESULT)
            .iter()
            .any(|u| matches!(u, Update::Final(_) | Update::Error(_)))
    );
    assert!(s.active, "the held steer runs next in this reply");
    assert_eq!(map_line(&mut s, &echo("kept")), [Update::Accepted, Update::Steered("kept".into())]);
    let result =
        r#"{"type":"result","subtype":"success","is_error":false,"result":"done","usage":{}}"#;
    assert_eq!(map_line(&mut s, result).last(), Some(&Update::Final("done".into())));
}

#[test]
fn a_stop_with_nothing_held_refuses_every_waiting_steer() {
    let mut s = State { active: true, ..State::default() };
    on_prompt(&mut s, "count");
    on_steer(&mut s, "more");
    on_cancel(&mut s);
    assert_eq!(map_line(&mut s, &interrupt_reply(1, &[])), [Update::SteerRefused("more".into())]);
    assert_eq!(map_line(&mut s, CANCELLED_RESULT), [Update::Final(String::new())]);
}

#[test]
fn a_late_interrupted_result_neither_fails_nor_ends_the_next_turn() {
    let mut s = State { active: true, ..State::default() };
    on_prompt(&mut s, "count");
    on_cancel(&mut s);
    on_prompt(&mut s, "next");
    assert!(
        !map_line(&mut s, CANCELLED_RESULT)
            .iter()
            .any(|u| matches!(u, Update::Final(_) | Update::Error(_)))
    );
    assert!(s.active);
    let failed = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["boom"],"usage":{}}"#;
    let out = map_line(&mut s, failed);
    assert!(out.contains(&Update::Error("boom".into())), "{out:?}");
}

#[test]
fn stopping_a_turn_only_an_idle_ask_opened_denies_it_and_closes_locally() {
    let ask = r#"{"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
    let mut s = State::default();
    map_line(&mut s, ask);
    let Stop::Closed(out) = on_cancel(&mut s) else { panic!("interrupted an idle CLI") };
    assert_eq!(out, [Update::Final(String::new())]);
    assert_eq!(s.outbox[0]["response"]["request_id"], "r");
    assert_eq!(s.outbox[0]["response"]["response"]["behavior"], "deny");
    assert!(!s.active && s.asks.is_empty());

    let mut s = State::default();
    map_line(&mut s, ask);
    map_line(
        &mut s,
        r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_start"}}"#,
    );
    assert!(matches!(on_cancel(&mut s), Stop::Interrupt(_)), "the woken turn has main output");
}

#[test]
fn a_failed_model_switch_warns_and_is_retried() {
    let mut s = State { chosen: "opus".into(), ..State::default() };
    assert!(!choose_model(&mut s, "opus"));
    assert!(choose_model(&mut s, "bogus"));
    assert!(!choose_model(&mut s, "bogus"), "already asked");
    let failed = r#"{"type":"control_response","response":{"subtype":"error","request_id":"aether-model-1","error":"Model 'bogus' not found"}}"#;
    assert_eq!(map_line(&mut s, failed), [Update::Warning("Model 'bogus' not found".into())]);
    assert_eq!(s.chosen, "opus");
    assert!(choose_model(&mut s, "haiku"));
    let ok = r#"{"type":"control_response","response":{"subtype":"success","request_id":"aether-model-2"}}"#;
    assert_eq!(map_line(&mut s, ok), []);
    assert!(!choose_model(&mut s, "haiku"));
}

#[test]
fn a_notification_for_a_gone_row_leaves_no_wake_reason() {
    let mut s = State::default();
    s.background.insert("toolu_gone".into());
    let done = r#"{"type":"system","subtype":"task_notification","task_id":"t9","tool_use_id":"toolu_gone","status":"completed","summary":"x"}"#;
    assert_eq!(map_line(&mut s, done), []);
    assert!(s.wake_reason.is_none());
}

#[test]
fn subagent_text_stops_buffering_after_its_first_line() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, AGENT_USE);
    let delta = |t: &str| {
        json!({"type":"stream_event","parent_tool_use_id":"toolu_a","event":{"delta":{"type":"text_delta","text":t}}}).to_string()
    };
    assert_eq!(calls(map_line(&mut s, &delta("Looking\n"))).len(), 1);
    assert_eq!(map_line(&mut s, &delta("more text")), []);
    assert_eq!(s.sub_text["toolu_a"], "Looking\n");
}

#[test]
fn a_late_failure_of_an_earlier_turns_row_is_a_failure_after_a_stop() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, BASH_USE);
    on_prompt(&mut s, "next");
    on_cancel(&mut s);
    let failed = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"boom","is_error":true}]}}"#;
    assert_eq!(calls(map_line(&mut s, failed))[0].status, ToolStatus::Failed);
}

#[test]
fn a_steer_queued_at_stop_runs_on_in_the_stopped_reply() {
    let out = replay("cancel-steer");
    assert!(
        !out.iter().any(|u| matches!(u, Update::SteerRefused(_) | Update::Error(_))),
        "{out:?}"
    );
    let steered = out.iter().position(|u| matches!(u, Update::Steered(_))).expect("steer echoed");
    let finals: Vec<usize> = out
        .iter()
        .enumerate()
        .filter_map(|(i, u)| matches!(u, Update::Final(_)).then_some(i))
        .collect();
    assert_eq!(finals.len(), 1, "the interrupted result doesn't end the reply");
    assert!(finals[0] > steered);
    assert!(!out.contains(&Update::Wake));
}

#[test]
fn answering_the_last_ask_of_a_quiet_woken_turn_closes_it() {
    let ask = |id: &str| {
        json!({"type":"control_request","request_id":id,"request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}).to_string()
    };
    let mut s = State::default();
    map_line(&mut s, &ask("r1"));
    map_line(&mut s, &ask("r2"));
    reply(&mut s, 1, &Answer::Deny);
    assert_eq!(quiet_turn_end(&mut s), None, "another ask still waits");
    reply(&mut s, 2, &Answer::Deny);
    assert_eq!(quiet_turn_end(&mut s), Some(Update::Final(String::new())));
    assert!(!s.active);
    let mut busy = State { active: true, ..State::default() };
    assert_eq!(quiet_turn_end(&mut busy), None);
}

#[test]
fn a_background_agents_ask_outlives_the_turn_as_a_new_live_card() {
    let mut s = State { active: true, ..State::default() };
    with_background_child(&mut s);
    let ask = |id: &str, agent: &str| {
        json!({"type":"control_request","request_id":id,"request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":id},"agent_id":agent}}).to_string()
    };
    let [(old, _)] = asks(map_line(&mut s, &ask("bg-req", "a1")))[..] else { panic!() };
    map_line(&mut s, &ask("fg-req", "other"));
    let done = r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{"input_tokens":1,"output_tokens":1}}"#;
    let out = map_line(&mut s, done);
    let at = out.iter().position(|u| matches!(u, Update::Final(_))).expect("the turn ends");
    let after = asks(out[at + 1..].to_vec());
    let [(id, Ask::Approval(card))] = &after[..] else { panic!("one live card: {out:?}") };
    assert_eq!(card.input, "bg-req");
    assert_ne!(*id, old);
    assert!(reply(&mut s, old, &Answer::Deny).is_none(), "the expired card's id is gone");
    let line = reply(&mut s, *id, &Answer::Deny).expect("answer reaches the CLI");
    assert_eq!(line["response"]["request_id"], "bg-req");
    assert!(s.asks.is_empty(), "the main agent's stale ask isn't kept");
    assert_eq!(quiet_turn_end(&mut s), Some(Update::Final(String::new())), "its side turn ends");
}

#[test]
fn a_steer_into_a_quiet_turn_makes_it_a_real_one() {
    let ask = r#"{"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
    let mut s = State::default();
    map_line(&mut s, ask);
    on_steer(&mut s, "and then?");
    reply(&mut s, 1, &Answer::Deny);
    assert_eq!(quiet_turn_end(&mut s), None, "the steer's turn is still to run");
    assert!(matches!(on_cancel(&mut s), Stop::Interrupt(_)), "the CLI runs the steer");
}

#[test]
fn a_prompt_or_a_result_ends_a_quiet_wake() {
    let ask = r#"{"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
    let mut s = State::default();
    map_line(&mut s, ask);
    on_prompt(&mut s, "hi");
    assert!(!s.quiet_wake);
    let mut s = State::default();
    map_line(&mut s, ask);
    let done = r#"{"type":"result","subtype":"success","is_error":false,"result":"","usage":{"input_tokens":1,"output_tokens":1}}"#;
    map_line(&mut s, done);
    assert!(!s.quiet_wake);
}

#[test]
fn only_the_latest_interrupt_can_time_out() {
    let mut s = State { active: true, ..State::default() };
    let Stop::Interrupt(first) = on_cancel(&mut s) else { panic!() };
    let Stop::Interrupt(second) = on_cancel(&mut s) else { panic!() };
    assert!(!overdue(&s, first), "the second Stop gets its own 10 s");
    assert!(overdue(&s, second));
}

fn test_opts() -> DriverStartOptions {
    DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: "/tmp".into(),
        model: "opus".into(),
        effort: "high".into(),
        access: AccessMode::Full,
        resume: "no-such-session-aether-test".into(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    }
}

#[test]
fn side_run_skips_session_persistence() {
    let has =
        |o: &DriverStartOptions| launch_args(o).0.iter().any(|a| a == "--no-session-persistence");
    assert!(!has(&test_opts()));
    assert!(has(&DriverStartOptions { ephemeral: true, ..test_opts() }));
}

#[test]
fn lead_launch_adds_aether_mcp_server() {
    let lead = crate::LeadTools {
        exe: "/x/aether".into(),
        sock: "/s/o.sock".into(),
        token: "tok123".into(),
        specs: Value::Null,
    };
    let opts = DriverStartOptions { lead: Some(lead), ..test_opts() };
    let (args, ..) = launch_args(&opts);
    let at = args.iter().position(|a| a == "--mcp-config").expect("mcp flag");
    let body = std::fs::read_to_string(&args[at + 1]).unwrap();
    let v: Value = serde_json::from_str(&body).unwrap();
    let s = &v["mcpServers"]["aether"];
    assert_eq!(s["command"], "/x/aether");
    assert_eq!(s["args"], json!(["mcp"]));
    assert_eq!(s["env"]["AETHER_ORCH_TOKEN"], "tok123");
    assert_eq!(s["env"]["AETHER_ORCH_SOCK"], "/s/o.sock");
    assert!(args.windows(2).any(|w| w[0] == "--allowedTools" && w[1].contains("mcp__aether")));
    assert!(!args.iter().any(|a| a.contains("tok123")), "token stays out of argv");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&args[at + 1]).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn non_lead_launch_has_no_mcp_flags() {
    let (args, ..) = launch_args(&test_opts());
    assert!(!args.iter().any(|a| a == "--mcp-config" || a == "--allowedTools"));
}

#[test]
fn approval_unknown_pick_sends_exact_deny() {
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, WRITE_REQUEST))[..] else { panic!() };
    assert_eq!(
        reply(&mut s, id, &Answer::Pick("s-99".into())),
        Some(json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": "984a7c5a-0adf-43be-bffc-1ee49a7ca0fd", "response": {"behavior": "deny", "message": DENIED}}
        }))
    );
}

#[test]
fn approval_reply_sends_exact_allow_without_permissions() {
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, WRITE_REQUEST))[..] else { panic!() };
    assert_eq!(
        reply(&mut s, id, &Answer::Reply("ok".into())),
        Some(json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": "984a7c5a-0adf-43be-bffc-1ee49a7ca0fd", "response": {"behavior": "allow", "updatedInput": {"file_path": "/private/tmp/claude-probe/probe1.txt", "content": "hi"}}}
        }))
    );
}

#[test]
fn approval_deny_with_blank_text_sends_exact_default_deny() {
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, WRITE_REQUEST))[..] else { panic!() };
    assert_eq!(
        reply(&mut s, id, &Answer::DenyWith("  ".into())),
        Some(json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": "984a7c5a-0adf-43be-bffc-1ee49a7ca0fd", "response": {"behavior": "deny", "message": DENIED}}
        }))
    );
}

const PLAN_SET_MODE: &str = r##"{"type":"control_request","request_id":"p","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"# P"},"permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"localSettings"}]}}"##;

#[test]
fn plan_set_mode_pick_sends_exact_session_scoped_permission() {
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, PLAN_SET_MODE))[..] else { panic!() };
    assert_eq!(
        reply(&mut s, id, &Answer::Pick("s-0".into())),
        Some(json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": "p", "response": {"behavior": "allow", "updatedInput": {"plan": "# P"}, "updatedPermissions": [{"type": "setMode", "mode": "acceptEdits", "destination": "session"}]}}
        }))
    );
    assert_eq!(s.requested, AccessMode::AutoEdit);
}

#[test]
fn plan_unknown_pick_sends_exact_deny() {
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, PLAN_SET_MODE))[..] else { panic!() };
    assert_eq!(
        reply(&mut s, id, &Answer::Pick("bogus".into())),
        Some(json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": "p", "response": {"behavior": "deny", "message": DENIED}}
        }))
    );
}

#[test]
fn questions_deny_with_sends_exact_note_deny() {
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, ASK_REQUEST))[..] else { panic!() };
    assert_eq!(
        reply(&mut s, id, &Answer::DenyWith("no color".into())),
        Some(json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": "b915b282-7cd4-43ee-806d-2ba964c791b2", "response": {"behavior": "deny", "message": "no color"}}
        }))
    );
}

#[test]
fn bash_approve_strips_display_notes_from_edited_command() {
    let line = r#"{"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls","run_in_background":true}}}"#;
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, line))[..] else { panic!() };
    let edited = Answer::Approve { input: "pwd\n(runs in the background)".into(), always: false };
    assert_eq!(
        reply(&mut s, id, &edited),
        Some(json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": "r", "response": {"behavior": "allow", "updatedInput": {"command": "pwd", "run_in_background": true}}}
        }))
    );
    let [(id, _)] = asks(map_line(&mut s, line))[..] else { panic!() };
    let same = Answer::Approve { input: "ls\n(runs in the background)".into(), always: false };
    assert_eq!(
        reply(&mut s, id, &same).unwrap()["response"]["response"]["updatedInput"]["command"],
        "ls"
    );
}

#[test]
fn side_question_request_has_empty_history() {
    let v = side_request("aether-btw-1", "why?");
    assert_eq!(v["type"], "control_request");
    assert_eq!(v["request_id"], "aether-btw-1");
    assert_eq!(v["request"]["subtype"], "side_question");
    assert_eq!(v["request"]["question"], "why?");
    assert_eq!(v["request"]["history"], json!([]));
}

#[test]
fn side_answer_routes_to_its_reply_only() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut s = State { active: true, ..State::default() };
    s.sides.insert("aether-btw-1".into(), tx);
    let progress = r#"{"type":"system","subtype":"control_request_progress","request_id":"aether-btw-1","status":"started"}"#;
    assert!(map_line(&mut s, progress).is_empty());
    let done = r#"{"type":"control_response","response":{"subtype":"success","request_id":"aether-btw-1","response":{"response":"because","synthetic":false}}}"#;
    assert!(map_line(&mut s, done).is_empty());
    assert_eq!(rx.try_recv().unwrap(), Update::Final("because".into()));
    assert!(s.sides.is_empty());
    assert!(s.active);
}

#[test]
fn synthetic_side_answer_is_an_error() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut s = State::default();
    s.sides.insert("aether-btw-3".into(), tx);
    let line = r#"{"type":"control_response","response":{"subtype":"success","request_id":"aether-btw-3","response":{"response":"(No answer available for this side question.)","synthetic":true}}}"#;
    assert!(map_line(&mut s, line).is_empty());
    assert_eq!(
        rx.try_recv().unwrap(),
        Update::Error("claude had no answer for this side question".into())
    );
}

#[test]
fn side_error_response_is_an_error() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut s = State::default();
    s.sides.insert("aether-btw-2".into(), tx);
    let line = r#"{"type":"control_response","response":{"subtype":"error","request_id":"aether-btw-2","error":"Unsupported control request subtype: side_question"}}"#;
    assert!(map_line(&mut s, line).is_empty());
    assert_eq!(
        rx.try_recv().unwrap(),
        Update::Error("Unsupported control request subtype: side_question".into())
    );
}

#[test]
fn process_exit_fails_pending_side_questions() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut s = State::default();
    s.sides.insert("aether-btw-3".into(), tx);
    on_exit(&mut s, "gone".into());
    assert_eq!(rx.try_recv().unwrap(), Update::Error("gone".into()));
}

#[test]
fn native_fork_launches_with_resume_and_fork_flag() {
    let opts = DriverStartOptions { resume: "source".into(), fork: true, ..test_opts() };
    let (args, id, resumed, _) = launch_args(&opts);
    assert!(args.windows(2).any(|pair| pair == ["--resume", "source"]));
    assert!(args.iter().any(|arg| arg == "--fork-session"));
    assert_eq!(id, "source");
    assert!(resumed);
}
