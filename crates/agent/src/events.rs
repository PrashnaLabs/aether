use std::collections::HashMap;

use adk_rust::prelude::*;
use adk_rust::serde_json::Value;
use aether_core::{ToolCall, ToolDetail, ToolStatus, Update};
use aether_driver::{INPUT_CHARS, clip, head_tail};

use crate::tools::{load_skill, web_search};

/// `inputs` carries lead tool args from call to result, which only has the output.
pub(crate) fn to_updates(
    event: Result<Event>,
    inputs: &mut HashMap<String, String>,
) -> Vec<Update> {
    let event = match event {
        Ok(event) => event,
        Err(e) => return vec![Update::Error(e.to_string())],
    };
    let partial = event.llm_response.partial;
    if let Some(error) = &event.llm_response.error_message {
        let error = error.clone();
        return vec![if partial { Update::Warning(error) } else { Update::Error(error) }];
    }
    let mut updates = Vec::new();
    if let Some(usage) = &event.llm_response.usage_metadata
        && usage.total_token_count > 0
    {
        updates.push(Update::Usage(usage.total_token_count as u64));
        updates.push(Update::Count {
            prompt: usage.prompt_token_count.max(0) as u64,
            completion: usage.candidates_token_count.max(0) as u64,
            cached: usage.cache_read_input_token_count.unwrap_or(0).max(0) as u64,
        });
    }
    let mut rows = Vec::new();
    let mut text = String::new();
    for part in event.content().into_iter().flat_map(|c| &c.parts) {
        match part {
            // Final events repeat the turn's reasoning; only deltas are new.
            Part::Thinking { thinking, .. } if partial => {
                updates.push(Update::Thinking(thinking.clone()))
            }
            Part::Text { text: t } => text.push_str(t),
            _ => {}
        }
    }
    if !partial {
        for call in event.tool_calls() {
            let arg = |key| call.args.get(key).and_then(Value::as_str);
            match call.name {
                web_search::NAME => updates.push(Update::Search {
                    id: call.call_id.unwrap_or_default().to_string(),
                    query: arg("query").unwrap_or_default().to_string(),
                }),
                load_skill::NAME => {
                    updates.push(Update::Skill(arg("name").unwrap_or_default().to_string()))
                }
                name if is_lead_tool(name) => {
                    let id = call.call_id.unwrap_or(name).to_string();
                    let input = clip(
                        &adk_rust::serde_json::to_string_pretty(call.args).unwrap_or_default(),
                        INPUT_CHARS,
                    );
                    inputs.insert(id.clone(), input.clone());
                    let detail =
                        ToolDetail::Other { name: name.to_string(), input, output: String::new() };
                    rows.push(Update::Tool(ToolCall::new(id, "", detail)))
                }
                // ask_user and propose_plan show up as request cards instead.
                _ => {}
            }
        }
        for result in event.tool_results() {
            if result.name == web_search::NAME {
                updates.push(Update::SearchDone {
                    id: result.call_id.unwrap_or_default().to_string(),
                    outcome: search_outcome(result.response),
                });
            } else if is_lead_tool(result.name) {
                let error = result.response.get("error").and_then(Value::as_str);
                let id = result.call_id.unwrap_or(result.name).to_string();
                let detail = ToolDetail::Other {
                    name: result.name.to_string(),
                    input: inputs.remove(&id).unwrap_or_default(),
                    output: head_tail(&result.response.to_string()),
                };
                rows.push(Update::Tool(ToolCall {
                    status: if error.is_some() { ToolStatus::Failed } else { ToolStatus::Done },
                    error: error.map(str::to_string),
                    ..ToolCall::new(id, "", detail)
                }));
            }
        }
    }
    if !partial {
        match event.llm_response.finish_reason {
            Some(adk_rust::FinishReason::MaxTokens) => {
                updates.push(Update::Warning("The reply hit the model's output limit".into()))
            }
            Some(adk_rust::FinishReason::Safety) => updates
                .push(Update::Warning("The provider's safety filter stopped this reply".into())),
            _ => {}
        }
    }
    updates.push(if partial { Update::Delta(text) } else { Update::Final(text) });
    // after Final: a new tool row moves turn_start, so Final would repeat the streamed text
    updates.extend(rows);
    updates
}

fn is_lead_tool(name: &str) -> bool {
    crate::orchestrator::tool_specs().as_array().into_iter().flatten().any(|s| s["name"] == name)
}

fn search_outcome(response: &Value) -> std::result::Result<usize, String> {
    match response.get("results").and_then(Value::as_array) {
        Some(results) => Ok(results.len()),
        None => Err(response.get("error").and_then(Value::as_str).unwrap_or("failed").to_string()),
    }
}

#[cfg(test)]
#[path = "../tests/unit/events.rs"]
mod tests;
