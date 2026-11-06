use super::*;

fn cmd(id: &str) -> ToolCall {
    let detail = ToolDetail::Command {
        command: "ls".into(),
        cwd: None,
        output: String::new(),
        exit_code: None,
    };
    ToolCall::new(id, "Run command", detail)
}

#[test]
fn chats_saved_with_string_details_still_load() {
    let saved = r#"{"role":"Assistant","text":"hi","steps":[{"Tool":{"id":"t1","title":"Run command","detail":"ls","status":"Done"}}],"thought_for":null,"context_tokens":null,"elapsed_ms":5,"ended_at_ms":null,"model":null,"requests":[]}"#;
    let msg: Message = serde_json::from_str(saved).unwrap();
    let call = ToolCall {
        status: ToolStatus::Done,
        ..ToolCall::new("t1", "Run command", ToolDetail::Plain("ls".into()))
    };
    assert_eq!(msg.steps, [Step::Tool(call)]);
}

#[test]
fn chats_saved_before_sent_time_load_without_it() {
    let saved = r#"{"role":"User","text":"hi","steps":[],"thought_for":null,"context_tokens":null,"elapsed_ms":null,"ended_at_ms":null,"model":null,"requests":[]}"#;
    let msg: Message = serde_json::from_str(saved).unwrap();
    assert_eq!(msg.sent_at_ms, None);
}

#[test]
fn chats_saved_before_attachments_load_without_them() {
    let saved = r#"{"role":"User","text":"hi","steps":[],"thought_for":null,"context_tokens":null,"elapsed_ms":null,"ended_at_ms":null,"model":null,"requests":[]}"#;
    let msg: Message = serde_json::from_str(saved).unwrap();
    assert!(msg.attachments.is_empty());
}

#[test]
fn unknown_details_load_as_plain_and_typed_ones_round_trip() {
    let future =
        r#"{"id":"t","title":"Teleport","detail":{"Teleport":{"to":"mars"}},"status":"Done"}"#;
    let call: ToolCall = serde_json::from_str(future).unwrap();
    assert_eq!(call.detail, ToolDetail::Plain(String::new()));
    let mut call = cmd("t");
    call.error = Some("boom".into());
    let json = serde_json::to_string(&Step::Tool(call.clone())).unwrap();
    assert_eq!(serde_json::from_str::<Step>(&json).unwrap(), Step::Tool(call));
    let quiet = serde_json::to_string(&cmd("t")).unwrap();
    assert!(!quiet.contains("error") && !quiet.contains("\"ms\""), "{quiet}");
}

#[test]
fn finish_drops_oldest_tool_payload_past_the_cap() {
    let mut msg = Message::assistant();
    for id in ["a", "b", "c"] {
        let mut call = cmd(id);
        call.status = ToolStatus::Done;
        if let ToolDetail::Command { output, exit_code, .. } = &mut call.detail {
            *output = "x".repeat(120_000);
            *exit_code = Some(1);
        }
        msg.apply(Update::Tool(call));
    }
    let mut edit = ToolCall::new(
        "d",
        "Edit file",
        ToolDetail::Edit { path: "a".into(), diff: "+y\n".repeat(10), added: 10, removed: 0 },
    );
    edit.status = ToolStatus::Done;
    msg.apply(Update::Tool(edit));
    msg.finish();
    let msg = msg.for_save();
    let outputs: Vec<&str> = msg
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Tool(ToolCall {
                detail: ToolDetail::Command { output, exit_code, .. }, ..
            }) => {
                assert_eq!(*exit_code, Some(1));
                Some(output.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(outputs[0], NOT_KEPT);
    assert_eq!((outputs[1].len(), outputs[2].len()), (120_000, 120_000));
    assert!(
        matches!(&msg.steps[3], Step::Tool(ToolCall { detail: ToolDetail::Edit { added: 10, diff, .. }, .. }) if diff.starts_with("+y"))
    );
}

#[test]
fn summaries_clip_to_two_hundred_chars() {
    let long = ToolDetail::Plain("a".repeat(500));
    let summary = long.summary();
    assert_eq!(summary.chars().count(), 201);
    assert!(summary.ends_with('…'));
}

#[test]
fn summaries_are_one_trimmed_line() {
    let command = ToolDetail::Command {
        command: "\n  cargo test \\\n  --all".into(),
        cwd: None,
        output: String::new(),
        exit_code: None,
    };
    assert_eq!(command.summary(), "cargo test \\");
    let search = |pattern: &str| ToolDetail::Search {
        pattern: pattern.into(),
        path: Some("src".into()),
        matches: None,
        output: String::new(),
    };
    assert_eq!(search("fn main").summary(), "fn main");
    assert_eq!(search("").summary(), "src");
    let item = |text: &str, status| PlanItem { text: text.into(), status };
    let plan = ToolDetail::Plan {
        items: vec![item("Read", PlanStatus::Done), item("Write tests", PlanStatus::Active)],
    };
    assert_eq!(plan.summary(), "Write tests");
    let other = ToolDetail::Other {
        name: "x".into(),
        input: "{\n  \"q\": 1\n}".into(),
        output: String::new(),
    };
    assert_eq!(other.summary(), "{ \"q\": 1 }");
    assert_eq!(ToolDetail::Plain("ls".into()).summary(), "ls");
}

#[test]
fn turns_stream_commit_and_separate() {
    let mut msg = Message::assistant();
    msg.apply(Update::Delta("Let me ".into()));
    msg.apply(Update::Delta("check.".into()));
    msg.apply(Update::Final("Let me check.".into()));
    msg.apply(Update::Final(String::new()));
    msg.apply(Update::Delta("Answer".into()));
    assert_eq!(msg.text, "Let me check.\n\nAnswer");
    msg.apply(Update::Final("Answer!".into()));
    assert_eq!(msg.text, "Let me check.\n\nAnswer!");
}

#[test]
fn requests_take_one_answer_and_expire_on_finish() {
    let mut msg = Message::assistant();
    msg.apply(Update::Final("Let me ask.".into()));
    let ask = |q: &str| Ask::Question { question: q.into(), choices: vec![] };
    msg.apply(Update::Ask { id: 1, ask: ask("a?") });
    msg.apply(Update::Ask { id: 2, ask: ask("b?") });
    assert_eq!(msg.pending_request().map(|r| r.id), Some(1));
    assert_eq!(msg.requests[0].at, "Let me ask.".len());
    assert!(msg.answer(1, Answer::Reply("yes".into())));
    assert!(!msg.answer(1, Answer::Deny));
    msg.finish();
    assert_eq!(msg.requests[0].status, RequestStatus::Answered(Answer::Reply("yes".into())));
    assert_eq!(msg.requests[1].status, RequestStatus::Expired);
}

#[test]
fn steps_record_thinking_and_searches() {
    let mut msg = Message::assistant();
    msg.apply(Update::Thinking("\n".into()));
    msg.apply(Update::Thinking("Need ".into()));
    msg.apply(Update::Thinking("sources.".into()));
    msg.apply(Update::Search { id: "a".into(), query: "a".into() });
    msg.apply(Update::Search { id: "b".into(), query: "b".into() });
    msg.apply(Update::SearchDone { id: "a".into(), outcome: Ok(8) });
    assert_eq!(msg.thought_for, None);

    msg.apply(Update::Delta("Done".into()));
    assert!(msg.thought_for.is_some());
    msg.finish();
    let ms = match &msg.steps[0] {
        Step::Thinking { ms, .. } => *ms,
        other => panic!("{other:?}"),
    };
    assert!(ms.is_some());
    assert_eq!(
        msg.steps,
        [
            Step::Thinking { text: "Need sources.".into(), ms },
            Step::Search { id: "a".into(), query: "a".into(), results: Some(Ok(8)) },
            Step::Search {
                id: "b".into(),
                query: "b".into(),
                results: Some(Err("stopped".into()))
            },
        ]
    );
    assert_eq!(msg.searches(), 2);
}

#[test]
fn title_update_leaves_text_alone() {
    let mut msg = Message::assistant();
    msg.apply(Update::Delta("hi".into()));
    msg.apply(Update::Title("New title".into()));
    assert_eq!(msg.text, "hi");
}

#[test]
fn tool_updates_upsert_one_step_and_never_fail_the_reply() {
    let mut msg = Message::assistant();
    let tool = |status| Update::Tool(ToolCall { status, ..cmd("t1") });
    msg.apply(tool(ToolStatus::Running));
    msg.apply(Update::Session("abc".into()));
    msg.apply(tool(ToolStatus::Failed));
    let [Step::Tool(call)] = &msg.steps[..] else { panic!() };
    assert_eq!(
        *call,
        ToolCall { status: ToolStatus::Failed, started_ms: call.started_ms, ..cmd("t1") }
    );
    assert!(call.started_ms.is_some());
    assert!(!msg.failed);
    assert!(msg.text.is_empty());
}

#[test]
fn finish_stops_tools_still_running() {
    let mut msg = Message::assistant();
    let tool = |id: &str, status| Update::Tool(ToolCall { status, ..cmd(id) });
    msg.apply(tool("a", ToolStatus::Running));
    msg.apply(tool("b", ToolStatus::Running));
    msg.apply(tool("b", ToolStatus::Done));
    msg.finish();
    let statuses: Vec<_> = msg
        .steps
        .iter()
        .map(|s| match s {
            Step::Tool(call) => call.status,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(statuses, [ToolStatus::Stopped, ToolStatus::Done]);
}

#[test]
fn text_before_a_tool_survives_the_final() {
    let mut msg = Message::assistant();
    let tool = |status| Update::Tool(ToolCall { status, ..cmd("t1") });
    msg.apply(Update::Delta("Let me look.".into()));
    msg.apply(tool(ToolStatus::Running));
    msg.apply(tool(ToolStatus::Done));
    msg.apply(Update::Delta("Found it".into()));
    msg.apply(Update::Final("Found it.".into()));
    assert_eq!(msg.text, "Let me look.\n\nFound it.");
}

#[test]
fn late_tool_status_keeps_the_segment_boundary() {
    let mut msg = Message::assistant();
    let tool = |status| Update::Tool(ToolCall { status, ..cmd("t") });
    msg.apply(Update::Delta("A".into()));
    msg.apply(tool(ToolStatus::Running));
    msg.apply(Update::Delta("B".into()));
    msg.apply(tool(ToolStatus::Done));
    msg.apply(Update::Final("B!".into()));
    assert_eq!(msg.text, "A\n\nB!");
}

#[test]
fn step_positions_track_interleaved_events() {
    let mut msg = Message::assistant();
    msg.apply(Update::Thinking("Initial plan".into()));
    assert_eq!(msg.step_at(0), 0);

    msg.apply(Update::Delta("First paragraph.".into()));
    let text_len_after_p1 = msg.text.len();

    let tool = |id, status| Update::Tool(ToolCall { status, ..cmd(id) });
    msg.apply(tool("t1", ToolStatus::Running));
    assert_eq!(msg.step_at(1), text_len_after_p1);

    msg.apply(Update::Delta("Second paragraph.".into()));
    let text_len_after_p2 = msg.text.len();

    msg.apply(tool("t2", ToolStatus::Running));
    assert_eq!(msg.step_at(2), text_len_after_p2);

    // Updating an existing tool should not change its position
    msg.apply(tool("t1", ToolStatus::Done));
    assert_eq!(msg.step_at(1), text_len_after_p1);
}

#[test]
fn approvals_saved_before_reasons_still_load() {
    let old = r#"{"Approval":{"tool":"Bash","title":"Run","input":"ls"}}"#;
    let ask: Ask = serde_json::from_str(old).unwrap();
    assert_eq!(
        ask,
        Ask::Approval(Approval {
            tool: "Bash".into(),
            title: "Run".into(),
            input: "ls".into(),
            ..Approval::default()
        })
    );
}

#[test]
fn unknown_variants_load_as_unknown() {
    let json = r#"{"role":"Assistant","text":"hi","steps":[{"Thought":"a"},{"Hologram":{"x":1}},{"Tool":{"id":"t","title":"T","detail":{"Plain":""},"status":"Teleporting"}}],"step_positions":[0,0,0],"thought_for":null,"context_tokens":null,"elapsed_ms":null,"ended_at_ms":null,"model":null,"requests":[{"id":1,"at":0,"ask":{"Telepathy":{}},"status":{"Answered":{"Hum":1}}}]}"#;
    let m: Message = serde_json::from_str(json).unwrap();
    assert_eq!(m.steps[1], Step::Unknown);
    assert!(matches!(&m.steps[2], Step::Tool(c) if c.status == ToolStatus::Unknown));
    assert_eq!(m.requests[0].ask, Ask::Unknown);
    assert_eq!(m.requests[0].status, RequestStatus::Expired);
}

#[test]
fn todays_format_round_trips() {
    let mut m = Message::assistant();
    m.apply(Update::Thinking("plan".into()));
    m.apply(Update::Tool(ToolCall::new("t1", "Run command", ToolDetail::Plain("ls".into()))));
    m.apply(Update::Ask {
        id: 4,
        ask: Ask::Question { question: "q".into(), choices: vec!["a".into()] },
    });
    m.apply(Update::Final("done".into()));
    m.finish();
    let back: Message = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&m).unwrap());
}

#[test]
fn finish_keeps_live_output_and_for_save_caps_it() {
    let mut m = Message::assistant();
    let big = "x".repeat(300 * 1024);
    let detail = ToolDetail::Command {
        command: "ls".into(),
        cwd: None,
        output: big.clone(),
        exit_code: Some(0),
    };
    m.apply(Update::Tool(ToolCall {
        status: ToolStatus::Done,
        ..ToolCall::new("t", "Run", detail)
    }));
    m.finish();
    let Step::Tool(live) = &m.steps[0] else { panic!() };
    assert!(matches!(&live.detail, ToolDetail::Command { output, .. } if *output == big));
    let saved = m.for_save();
    let Step::Tool(kept) = &saved.steps[0] else { panic!() };
    assert!(matches!(&kept.detail, ToolDetail::Command { output, .. } if output == NOT_KEPT));
}

fn notice(id: &str, state: NoticeState) -> Notice {
    Notice {
        id: id.into(),
        kind: NoticeKind::Retry,
        text: "retrying".into(),
        state,
        until_ms: None,
        url: None,
        detail: None,
    }
}

#[test]
fn notices_upsert_by_id_in_place() {
    let mut m = Message::assistant();
    m.apply(Update::Delta("a".into()));
    m.apply(Update::Notice(notice("r1", NoticeState::Running)));
    m.apply(Update::Delta("b".into()));
    m.apply(Update::Notice(notice("r1", NoticeState::Done)));
    assert_eq!(m.steps, [Step::Notice(notice("r1", NoticeState::Done))]);
    assert_eq!(m.step_positions, [1]);
}

#[test]
fn warnings_are_notices_and_errors_stay_out_of_the_text() {
    let mut m = Message::assistant();
    m.apply(Update::Delta("partial".into()));
    m.apply(Update::Warning("stderr: deprecated flag".into()));
    assert!(!m.failed);
    assert!(
        matches!(&m.steps[0], Step::Notice(n) if n.kind == NoticeKind::Warning && n.text == "stderr: deprecated flag")
    );
    m.apply(Update::Error("claude exited (1)\npanic: boom".into()));
    assert!(m.failed);
    assert_eq!(m.text, "partial");
    assert_eq!(m.error.as_deref(), Some("claude exited (1)\npanic: boom"));
}

#[test]
fn thinking_after_text_starts_a_new_timed_step() {
    let mut m = Message::assistant();
    m.apply(Update::Thinking("one".into()));
    m.apply(Update::Thinking(" more".into()));
    m.apply(Update::Delta("text".into()));
    m.apply(Update::Thinking("two".into()));
    let thoughts: Vec<(&str, bool)> = m
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Thinking { text, ms } => Some((text.as_str(), ms.is_some())),
            _ => None,
        })
        .collect();
    assert_eq!(thoughts, [("one more", true), ("two", false)]);
    assert_eq!(m.step_positions, [0, 4]);
}

#[test]
fn steered_marks_the_spot_and_final_keeps_text_before_it() {
    let mut m = Message::assistant();
    m.apply(Update::Delta("before".into()));
    m.apply(Update::Steered("also dark mode".into()));
    m.apply(Update::Final("after".into()));
    assert_eq!(m.steps, [Step::Steer("also dark mode".into())]);
    assert_eq!(m.text, "before\n\nafter");
}

#[test]
fn cost_model_and_resolved_asks() {
    let mut m = Message::assistant();
    m.apply(Update::Cost(1_500));
    m.apply(Update::Cost(500));
    m.apply(Update::Model("claude-sonnet-5-5".into()));
    m.apply(Update::Ask { id: 3, ask: Ask::Question { question: "q".into(), choices: vec![] } });
    m.apply(Update::AskResolved(3));
    assert_eq!((m.cost_micros, m.served_model.as_deref()), (2_000, Some("claude-sonnet-5-5")));
    assert_eq!(m.requests[0].status, RequestStatus::Withdrawn);
    assert!(m.pending_request().is_none());
}

#[test]
fn parallel_searches_resolve_their_own_rows() {
    let mut m = Message::assistant();
    m.apply(Update::Search { id: "a".into(), query: "one".into() });
    m.apply(Update::Search { id: "b".into(), query: "two".into() });
    m.apply(Update::SearchDone { id: "b".into(), outcome: Ok(3) });
    assert!(matches!(&m.steps[0], Step::Search { results: None, .. }));
    assert!(matches!(&m.steps[1], Step::Search { results: Some(Ok(3)), .. }));
}

#[test]
fn old_search_step_without_id_loads() {
    let step: Step = serde_json::from_str(r#"{"Search":{"query":"q","results":null}}"#).unwrap();
    assert!(matches!(step, Step::Search { ref id, .. } if id.is_empty()));
}

fn tool(id: &str, status: ToolStatus) -> Update {
    Update::Tool(ToolCall {
        status,
        ..ToolCall::new(id, "Run command", ToolDetail::Plain(String::new()))
    })
}

#[test]
fn route_sends_known_tool_ids_to_their_own_message() {
    let mut first = Message::assistant();
    first.apply(tool("bg1", ToolStatus::Background));
    first.finish();
    let messages = vec![Message::user("go"), first, Message::user("next"), Message::assistant()];
    assert_eq!(route(&messages, 3, &tool("bg1", ToolStatus::Done)), 1);
    assert_eq!(route(&messages, 3, &tool("new", ToolStatus::Running)), 3);
    assert_eq!(route(&messages, 3, &Update::Delta("x".into())), 3);
}

#[test]
fn finish_leaves_background_rows_running() {
    let mut m = Message::assistant();
    m.apply(tool("a", ToolStatus::Running));
    m.apply(tool("b", ToolStatus::Background));
    m.apply(tool("c", ToolStatus::Waiting));
    m.finish();
    let st: Vec<ToolStatus> = m
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Tool(c) => Some(c.status),
            _ => None,
        })
        .collect();
    assert_eq!(st, [ToolStatus::Stopped, ToolStatus::Background, ToolStatus::Stopped]);
}

#[test]
fn interrupt_stops_live_rows_with_a_reason() {
    let mut m = Message::assistant();
    m.apply(tool("a", ToolStatus::Running));
    m.apply(tool("b", ToolStatus::Background));
    let failed = ToolCall {
        error: Some("boom".into()),
        ..ToolCall::new("e", "Run command", ToolDetail::Plain(String::new()))
    };
    m.apply(Update::Tool(failed));
    m.apply(Update::Delta("kept".into()));
    m.apply(Update::Search { id: "s".into(), query: "q".into() });
    m.apply(Update::Thinking("hmm".into()));
    m.apply(Update::Ask { id: 7, ask: Ask::Plan(Plan::default()) });
    m.interrupt("Interrupted: Aether closed");
    assert_eq!(m.text, "kept");
    for s in &m.steps {
        match s {
            Step::Tool(c) if c.id == "e" => {
                assert_eq!((c.status, c.error.as_deref()), (ToolStatus::Stopped, Some("boom")))
            }
            Step::Tool(c) => assert_eq!(
                (c.status, c.error.as_deref()),
                (ToolStatus::Stopped, Some("Interrupted: Aether closed"))
            ),
            Step::Search { results, .. } => {
                assert_eq!(results, &Some(Err("Interrupted: Aether closed".to_string())))
            }
            Step::Thinking { ms, .. } => assert!(ms.is_some()),
            _ => {}
        }
    }
    assert_eq!(m.requests[0].status, RequestStatus::Expired);
}

#[test]
fn nested_calls_keep_their_parent() {
    let mut m = Message::assistant();
    let child = ToolCall {
        parent: Some("task1".into()),
        ..ToolCall::new("c1", "Read", ToolDetail::Plain(String::new()))
    };
    m.apply(Update::Tool(child.clone()));
    m.apply(tool("c1", ToolStatus::Done));
    let back: Message = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert!(matches!(&back.steps[0], Step::Tool(c) if c.parent.as_deref() == Some("task1")));
}

#[test]
fn live_rows_and_asks_carry_their_start_time() {
    let mut m = Message::assistant();
    m.apply(Update::Tool(ToolCall::new("t", "Run", ToolDetail::Plain(String::new()))));
    let Step::Tool(first) = &m.steps[0] else { panic!() };
    let started = first.started_ms.expect("stamped on insert");
    m.apply(Update::Tool(ToolCall {
        status: ToolStatus::Done,
        ..ToolCall::new("t", "Run", ToolDetail::Plain(String::new()))
    }));
    let Step::Tool(done) = &m.steps[0] else { panic!() };
    assert_eq!(done.started_ms, Some(started), "an upsert keeps the first stamp");
    m.apply(Update::Ask { id: 1, ask: Ask::Unknown });
    assert!(m.requests[0].asked_ms.is_some());
}

#[test]
fn a_driver_supplied_start_time_wins() {
    let mut m = Message::assistant();
    m.apply(Update::Tool(ToolCall {
        started_ms: Some(42),
        ..ToolCall::new("t", "Run", ToolDetail::Plain(String::new()))
    }));
    let Step::Tool(c) = &m.steps[0] else { panic!() };
    assert_eq!(c.started_ms, Some(42));
}

#[test]
fn live_thinking_time_only_while_the_thought_is_open() {
    let mut m = Message::assistant();
    assert_eq!(m.live_thinking_ms(), None);
    m.apply(Update::Thinking("hmm".into()));
    assert!(m.live_thinking_ms().is_some());
    m.apply(Update::Delta("x".into()));
    assert_eq!(m.live_thinking_ms(), None);
}

#[test]
fn rows_saved_without_a_start_time_load() {
    let call: ToolCall = serde_json::from_str(r#"{"id":"t","title":"T","status":"Done"}"#).unwrap();
    assert_eq!(call.started_ms, None);
}

#[test]
fn route_keeps_reused_ids_out_of_finished_replies() {
    let mut first = Message::assistant();
    first.apply(tool("compaction", ToolStatus::Done));
    first.apply(tool("bg1", ToolStatus::Background));
    first.finish();
    let mut live = Message::assistant();
    live.apply(tool("bg1", ToolStatus::Running));
    let mut messages =
        vec![Message::user("go"), first, Message::user("next"), Message::assistant()];
    assert_eq!(route(&messages, 3, &tool("compaction", ToolStatus::Running)), 3);
    messages[3] = live;
    assert_eq!(route(&messages, 3, &tool("bg1", ToolStatus::Done)), 3, "live reply first");
}

#[test]
fn a_second_error_keeps_the_first_cause_on_top() {
    let mut m = Message::assistant();
    m.apply(Update::Error("rate limited".into()));
    m.apply(Update::Error("claude exited (1)".into()));
    assert_eq!(m.error.as_deref(), Some("rate limited\nclaude exited (1)"));
}

#[test]
fn notices_steers_thinking_withdrawn_asks_and_nested_background_tools_round_trip() {
    let mut m = Message::assistant();
    m.apply(Update::Notice(notice("n1", NoticeState::Running)));
    m.apply(Update::Steered("also dark mode".into()));
    m.steps.push(Step::Thinking { text: "hmm".into(), ms: Some(40) });
    m.step_positions.push(0);
    m.apply(Update::Ask { id: 2, ask: Ask::Question { question: "q".into(), choices: vec![] } });
    m.apply(Update::AskResolved(2));
    let image = Attachment {
        name: "shot.png".into(),
        path: "/tmp/shot.png".into(),
        mime: "image/png".into(),
        size: 3,
    };
    m.apply(Update::Tool(ToolCall {
        status: ToolStatus::Background,
        parent: Some("task1".into()),
        images: vec![image],
        ..cmd("bg")
    }));
    let back: Message = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back.requests[0].status, RequestStatus::Withdrawn);
    assert_eq!(
        (back.steps, back.step_positions, back.requests),
        (m.steps, m.step_positions, m.requests)
    );
}

#[test]
fn a_restored_reply_never_claims_a_thinking_time() {
    let mut msg = Message::assistant();
    msg.restored = true;
    msg.apply(Update::Thinking("plan".into()));
    msg.apply(Update::Final("done".into()));
    msg.finish();
    assert_eq!(msg.thought_for, None);
}

#[test]
fn a_new_notice_is_done_and_a_refused_steer_leaves_the_reply_alone() {
    let n = Notice::new("info-x", NoticeKind::Info, "hi");
    assert_eq!((n.state, n.until_ms, n.url, n.detail), (NoticeState::Done, None, None, None));
    let mut msg = Message::assistant();
    msg.apply(Update::SteerRefused("more".into()));
    assert!(msg.steps.is_empty() && msg.text.is_empty());
}

#[test]
fn replace_swaps_the_segment_without_ending_the_turn() {
    let mut msg = Message::assistant();
    msg.apply(Update::Delta("Fine. Refused.".into()));
    msg.apply(Update::Replace("Fine.".into()));
    msg.apply(Update::Delta(" More".into()));
    assert_eq!(msg.text, "Fine. More");
    msg.apply(Update::Final("Fine. More!".into()));
    assert_eq!(msg.text, "Fine. More!");
    msg.apply(Update::Delta("Next".into()));
    assert_eq!(msg.text, "Fine. More!\n\nNext");
}

fn child(id: &str, parent: &str, status: ToolStatus) -> Update {
    Update::Tool(ToolCall {
        status,
        parent: Some(parent.into()),
        ..ToolCall::new(id, "Read", ToolDetail::Plain(String::new()))
    })
}

fn statuses(m: &Message) -> Vec<(String, ToolStatus)> {
    m.steps
        .iter()
        .filter_map(|s| match s {
            Step::Tool(c) => Some((c.id.clone(), c.status)),
            _ => None,
        })
        .collect()
}

#[test]
fn children_of_a_background_subagent_outlive_the_turn_and_route_home() {
    let mut first = Message::assistant();
    first.apply(tool("bg", ToolStatus::Background));
    first.apply(child("c1", "bg", ToolStatus::Running));
    first.apply(child("g1", "c1", ToolStatus::Waiting));
    first.apply(tool("fg", ToolStatus::Running));
    first.apply(child("c0", "fg", ToolStatus::Running));
    first.finish();
    let st = |id: &str| statuses(&first).into_iter().find(|(i, _)| i == id).unwrap().1;
    assert_eq!(st("c1"), ToolStatus::Running);
    assert_eq!(st("g1"), ToolStatus::Waiting, "grandchild of a live subagent");
    assert_eq!((st("fg"), st("c0")), (ToolStatus::Stopped, ToolStatus::Stopped));
    let mut messages =
        vec![Message::user("go"), first, Message::user("next"), Message::assistant()];
    for update in [
        child("c1", "bg", ToolStatus::Done),
        child("c2", "bg", ToolStatus::Running),
        child("g2", "c2", ToolStatus::Running),
    ] {
        let at = route(&messages, 3, &update);
        assert_eq!(at, 1);
        messages[at].apply(update);
    }
    assert_eq!(route(&messages, 3, &child("x", "fg", ToolStatus::Running)), 3);
    assert_eq!(route(&messages, 3, &tool("c0", ToolStatus::Done)), 3, "stopped row's id reused");
    let ids: Vec<String> = statuses(&messages[1]).into_iter().map(|(i, _)| i).collect();
    assert_eq!(ids, ["bg", "c1", "g1", "fg", "c0", "c2", "g2"]);
}

#[test]
fn a_background_agent_ending_stops_its_live_descendants() {
    let mut first = Message::assistant();
    first.apply(tool("bg", ToolStatus::Background));
    first.apply(child("c1", "bg", ToolStatus::Running));
    first.apply(child("g1", "c1", ToolStatus::Waiting));
    first.apply(child("d1", "bg", ToolStatus::Done));
    first.finish();
    let mut messages =
        vec![Message::user("go"), first, Message::user("next"), Message::assistant()];
    let stop = tool("bg", ToolStatus::Stopped);
    let at = route(&messages, 3, &stop);
    assert_eq!(at, 1);
    messages[at].apply(stop);
    assert_eq!(
        statuses(&messages[1]),
        [
            ("bg".to_string(), ToolStatus::Stopped),
            ("c1".to_string(), ToolStatus::Stopped),
            ("g1".to_string(), ToolStatus::Stopped),
            ("d1".to_string(), ToolStatus::Done),
        ]
    );
    assert!(messages[3].steps.is_empty(), "nothing lands in the live reply");

    let st = |m: &Message, id: &str| statuses(m).into_iter().find(|(i, _)| i == id).unwrap().1;
    let mut live = Message::assistant();
    live.apply(tool("bg", ToolStatus::Background));
    live.apply(child("c1", "bg", ToolStatus::Running));
    live.apply(child("g1", "c1", ToolStatus::Running));
    live.apply(child("c1", "bg", ToolStatus::Done));
    assert_eq!(st(&live, "g1"), ToolStatus::Stopped, "a child under a live agent ending");
    let mut fg = Message::assistant();
    fg.apply(tool("fg", ToolStatus::Running));
    fg.apply(child("c0", "fg", ToolStatus::Running));
    fg.apply(tool("fg", ToolStatus::Done));
    assert_eq!(st(&fg, "c0"), ToolStatus::Running, "foreground rows wait for finish");
}

#[test]
fn finish_closes_running_notices_except_rate_limits() {
    let rate = Notice { kind: NoticeKind::RateLimit, ..notice("rl", NoticeState::Running) };
    let mut ok = Message::assistant();
    ok.apply(Update::Notice(notice("r1", NoticeState::Running)));
    ok.apply(Update::Notice(rate.clone()));
    ok.finish();
    let mut failed = Message::assistant();
    failed.apply(Update::Notice(notice("r1", NoticeState::Running)));
    failed.apply(Update::Error("boom".into()));
    failed.finish();
    let states = |m: &Message| -> Vec<NoticeState> {
        m.steps
            .iter()
            .filter_map(|s| match s {
                Step::Notice(n) => Some(n.state),
                _ => None,
            })
            .collect()
    };
    assert_eq!(states(&ok), [NoticeState::Done, NoticeState::Running]);
    assert_eq!(states(&failed)[0], NoticeState::Failed);
}
