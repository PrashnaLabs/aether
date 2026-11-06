use super::*;
use std::collections::HashMap;

use adk_rust::serde_json::json;

fn event(parts: Vec<Part>, partial: bool) -> Result<Event> {
    let mut event = Event::new("inv");
    event.set_content(Content { role: "model".into(), parts });
    event.llm_response.partial = partial;
    Ok(event)
}

#[test]
fn maps_reasoning_text_and_search_steps() {
    let thinking = || Part::Thinking { thinking: "hmm".into(), signature: None };
    assert_eq!(
        to_updates(
            event(vec![thinking(), Part::Text { text: "Hel".into() }], true),
            &mut HashMap::new()
        ),
        [Update::Thinking("hmm".into()), Update::Delta("Hel".into())]
    );

    let call = Part::FunctionCall {
        name: "web_search".into(),
        args: json!({ "query": "rust" }),
        id: Some("c1".into()),
        thought_signature: None,
    };
    assert_eq!(
        to_updates(event(vec![thinking(), call], false), &mut HashMap::new()),
        [Update::Search { id: "c1".into(), query: "rust".into() }, Update::Final(String::new())]
    );

    let mut done = event(vec![Part::Text { text: "Hi".into() }], false).unwrap();
    done.llm_response.usage_metadata = Some(adk_rust::UsageMetadata {
        total_token_count: 1234,
        prompt_token_count: 1000,
        candidates_token_count: 234,
        ..Default::default()
    });
    assert_eq!(
        to_updates(Ok(done), &mut HashMap::new()),
        [
            Update::Usage(1234),
            Update::Count { prompt: 1000, completion: 234, cached: 0 },
            Update::Final("Hi".into())
        ]
    );

    assert_eq!(search_outcome(&json!({ "results": [1, 2, 3] })), Ok(3));
    assert_eq!(search_outcome(&json!({ "error": "rate limited" })), Err("rate limited".into()));
}

#[test]
fn partial_errors_and_cut_off_replies_are_warnings() {
    let mut chunk = event(vec![Part::Text { text: "Hi".into() }], true).unwrap();
    chunk.llm_response.error_message = Some("upstream hiccup".into());
    assert_eq!(
        to_updates(Ok(chunk), &mut HashMap::new()),
        [Update::Warning("upstream hiccup".into())]
    );
    let mut done = event(vec![Part::Text { text: "Hi".into() }], false).unwrap();
    done.llm_response.finish_reason = Some(adk_rust::FinishReason::MaxTokens);
    assert_eq!(
        to_updates(Ok(done), &mut HashMap::new()),
        [
            Update::Warning("The reply hit the model's output limit".into()),
            Update::Final("Hi".into())
        ]
    );
    let mut safe = event(vec![], false).unwrap();
    safe.llm_response.finish_reason = Some(adk_rust::FinishReason::Safety);
    assert_eq!(
        to_updates(Ok(safe), &mut HashMap::new()),
        [
            Update::Warning("The provider's safety filter stopped this reply".into()),
            Update::Final(String::new())
        ]
    );
    let mut failed = event(vec![], false).unwrap();
    failed.llm_response.error_message = Some("bad key".into());
    assert_eq!(to_updates(Ok(failed), &mut HashMap::new()), [Update::Error("bad key".into())]);
}

#[test]
fn native_lead_tool_emits_a_tool_row() {
    let mut inputs = HashMap::new();
    let args = json!({ "provider": "claude", "title": "Fix" });
    let call = Part::FunctionCall {
        name: "spawn_agent".into(),
        args: args.clone(),
        id: Some("c1".into()),
        thought_signature: None,
    };
    let detail = |output: &str| ToolDetail::Other {
        name: "spawn_agent".into(),
        input: adk_rust::serde_json::to_string_pretty(&args).unwrap(),
        output: output.into(),
    };
    let calling =
        to_updates(event(vec![Part::Text { text: "On it".into() }, call], false), &mut inputs);
    assert_eq!(
        calling,
        [Update::Final("On it".into()), Update::Tool(ToolCall::new("c1", "", detail("")))]
    );

    let respond = |response| Part::FunctionResponse {
        function_response: adk_rust::FunctionResponseData::new("spawn_agent", response),
        id: Some("c1".into()),
        annotations: None,
    };
    let done =
        ToolCall { status: ToolStatus::Done, ..ToolCall::new("c1", "", detail(r#"{"id":7}"#)) };
    assert_eq!(
        to_updates(event(vec![respond(json!({ "id": 7 }))], false), &mut inputs),
        [Update::Final(String::new()), Update::Tool(done)]
    );
    let failed = ToolCall {
        status: ToolStatus::Failed,
        error: Some("no folder".into()),
        ..ToolCall::new(
            "c1",
            "",
            ToolDetail::Other {
                name: "spawn_agent".into(),
                input: String::new(),
                output: r#"{"error":"no folder"}"#.into(),
            },
        )
    };
    assert_eq!(
        to_updates(event(vec![respond(json!({ "error": "no folder" }))], false), &mut inputs),
        [Update::Final(String::new()), Update::Tool(failed)]
    );

    let mut reply = aether_core::Message::assistant();
    reply.apply(Update::Delta("On it".into()));
    calling.into_iter().for_each(|u| reply.apply(u));
    assert_eq!(reply.text, "On it");
}

#[test]
fn native_lead_tool_rows_clip_long_input_and_output() {
    let mut inputs = HashMap::new();
    let call = Part::FunctionCall {
        name: "message_agent".into(),
        args: json!({ "id": 7, "text": "a".repeat(50_000) }),
        id: Some("c1".into()),
        thought_signature: None,
    };
    let response = Part::FunctionResponse {
        function_response: adk_rust::FunctionResponseData::new(
            "read_agent",
            json!({ "replies": ["line\n".repeat(10_000)] }),
        ),
        id: Some("c2".into()),
        annotations: None,
    };
    let rows = to_updates(event(vec![call, response], false), &mut inputs);
    let sizes: Vec<(usize, usize)> = rows
        .iter()
        .filter_map(|u| match u {
            Update::Tool(ToolCall { detail: ToolDetail::Other { input, output, .. }, .. }) => {
                Some((input.chars().count(), output.chars().count()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(sizes.len(), 2);
    assert!(sizes[0].0 <= 4_001, "input clipped: {sizes:?}");
    assert!(sizes[1].1 < 17_000, "output clipped: {sizes:?}");
}
