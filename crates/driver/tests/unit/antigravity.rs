use super::*;
use crate::record::fixtures;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use crate::record::last_calls;
use crate::tool::classify;
use aether_core::{ApprovalKind, ToolDetail};
use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};

fn replay(scenario: &str) -> Vec<Update> {
    let values: Vec<Value> = crate::record::fixture("agy", scenario)
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let at = values.iter().position(|v| startup(v).is_some()).expect("no init line");
    let conversation = startup(&values[at]).unwrap().unwrap_or_default();
    let mut s = State { active: true, conversation, ..State::default() };
    let mut out = Vec::new();
    for v in &values[at + 1..] {
        match v["aether"].as_str() {
            Some("prompt") => {
                on_prompt(&mut s, v["text"].as_str().unwrap());
            }
            Some("steer") => {
                on_steer(&mut s, v["text"].as_str().unwrap());
            }
            Some("cancel") => {
                on_cancel(&mut s);
            }
            Some("answer") => {}
            _ => out.extend(map_event(&mut s, v)),
        }
    }
    out
}

const INIT: &str = r#"{"event":"init","conversation_id":"a4d41e01-5d10-47c8-b897-a567b78f030a","init":{"cwd":"/tmp/agy-probe","tools":["run_command","view_file"],"permission_mode":"request-review"}}"#;

const BAD_MODEL: &str = r#"{"event":"result","result":{"conversation_id":"","status":"ERROR","response":"","error":"invalid model selection (--model \"bogus-model\" --effort \"\"): model bogus-model is not recognized as a known model or custom model in settings","duration_seconds":0,"num_turns":0,"usage":{"input_tokens":0,"output_tokens":0,"thinking_tokens":0,"cache_read_tokens":0,"total_tokens":0}}}"#;

const TEXT_TURN: &str = r#"{"event":"step_update","step_update":{"conversation_id":"a4d41e01-5d10-47c8-b897-a567b78f030a","step_index":0,"state":"DONE","step_type":"user_input"}}
{"event":"step_update","step_update":{"conversation_id":"a4d41e01-5d10-47c8-b897-a567b78f030a","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"OK"}}
{"event":"step_update","step_update":{"conversation_id":"a4d41e01-5d10-47c8-b897-a567b78f030a","step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"\n","duration_seconds":5.34886,"usage":{"input_tokens":13533,"output_tokens":321,"thinking_tokens":320,"cache_read_tokens":0,"total_tokens":13854}}}
{"event":"result","result":{"conversation_id":"a4d41e01-5d10-47c8-b897-a567b78f030a","status":"SUCCESS","response":"OK\n","duration_seconds":5.896207,"num_turns":1,"usage":{"input_tokens":13533,"output_tokens":321,"thinking_tokens":320,"cache_read_tokens":0,"total_tokens":13854}}}"#;

const TOOL_TURN: &str = r#"{"event":"step_update","step_update":{"conversation_id":"86d470d6-8d42-4d3e-9283-2182db5ba298","step_index":1,"state":"DONE","step_type":"agent_response","duration_seconds":3.092648,"usage":{"input_tokens":13545,"output_tokens":128,"thinking_tokens":69,"cache_read_tokens":0,"total_tokens":13673}}}
{"event":"step_update","step_update":{"conversation_id":"86d470d6-8d42-4d3e-9283-2182db5ba298","step_index":2,"state":"ACTIVE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"uname -s"}}}}
{"event":"step_update","step_update":{"conversation_id":"86d470d6-8d42-4d3e-9283-2182db5ba298","step_index":2,"state":"DONE","step_type":"tool","tool_name":"run_command","duration_seconds":0.179418,"tool_info":{"name":"run_command","parameters":{"CommandLine":"uname -s"},"output":"Darwin\r\n"}}}
{"event":"step_update","step_update":{"conversation_id":"86d470d6-8d42-4d3e-9283-2182db5ba298","step_index":3,"state":"ACTIVE","step_type":"agent_response","text_delta":"The output"}}
{"event":"step_update","step_update":{"conversation_id":"86d470d6-8d42-4d3e-9283-2182db5ba298","step_index":3,"state":"DONE","step_type":"agent_response","text_delta":"\n","duration_seconds":2.497923,"usage":{"input_tokens":13760,"output_tokens":32,"thinking_tokens":16,"cache_read_tokens":0,"total_tokens":13792}}}
{"event":"result","result":{"conversation_id":"86d470d6-8d42-4d3e-9283-2182db5ba298","status":"SUCCESS","response":"The output\n","duration_seconds":6.702212,"num_turns":1,"usage":{"input_tokens":27305,"output_tokens":160,"thinking_tokens":85,"cache_read_tokens":0,"total_tokens":27465}}}"#;

const DENIED_TURN: &str = r#"{"event":"step_update","step_update":{"conversation_id":"704258e6-6fe4-41f6-9b50-390618fa3ab0","step_index":2,"state":"ACTIVE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"touch denied2.txt"}}}}
{"event":"step_update","step_update":{"conversation_id":"704258e6-6fe4-41f6-9b50-390618fa3ab0","step_index":2,"state":"DONE","step_type":"tool","tool_name":"run_command","duration_seconds":0.185038,"tool_info":{"name":"run_command","parameters":{"CommandLine":"touch denied2.txt"}}}}
{"event":"result","result":{"conversation_id":"704258e6-6fe4-41f6-9b50-390618fa3ab0","status":"SUCCESS","response":"","duration_seconds":4.083536,"num_turns":1,"usage":{"input_tokens":12745,"output_tokens":251,"thinking_tokens":190,"cache_read_tokens":0,"total_tokens":12996},"denied_actions":[{"action":"command","display_name":"RunCommand"}]}}"#;

/// A turn that a steer, written to agy mid-turn, queues behind; tool steps dropped.
const STEERED: &str = r#"{"event":"step_update","step_update":{"conversation_id":"f85aa0b2-72de-428b-ab92-9323f2611f42","step_index":0,"state":"DONE","step_type":"user_input"}}
{"event":"step_update","step_update":{"conversation_id":"f85aa0b2-72de-428b-ab92-9323f2611f42","step_index":1,"state":"DONE","step_type":"agent_response","duration_seconds":5.564523,"usage":{"input_tokens":12709,"output_tokens":245,"thinking_tokens":183,"cache_read_tokens":0,"total_tokens":12954}}}
{"event":"step_update","step_update":{"conversation_id":"f85aa0b2-72de-428b-ab92-9323f2611f42","step_index":3,"state":"ACTIVE","step_type":"agent_response","text_delta":"1\n2\n3\n4\n5"}}
{"event":"step_update","step_update":{"conversation_id":"f85aa0b2-72de-428b-ab92-9323f2611f42","step_index":3,"state":"DONE","step_type":"agent_response","text_delta":"\n","duration_seconds":9.357852,"usage":{"input_tokens":14174,"output_tokens":649,"thinking_tokens":640,"cache_read_tokens":0,"total_tokens":14823}}}"#;

/// agy runs the queued steer as its own turn, with its own `result`.
const STEER_TURN: &str = r#"{"event":"result","result":{"conversation_id":"f85aa0b2-72de-428b-ab92-9323f2611f42","status":"SUCCESS","response":"1\n2\n3\n4\n5\n","duration_seconds":15.823489,"num_turns":1,"usage":{"input_tokens":26883,"output_tokens":894,"thinking_tokens":823,"cache_read_tokens":0,"total_tokens":27777}}}
{"event":"step_update","step_update":{"conversation_id":"f85aa0b2-72de-428b-ab92-9323f2611f42","step_index":4,"state":"DONE","step_type":"user_input"}}
{"event":"step_update","step_update":{"conversation_id":"f85aa0b2-72de-428b-ab92-9323f2611f42","step_index":5,"state":"ACTIVE","step_type":"agent_response","text_delta":"1\n2\n3\n4\n5\nBANANA"}}
{"event":"step_update","step_update":{"conversation_id":"f85aa0b2-72de-428b-ab92-9323f2611f42","step_index":5,"state":"DONE","step_type":"agent_response","text_delta":"\n","duration_seconds":3.690227,"usage":{"input_tokens":15079,"output_tokens":62,"thinking_tokens":50,"cache_read_tokens":0,"total_tokens":15141}}}
{"event":"result","result":{"conversation_id":"f85aa0b2-72de-428b-ab92-9323f2611f42","status":"SUCCESS","response":"1\n2\n3\n4\n5\nBANANA\n","duration_seconds":20.348331,"num_turns":2,"usage":{"input_tokens":41962,"output_tokens":956,"thinking_tokens":873,"cache_read_tokens":0,"total_tokens":42918}}}"#;

const INTERRUPTED: &str = r#"{"event":"result","result":{"conversation_id":"86d470d6-8d42-4d3e-9283-2182db5ba298","status":"ERROR","response":"1\n2\n9","error":"interrupted","duration_seconds":55.089519,"num_turns":4,"usage":{"input_tokens":98856,"output_tokens":1152,"thinking_tokens":630,"cache_read_tokens":0,"total_tokens":100008}}}"#;

fn opts(model: &str, effort: &str, access: AccessMode) -> DriverStartOptions {
    DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: "/tmp".into(),
        model: model.into(),
        effort: effort.into(),
        access,
        resume: String::new(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    }
}

fn running() -> State {
    State {
        active: true,
        inputs: VecDeque::from([Input { text: "go".into(), steer: false }]),
        ..State::default()
    }
}

fn feed(s: &mut State, lines: &str) -> Vec<Update> {
    lines.lines().flat_map(|l| map_event(s, &serde_json::from_str(l).unwrap())).collect()
}

#[test]
fn launch_args_per_access_mode() {
    let args = |access, full| launch_args(&opts("", "", access), "", full).join(" ");
    let base = "--input-format stream-json --output-format stream-json";
    let full = format!("{base} --dangerously-skip-permissions");
    let accept = format!("{base} --mode accept-edits");
    assert_eq!(args(AccessMode::Full, true), full);
    assert_eq!(args(AccessMode::AutoEdit, false), accept);
    assert_eq!(args(AccessMode::Auto, false), format!("{base} --mode auto"));
    assert_eq!(args(AccessMode::Ask, false), accept);
    assert_eq!(args(AccessMode::ReadOnly, false), format!("{base} --mode plan"));
    assert_eq!(requested_mode(AccessMode::ReadOnly), AccessMode::ReadOnly);
    assert_eq!(args(AccessMode::Auto, true), full);
}

#[test]
fn launch_args_strip_the_model_effort_suffix_only_with_effort() {
    let args = |model, effort| {
        launch_args(&opts(model, effort, AccessMode::Auto), "conv-1", false).join(" ")
    };
    assert!(args("gemini-3.8-flash-medium", "high").starts_with(
        "--input-format stream-json --output-format stream-json --conversation conv-1 \
         --model gemini-3.8-flash --effort high --mode auto"
    ));
    assert!(args("gemini-3.8-flash-medium", "").contains("--model gemini-3.8-flash-medium --mode"));
    assert!(args("claude-opus-5-5-xhigh", "low").contains("--model claude-opus-5-5 --effort low"));
    assert!(args("gpt-oss-120b", "max").contains("--model gpt-oss-120b --effort max"));
}

#[test]
fn startup_reads_init_or_fatal_result() {
    let line = |l: &str| startup(&serde_json::from_str(l).unwrap());
    assert_eq!(line(INIT), Some(Ok("a4d41e01-5d10-47c8-b897-a567b78f030a".into())));
    let Some(Err(why)) = line(BAD_MODEL) else { panic!() };
    assert!(why.starts_with("invalid model selection"));
    assert_eq!(line(TEXT_TURN.lines().next().unwrap()), None);
}

#[test]
fn text_turn_streams_deltas_and_per_call_usage() {
    let mut s = running();
    assert_eq!(
        feed(&mut s, TEXT_TURN),
        [
            Update::Accepted,
            Update::Delta("OK".into()),
            Update::Delta("\n".into()),
            Update::Usage(13854),
            Update::Count { prompt: 13533, completion: 321, cached: 0 },
            Update::Final(String::new()),
        ]
    );
    assert!(!s.active);
}

#[test]
fn tool_steps_become_a_typed_command_row() {
    let mut s = State { conversation: "c".into(), ..running() };
    let out = feed(&mut s, TOOL_TURN);
    let rows: Vec<&ToolCall> = out
        .iter()
        .filter_map(|u| match u {
            Update::Tool(call) => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].id.as_str(), rows[0].status), ("c:2", ToolStatus::Running));
    assert_eq!(
        (rows[1].id.as_str(), rows[1].status, rows[1].ms),
        ("c:2", ToolStatus::Done, Some(179))
    );
    let ToolDetail::Command { command, output, .. } = &rows[1].detail else { panic!() };
    assert_eq!((command.as_str(), output.as_str()), ("uname -s", "Darwin\n"));
    assert!(out.contains(&Update::Delta("The output".into())));
    assert_eq!(out.last(), Some(&Update::Final(String::new())));
}

#[test]
fn separate_text_steps_get_a_paragraph_break_unless_a_tool_sits_between() {
    let delta = |i: u64, t: &str| {
        format!(
            r#"{{"event":"step_update","step_update":{{"step_index":{i},"state":"ACTIVE","step_type":"agent_response","text_delta":"{t}"}}}}"#
        )
    };
    let tool = r#"{"event":"step_update","step_update":{"step_index":3,"state":"ACTIVE","step_type":"tool","tool_name":"list_dir","tool_info":{"name":"list_dir","parameters":{"DirectoryPath":"/tmp"}}}}"#;
    let lines =
        [delta(1, "A"), delta(1, "a"), delta(2, "B"), tool.into(), delta(4, "C")].join("\n");
    let texts: Vec<Update> = feed(&mut running(), &lines)
        .into_iter()
        .filter(|u| matches!(u, Update::Delta(_)))
        .collect();
    let d = |t: &str| Update::Delta(t.into());
    assert_eq!(texts, [d("A"), d("a"), d("\n\nB"), d("C")]);
}

#[test]
fn unstreamed_response_is_sent_before_final() {
    let line = r#"{"event":"result","result":{"status":"SUCCESS","response":"Hi"}}"#;
    assert_eq!(
        feed(&mut running(), line),
        [Update::Delta("Hi".into()), Update::Final(String::new())]
    );
}

#[test]
fn denied_actions_mark_the_row_and_ask_to_retry_without_final() {
    let mut s = State { conversation: "c".into(), ..running() };
    let out = feed(&mut s, DENIED_TURN);
    let Update::Tool(row) = &out[2] else { panic!("{out:?}") };
    assert_eq!(row.status, ToolStatus::Denied);
    assert_eq!(
        out[3],
        Update::Ask {
            id: 1,
            ask: Ask::Approval(Approval {
                tool: "Antigravity".into(),
                title: "Rerun with full access?".into(),
                input: "touch denied2.txt".into(),
                reason: "agy blocked these actions. Allowing reruns this turn with full permissions: every action in it runs without asking, not only these.".into(),
                tool_id: Some("c:2".into()),
                kind: ApprovalKind::Command,
                ..Approval::default()
            }),
        }
    );
    assert_eq!(out.len(), 4);
    assert!(s.active);
    assert_eq!((s.inputs.len(), s.ask), (0, Some((1, vec!["touch denied2.txt".into()]))));
}

const TWO_DENIED: &str = r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":1,"state":"DONE","step_type":"tool","tool_name":"run_command","duration_seconds":0.1,"tool_info":{"name":"run_command","parameters":{"CommandLine":"touch one.txt"}}}}
{"event":"step_update","step_update":{"conversation_id":"c","step_index":2,"state":"DONE","step_type":"tool","tool_name":"run_command","duration_seconds":0.1,"tool_info":{"name":"run_command","parameters":{"CommandLine":"touch denied2.txt"}}}}
{"event":"result","result":{"conversation_id":"c","status":"SUCCESS","response":"","denied_actions":[{"action":"command","display_name":"RunCommand"},{"action":"command","display_name":"RunCommand"}]}}"#;

#[test]
fn every_denied_action_marks_its_own_row() {
    let mut s = running();
    let out = feed(&mut s, TWO_DENIED);
    let denied = out
        .iter()
        .filter(|u| matches!(u, Update::Tool(c) if c.status == ToolStatus::Denied))
        .count();
    assert_eq!(denied, 2, "{out:?}");
    let Some(Update::Ask { ask: Ask::Approval(Approval { input, .. }), .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert_eq!(input, "touch one.txt\ntouch denied2.txt");
}

#[test]
fn unmatched_denied_actions_are_listed_by_display_name() {
    let one = TWO_DENIED.lines().nth(1).unwrap().replace("denied2", "x");
    let result = TWO_DENIED.lines().nth(2).unwrap().replace(
        r#"{"action":"command","display_name":"RunCommand"}]"#,
        r#"{"action":"command","display_name":"RunCommand"},{"action":"command","display_name":"RunCommand"}]"#,
    );
    let mut s = running();
    let out = feed(&mut s, &format!("{one}\n{result}"));
    let Some(Update::Ask { ask: Ask::Approval(Approval { input, .. }), .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert_eq!(input, "touch x.txt\nRunCommand\nRunCommand");
    assert_eq!(s.ask.unwrap().1.len(), 3);
}

#[test]
fn retry_message_carries_the_full_list() {
    let text = retry_prompt(&["touch x".into(), "RunCommand".into()]);
    assert_eq!(
        text,
        "The user approved the blocked actions:\ntouch x\nRunCommand\nRetry them and continue."
    );
}

#[test]
fn a_new_prompt_clears_blocked() {
    let blocked = vec![("run_command".to_string(), start("x", "bash", &json!({"command": "c"})))];
    let (mut d, _rx) = idle(State { blocked, ..running() });
    d.prompt("hi".into(), vec![]);
    assert!(d.state.lock().unwrap().blocked.is_empty());
}

#[test]
fn denied_actions_under_full_access_just_end_the_turn() {
    let mut s = State { full: true, ..running() };
    let out = feed(&mut s, DENIED_TURN);
    assert_eq!(out.last(), Some(&Update::Final(String::new())));
    assert!(!out.iter().any(|u| matches!(u, Update::Ask { .. })));
    assert!(s.ask.is_none());
}

#[test]
fn mid_turn_steer_runs_as_the_next_agy_turn_in_the_steer_reply() {
    let (mut d, mut rx) = idle(running());
    let mut sink = Command::new("cat").stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
    d.stdin = sink.stdin.take();
    d.child = Some(sink);
    assert_eq!(delta(&feed(&mut d.state.lock().unwrap(), STEERED)), "1\n2\n3\n4\n5\n");
    assert!(d.steer("Also end with the word BANANA.".into(), vec![]));
    assert!(drain(&mut rx).is_empty());
    assert_eq!(
        feed(&mut d.state.lock().unwrap(), STEER_TURN),
        [
            Update::Steered("Also end with the word BANANA.".into()),
            Update::Delta("1\n2\n3\n4\n5\nBANANA".into()),
            Update::Delta("\n".into()),
            Update::Usage(15141),
            Update::Count { prompt: 15079, completion: 62, cached: 0 },
            Update::Final(String::new()),
        ]
    );
}

#[test]
fn cancel_result_ends_the_turn_without_error() {
    let mut s = State { cancelled: true, texted: true, ..running() };
    assert_eq!(feed(&mut s, INTERRUPTED), [Update::Final(String::new())]);
    let mut s = State { texted: true, ..running() };
    assert_eq!(
        feed(&mut s, INTERRUPTED),
        [Update::Error("interrupted".into()), Update::Final(String::new())]
    );
}

#[test]
fn agy_tools_map_to_typed_details() {
    let detail = |name, p: Value| {
        let (name, input) = tool_input(name, &p);
        start("x", name, &input).detail
    };
    let ToolDetail::Read { path, .. } = detail("view_file", json!({"AbsolutePath": "/a.rs"}))
    else {
        panic!()
    };
    assert_eq!(path, "/a.rs");
    let ToolDetail::Search { pattern, path, .. } =
        detail("grep_search", json!({"Query": "fn main", "SearchPath": "/src"}))
    else {
        panic!()
    };
    assert_eq!((pattern.as_str(), path.as_deref()), ("fn main", Some("/src")));
    let ToolDetail::Search { pattern, path, .. } =
        detail("find_by_name", json!({"Pattern": "*.rs", "SearchDirectory": "/src"}))
    else {
        panic!()
    };
    assert_eq!((pattern.as_str(), path.as_deref()), ("*.rs", Some("/src")));
    let ToolDetail::Search { path, .. } = detail("list_dir", json!({"DirectoryPath": "/src"}))
    else {
        panic!()
    };
    assert_eq!(path.as_deref(), Some("/src"));
    let edit = |name, p| match detail(name, p) {
        ToolDetail::Edit { path, added, removed, .. } => (path, added, removed),
        other => panic!("{other:?}"),
    };
    let write = json!({"TargetFile": "/a.txt", "CodeContent": "hi\n"});
    assert_eq!(edit("write_to_file", write), ("/a.txt".into(), 1, 0));
    let replace = json!({"TargetFile": "/a.txt", "TargetContent": "a", "ReplacementContent": "b"});
    assert_eq!(edit("replace_file_content", replace), ("/a.txt".into(), 1, 1));
    let multi = json!({"TargetFile": "/a.txt", "ReplacementChunks": [
        {"TargetContent": "a", "ReplacementContent": "b"},
        {"TargetContent": "c", "ReplacementContent": "d"},
    ]});
    assert_eq!(edit("multi_replace_file_content", multi), ("/a.txt".into(), 2, 2));
    let web = |name, p| match detail(name, p) {
        ToolDetail::Web { target, .. } => target,
        other => panic!("{other:?}"),
    };
    assert_eq!(web("read_url_content", json!({"Url": "https://x.dev"})), "https://x.dev");
    assert_eq!(web("search_web", json!({"query": "rust"})), "rust");
    let ToolDetail::Other { name, .. } = detail("browser_get_dom", json!({"PageId": "1"})) else {
        panic!()
    };
    assert_eq!(name, "browser_get_dom");
}

#[test]
fn terminal_only_commands_are_caught_by_first_word() {
    assert_eq!(terminal_only("/usage"), Some("usage"));
    assert_eq!(terminal_only("  /clear now"), Some("clear"));
    assert_eq!(terminal_only("/my-skill do it"), None);
    assert_eq!(terminal_only("explain /usage"), None);
}

#[test]
fn pbtxt_title_reads_the_quoted_field() {
    assert_eq!(pbtxt_title(r#"title:"Friendly Greeting""#), Some("Friendly Greeting".into()));
    assert_eq!(
        pbtxt_title("id: 1\n  title: \"Say \\\"hi\\\" to C:\\\\tmp\"\n"),
        Some(r#"Say "hi" to C:\tmp"#.into())
    );
    assert_eq!(pbtxt_title(r#"title:"Half writ"#), None);
    assert_eq!(pbtxt_title(r#"title:"  ""#), None);
    assert_eq!(pbtxt_title(r#"subtitle:"x""#), None);
}

#[test]
fn titles_are_sent_only_when_new_or_changed() {
    let mut s = State::default();
    assert_eq!(new_title(&mut s, r#"title:"Friendly Greeting""#), Some("Friendly Greeting".into()));
    assert_eq!(new_title(&mut s, r#"title:"Friendly Greeting""#), None);
    assert_eq!(new_title(&mut s, ""), None);
    assert_eq!(new_title(&mut s, r#"title:"Simple Greeting""#), Some("Simple Greeting".into()));
    assert_eq!(s.title, "Simple Greeting");
}

fn idle(state: State) -> (AntigravityDriver, UnboundedReceiver<Update>) {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    let d = AntigravityDriver {
        opts: opts("", "", AccessMode::AutoEdit),
        tx,
        state: Arc::new(Mutex::new(state)),
        child: None,
        stdin: None,
        resumed: false,
        rec: None,
    };
    (d, rx)
}

fn drain(rx: &mut UnboundedReceiver<Update>) -> Vec<Update> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

#[test]
fn terminal_only_prompt_is_refused_without_a_process() {
    let (mut d, mut rx) = idle(State::default());
    d.prompt("/usage".into(), vec![]);
    assert_eq!(
        drain(&mut rx),
        [
            Update::Warning("`/usage` only works in the agy terminal".into()),
            Update::Final(String::new())
        ]
    );
}

#[cfg(unix)]
#[test]
fn denying_the_retry_or_cancelling_it_ends_the_turn() {
    let waiting =
        || State { active: true, ask: Some((1, vec!["RunCommand".into()])), ..State::default() };
    let (mut d, mut rx) = idle(waiting());
    d.answer(2, Answer::Deny);
    assert!(drain(&mut rx).is_empty());
    d.answer(1, Answer::Deny);
    assert_eq!(drain(&mut rx), [Update::Final(String::new())]);
    let (mut d, mut rx) = idle(waiting());
    d.cancel();
    assert_eq!(drain(&mut rx), [Update::Final(String::new())]);
    assert!(d.state.lock().unwrap().ask.is_none());
}

/// Fake agy: echoes its args as a delta; without full access every turn hits a denied command.
#[cfg(unix)]
const FAKE: &str = r#"#!/bin/sh
echo '{"event":"init","conversation_id":"c1"}'
trap 'echo "{\"event\":\"result\",\"result\":{\"status\":\"ERROR\",\"error\":\"interrupted\"}}"; exit 1' INT
while read -r line; do
  echo "{\"event\":\"step_update\",\"step_update\":{\"step_index\":1,\"state\":\"DONE\",\"step_type\":\"agent_response\",\"text_delta\":\"$*\"}}"
  case "$line $*" in
    *wait*) sleep 5 >/dev/null & wait ;;
    *dangerously*) echo '{"event":"result","result":{"status":"SUCCESS"}}' ;;
    *) echo '{"event":"step_update","step_update":{"step_index":2,"state":"DONE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"touch x"}}}}'
       echo '{"event":"result","result":{"status":"SUCCESS","denied_actions":[{"action":"command","display_name":"RunCommand"}]}}' ;;
  esac
done
"#;

#[cfg(unix)]
fn fake_agy(name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("aether-fake-agy-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("agy");
    std::fs::write(&path, FAKE).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
fn until(rx: &mut UnboundedReceiver<Update>, stop: fn(&Update) -> bool) -> Vec<Update> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut out = Vec::new();
    while Instant::now() < deadline {
        match rx.try_recv() {
            Ok(u) => {
                let done = stop(&u);
                out.push(u);
                if done {
                    return out;
                }
            }
            Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break,
        }
    }
    panic!("no stop update in {out:?}");
}

fn delta(out: &[Update]) -> String {
    out.iter()
        .filter_map(|u| match u {
            Update::Delta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

#[cfg(unix)]
fn ask_id(out: &[Update]) -> u64 {
    out.iter()
        .find_map(|u| match u {
            Update::Ask { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap()
}

#[cfg(unix)]
#[test]
fn approved_retry_respawns_with_full_access_then_back() {
    let bin = fake_agy("retry");
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let o = DriverStartOptions {
        binary: bin.display().to_string(),
        ..opts("", "", AccessMode::AutoEdit)
    };
    let mut d = AntigravityDriver::spawn(o, tx);
    assert!(d.alive() && !d.resumed());
    assert!(drain(&mut rx).contains(&Update::Session("c1".into())));

    d.prompt("go".into(), vec![]);
    let out = until(&mut rx, |u| matches!(u, Update::Ask { .. }));
    assert!(!delta(&out).contains("--conversation"));
    assert!(out.iter().any(|u| matches!(u, Update::Tool(c) if c.status == ToolStatus::Denied)));

    d.answer(ask_id(&out), Answer::Approve { input: String::new(), always: false });
    let out = until(&mut rx, |u| matches!(u, Update::Final(_)));
    assert!(delta(&out).contains("--conversation c1 --dangerously-skip-permissions"), "{out:?}");
    assert!(!out.iter().any(|u| matches!(u, Update::Error(_))));

    d.prompt("go".into(), vec![]);
    let out = until(&mut rx, |u| matches!(u, Update::Ask { .. }));
    assert!(delta(&out).contains("--conversation c1 --mode accept-edits"), "{out:?}");
    drop(d);
    let _ = std::fs::remove_dir_all(bin.parent().unwrap());
}

#[cfg(unix)]
#[test]
fn cancel_interrupts_the_process_and_next_prompt_respawns() {
    let bin = fake_agy("cancel");
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let o =
        DriverStartOptions { binary: bin.display().to_string(), ..opts("", "", AccessMode::Full) };
    let mut d = AntigravityDriver::spawn(o, tx);
    d.prompt("please wait".into(), vec![]);
    until(&mut rx, |u| matches!(u, Update::Delta(_)));
    d.cancel();
    assert_eq!(until(&mut rx, |u| matches!(u, Update::Final(_))), [Update::Final(String::new())]);

    d.prompt("again".into(), vec![]);
    let out = until(&mut rx, |u| matches!(u, Update::Final(_)));
    assert!(out.contains(&Update::Session("c1".into())));
    assert!(delta(&out).contains("--conversation c1"));
    assert!(!out.iter().any(|u| matches!(u, Update::Error(_))));
    drop(d);
    let _ = std::fs::remove_dir_all(bin.parent().unwrap());
}

#[test]
#[ignore]
fn live_antigravity_smoke() {
    let dir = std::env::temp_dir().join("agy-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    let start = Instant::now();
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let o = DriverStartOptions { cwd: dir, ..opts("", "", AccessMode::Full) };
    let mut d = AntigravityDriver::spawn(o, tx);
    println!("{:>6}ms spawned, alive {}", start.elapsed().as_millis(), d.alive());
    d.prompt("Reply with the single word OK.".into(), vec![]);
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        assert!(Instant::now() < deadline, "timed out");
        match rx.try_recv() {
            Ok(u) => {
                println!("{:>6}ms {u:?}", start.elapsed().as_millis());
                if matches!(u, Update::Final(_)) {
                    break;
                }
            }
            Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => panic!("channel closed"),
        }
    }
}

#[test]
fn every_fixture_replays_without_panicking() {
    let all = fixtures("agy");
    assert!(!all.is_empty());
    for (name, _) in all {
        let out = replay(&name);
        // agy fatal: bad model dies before the init line, the Error is not in the stream
        assert!(name == "fatal" || !out.is_empty(), "{name} produced no updates");
    }
}

const USER_INPUT: &str = r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":5,"state":"DONE","step_type":"user_input"}}"#;
const RESULT_OK: &str = r#"{"event":"result","result":{"conversation_id":"c","status":"SUCCESS","response":"","duration_seconds":1,"num_turns":1,"usage":{"input_tokens":1,"output_tokens":1,"thinking_tokens":0,"cache_read_tokens":0,"total_tokens":2}}}"#;

#[test]
fn steers_are_marked_at_their_user_input_step_and_results_pair_up() {
    let mut s = State::default();
    on_prompt(&mut s, "count");
    on_steer(&mut s, "also say done");
    assert_eq!(feed(&mut s, USER_INPUT), [Update::Accepted]);
    assert_eq!(feed(&mut s, USER_INPUT), [Update::Steered("also say done".into())]);
    assert_eq!(feed(&mut s, RESULT_OK), [], "the steer's own agy turn is still running");
    assert_eq!(feed(&mut s, RESULT_OK), [Update::Final(String::new())]);
}

#[test]
fn events_outside_a_turn_wake_it() {
    let mut s = State::default();
    let text = r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":9,"state":"DONE","step_type":"agent_response","text_delta":"bg done"}}"#;
    let out = feed(&mut s, text);
    assert!(
        matches!(&out[..], [Update::Wake, Update::Notice(n), Update::Delta(_)] if n.kind == NoticeKind::Wake),
        "{out:?}"
    );
    assert_eq!(feed(&mut s, RESULT_OK), [Update::Final(String::new())]);
}

#[test]
fn init_reports_the_mode_in_effect() {
    let mut s = State { requested: AccessMode::Auto, ..State::default() };
    let out = init(
        &mut s,
        &serde_json::from_str(&INIT.replace("request-review", "accept-edits")).unwrap(),
    );
    assert!(out.contains(&Update::Mode(AccessMode::AutoEdit)));
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::ModeChange)));
}

#[test]
fn init_notes_a_lost_conversation_and_full_launch() {
    let mut s = State { conversation: "old".into(), full: true, ..State::default() };
    let out = init(&mut s, &serde_json::from_str(INIT).unwrap());
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.id == "info-resume")));
    assert!(out.contains(&Update::Mode(AccessMode::Full)));
    assert_eq!(s.conversation, "a4d41e01-5d10-47c8-b897-a567b78f030a");
}

#[test]
fn agy_modes_map_and_unknown_is_none() {
    assert_eq!(agy_mode("accept-edits"), Some(AccessMode::AutoEdit));
    assert_eq!(agy_mode("auto"), Some(AccessMode::Auto));
    assert_eq!(agy_mode("plan"), Some(AccessMode::ReadOnly));
    assert_eq!(agy_mode("yolo"), None);
}

#[test]
fn a_new_prompt_resolves_the_pending_rerun_ask() {
    let mut s = State { ask: Some((3, vec!["x".into()])), ..running() };
    assert_eq!(on_prompt(&mut s, "next"), [Update::AskResolved(3)]);
    assert!(s.ask.is_none());
}

#[test]
fn launch_scopes_add_dir_to_the_chat_and_never_uses_the_ide_binary() {
    let dir = std::env::temp_dir().join(format!("aether-agy-att-{}", std::process::id()));
    let o =
        DriverStartOptions { attachments: Some(dir.clone()), ..opts("", "", AccessMode::AutoEdit) };
    let args = launch_args(&o, "", false);
    assert!(args.windows(2).any(|w| w[0] == "--add-dir" && w[1] == dir.to_string_lossy()));
    assert!(
        !launch_args(&opts("", "", AccessMode::AutoEdit), "", false)
            .contains(&"--add-dir".to_string())
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn steer_fixture_marks_the_steer_and_ends_once() {
    let out = replay("steer");
    assert!(out.iter().any(|u| matches!(u, Update::Steered(_))));
    assert_eq!(out.iter().filter(|u| matches!(u, Update::Final(_))).count(), 1);
}

#[cfg(unix)]
#[test]
fn rerun_answer_does_not_block_on_init() {
    let dir = std::env::temp_dir().join(format!("aether-slow-agy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("agy");
    std::fs::write(&path, "#!/bin/sh\nsleep 5\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    let state = State { active: true, ask: Some((1, vec!["x".into()])), ..State::default() };
    let mut d = AntigravityDriver {
        opts: DriverStartOptions {
            binary: path.display().to_string(),
            ..opts("", "", AccessMode::AutoEdit)
        },
        tx,
        state: Arc::new(Mutex::new(state)),
        child: None,
        stdin: None,
        resumed: false,
        rec: None,
    };
    let t = Instant::now();
    d.answer(1, Answer::Approve { input: String::new(), always: false });
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    drop(d);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn subagent_and_view_file_inputs_are_typed() {
    let (name, input) = tool_input(
        "invoke_subagent",
        &json!({"Subagents":[{"TypeName":"researcher","Role":"Find docs","Prompt":"look up X"}]}),
    );
    assert!(
        matches!(classify(name, &input).1, ToolDetail::Task { ref agent, ref prompt, .. } if agent == "researcher" && prompt == "look up X")
    );
    let (_, input) = tool_input(
        "invoke_subagent",
        &json!({"Subagents":[{"TypeName":"a","Prompt":"p"},{"TypeName":"b","Prompt":"q"}]}),
    );
    assert_eq!(input["description"], "2 subagents: a, b");
    let (name, input) = tool_input(
        "browser_subagent",
        &json!({"TaskName":"Open it","Task":"go","TaskSummary":"s"}),
    );
    assert_eq!(
        (name, input["description"].as_str(), input["subagent_type"].as_str()),
        ("agent", Some("Open it"), Some("browser"))
    );
    let (name, input) =
        tool_input("view_file", &json!({"AbsolutePath":"/w/a.rs","StartLine":10,"EndLine":19}));
    assert!(
        matches!(classify(name, &input).1, ToolDetail::Read { range: Some(ref r), .. } if r == "lines 10–19")
    );
    let (_, input) = tool_input("search_web", &json!({"Query":"rust 1.99"}));
    assert_eq!(input["query"], "rust 1.99");
}

#[test]
fn denied_actions_mark_rows_by_tool_name() {
    let lines = [
        r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":1,"state":"DONE","step_type":"tool","tool_name":"write_to_file","tool_info":{"name":"write_to_file","parameters":{"TargetFile":"/w/a.txt","CodeContent":"x"}}}}"#,
        r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":2,"state":"DONE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"touch b"}}}}"#,
        r#"{"event":"result","result":{"conversation_id":"c","status":"SUCCESS","response":"","denied_actions":[{"action":"command","display_name":"RunCommand"}]}}"#,
    ]
    .join("\n");
    let mut s = State { conversation: "c".into(), ..running() };
    let out = feed(&mut s, &lines);
    let denied: Vec<String> = out
        .iter()
        .filter_map(|u| match u {
            Update::Tool(c) if c.status == ToolStatus::Denied => Some(c.id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(denied, ["c:2"]);
    assert!(
        matches!(out.last(), Some(Update::Ask { ask: Ask::Approval(a), .. }) if a.tool_id.as_deref() == Some("c:2") && a.kind == ApprovalKind::Command && a.options.is_empty())
    );
}

#[test]
fn context_window_comes_from_the_model_family() {
    assert_eq!(window("gemini-3.8-flash-medium"), Some(1_048_576));
    assert_eq!(window("claude-opus-5-5-high"), Some(200_000));
    assert_eq!(window("gpt-oss-120b"), None);
    assert_eq!(window(""), None);
    let mut s = State { model: "gemini-3.8-flash-medium".into(), ..State::default() };
    assert!(
        init(&mut s, &serde_json::from_str(INIT).unwrap()).contains(&Update::Window(1_048_576))
    );
}

#[test]
fn background_run_fixture_has_no_background_marker_and_status_row_settles_nothing() {
    let out = replay("background-run");
    let calls = last_calls(&out);
    assert!(
        calls.iter().all(|c| c.status != ToolStatus::Background && c.status != ToolStatus::Running)
    );
    assert!(calls.iter().any(|c| c.status == ToolStatus::Done && matches!(c.detail, ToolDetail::Command { .. })));
}

#[test]
fn manage_task_status_done_settles_a_background_row() {
    let mut s = State { conversation: "c".into(), ..running() };
    feed(
        &mut s,
        &tool_line(
            1,
            "run_command",
            r#"{"CommandLine":"x"}"#,
            &format!("{}c/task-9\n", BACKGROUND_MARKS[1]),
        ),
    );
    assert_eq!(s.background.keys().next().map(String::as_str), Some("c/task-9"));
    let out = feed(
        &mut s,
        &tool_line(
            2,
            "manage_task",
            r#"{"Action":"status","TaskId":"c/task-9"}"#,
            "Task: c/task-9\nStatus: DONE\nLog output:\nbg-done\n",
        ),
    );
    let bg = last_calls(&out).into_iter().find(|c| c.id == "c:1").unwrap();
    assert_eq!(bg.status, ToolStatus::Done);
}

#[test]
fn background_fixture_settles_its_row() {
    let calls = last_calls(&replay("background"));
    assert!(
        calls
            .iter()
            .any(|c| c.status == ToolStatus::Done && matches!(c.detail, ToolDetail::Command { .. })),
        "{calls:?}"
    );
    assert!(calls.iter().all(|c| c.status != ToolStatus::Running));
}

#[test]
fn subagent_fixture_has_a_task_row() {
    let out = replay("subagent");
    let task: Vec<ToolStatus> = out
        .iter()
        .filter_map(|u| match u {
            Update::Tool(c) if matches!(c.detail, ToolDetail::Task { .. }) => Some(c.status),
            _ => None,
        })
        .collect();
    let (last, before) = task.split_last().expect("a task row");
    assert_eq!(*last, ToolStatus::Done);
    assert!(before.len() >= 2 && before.iter().all(|s| *s == ToolStatus::Running), "{task:?}");
    let rows = last_calls(&out);
    assert!(
        rows.iter().any(|c| c.status != ToolStatus::Running
            && matches!(&c.detail, ToolDetail::Task { agent, .. } if !agent.is_empty())),
        "{rows:?}"
    );
}

const MARK: &str = BACKGROUND_MARKS[0];

fn tool_line(index: u32, name: &str, params: &str, output: &str) -> String {
    json!({"event":"step_update","step_update":{"conversation_id":"c","step_index":index,"state":"DONE","step_type":"tool","tool_name":name,"tool_info":{"name":name,"parameters":serde_json::from_str::<Value>(params).unwrap(),"output":output}}}).to_string()
}

#[test]
fn background_command_stays_background_until_a_status_step_settles_it() {
    let mut s = State { conversation: "c".into(), ..running() };
    let start = tool_line(
        1,
        "run_command",
        r#"{"CommandLine":"sleep 9"}"#,
        &format!("started\n{MARK}task-7\n"),
    );
    let out = feed(&mut s, &start);
    let Some(Update::Tool(row)) = out.last() else { panic!("{out:?}") };
    assert_eq!(row.status, ToolStatus::Background);
    assert_eq!(s.background.get("task-7").map(String::as_str), Some("c:1"));
    s.active = true;
    let running = tool_line(2, "command_status", r#"{"CommandId":"task-7"}"#, "still going");
    let out = feed(&mut s, &running);
    assert!(out.iter().any(
        |u| matches!(u, Update::Tool(c) if c.id == "c:1" && c.status == ToolStatus::Background)
    ));
    let done = tool_line(
        3,
        "command_status",
        r#"{"CommandId":"task-7"}"#,
        "Command exited with code 0.\n\nCommand output:\nbg-done\n",
    );
    let out = feed(&mut s, &done);
    let calls = last_calls(&out);
    let bg = calls.iter().find(|c| c.id == "c:1").unwrap();
    assert_eq!(bg.status, ToolStatus::Done);
    assert!(matches!(&bg.detail, ToolDetail::Command { output, .. } if output.contains("bg-done")));
    assert!(s.background.is_empty());
}

#[test]
fn killing_a_background_task_stops_its_row_and_rows_outlive_a_turn() {
    let mut s = State { conversation: "c".into(), ..running() };
    feed(&mut s, &tool_line(1, "run_command", r#"{"CommandLine":"x"}"#, &format!("{MARK}t1")));
    begin(&mut s);
    assert_eq!(s.background.len(), 1);
    let out =
        feed(&mut s, &tool_line(4, "manage_task", r#"{"Action":"kill","TaskId":"t1"}"#, "killed"));
    let bg = last_calls(&out).into_iter().find(|c| c.id == "c:1").unwrap();
    assert_eq!(bg.status, ToolStatus::Stopped);
    assert!(s.background.is_empty());
}

#[test]
fn wake_after_cancel_is_not_dropped() {
    let (mut d, mut rx) = idle(State { active: true, ..State::default() });
    d.cancel();
    assert_eq!(drain(&mut rx), [Update::Final(String::new())]);
    let text = r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":9,"state":"DONE","step_type":"agent_response","text_delta":"bg done"}}"#;
    let out = feed(&mut d.state.lock().unwrap(), text);
    assert!(matches!(out.first(), Some(Update::Wake)), "{out:?}");
}

#[test]
fn agy_error_line_is_the_headline_and_not_repeated_in_the_tail() {
    let mut s = State::default();
    for line in [
        "warning: noise",
        "starting",
        r#"AGY_ERROR: {"message":"quota exceeded"}"#,
        "trailing line",
    ] {
        on_stderr(&mut s, line);
    }
    let why = fatal_text(&s);
    assert!(why.starts_with("quota exceeded\n"), "{why}");
    assert!(why.contains("starting") && why.contains("trailing line"));
    assert!(!why.contains("AGY_ERROR") && !why.contains("noise"), "{why}");
}

#[test]
fn mode_auto_unavailable_on_stderr_falls_back_to_accept_edits() {
    let mut s = State { requested: AccessMode::Auto, epoch: 3, ..State::default() };
    let out = on_stderr(&mut s, "error: --mode auto is unavailable for this account");
    assert_eq!(out[0], Update::Mode(AccessMode::AutoEdit));
    assert!(
        matches!(&out[1], Update::Notice(n) if n.kind == NoticeKind::ModeChange && n.id == "mode-3")
    );
    s.requested = AccessMode::AutoEdit;
    assert!(on_stderr(&mut s, "error: --mode auto is unavailable").is_empty());
}

#[test]
fn process_death_mid_wake_ends_the_turn_with_the_stderr_tail() {
    let mut s = State { active: true, live: true, ..State::default() };
    on_stderr(&mut s, "boom happened");
    let out = closed(&mut s, true);
    assert!(!s.active && !s.live);
    assert!(
        matches!(&out[..], [Update::Error(e), Update::Final(f)] if e.contains("boom happened") && f.is_empty()),
        "{out:?}"
    );
    s.active = true;
    s.ask = Some((1, vec![]));
    assert!(closed(&mut s, true).is_empty(), "a pending rerun ask survives");
}

#[test]
fn failed_spawn_ends_the_startup_notice_as_failed() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut s = State { active: true, ..running() };
    fail(&mut s, &tx, "agy broke".into());
    let out = drain(&mut rx);
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.id == "startup-spawn" && n.state == NoticeState::Failed)), "{out:?}");
    let out = closed(&mut State::default(), false);
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.id == "startup-spawn" && n.state == NoticeState::Failed)));
}

#[test]
fn flat_subagent_active_step_and_subagent_done_step_make_one_finished_row() {
    let mut s = State { conversation: "c".into(), ..running() };
    let active = r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":4,"state":"ACTIVE","step_type":"tool","tool_name":"invoke_subagent","tool_info":{"name":"invoke_subagent","parameters":{"Role":"Codebase Researcher","TypeName":"research"}}}}"#;
    let done = r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":4,"state":"DONE","step_type":"subagent","tool_name":"invoke_subagent","duration_seconds":0.5,"subagent_info":{"subagents":[{"type_name":"research","role":"Codebase Researcher","initial_prompt":"read it"}]}}}"#;
    let rows = last_calls(&feed(&mut s, &format!("{active}\n{done}")));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].status, ToolStatus::Running, "dispatched, not done");
    let report = r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":6,"state":"DONE","step_type":"system_message"}}"#;
    let rows = last_calls(&feed(&mut s, report));
    assert_eq!(rows[0].status, ToolStatus::Done);
    assert!(
        matches!(&rows[0].detail, ToolDetail::Task { agent, prompt, description, .. } if agent == "research" && prompt == "read it" && description == "Codebase Researcher"),
        "{rows:?}"
    );
}

#[test]
fn agy_1_4_read_and_write_file_use_path() {
    let (name, input) =
        tool_input("read_file", &json!({"Path":"/w/a.rs","StartLine":3,"EndLine":4}));
    assert!(
        matches!(classify(name, &input).1, ToolDetail::Read { ref path, range: Some(ref r), .. } if path == "/w/a.rs" && r == "lines 3–4")
    );
    let (name, input) = tool_input("write_file", &json!({"Path":"/w/b.txt","CodeContent":"hi"}));
    assert!(
        matches!(classify(name, &input).1, ToolDetail::Edit { ref path, .. } if path == "/w/b.txt")
    );
}

#[test]
fn kill_with_empty_output_still_stops_the_background_row() {
    let mut s = State { conversation: "c".into(), ..running() };
    feed(&mut s, &tool_line(1, "run_command", r#"{"CommandLine":"x"}"#, &format!("{MARK}t1")));
    let kill = json!({"event":"step_update","step_update":{"conversation_id":"c","step_index":2,"state":"DONE","step_type":"tool","tool_name":"manage_task","tool_info":{"name":"manage_task","parameters":{"Action":"kill","TaskId":"t1"}}}}).to_string();
    let out = feed(&mut s, &kill);
    let bg = last_calls(&out).into_iter().find(|c| c.id == "c:1").unwrap();
    assert_eq!(bg.status, ToolStatus::Stopped);
    assert!(s.background.is_empty() && s.blocked.is_empty());
}

#[test]
fn background_rows_stop_when_agy_exits_or_is_respawned() {
    let mut s = State { conversation: "c".into(), live: true, active: false, ..running() };
    feed(&mut s, &tool_line(1, "run_command", r#"{"CommandLine":"x"}"#, &format!("{MARK}t1")));
    s.active = false;
    let out = closed(&mut s, true);
    assert!(
        matches!(&out[..], [Update::Tool(c)] if c.id == "c:1" && c.status == ToolStatus::Stopped),
        "{out:?}"
    );
    assert!(s.background.is_empty());
    feed(&mut s, &tool_line(2, "run_command", r#"{"CommandLine":"y"}"#, &format!("{MARK}t2")));
    let out = stop_background(&mut s);
    assert!(
        matches!(&out[..], [Update::Tool(c)] if c.id == "c:2" && c.status == ToolStatus::Stopped)
    );
    assert!(s.tools.is_empty());
}

#[cfg(unix)]
#[test]
fn deny_then_next_prompt_still_gets_a_turn() {
    let bin = fake_agy("deny");
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let o = DriverStartOptions {
        binary: bin.display().to_string(),
        ..opts("", "", AccessMode::AutoEdit)
    };
    let mut d = AntigravityDriver::spawn(o, tx);
    drain(&mut rx);
    d.prompt("go".into(), vec![]);
    let out = until(&mut rx, |u| matches!(u, Update::Ask { .. }));
    d.answer(ask_id(&out), Answer::Deny);
    until(&mut rx, |u| matches!(u, Update::Final(_)));
    d.prompt("again".into(), vec![]);
    until(&mut rx, |u| matches!(u, Update::Ask { .. }));
    drop(d);
    let _ = std::fs::remove_dir_all(bin.parent().unwrap());
}

#[test]
fn sync_command_output_that_mentions_the_markers_stays_done() {
    let mut s = State { conversation: "c".into(), ..running() };
    let body = format!(
        "Command exited with code 0.\n\nCommand output:\n{}x\n{}y\n",
        BACKGROUND_MARKS[0], BACKGROUND_MARKS[1]
    );
    let out = feed(&mut s, &tool_line(1, "run_command", r#"{"CommandLine":"cat docs"}"#, &body));
    assert_eq!(last_calls(&out)[0].status, ToolStatus::Done);
    assert!(s.background.is_empty());
}

#[test]
fn status_done_in_a_task_log_does_not_settle_a_running_task() {
    let mut s = State { conversation: "c".into(), ..running() };
    feed(&mut s, &tool_line(1, "run_command", r#"{"CommandLine":"x"}"#, &format!("{MARK}t1")));
    let out = feed(
        &mut s,
        &tool_line(
            2,
            "manage_task",
            r#"{"Action":"status","TaskId":"t1"}"#,
            "Task: t1\nStatus: RUNNING\nLog output:\nStatus: DONE\n",
        ),
    );
    let bg = last_calls(&out).into_iter().find(|c| c.id == "c:1").unwrap();
    assert_eq!(bg.status, ToolStatus::Background);
    assert!(!s.background.is_empty());
}

#[test]
fn subagent_step_respects_active_and_cancel() {
    let step = |state: &str| {
        format!(
            r#"{{"event":"step_update","step_update":{{"conversation_id":"c","step_index":4,"state":"{state}","step_type":"subagent","subagent_info":{{"subagents":[{{"type_name":"r","role":"R","initial_prompt":"p"}}]}}}}}}"#
        )
    };
    let mut s = State { conversation: "c".into(), ..running() };
    assert_eq!(last_calls(&feed(&mut s, &step("ACTIVE")))[0].status, ToolStatus::Running);
    s.cancelled = true;
    assert_eq!(last_calls(&feed(&mut s, &step("ERROR")))[0].status, ToolStatus::Stopped);
}

#[test]
fn kill_after_cancel_stops_background_rows_and_finals() {
    let mut s = State { conversation: "c".into(), active: true, ..running() };
    feed(&mut s, &tool_line(1, "run_command", r#"{"CommandLine":"x"}"#, &format!("{MARK}t1")));
    s.active = true;
    let epoch = s.epoch;
    let out = killed(&mut s);
    assert!(
        matches!(&out[..], [Update::Tool(c), Update::Final(_)] if c.status == ToolStatus::Stopped),
        "{out:?}"
    );
    assert!(!s.active && s.epoch == epoch + 1 && s.background.is_empty());
}

fn with_steer(s: &mut State, taken: usize) {
    s.inputs.push_back(Input { text: "more".into(), steer: true });
    s.taken = taken;
}

#[test]
fn every_death_path_refuses_the_steer_agy_never_took() {
    let mut s = State { live: true, ..running() };
    with_steer(&mut s, 1);
    let out = closed(&mut s, true);
    assert!(
        matches!(&out[..], [Update::SteerRefused(t), Update::Error(_), Update::Final(_)] if t == "more"),
        "{out:?}"
    );

    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut s = running();
    with_steer(&mut s, 1);
    fail(&mut s, &tx, "agy broke".into());
    assert!(drain(&mut rx).contains(&Update::SteerRefused("more".into())));

    let mut s = running();
    with_steer(&mut s, 1);
    assert!(killed(&mut s).contains(&Update::SteerRefused("more".into())));

    let mut s = running();
    with_steer(&mut s, 2);
    assert!(!closed(&mut s, true).iter().any(|u| matches!(u, Update::SteerRefused(_))), "taken");

    let mut s = running();
    with_steer(&mut s, 1);
    let (mut d, mut rx) = idle(s);
    d.send("again");
    assert_eq!(
        drain(&mut rx),
        [
            Update::SteerRefused("more".into()),
            Update::Error("agy is not running".into()),
            Update::Final(String::new())
        ]
    );
}

#[test]
fn a_terminal_only_steer_goes_back_to_the_queue() {
    let (mut d, mut rx) = idle(running());
    assert!(d.steer("/usage\n\nAttached files:\n- /x/a.png (image/png)".into(), vec![]));
    assert_eq!(
        drain(&mut rx),
        [
            Update::SteerRefused("/usage".into()),
            Update::Warning("`/usage` only works in the agy terminal".into())
        ]
    );
    assert!(d.state.lock().unwrap().active);
}

#[test]
fn stopping_a_turn_agy_started_on_its_own_stops_agy() {
    let (mut d, mut rx) = idle(State { active: true, live: true, ..State::default() });
    d.child = Some(std::process::Command::new("sleep").arg("5").spawn().unwrap());
    d.cancel();
    let s = d.state.lock().unwrap();
    assert!(!s.live && s.active, "signalled, ends when agy exits");
    drop(s);
    assert!(drain(&mut rx).is_empty());
    if let Some(mut child) = d.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[test]
fn denials_before_a_steer_still_offer_the_rerun_when_the_turn_ends() {
    let conv = "704258e6-6fe4-41f6-9b50-390618fa3ab0";
    let mut s = State { conversation: conv.into(), ..running() };
    with_steer(&mut s, 1);
    let out = feed(&mut s, DENIED_TURN);
    assert!(!out.iter().any(|u| matches!(u, Update::Ask { .. } | Update::Final(_))), "{out:?}");
    let done = json!({"event":"result","result":{"conversation_id":conv,"status":"SUCCESS","response":"ok"}}).to_string();
    let out = feed(&mut s, &done);
    assert!(
        out.iter().any(|u| matches!(u, Update::Ask { ask: Ask::Approval(a), .. } if a.input.contains("touch denied2.txt"))),
        "{out:?}"
    );
}

fn lead_opts(access: AccessMode) -> DriverStartOptions {
    let lead = crate::LeadTools {
        exe: "/x/aether".into(),
        sock: "/s/o.sock".into(),
        token: "tok123".into(),
        specs: Value::Null,
    };
    DriverStartOptions { lead: Some(lead), ..opts("", "", access) }
}

#[test]
fn lead_launch_allows_aether_mcp_without_token_in_args() {
    for access in [AccessMode::Ask, AccessMode::AutoEdit, AccessMode::Auto, AccessMode::ReadOnly] {
        let o = lead_opts(access);
        let args = launch_args(&o, "", false);
        let at = args.iter().position(|a| a == "--settings").expect("--settings");
        assert_eq!(args[at + 1], r#"{"permissions":{"allow":["mcp(aether/*)"]}}"#);
        assert_eq!(args.iter().filter(|a| *a == "--settings").count(), 1);
        assert!(!args.iter().any(|a| a.contains("tok123")), "token stays out of argv");
        assert_eq!(
            spawn_env(&o),
            vec![
                ("AETHER_ORCH_SOCK".to_string(), "/s/o.sock".to_string()),
                ("AETHER_ORCH_TOKEN".to_string(), "tok123".to_string()),
            ]
        );
    }
}

#[test]
fn full_mode_lead_adds_no_settings() {
    let o = lead_opts(AccessMode::Full);
    let args = launch_args(&o, "", true);
    assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
    assert!(!args.contains(&"--settings".to_string()));
    let rerun = launch_args(&lead_opts(AccessMode::AutoEdit), "", true);
    assert!(!rerun.contains(&"--settings".to_string()));
    assert_eq!(spawn_env(&o).len(), 2);
}

#[test]
fn non_lead_launch_is_unchanged() {
    for access in AccessMode::ALL {
        for full in [false, true] {
            let o = opts("m", "", access);
            assert!(!launch_args(&o, "c", full).contains(&"--settings".to_string()));
            assert!(spawn_env(&o).is_empty());
        }
    }
}

#[test]
fn call_mcp_tool_for_aether_renders_inner_tool() {
    let row = |server: &str| {
        let step = json!({"event": "step_update", "step_update": {"step_index": 3, "state": "ACTIVE",
            "step_type": "tool", "tool_name": "call_mcp_tool", "tool_info": {"name": "call_mcp_tool",
            "parameters": {"ServerName": server, "ToolName": "spawn_agent", "Arguments": {"task": "x"}}}}});
        let mut s = State { conversation: "c".into(), ..running() };
        feed(&mut s, &step.to_string())
            .into_iter()
            .find_map(|u| match u {
                Update::Tool(call) => Some(call.detail),
                _ => None,
            })
            .unwrap()
    };
    let ToolDetail::Other { name, input, .. } = row("aether") else { panic!() };
    assert_eq!(name, "spawn_agent");
    assert!(input.contains(r#""task""#), "{input}");
    let ToolDetail::Other { name, .. } = row("gh") else { panic!() };
    assert_eq!(name, "call_mcp_tool");
}

#[test]
fn call_mcp_tool_without_a_tool_name_stays_call_mcp_tool() {
    let step = json!({"event": "step_update", "step_update": {"step_index": 3, "state": "ACTIVE",
        "step_type": "tool", "tool_name": "call_mcp_tool", "tool_info": {"name": "call_mcp_tool",
        "parameters": {"ServerName": "aether", "ToolName": 7}}}});
    let mut s = State { conversation: "c".into(), ..running() };
    let out = feed(&mut s, &step.to_string());
    let Some(Update::Tool(call)) = out.iter().find(|u| matches!(u, Update::Tool(_))) else {
        panic!("{out:?}")
    };
    let ToolDetail::Other { name, input, .. } = &call.detail else { panic!() };
    assert_eq!(name, "call_mcp_tool");
    assert!(input.contains("ServerName"), "{input}");
}

#[cfg(unix)]
fn script_agy(name: &str, body: &str) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("aether-script-agy-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (path, log) = (dir.join("agy"), dir.join("argv"));
    let _ = std::fs::remove_file(&log);
    std::fs::write(&path, format!("#!/bin/sh\necho \"$@\" >> '{}'\n{body}\n", log.display()))
        .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    (path, log)
}

fn agy_config(name: &str, body: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aether-agy-config-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mcp_config.json");
    let _ = std::fs::remove_file(&path);
    if let Some(body) = body {
        std::fs::write(&path, body).unwrap();
    }
    path
}

#[test]
fn agy_aether_entry_reads_only_the_aether_server() {
    let entry = |name, body| agy_aether_entry(&agy_config(name, body));
    assert_eq!(entry("missing", None), None);
    assert_eq!(entry("empty", Some("")), None);
    assert_eq!(entry("cleared", Some(r#"{"mcpServers": {}}"#)), None);
    let other =
        r#"{"mcpServers": {"fs": {"command": "/x/aether/bin/fs", "args": ["/x/aether/bin"]}}}"#;
    assert_eq!(entry("other", Some(other)), None);
    let ours = r#"{"mcpServers": {"aether": {"command": "/x/aether", "args": ["mcp"]}}}"#;
    assert_eq!(
        entry("ours", Some(ours)),
        Some(AgyEntry { command: "/x/aether".into(), disabled: false })
    );
    let off = r#"{"mcpServers": {"aether": {"command": "/x/aether", "args": ["mcp"], "disabled": true}}}"#;
    assert_eq!(entry("off", Some(off)).map(|e| e.disabled), Some(true));
}

#[cfg(unix)]
#[test]
fn register_agy_aether_adds_without_env() {
    let (bin, log) = script_agy("add", "true");
    register_agy_aether(bin.to_str().unwrap(), Path::new("/x/aether")).unwrap();
    assert_eq!(std::fs::read_to_string(log).unwrap(), "mcp add aether /x/aether mcp\n");
    let (bin, _) = script_agy("fail", "echo 'bad config' >&2; exit 2");
    let err = register_agy_aether(bin.to_str().unwrap(), Path::new("/x/aether")).unwrap_err();
    assert!(err.to_string().contains("bad config"), "{err}");
    assert!(register_agy_aether("/nonexistent/agy", Path::new("/x/aether")).is_err());
}

#[test]
fn forget_conversation_removes_only_that_conversation() {
    let root = std::env::temp_dir().join(format!("aether-agy-forget-{}", std::process::id()));
    let (id, keep) = ("c2f4e33d-4134", "aaaa-bbbb");
    for dir in ["conversations", "annotations", "presence", "brain"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::create_dir_all(root.join("brain").join(id)).unwrap();
    for name in [id, keep] {
        std::fs::write(root.join(format!("conversations/{name}.db")), "x").unwrap();
    }
    std::fs::write(root.join(format!("annotations/{id}.pbtxt")), "x").unwrap();
    forget_conversation(&root, id);
    forget_conversation(&root, "../conversations");
    assert!(!root.join(format!("conversations/{id}.db")).exists());
    assert!(!root.join(format!("annotations/{id}.pbtxt")).exists());
    assert!(!root.join("brain").join(id).exists());
    assert!(root.join(format!("conversations/{keep}.db")).exists());
    std::fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn an_oversized_stream_line_warns_and_the_next_event_still_maps() {
    let script = format!(
        "echo '{INIT}'; head -c {} /dev/zero | tr '\\0' x; echo; \
         echo '{{\"event\":\"step_update\",\"step_update\":{{\"step_index\":1,\"state\":\"DONE\",\"step_type\":\"agent_response\",\"text_delta\":\"after\"}}}}'",
        crate::lines::MAX_LINE + 1000
    );
    let mut child = std::process::Command::new("sh")
        .args(["-c", &script])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let state = Arc::new(Mutex::new(State { active: true, ..State::default() }));
    read(child.stdout.take().unwrap(), 0, &state, &tx, None, None);
    child.wait().unwrap();
    let out = drain(&mut rx);
    assert!(out.iter().any(|u| matches!(u, Update::Warning(w) if w.contains("16 MB"))), "{out:?}");
    assert!(delta(&out).contains("after"), "{out:?}");
}

#[test]
fn completed_user_input_accepts_prompt_once() {
    let mut s = State::default();
    on_prompt(&mut s, "hello");
    let v: Value = serde_json::from_str(USER_INPUT).unwrap();
    assert_eq!(map_event(&mut s, &v), [Update::Accepted]);
    assert_eq!(map_event(&mut s, &v), []);
}
