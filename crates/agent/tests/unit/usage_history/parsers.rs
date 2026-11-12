use super::*;
use aether_core::{AccessMode, Message, Provider, Update};
use chrono::Local;

#[test]
fn claude_lines_parse_usage_and_dedupe_key() {
    let line = r#"{"type":"assistant","timestamp":"2026-08-08T15:18:37.487Z","requestId":"req_1","sessionId":"session-1","costUSD":null,"cwd":"/Users/me/dev/aether","message":{"id":"msg_1","model":"claude-sonnet-4-5","usage":{"input_tokens":2,"cache_creation_input_tokens":50700,"cache_read_input_tokens":0,"output_tokens":1238}}}"#;
    let record = parse_claude_line(line).expect("the line carries usage");
    assert_eq!(record.provider, UsageProvider::Claude);
    assert_eq!(record.model, "claude-sonnet-4-5");
    assert_eq!(record.session_id, "session-1");
    assert_eq!(record.project, "/Users/me/dev/aether");
    assert_eq!(record.dedupe_key.as_deref(), Some("msg_1:req_1"));
    assert_eq!(record.totals.uncached_input, 2);
    assert_eq!(record.totals.cache_creation, 50700);
    assert_eq!(record.totals.output, 1238);
}

#[test]
fn codex_lines_carry_model_forward_and_skip_duplicates() {
    let mut state = CodexScanState::new();
    let meta = r#"{"timestamp":"2026-08-06T16:31:19.166Z","type":"session_meta","payload":{"id":"codex-session","cwd":"/Users/me/dev/aether"}}"#;
    let context = r#"{"timestamp":"2026-08-06T16:31:20.000Z","type":"turn_context","payload":{"model":"gpt-5.3-codex"}}"#;
    let count = r#"{"timestamp":"2026-08-06T16:31:25.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":21047,"cached_input_tokens":1000,"cache_write_input_tokens":47,"output_tokens":280,"reasoning_output_tokens":165}}}}"#;

    assert!(parse_codex_line(count, &mut state).is_none());
    assert!(parse_codex_line(meta, &mut state).is_none());
    assert!(parse_codex_line(context, &mut state).is_none());

    let record = parse_codex_line(count, &mut state).expect("model is known now");
    assert_eq!(record.provider, UsageProvider::Codex);
    assert_eq!(record.model, "gpt-5.3-codex");
    assert_eq!(record.session_id, "codex-session");
    assert_eq!(record.project, "/Users/me/dev/aether");
    assert_eq!(record.totals.uncached_input, 21047 - 1000 - 47);
    assert_eq!(record.totals.cached_input, 1000);
    assert_eq!(record.totals.cache_creation, 47);
    assert_eq!(record.totals.reasoning, 165);
    assert!(parse_codex_line(count, &mut state).is_none());
}

#[test]
fn pi_records_extract_session_header_usage_and_cost() {
    let jsonl = [
        r#"{"type":"session","id":"pi-sess-1","cwd":"/Users/me/dev/aether","timestamp":"2026-08-08T10:00:00Z"}"#,
        r#"{"type":"message","id":"m1","timestamp":"2026-08-08T10:01:00Z","message":{"role":"assistant","model":"claude-sonnet-4-5","usage":{"input":500,"output":200,"cacheRead":1500,"cacheWrite":100,"cost":{"total":0.0125}}}}"#,
    ]
    .join("\n");
    let records = parse_pi_records(jsonl.as_bytes(), "fallback");
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.provider, UsageProvider::Pi);
    assert_eq!(r.session_id, "pi-sess-1");
    assert_eq!(r.project, "/Users/me/dev/aether");
    assert_eq!(r.model, "claude-sonnet-4-5");
    assert_eq!(r.totals.uncached_input, 500);
    assert_eq!(r.totals.cached_input, 1500);
    assert_eq!(r.totals.cache_creation, 100);
    assert_eq!(r.totals.output, 200);
    assert_eq!(r.reported_cost_usd, Some(0.0125));
}

#[test]
fn antigravity_records_extract_planner_tokens_model_and_cwd() {
    let jsonl = [
        r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-16T08:14:41Z","content":"<USER_REQUEST>\nhi\n</USER_REQUEST>\n<USER_SETTINGS_CHANGE>\nThe user changed setting `Model Selection` from None to Gemini 2.5 Pro. No need to comment.\n</USER_SETTINGS_CHANGE>"}"#,
        r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-10-16T08:14:42Z","input_tokens":13459,"cache_read_tokens":2000,"output_tokens":274,"tool_calls":[{"name":"run_command","args":{"Cwd":"\"/Users/me/dev/aether\"","CommandLine":"\"ls\""}}]}"#,
    ]
    .join("\n");
    let records = parse_antigravity_records(jsonl.as_bytes(), "agy-conv-1");
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.provider, UsageProvider::Antigravity);
    assert_eq!(r.session_id, "agy-conv-1");
    assert_eq!(r.project, "/Users/me/dev/aether");
    assert_eq!(r.model, "gemini-2.5-pro");
    assert_eq!(r.totals.uncached_input, 13459);
    assert_eq!(r.totals.cached_input, 2000);
    assert_eq!(r.totals.output, 274);
    assert_eq!(r.dedupe_key.as_deref(), Some("agy:agy-conv-1:1"));
}

#[test]
fn opencode_and_saved_chats_convert_into_usage_records() {
    let mut msg = Message::assistant();
    msg.model = Some("anthropic/claude-sonnet-4-5".into());
    msg.apply(Update::Count { prompt: 1200, completion: 300, cached: 800 });
    msg.ended_at_ms = Some(Local::now().timestamp_millis());
    let chat = SavedChat {
        id: 42,
        title: "Test".into(),
        messages: vec![msg],
        pinned: false,
        archived: false,
        provider: Provider::OpenRouter,
        provider_model: "anthropic/claude-sonnet-4-5".into(),
        cli_session: String::new(),
        agent_sessions: vec![],
        handoff_seen: None,
        access: AccessMode::Ask,
        effort: String::new(),
        cwd: "/Users/me/dev/aether".into(),
        project: "/Users/me/dev/aether".into(),
        worktree_path: String::new(),
        worktree_branch: String::new(),
        parent: None,
        lead: false,
        lead_seen: None,
        pending_queue: vec![],
        pending_deliveries: vec![],
        handoff_from: None,
    };
    let records = collect_saved_chat_records(&[chat]);
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.provider, UsageProvider::OpenRouter);
    assert_eq!(r.totals.uncached_input, 400);
    assert_eq!(r.totals.cached_input, 800);
    assert_eq!(r.totals.output, 300);

    let mut dirs = HashMap::new();
    dirs.insert("ses_1".to_owned(), "/Users/me/dev/aether".to_owned());
    let oc_json = r#"{"id":"msg_1","sessionID":"ses_1","role":"assistant","modelID":"claude-sonnet-4-5","cost":0.02,"tokens":{"input":300,"output":100,"reasoning":50,"cache":{"read":700,"write":0}},"time":{"created":1754668800000.0}}"#;
    let oc = parse_opencode_message_json(oc_json, &dirs).expect("opencode message parses");
    assert_eq!(oc.provider, UsageProvider::OpenCode);
    assert_eq!(oc.session_id, "ses_1");
    assert_eq!(oc.project, "/Users/me/dev/aether");
    assert_eq!(oc.totals.uncached_input, 300);
    assert_eq!(oc.totals.cached_input, 700);
    assert_eq!(oc.totals.output, 100);
    assert_eq!(oc.totals.reasoning, 50);
    assert_eq!(oc.reported_cost_usd, Some(0.02));
}
