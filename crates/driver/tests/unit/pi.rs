use super::*;
use crate::record::fixtures;
use aether_core::{FieldKind, Form, NoticeKind, NoticeState, ToolDetail, ToolStatus};

const SETTLED: &str = r#"{"type":"agent_settled","aborted":false}"#;

fn ev(s: &mut State, line: &str) -> Vec<Update> {
    map_event(s, &serde_json::from_str(line).unwrap())
}

fn replay(scenario: &str) -> Vec<Update> {
    let values: Vec<Value> = crate::record::fixture("pi", scenario)
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let mut s = State::default();
    let mut out = Vec::new();
    for (i, v) in values.iter().enumerate() {
        let next_id = || {
            let reply = values[i..].iter().find(|v| v["type"] == "response");
            reply.and_then(|v| v["id"].as_str()).unwrap_or_default().to_string()
        };
        match v["aether"].as_str() {
            Some("prompt") => on_prompt(&mut s, &next_id()),
            Some("steer") => {
                let text = v["text"].as_str().unwrap();
                if on_steer(&mut s, text) {
                    s.steer_acks.insert(next_id(), without_attached_files(text).to_string());
                }
            }
            Some("cancel") => {
                on_cancel(&mut s);
            }
            Some("answer") => {
                let answer = serde_json::from_value(v["answer"].clone()).unwrap();
                reply(&mut s, v["id"].as_u64().unwrap(), answer);
            }
            _ => out.extend(map_event(&mut s, v)),
        }
    }
    out
}

fn user_start(text: &str) -> String {
    json!({"type":"message_start","message":{"role":"user","content":[{"type":"text","text":text}]}}).to_string()
}

fn bash(output: &str, status: ToolStatus, ms: Option<u64>) -> Update {
    let detail = ToolDetail::Command {
        command: "ls -la".into(),
        cwd: None,
        output: output.into(),
        exit_code: None,
    };
    Update::Tool(ToolCall { status, ms, ..ToolCall::new("call_1", "Run command", detail) })
}

#[test]
fn turn_streams_text_and_tools_then_one_final() {
    let mut s = State { active: true, ..State::default() };
    let delta =
        r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"he"}}"#;
    assert_eq!(ev(&mut s, delta), [Update::Delta("he".into())]);
    let start = r#"{"type":"tool_execution_start","toolCallId":"call_1","toolName":"bash","args":{"command":"ls -la"}}"#;
    assert_eq!(ev(&mut s, start), [bash("", ToolStatus::Running, None)]);
    let partial = r#"{"type":"tool_execution_update","toolCallId":"call_1","toolName":"bash","args":{"command":"ls -la"},"partialResult":{"content":[{"type":"text","text":"partial output"}],"details":{}}}"#;
    assert_eq!(ev(&mut s, partial), [bash("partial output", ToolStatus::Running, None)]);
    let end = r#"{"type":"tool_execution_end","toolCallId":"call_1","toolName":"bash","result":{"content":[{"type":"text","text":"x"}]},"isError":false,"durationMs":12}"#;
    assert_eq!(ev(&mut s, end), [bash("x", ToolStatus::Done, Some(12))]);
    assert_eq!(ev(&mut s, partial), []);
    assert_eq!(ev(&mut s, SETTLED), [Update::Final(String::new())]);
    assert_eq!(ev(&mut s, SETTLED), []);
    s.active = true;
    for line in [delta, start, end, delta] {
        ev(&mut s, line);
    }
    assert_eq!(ev(&mut s, SETTLED), [Update::Final(String::new())], "the deltas already said it");
    let orphan = r#"{"type":"tool_execution_end","toolCallId":"call_9","toolName":"read","result":{"content":[{"type":"text","text":"ENOENT: no such file"}]},"isError":true}"#;
    let failed = ToolCall {
        status: ToolStatus::Failed,
        error: Some("ENOENT: no such file".into()),
        ..ToolCall::new(
            "call_9",
            "Read file",
            ToolDetail::Read { path: String::new(), range: None },
        )
    };
    assert_eq!(ev(&mut s, orphan), [Update::Tool(failed)]);
}

#[test]
fn tools_cut_by_a_cancel_are_stopped() {
    let mut s = State { active: true, cancelled: true, ..State::default() };
    ev(
        &mut s,
        r#"{"type":"tool_execution_start","toolCallId":"c","toolName":"bash","args":{"command":"sleep 9"}}"#,
    );
    let end = r#"{"type":"tool_execution_end","toolCallId":"c","toolName":"bash","result":{"content":[{"type":"text","text":"aborted"}]},"isError":true}"#;
    let out = ev(&mut s, end);
    assert!(
        matches!(out.as_slice(), [Update::Tool(call)] if call.status == ToolStatus::Stopped),
        "{out:?}"
    );
}

const HANDLED: &str = r#"{"type":"response","id":"aether-1","command":"prompt","success":true,"data":{"disposition":"handled"}}"#;

#[test]
fn handled_prompt_waits_for_a_run_an_extension_starts() {
    let mut s = State { active: true, prompt: "aether-1".into(), ..State::default() };
    assert_eq!(ev(&mut s, HANDLED), []);
    assert!(s.grace && s.active);
    assert_eq!(ev(&mut s, r#"{"type":"agent_start"}"#), []);
    assert!(!s.grace);
    assert_eq!(ev(&mut s, SETTLED), [Update::Final(String::new())]);
    let late = ev(&mut s, r#"{"type":"agent_start"}"#);
    assert!(matches!(&late[..], [Update::Wake, Update::Notice(_)]), "{late:?}");
    s.active = true;
    s = State { active: true, running: true, prompt: "aether-1".into(), ..State::default() };
    assert_eq!(ev(&mut s, HANDLED), []);
    assert!(!s.grace);
    assert_eq!(ev(&mut s, SETTLED), [Update::Final(String::new())]);
}

#[test]
fn settle_waits_for_an_unacked_prompt() {
    let mut s =
        State { active: true, running: true, prompt: "aether-2".into(), ..State::default() };
    assert_eq!(ev(&mut s, SETTLED), []);
    let started = r#"{"type":"response","id":"aether-2","command":"prompt","success":true,"data":{"disposition":"started"}}"#;
    assert_eq!(ev(&mut s, started), []);
    ev(&mut s, r#"{"type":"agent_start"}"#);
    assert_eq!(ev(&mut s, SETTLED), [Update::Final(String::new())]);
    let untracked = r#"{"type":"response","command":"clear_queue","success":true,"data":{"steering":["x"],"followUp":[]}}"#;
    s.active = true;
    assert_eq!(ev(&mut s, untracked), []);
    assert_eq!(ev(&mut s, SETTLED), [Update::Final(String::new())]);
}

#[test]
fn rejected_prompt_errors_then_settles() {
    let mut s = State { active: true, prompt: "aether-1".into(), ..State::default() };
    let rejected =
        r#"{"type":"response","id":"aether-1","command":"prompt","success":false,"error":"busy"}"#;
    assert_eq!(ev(&mut s, rejected), [Update::Error("busy".into()), Update::Final(String::new())]);
    assert_eq!(ev(&mut s, SETTLED), []);
    s = State { active: true, running: true, prompt: "aether-1".into(), ..State::default() };
    assert_eq!(ev(&mut s, rejected), [Update::Error("busy".into())]);
    assert_eq!(ev(&mut s, SETTLED), [Update::Final(String::new())]);
}

#[test]
fn images_ride_prompt_and_steer_as_base64_image_content() {
    let path = std::env::temp_dir().join(format!("aether-pi-img-{}.png", std::process::id()));
    std::fs::write(&path, b"png").unwrap();
    let img =
        Attachment { name: "a.png".into(), path: path.clone(), mime: "image/png".into(), size: 3 };
    let image = json!([{"type": "image", "data": "cG5n", "mimeType": "image/png"}]);
    let cmd = prompt_cmd("aether-1", "look", std::slice::from_ref(&img));
    assert_eq!(cmd["images"], image);
    assert_eq!(cmd["streamingBehavior"], "followUp");
    let cmd = message_cmd("aether-2", "look", "steer", std::slice::from_ref(&img));
    assert_eq!((cmd["type"].as_str(), cmd["images"].clone()), (Some("prompt"), image));
    assert_eq!(cmd["streamingBehavior"], "steer");
    assert!(prompt_cmd("aether-3", "hi", &[])["images"].is_null());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn compact_command_settles_on_its_reply() {
    assert_eq!(
        prompt_cmd("aether-1", "/compact", &[]),
        json!({"id": "aether-1", "type": "compact"})
    );
    assert_eq!(
        prompt_cmd("aether-1", "/compact  keep the API notes ", &[]),
        json!({"id": "aether-1", "type": "compact", "customInstructions": "keep the API notes"})
    );
    for text in ["/compactor", "/skill:review x", "hi"] {
        let cmd = prompt_cmd("aether-1", text, &[]);
        assert_eq!((cmd["type"].as_str(), cmd["message"].as_str()), (Some("prompt"), Some(text)));
        assert_eq!(cmd["streamingBehavior"], "followUp");
    }
    let mut s = State { active: true, direct: true, prompt: "aether-1".into(), ..State::default() };
    let begin = r#"{"type":"compaction_start","reason":"manual"}"#;
    let [Update::Notice(n)] = &ev(&mut s, begin)[..] else { panic!() };
    assert_eq!((n.kind, n.state), (NoticeKind::Compaction, NoticeState::Running));
    assert_eq!(n.text, "Compacting context (manual)");
    let failed = r#"{"type":"compaction_end","reason":"manual","aborted":false,"willRetry":false,"errorMessage":"Compaction failed: Nothing to compact (session too small)"}"#;
    let why = "Compaction failed: Nothing to compact (session too small)";
    let [Update::Notice(f)] = &ev(&mut s, failed)[..] else { panic!() };
    assert_eq!(
        (f.id.as_str(), f.state, f.text.as_str()),
        (n.id.as_str(), NoticeState::Failed, why)
    );
    let rejected = r#"{"id":"aether-1","type":"response","command":"compact","success":false,"error":"Nothing to compact (session too small)"}"#;
    assert_eq!(ev(&mut s, rejected), [Update::Final(String::new())], "the row already says why");
    s = State { active: true, prompt: "aether-3".into(), ..State::default() };
    assert_eq!(
        ev(&mut s, &rejected.replace("aether-1", "aether-3")),
        [
            Update::Error("Nothing to compact (session too small)".into()),
            Update::Final(String::new())
        ],
        "with no row the reply carries the error"
    );
    s = State { active: true, direct: true, prompt: "aether-2".into(), ..State::default() };
    ev(&mut s, begin);
    let done = r#"{"type":"compaction_end","reason":"manual","result":{"summary":"s","tokensBefore":150000,"estimatedTokensAfter":32000},"aborted":false,"willRetry":false}"#;
    let out = ev(&mut s, done);
    assert!(matches!(&out[0], Update::Notice(d) if d.state == NoticeState::Done
        && d.text == "Compacted 150000 → ~32000 tokens" && d.detail.as_deref() == Some("s")));
    assert_eq!(out[1], Update::Usage(32000));
    let ok = r#"{"id":"aether-2","type":"response","command":"compact","success":true,"data":{}}"#;
    assert_eq!(ev(&mut s, ok), [Update::Final(String::new())]);
    s = State { active: true, ..State::default() };
    ev(&mut s, begin);
    let stopped = r#"{"type":"compaction_end","reason":"manual","aborted":true,"willRetry":false}"#;
    assert!(
        matches!(&ev(&mut s, stopped)[..], [Update::Notice(n)] if n.text == "Compaction stopped")
    );
}

#[test]
fn side_errors_and_titles_surface() {
    let mut s = State { active: true, ..State::default() };
    let thrown = r#"{"type":"extension_error","extensionPath":"/x/guard.ts","event":"tool_call","error":"boom"}"#;
    assert_eq!(ev(&mut s, thrown), [Update::Warning("/x/guard.ts: boom".into())]);
    let notify = |kind: &str| {
        format!(
            r#"{{"type":"extension_ui_request","id":"u","method":"notify","message":"blocked","notifyType":"{kind}"}}"#
        )
    };
    assert_eq!(ev(&mut s, &notify("error")), [Update::Warning("blocked".into())]);
    assert!(matches!(&ev(&mut s, &notify("warning"))[..], [Update::Notice(n)]
        if n.kind == NoticeKind::Info && n.id == "info-notify-1" && n.text == "blocked"));
    let named = r#"{"type":"session_info_changed","name":"Fix login"}"#;
    assert_eq!(ev(&mut s, named), [Update::Title("Fix login".into())]);
    assert_eq!(ev(&mut s, r#"{"type":"session_info_changed"}"#), []);
}

#[test]
fn message_end_gap_fills_text_and_carries_errors() {
    let mut s = State { active: true, window: Some(262_144), ..State::default() };
    let end = r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"ok"}],"stopReason":"stop","usage":{"input":16008,"output":628,"cacheRead":80,"cacheWrite":0,"reasoning":625,"totalTokens":16716}}}"#;
    let count = Update::Count { prompt: 16088, completion: 628, cached: 80 };
    let window = Update::Window(262_144);
    assert_eq!(ev(&mut s, end), [Update::Delta("ok".into()), window, Update::Usage(16716), count]);
    assert_eq!(ev(&mut s, SETTLED), [Update::Final(String::new())]);
    s = State { active: true, ..State::default() };
    let untotalled = r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"stop","usage":{"input":7,"output":3,"cacheRead":2,"cacheWrite":1}}}"#;
    let count = Update::Count { prompt: 10, completion: 3, cached: 2 };
    assert_eq!(ev(&mut s, untotalled), [Update::Usage(13), count]);
    s.active = true;
    let failed = r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"boom"}}"#;
    ev(&mut s, failed);
    assert_eq!(ev(&mut s, SETTLED), [Update::Error("boom".into()), Update::Final(String::new())]);
}

fn form(u: &Update) -> &Form {
    match u {
        Update::Ask { ask: Ask::Form(f), .. } => f,
        other => panic!("{other:?}"),
    }
}

#[test]
fn dialogs_ask_and_replies_echo_request_id() {
    let mut s = State::default();
    let confirm = r#"{"type":"extension_ui_request","id":"uuid-2","method":"confirm","title":"Clear session?","message":"All messages will be lost."}"#;
    let out = ev(&mut s, confirm);
    let f = form(&out[0]);
    assert_eq!(
        (f.title.as_str(), f.message.as_str()),
        ("Clear session?", "All messages will be lost.")
    );
    assert!(f.fields.is_empty());
    assert_eq!(
        reply(&mut s, 1, Answer::Form(vec![])),
        Some(json!({"type": "extension_ui_response", "id": "uuid-2", "confirmed": true}))
    );
    ev(&mut s, &confirm.replace("uuid-2", "uuid-9"));
    assert_eq!(
        reply(&mut s, 2, Answer::Deny),
        Some(json!({"type": "extension_ui_response", "id": "uuid-9", "confirmed": false}))
    );
    let select = r#"{"type":"extension_ui_request","id":"uuid-1","method":"select","title":"Allow dangerous command?","options":["Allow","Block"]}"#;
    let out = ev(&mut s, select);
    let f = form(&out[0]);
    assert_eq!(f.title, "Allow dangerous command?");
    assert_eq!(f.fields[0].kind, FieldKind::Choice);
    let labels: Vec<&str> = f.fields[0].options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["Allow", "Block"]);
    assert_eq!(
        reply(&mut s, 3, Answer::Form(vec![("value".into(), vec!["Block".into()])])),
        Some(json!({"type": "extension_ui_response", "id": "uuid-1", "value": "Block"}))
    );
    assert_eq!(reply(&mut s, 3, Answer::Deny), None);
    ev(&mut s, select);
    assert_eq!(
        reply(&mut s, 4, Answer::Deny),
        Some(json!({"type": "extension_ui_response", "id": "uuid-1", "cancelled": true}))
    );
    let notify =
        r#"{"type":"extension_ui_request","id":"uuid-5","method":"notify","message":"hi"}"#;
    assert!(matches!(&ev(&mut s, notify)[..], [Update::Notice(n)] if n.text == "hi"));
    let input = r#"{"type":"extension_ui_request","id":"uuid-3","method":"input","title":"Enter a value","placeholder":"type something...","timeout":10000}"#;
    let out = ev(&mut s, input);
    let f = form(&out[0]);
    assert_eq!((f.title.as_str(), f.timeout_ms), ("Enter a value", Some(10000)));
    assert_eq!(f.fields[0].kind, FieldKind::Text);
    assert_eq!(f.fields[0].label, "type something...");
    assert_eq!(
        reply(&mut s, 5, Answer::Form(vec![("value".into(), vec!["hello".into()])])),
        Some(json!({"type": "extension_ui_response", "id": "uuid-3", "value": "hello"}))
    );
    let editor = r#"{"type":"extension_ui_request","id":"uuid-4","method":"editor","title":"Edit some text","prefill":"Line 1"}"#;
    let out = ev(&mut s, editor);
    assert_eq!(form(&out[0]).fields[0].default, ["Line 1"]);
    assert_eq!(
        reply(&mut s, 6, Answer::Deny),
        Some(json!({"type": "extension_ui_response", "id": "uuid-4", "cancelled": true}))
    );
}

fn opts_for_test() -> DriverStartOptions {
    DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: "/tmp".into(),
        model: String::new(),
        effort: "high".into(),
        access: AccessMode::ReadOnly,
        resume: String::new(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    }
}

#[test]
fn read_only_args_and_model_split() {
    let opts = opts_for_test();
    assert_eq!(
        launch_args(&opts, false),
        ["--mode", "rpc", "--tools", "read,grep,find,ls", "--exclude-tools", "mcp__*"]
    );
    assert_eq!(
        launch_args(&DriverStartOptions { access: AccessMode::Full, ..opts.clone() }, false),
        ["--mode", "rpc"]
    );
    let side = DriverStartOptions { access: AccessMode::Full, ephemeral: true, ..opts };
    assert_eq!(launch_args(&side, false), ["--mode", "rpc", "--no-session"]);
    let m = set_model("openrouter/z-ai/glm-5");
    assert_eq!(
        (m["provider"].as_str(), m["modelId"].as_str()),
        (Some("openrouter"), Some("z-ai/glm-5"))
    );
    assert_eq!(m["type"], "set_model");
}

fn lead_opts(access: AccessMode) -> DriverStartOptions {
    let lead = crate::LeadTools {
        exe: "/x/aether".into(),
        sock: "/s/o.sock".into(),
        token: "tok123".into(),
        specs: json!([{"name": "list_agents"}, {"name": "spawn_agent"}]),
    };
    DriverStartOptions { lead: Some(lead), access, ..opts_for_test() }
}

fn lead_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aether-pi-lead-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn lead_launch_loads_aether_extension() {
    let o = lead_opts(AccessMode::Full);
    let dir = lead_dir("launch");
    let ext = lead_extension_in(&dir).unwrap();
    let args = launch_args_with(&o, false, Some(&ext));
    let at = args.iter().position(|a| a == "-e").expect("-e flag");
    assert_eq!(Path::new(&args[at + 1]), dir.join("pi-lead.ts"));
    assert_eq!(std::fs::read_to_string(&ext).unwrap(), LEAD_EXTENSION);
    #[cfg(unix)]
    assert_eq!(mode(&ext), 0o600);
    assert!(!args.iter().any(|a| a.contains("tok123")), "token stays out of argv");
    let env: HashMap<String, String> = spawn_env(&o).into_iter().collect();
    assert_eq!(env.len(), 3);
    assert_eq!(env["AETHER_ORCH_SOCK"], "/s/o.sock");
    assert_eq!(env["AETHER_ORCH_TOKEN"], "tok123");
    let specs: Value = serde_json::from_str(&env["AETHER_ORCH_SPECS"]).unwrap();
    assert_eq!(specs, o.lead.unwrap().specs);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lead_extension_overwrite_is_whole_and_private() {
    let dir = lead_dir("overwrite");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pi-lead.ts");
    std::fs::write(&path, "stale").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    assert_eq!(lead_extension_in(&dir).unwrap(), path);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LEAD_EXTENSION);
    #[cfg(unix)]
    assert_eq!(mode(&path), 0o600);
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names, ["pi-lead.ts"], "no leftover tmp file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_only_lead_allowlists_aether_tools() {
    let ext = Path::new("/x/pi-lead.ts");
    let args = launch_args_with(&lead_opts(AccessMode::ReadOnly), false, Some(ext));
    let at = args.iter().position(|a| a == "--tools").expect("--tools flag");
    assert_eq!(args[at + 1], "read,grep,find,ls,aether_list_agents,aether_spawn_agent");
    assert!(args.windows(2).any(|w| w[0] == "--exclude-tools" && w[1] == "mcp__*"));
}

#[test]
fn non_lead_launch_is_unchanged() {
    let ro = opts_for_test();
    assert_eq!(
        launch_args(&ro, false),
        ["--mode", "rpc", "--tools", "read,grep,find,ls", "--exclude-tools", "mcp__*"]
    );
    let full = DriverStartOptions { access: AccessMode::Full, ..ro };
    assert_eq!(launch_args(&full, false), ["--mode", "rpc"]);
    assert!(spawn_env(&full).is_empty());
}

fn turn(
    d: &mut PiDriver,
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

fn session(updates: &[Update]) -> Option<String> {
    updates.iter().find_map(|u| match u {
        Update::Session(file) => Some(file.clone()),
        _ => None,
    })
}

#[test]
#[ignore]
fn live_pi_smoke() {
    let dir = std::env::temp_dir().join(format!("aether-pi-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let opts = |resume: &str| DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: dir.clone(),
        model: std::env::var("AETHER_PI_SMOKE_MODEL").unwrap_or_default(),
        effort: String::new(),
        access: AccessMode::ReadOnly,
        resume: resume.into(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut d = PiDriver::spawn(opts(""), tx);
    assert!(!d.resumed());
    println!("-- turn 1");
    d.prompt("Reply with exactly: ok".into(), vec![]);
    let first = turn(&mut d, &mut rx, false);
    let file = session(&first).expect("session file");
    assert!(text(&first).contains("ok"));
    drop(d);

    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut d = PiDriver::spawn(opts(&file), tx);
    assert!(d.resumed());
    println!("-- turn 2 (resumed {file})");
    d.prompt("Reply with exactly: again".into(), vec![]);
    let second = turn(&mut d, &mut rx, false);
    assert_eq!(session(&second).as_deref(), Some(file.as_str()));
    assert!(text(&second).contains("again"));
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
fn every_fixture_replays_without_panicking() {
    let all = fixtures("pi");
    assert!(!all.is_empty());
    for (name, _) in all {
        let out = replay(&name);
        assert!(!out.is_empty(), "{name} produced no updates");
    }
}

#[test]
fn steers_are_marked_when_pi_takes_them() {
    let mut s = State { active: true, ..State::default() };
    on_steer(&mut s, "also dark mode");
    on_steer(&mut s, "and tests");
    assert_eq!(
        ev(&mut s, &user_start("also dark mode")),
        [Update::Steered("also dark mode".into())]
    );
    let queue =
        |items: &[&str]| json!({"type":"queue_update","steering":items,"followUp":[]}).to_string();
    assert_eq!(ev(&mut s, &queue(&["and tests"])), []);
    assert_eq!(ev(&mut s, &queue(&[])), [Update::Steered("and tests".into())]);
    assert_eq!(ev(&mut s, &user_start("and tests")), []);
}

#[test]
fn cancel_closes_open_dialogs_first() {
    let mut s = State { active: true, ..State::default() };
    let editor = r#"{"type":"extension_ui_request","id":"ui-1","method":"editor","title":"Edit","prefill":"x"}"#;
    let [Update::Ask { id, .. }] = ev(&mut s, editor)[..] else { panic!() };
    let (lines, updates) = cancel_lines(&mut s, "aether-9");
    let kinds: Vec<&str> = lines.iter().map(|l| l["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["extension_ui_response", "clear_queue", "abort"]);
    assert_eq!(lines[0], json!({"type":"extension_ui_response","id":"ui-1","cancelled":true}));
    assert_eq!(updates, [Update::AskResolved(id)]);
}

#[test]
fn cleared_steers_return_to_the_queue() {
    let mut s = State { active: true, clearing: "aether-9".into(), ..State::default() };
    on_steer(&mut s, "also x");
    let cleared = r#"{"type":"response","id":"aether-9","command":"clear_queue","success":true,"data":{"steering":["also x"],"followUp":[]}}"#;
    assert_eq!(ev(&mut s, cleared), [Update::SteerRefused("also x".into())]);
    assert!(s.steers.is_empty());
}

#[test]
fn dialog_timeouts_resolve_the_ask() {
    let mut s = State { active: true, ..State::default() };
    let select = r#"{"type":"extension_ui_request","id":"ui-2","method":"select","title":"Pick","options":["a","b"],"timeout":5000}"#;
    let [Update::Ask { id, ask: Ask::Form(f) }] = &ev(&mut s, select)[..] else { panic!() };
    assert_eq!(f.timeout_ms, Some(5000));
    assert_eq!(expire_dialog(&mut s, *id), Some(Update::AskResolved(*id)));
    assert_eq!(expire_dialog(&mut s, *id), None);
}

#[test]
fn runs_pi_starts_alone_wake_the_chat() {
    let mut s = State::default();
    assert!(
        matches!(&ev(&mut s, r#"{"type":"agent_start"}"#)[..], [Update::Wake, Update::Notice(n)] if n.kind == NoticeKind::Wake)
    );
    assert!(s.active);
}

#[test]
fn stderr_warns_instead_of_failing() {
    let line = "Warning: Extension \"x\" project_trust error: boom";
    assert_eq!(stderr_update(line), Some(Update::Warning(line.into())));
    assert_eq!(stderr_update("debug: cache warm"), None);
}

#[test]
fn launch_resumes_by_session_file_and_can_approve() {
    let file = std::env::temp_dir().join(format!("aether-pi-{}.jsonl", std::process::id()));
    std::fs::write(&file, "").unwrap();
    let opts = DriverStartOptions { resume: file.to_string_lossy().into(), ..opts_for_test() };
    let args = launch_args(&opts, true);
    assert!(args.windows(2).any(|w| w == ["--session", opts.resume.as_str()]));
    assert!(args.contains(&"--approve".to_string()));
    let none = launch_args(&DriverStartOptions { resume: "/nope.jsonl".into(), ..opts }, false);
    assert!(!none.iter().any(|a| a == "--session" || a == "--approve"));
    std::fs::remove_file(file).unwrap();
}

#[test]
fn untrusted_projects_offer_trust() {
    let dir = std::env::temp_dir().join(format!("aether-pi-trust-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".pi")).unwrap();
    assert!(!untrusted_project(&dir, &json!({}), "ask"), "a bare .pi needs no trust");
    std::fs::create_dir_all(dir.join(".pi/extensions")).unwrap();
    assert!(untrusted_project(&dir, &json!({}), "ask"));
    let canonical = aether_core::canonicalize(&dir).unwrap().to_string_lossy().into_owned();
    assert!(!untrusted_project(&dir, &json!({ canonical: true }), "ask"));
    assert!(!untrusted_project(&dir, &json!({}), "always"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn steer_fixture_marks_the_steer() {
    assert!(replay("steer").iter().any(|u| matches!(u, Update::Steered(_))));
}

fn last_calls(out: &[Update]) -> Vec<&ToolCall> {
    out.iter().filter_map(|u| if let Update::Tool(c) = u { Some(c) } else { None }).collect()
}

fn upd(event: Value) -> String {
    json!({"type":"message_update","assistantMessageEvent": event}).to_string()
}

#[test]
fn tool_input_streams_before_execution() {
    let mut s = State { active: true, ..State::default() };
    ev(
        &mut s,
        &upd(json!({"type":"toolcall_start","contentIndex":1,"id":"tc1","toolName":"bash"})),
    );
    let part = ev(
        &mut s,
        &upd(json!({"type":"toolcall_delta","contentIndex":1,"delta":"{\"command\":\"ls"})),
    );
    assert!(part.is_empty());
    let out = ev(&mut s, &upd(json!({"type":"toolcall_delta","contentIndex":1,"delta":" -la\"}"})));
    assert!(
        matches!(&last_calls(&out)[..], [c] if matches!(&c.detail, ToolDetail::Command { command, .. } if command == "ls -la"))
    );
    let end = upd(
        json!({"type":"toolcall_end","contentIndex":1,"toolCall":{"id":"tc1","name":"bash","arguments":{"command":"ls -lah"}}}),
    );
    let out = ev(&mut s, &end);
    assert!(
        matches!(&last_calls(&out)[..], [c] if matches!(&c.detail, ToolDetail::Command { command, .. } if command == "ls -lah"))
    );
}

#[test]
fn details_parents_and_images_are_kept() {
    let mut s = State { active: true, ..State::default() };
    let end = json!({"type":"tool_execution_end","toolCallId":"p/1","toolName":"edit","parentToolCallId":"p","isError":false,
        "result":{"content":[{"type":"text","text":"ok"}],"details":{"patch":"--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b"}}}).to_string();
    let [Update::Tool(c)] = &ev(&mut s, &end)[..] else { panic!() };
    assert_eq!(c.parent.as_deref(), Some("p"));
    assert!(matches!(&c.detail, ToolDetail::Edit { added: 1, removed: 1, .. }));
    let bash = |details: Value| {
        json!({"type":"tool_execution_end","toolCallId":"b","toolName":"bash","isError":false,
        "result":{"content":[{"type":"text","text":"tail"}],"details":details}})
        .to_string()
    };
    let cut = json!({"truncated":true,"outputLines":2000,"totalLines":9000});
    let with_path = bash(json!({"truncation":cut,"fullOutputPath":"/tmp/pi-out.txt"}));
    let [Update::Tool(c)] = &ev(&mut s, &with_path)[..] else { panic!() };
    assert!(
        matches!(&c.detail, ToolDetail::Command { output, .. } if output.ends_with("Showing 2000 of 9000 lines. Full output: /tmp/pi-out.txt"))
    );
    let [Update::Tool(c)] = &ev(&mut s, &bash(json!({"truncation":cut})))[..] else { panic!() };
    assert!(
        matches!(&c.detail, ToolDetail::Command { output, .. } if output.ends_with("Showing 2000 of 9000 lines."))
    );
    let dir = std::env::temp_dir().join(format!("aether-pi-shots-{}", std::process::id()));
    s.attachments = Some(dir.clone());
    let shot =
        json!({"type":"tool_execution_end","toolCallId":"r","toolName":"read","isError":false,
        "result":{"content":[{"type":"image","data":"cG5n","mimeType":"image/png"}]}})
        .to_string();
    let [Update::Tool(c)] = &ev(&mut s, &shot)[..] else { panic!() };
    assert_eq!(c.images.len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn message_end_reports_cost_length_and_model() {
    let mut s = State { active: true, ..State::default() };
    let end = json!({"type":"message_end","message":{"role":"assistant","content":[],"model":"glm-5","stopReason":"length",
        "usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":2,"cost":{"total":0.0042}}}}).to_string();
    let out = ev(&mut s, &end);
    assert!(out.contains(&Update::Cost(4_200)) && out.contains(&Update::Model("glm-5".into())));
    assert!(out.iter().any(|u| matches!(u, Update::Warning(_))));
    let again = ev(&mut s, &end);
    assert!(!again.iter().any(|u| matches!(u, Update::Model(_))), "same model is reported once");
}

#[test]
fn bash_and_custom_messages_become_rows_and_notices() {
    let mut s = State { active: true, ..State::default() };
    let run = json!({"type":"message_end","message":{"role":"bashExecution","command":"ls","output":"a","exitCode":2}}).to_string();
    let [Update::Tool(c)] = &ev(&mut s, &run)[..] else { panic!() };
    assert_eq!((c.id.as_str(), c.status), ("bash-1", ToolStatus::Failed));
    let custom = json!({"type":"message_end","message":{"role":"custom","display":true,"content":"first\nsecond"}}).to_string();
    assert!(
        matches!(&ev(&mut s, &custom)[..], [Update::Notice(n)] if n.id == "info-custom-1" && n.text == "first")
    );
    let hidden = custom.replace("true", "false");
    assert_eq!(ev(&mut s, &hidden), []);
}

#[test]
fn retries_and_compaction_are_notices() {
    let mut s = State { active: true, ..State::default() };
    let [Update::Notice(r)] = &ev(
        &mut s,
        r#"{"type":"auto_retry_start","attempt":1,"maxAttempts":3,"delayMs":2000,"errorMessage":"overloaded"}"#,
    )[..] else {
        panic!()
    };
    assert_eq!(
        (r.kind, r.state, r.until_ms.is_some()),
        (NoticeKind::Retry, NoticeState::Running, true)
    );
    assert_eq!(r.text, "Retrying (1/3): overloaded");
    assert!(
        matches!(&ev(&mut s, r#"{"type":"auto_retry_end","success":true,"attempt":1}"#)[..], [Update::Notice(d)] if d.id == r.id && d.state == NoticeState::Done)
    );
    ev(
        &mut s,
        r#"{"type":"auto_retry_start","attempt":3,"maxAttempts":3,"delayMs":1,"errorMessage":"x"}"#,
    );
    assert!(
        matches!(&ev(&mut s, r#"{"type":"auto_retry_end","success":false,"attempt":3,"finalError":"gave up"}"#)[..], [Update::Notice(d)] if d.state == NoticeState::Failed && d.text == "gave up")
    );
    let sum = r#"{"type":"summarization_retry_scheduled","attempt":1,"maxAttempts":2,"delayMs":500,"errorMessage":"busy"}"#;
    assert!(
        matches!(&ev(&mut s, sum)[..], [Update::Notice(n)] if n.id.starts_with("retry-summary-") && n.state == NoticeState::Running)
    );
    assert!(
        matches!(&ev(&mut s, r#"{"type":"summarization_retry_finished"}"#)[..], [Update::Notice(n)] if n.id.starts_with("retry-summary-") && n.state == NoticeState::Done)
    );
    ev(&mut s, r#"{"type":"compaction_start","reason":"manual"}"#);
    let out = ev(
        &mut s,
        r#"{"type":"compaction_end","reason":"manual","aborted":false,"willRetry":false,"result":{"summary":"Did X.","tokensBefore":90000,"estimatedTokensAfter":12000}}"#,
    );
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::Compaction && n.detail.as_deref() == Some("Did X."))));
}

#[test]
fn thinking_level_downgrade_is_noticed() {
    let mut s = State { active: true, effort: "high".into(), ..State::default() };
    let same = r#"{"type":"thinking_level_changed","level":"high"}"#;
    assert_eq!(ev(&mut s, same), []);
    let low = r#"{"type":"thinking_level_changed","level":"low"}"#;
    assert!(matches!(&ev(&mut s, low)[..], [Update::Notice(n)] if n.id == "info-thinking"
        && n.text == "Pi is using thinking level low. high isn't available for this model."));
}

#[test]
fn extension_ui_feeds_notices_titles_and_forms() {
    let mut s = State { active: true, ..State::default() };
    let notify = r#"{"type":"extension_ui_request","id":"u1","method":"notify","message":"Sign in to Linear: https://linear.app/oauth?x=1","notifyType":"info"}"#;
    assert!(
        matches!(&ev(&mut s, notify)[..], [Update::Notice(n)] if n.kind == NoticeKind::McpAuth && n.id == "mcp-linear.app" && n.url.as_deref() == Some("https://linear.app/oauth?x=1"))
    );
    assert_eq!(
        ev(
            &mut s,
            r#"{"type":"extension_ui_request","id":"u2","method":"setTitle","title":"Fix login"}"#
        ),
        [Update::Title("Fix login".into())]
    );
    let status = r#"{"type":"extension_ui_request","id":"u3","method":"setStatus","statusKey":"mcp","statusText":"2 servers"}"#;
    assert!(
        matches!(&ev(&mut s, status)[..], [Update::Notice(n)] if n.id == "info-status-mcp" && n.text == "2 servers")
    );
    let cleared =
        r#"{"type":"extension_ui_request","id":"u3","method":"setStatus","statusKey":"mcp"}"#;
    assert!(
        matches!(&ev(&mut s, cleared)[..], [Update::Notice(n)] if n.id == "info-status-mcp" && n.text.is_empty())
    );
    let widget = r#"{"type":"extension_ui_request","id":"u5","method":"setWidget","widgetKey":"todo","widgetLines":["1 open","- a","- b"]}"#;
    assert!(
        matches!(&ev(&mut s, widget)[..], [Update::Notice(n)] if n.id == "info-widget-todo" && n.text == "1 open" && n.detail.as_deref() == Some("1 open\n- a\n- b"))
    );
    assert_eq!(
        ev(
            &mut s,
            r#"{"type":"extension_ui_request","id":"u6","method":"set_editor_text","text":"x"}"#
        ),
        []
    );
    let confirm = r#"{"type":"extension_ui_request","id":"u4","method":"confirm","title":"Delete?","message":"rm -rf dist"}"#;
    let [Update::Ask { id, ask: Ask::Form(f) }] = &ev(&mut s, confirm)[..] else { panic!() };
    assert!(f.fields.is_empty());
    assert_eq!(
        reply(&mut s, *id, Answer::Form(vec![])),
        Some(json!({"type":"extension_ui_response","id":"u4","confirmed":true}))
    );
}

#[test]
fn tui_commands_are_intercepted_or_reported() {
    assert!(
        matches!(command("/thinking high", "a1"), Some(PiCommand::Rpc(v)) if v["type"] == "set_thinking_level" && v["level"] == "high" && v["id"] == "a1")
    );
    assert!(matches!(command("/tree", "a2"), Some(PiCommand::TuiOnly(c)) if c == "tree"));
    assert!(matches!(command("/model", "a2"), Some(PiCommand::TuiOnly(c)) if c == "model"));
    assert!(command("/my-extension-cmd arg", "a3").is_none());
    assert!(command("/compactor", "a3").is_none());
    assert!(command("hello", "a4").is_none());
    assert!(
        matches!(command("/model openrouter/glm-5", "a5"), Some(PiCommand::Rpc(v)) if v["type"] == "set_model" && v["provider"] == "openrouter" && v["modelId"] == "glm-5")
    );
    assert!(
        matches!(command("/name Fix it", "a6"), Some(PiCommand::Rpc(v)) if v["type"] == "set_session_name" && v["name"] == "Fix it")
    );
    assert!(
        matches!(command("/session", "a7"), Some(PiCommand::Rpc(v)) if v["type"] == "get_session_stats")
    );
    assert!(
        matches!(command("/export /tmp/x.html", "a8"), Some(PiCommand::Rpc(v)) if v["type"] == "export_html" && v["outputPath"] == "/tmp/x.html")
    );
    assert!(
        matches!(command("/export", "a9"), Some(PiCommand::Rpc(v)) if v["outputPath"].is_null())
    );
}

#[test]
fn command_replies_settle_with_their_updates() {
    let mut s = State { active: true, direct: true, prompt: "a1".into(), ..State::default() };
    s.after.insert("a1".into(), Update::Title("Fix it".into()));
    let ok = r#"{"id":"a1","type":"response","command":"set_session_name","success":true}"#;
    assert_eq!(ev(&mut s, ok), [Update::Title("Fix it".into()), Update::Final(String::new())]);
    s = State { active: true, direct: true, prompt: "a2".into(), ..State::default() };
    let model = r#"{"id":"a2","type":"response","command":"set_model","success":true,"data":{"id":"glm-5"}}"#;
    assert_eq!(ev(&mut s, model), [Update::Model("glm-5".into()), Update::Final(String::new())]);
    s = State { active: true, direct: true, prompt: "a3".into(), ..State::default() };
    let stats = r#"{"id":"a3","type":"response","command":"get_session_stats","success":true,"data":{"toolCalls":3}}"#;
    let out = ev(&mut s, stats);
    assert!(
        matches!(&out[0], Update::Notice(n) if n.id == "info-session" && n.detail.as_deref().is_some_and(|d| d.contains("toolCalls")))
    );
    assert_eq!(out[1], Update::Final(String::new()));
    s = State { active: true, direct: true, prompt: "a4".into(), ..State::default() };
    let export = r#"{"id":"a4","type":"response","command":"export_html","success":true,"data":{"path":"/tmp/s.html"}}"#;
    assert!(
        matches!(&ev(&mut s, export)[0], Update::Notice(n) if n.text == "Exported to /tmp/s.html")
    );
    s = State { active: true, direct: true, prompt: "a5".into(), ..State::default() };
    let bad = r#"{"id":"a5","type":"response","command":"set_model","success":false,"error":"no such model"}"#;
    assert_eq!(
        ev(&mut s, bad),
        [Update::Error("no such model".into()), Update::Final(String::new())]
    );
}

#[test]
fn every_fixture_settles() {
    for (name, _) in fixtures("pi") {
        let out = replay(&name);
        assert!(
            out.iter()
                .any(|u| matches!(u, Update::Final(_) | Update::Ask { .. } | Update::Error(_))),
            "{name}"
        );
    }
}

#[test]
fn a_stale_queue_update_does_not_steer_early() {
    let mut s = State { active: true, ..State::default() };
    on_steer(&mut s, "now this");
    let queue =
        |items: &[&str]| json!({"type":"queue_update","steering":items,"followUp":[]}).to_string();
    assert_eq!(ev(&mut s, &queue(&[])), [], "never listed yet");
    assert_eq!(ev(&mut s, &queue(&["now this"])), []);
    assert_eq!(ev(&mut s, &queue(&[])), [Update::Steered("now this".into())]);
    assert_eq!(ev(&mut s, &user_start("now this")), []);
    on_steer(&mut s, "second");
    assert_eq!(
        ev(&mut s, &user_start("second")),
        [Update::Steered("second".into())],
        "message_start still marks it"
    );
}

#[test]
fn prompts_and_settles_drop_steer_bookkeeping() {
    let mut s = State { active: true, ..State::default() };
    on_steer(&mut s, "x");
    s.taken = 1;
    s.seen.insert("x".into());
    on_prompt(&mut s, "aether-1");
    assert_eq!((s.steers.is_empty(), s.taken, s.seen.is_empty()), (true, 0, true));
    on_steer(&mut s, "y");
    s.taken = 1;
    s.seen.insert("y".into());
    s.prompt.clear();
    ev(&mut s, SETTLED);
    assert_eq!((s.steers.is_empty(), s.taken, s.seen.is_empty()), (true, 0, true));
}

#[test]
fn idle_cancel_still_closes_dialogs() {
    let mut s = State::default();
    let select = r#"{"type":"extension_ui_request","id":"ui-7","method":"select","title":"Pick","options":["a"]}"#;
    let [Update::Ask { id, .. }] = ev(&mut s, select)[..] else { panic!() };
    assert!(!on_cancel(&mut s));
    let (lines, updates) = close_dialogs(&mut s);
    assert_eq!(lines, [json!({"type":"extension_ui_response","id":"ui-7","cancelled":true})]);
    assert_eq!(updates, [Update::AskResolved(id)]);
    assert_eq!(close_dialogs(&mut s), (vec![], vec![]));
}

#[test]
fn shut_reaps_the_old_child() {
    let mut child = std::process::Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let stdin = Mutex::new(child.stdin.take());
    let child = Mutex::new(Some(child));
    shut(&child, &stdin);
    assert!(child.lock().unwrap().is_none() && stdin.lock().unwrap().is_none());
    let alive = std::process::Command::new("kill").arg("-0").arg(pid.to_string()).output().unwrap();
    assert!(!alive.status.success(), "child must be gone before the new Pi starts");
}

fn tool_start(id: &str) -> String {
    upd(json!({"type":"toolcall_start","contentIndex":0,"id":id,"toolName":"bash"}))
}

#[test]
fn streamed_rows_that_never_run_are_halted() {
    for (reason, status) in [("aborted", ToolStatus::Stopped), ("error", ToolStatus::Failed)] {
        let mut s = State { active: true, ..State::default() };
        ev(&mut s, &tool_start("t1"));
        ev(&mut s, &tool_start("t2"));
        ev(
            &mut s,
            r#"{"type":"tool_execution_start","toolCallId":"t2","toolName":"bash","args":{}}"#,
        );
        let end = json!({"type":"message_end","message":{"role":"assistant","content":[],"stopReason":reason}}).to_string();
        let out = ev(&mut s, &end);
        let rows: Vec<_> = out
            .iter()
            .filter_map(|u| match u {
                Update::Tool(c) => Some(c),
                _ => None,
            })
            .collect();
        assert!(
            matches!(&rows[..], [c] if c.id == "t1" && c.status == status),
            "{reason}: {rows:?}"
        );
        assert!(s.args.is_empty() && s.names.is_empty() && s.tools.len() == 1, "{reason}");
    }
}

#[test]
fn thinking_command_records_the_requested_effort() {
    let mut s = State::default();
    let line = command("/thinking high", "a1");
    let Some(PiCommand::Rpc(line)) = line else { panic!() };
    track_command(&mut s, "a1", &line);
    assert_eq!((s.effort.as_str(), s.direct), ("high", true));
    track_command(&mut s, "a2", &json!({"id":"a2","type":"prompt","message":"hi"}));
    assert!(!s.direct);
}

#[test]
fn queue_update_during_cancel_does_not_mark_steers() {
    let mut s = State { active: true, ..State::default() };
    on_steer(&mut s, "later");
    let queue =
        |items: &[&str]| json!({"type":"queue_update","steering":items,"followUp":[]}).to_string();
    ev(&mut s, &queue(&["later"]));
    on_cancel(&mut s);
    cancel_lines(&mut s, "c1");
    assert_eq!(ev(&mut s, &queue(&[])), []);
    let cleared = r#"{"type":"response","id":"c1","command":"clear_queue","success":true,"data":{"steering":["later"],"followUp":[]}}"#;
    assert_eq!(ev(&mut s, cleared), [Update::SteerRefused("later".into())]);
}

#[test]
fn set_model_reply_refreshes_the_window() {
    let mut s = State { active: true, direct: true, prompt: "a1".into(), ..State::default() };
    let model = r#"{"id":"a1","type":"response","command":"set_model","success":true,"data":{"id":"glm-5","contextWindow":200000}}"#;
    ev(&mut s, model);
    assert_eq!(s.window, Some(200_000));
}

#[test]
fn cancelled_bash_message_is_stopped() {
    let mut s = State { active: true, ..State::default() };
    let end = json!({"type":"message_end","message":{"role":"bashExecution","command":"sleep 9","output":"","exitCode":null,"cancelled":true}}).to_string();
    let [Update::Tool(c)] = &ev(&mut s, &end)[..] else { panic!() };
    assert_eq!(c.status, ToolStatus::Stopped);
    assert!(matches!(&c.detail, ToolDetail::Command { exit_code: None, .. }));
}

#[test]
fn search_output_gets_the_truncation_note() {
    let mut s = State { active: true, ..State::default() };
    let end = json!({"type":"tool_execution_end","toolCallId":"g","toolName":"grep","isError":false,
        "result":{"content":[{"type":"text","text":"a.rs:1:x"}],"details":{"truncation":{"truncated":true,"outputLines":10,"totalLines":99}}}}).to_string();
    let [Update::Tool(c)] = &ev(&mut s, &end)[..] else { panic!() };
    assert!(
        matches!(&c.detail, ToolDetail::Search { output, .. } if output.ends_with("Showing 10 of 99 lines.")),
        "{:?}",
        c.detail
    );
}

#[test]
fn an_exit_refuses_the_steers_pi_never_took() {
    let mut s = State { active: true, ..State::default() };
    on_steer(&mut s, "taken");
    on_steer(&mut s, "waiting\n\nAttached files:\n- /x/a.png (image/png)");
    s.taken = 1;
    let out = on_exit(&mut s, "pi exited".into());
    assert_eq!(
        out,
        [
            Update::SteerRefused("waiting".into()),
            Update::Error("pi exited".into()),
            Update::Final(String::new())
        ]
    );
    assert!(s.dead.is_some() && s.steers.is_empty());
    let mut idle = State::default();
    assert_eq!(on_exit(&mut idle, "x".into()), []);
}

#[test]
fn a_failed_run_refuses_its_waiting_steers() {
    let mut s = State { active: true, ..State::default() };
    on_steer(&mut s, "more");
    let failed = r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"boom"}}"#;
    ev(&mut s, failed);
    assert_eq!(
        ev(&mut s, SETTLED),
        [
            Update::SteerRefused("more".into()),
            Update::Error("boom".into()),
            Update::Final(String::new())
        ]
    );
}

#[test]
fn a_rejected_steer_returns_to_the_queue_without_failing_the_reply() {
    let mut s =
        State { active: true, running: true, prompt: "aether-1".into(), ..State::default() };
    on_steer(&mut s, "more");
    s.steer_acks.insert("aether-2".into(), "more".into());
    let rejected =
        r#"{"type":"response","id":"aether-2","command":"prompt","success":false,"error":"busy"}"#;
    let out = ev(&mut s, rejected);
    assert!(
        matches!(&out[..], [Update::SteerRefused(t), Update::Warning(w)] if t == "more" && w == "busy"),
        "{out:?}"
    );
    assert!(s.steers.is_empty());
    assert_eq!(s.prompt, "aether-1", "the prompt still waits for its own ack");
    let started = r#"{"type":"response","id":"aether-1","command":"prompt","success":true,"data":{"disposition":"started"}}"#;
    assert_eq!(ev(&mut s, started), []);
    assert_eq!(ev(&mut s, SETTLED), [Update::Final(String::new())]);
}

#[test]
fn the_steer_fixture_never_repeats_text_in_its_final() {
    let out = replay("steer");
    assert!(out.iter().all(|u| !matches!(u, Update::Final(t) if !t.is_empty())), "{out:?}");
    let streamed: String = out
        .iter()
        .filter_map(|u| match u {
            Update::Delta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    let mut m = aether_core::Message::assistant();
    for u in out {
        m.apply(u);
    }
    assert_eq!(m.text.replace("\n\n", ""), streamed.replace("\n\n", ""));
}

#[test]
fn an_unknown_dialog_is_cancelled_and_warned_every_time() {
    let mut s = State { active: true, ..State::default() };
    let odd = r#"{"type":"extension_ui_request","id":"ui-9","method":"wizard","title":"x"}"#;
    for _ in 0..2 {
        let out = ev(&mut s, odd);
        assert!(matches!(&out[..], [Update::Warning(w)] if w.contains("wizard")), "{out:?}");
    }
    assert_eq!(
        s.outbox,
        vec![json!({"type": "extension_ui_response", "id": "ui-9", "cancelled": true}); 2]
    );
}

#[test]
fn settle_forgets_rows_of_the_ended_run() {
    let mut s = State { active: true, ..State::default() };
    let start = r#"{"type":"message_update","assistantMessageEvent":{"type":"toolcall_start","contentIndex":0,"id":"t1","toolName":"bash"}}"#;
    ev(&mut s, start);
    ev(&mut s, SETTLED);
    assert!(s.names.is_empty() && s.tools.is_empty() && s.args.is_empty());
}

#[test]
fn an_expanded_slash_steer_does_not_block_the_next_one() {
    let mut s = State { active: true, ..State::default() };
    on_steer(&mut s, "/review src");
    on_steer(&mut s, "plain");
    assert_eq!(
        ev(&mut s, &user_start("Review the code in src carefully")),
        [Update::Steered("/review src".into())]
    );
    assert_eq!(ev(&mut s, &user_start("plain")), [Update::Steered("plain".into())]);
    on_steer(&mut s, "words");
    assert_eq!(ev(&mut s, &user_start("something else")), [], "only a slash steer may expand");
}

#[test]
fn a_model_switched_by_command_is_the_model_options_compare_to() {
    let mut s = State { active: true, prompt: "a2".into(), direct: true, ..State::default() };
    let model = r#"{"id":"a2","type":"response","command":"set_model","success":true,"data":{"id":"glm-5","provider":"openrouter"}}"#;
    ev(&mut s, model);
    assert_eq!(s.model, "openrouter/glm-5");
}

#[test]
fn fragmented_tool_arguments_scan_boundaries_once() {
    let mut args = StreamedArgs::default();
    assert!(args.push(r#"{"command":""#).is_none());
    // Every fragment ends in a brace, including escaped quotes across fragments.
    for _ in 0..10_000 {
        assert!(args.push(r#"abc\"}"#).is_none());
        assert!(!args.complete);
    }
    let input = args.push(r#"" ,"nested":[{},{"x":{}}]}"#).unwrap();
    assert_eq!(input["command"].as_str().unwrap().len(), 50_000);
    assert!(args.complete);
    // A completed root cannot trigger another whole-buffer parse.
    let len = args.json.len();
    assert!(args.push("}").is_none());
    assert_eq!(args.json.len(), len);
}

#[test]
fn streamed_argument_escapes_can_cross_delta_boundaries() {
    let mut args = StreamedArgs::default();
    assert!(args.push("{\"command\":\"a\\").is_none());
    assert!(args.push("\"}").is_none());
    assert!(!args.complete);
    assert_eq!(args.push("\"}").unwrap()["command"], "a\"}");
}
