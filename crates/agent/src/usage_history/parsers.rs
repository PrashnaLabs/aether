use std::collections::{HashMap, HashSet};
use std::io::BufRead;

use aether_core::Role;
use chrono::DateTime;
use serde_json::Value;

use super::{TokenTotals, UsageProvider, UsageRecord};
use crate::store::SavedChat;

fn parse_rfc3339_ms(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text).ok().map(|parsed| parsed.timestamp_millis())
}

pub(super) fn parse_any_timestamp_ms(value: &Value) -> Option<i64> {
    if let Some(s) = value.as_str() {
        return parse_rfc3339_ms(s);
    }
    if let Some(n) = value.as_f64().filter(|n| n.is_finite() && *n > 0.0) {
        let raw = n as i64;
        return Some(if raw > 100_000_000_000 { raw } else { raw * 1000 });
    }
    if let Some(n) = value.as_i64().filter(|n| *n > 0) {
        return Some(if n > 100_000_000_000 { n } else { n * 1000 });
    }
    None
}

pub(super) fn parse_claude_line(line: &str) -> Option<UsageRecord> {
    let record: Value = serde_json::from_str(line).ok()?;
    if record.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let timestamp_ms = parse_rfc3339_ms(record.get("timestamp")?.as_str()?)?;
    let message = record.get("message")?;
    let model = message.get("model")?.as_str()?.trim();
    if model.is_empty() {
        return None;
    }
    let usage = message.get("usage")?;
    let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let totals = TokenTotals {
        uncached_input: count("input_tokens"),
        cached_input: count("cache_read_input_tokens"),
        cache_creation: count("cache_creation_input_tokens"),
        output: count("output_tokens"),
        reasoning: 0,
    };
    if totals.total() == 0 {
        return None;
    }
    let session_id = record.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_owned();
    let project = record.get("cwd").and_then(Value::as_str).unwrap_or_default().to_owned();
    let reported_cost_usd = record
        .get("costUSD")
        .and_then(Value::as_f64)
        .filter(|cost| cost.is_finite() && *cost > 0.0);
    let dedupe_key = match (
        message.get("id").and_then(Value::as_str),
        record.get("requestId").and_then(Value::as_str),
    ) {
        (Some(msg_id), Some(req_id)) if !msg_id.is_empty() && !req_id.is_empty() => {
            Some(format!("{msg_id}:{req_id}"))
        }
        _ => None,
    };
    Some(UsageRecord {
        provider: UsageProvider::Claude,
        timestamp_ms,
        model: model.to_owned(),
        session_id,
        project,
        totals,
        reported_cost_usd,
        dedupe_key,
    })
}

const CODEX_FORK_COPY_GAP_MS: i64 = 1_000;

pub struct CodexScanState {
    model: Option<String>,
    session_id: String,
    cwd: String,
    forked: bool,
    suppressing_copied_history: bool,
    last_copied_event_ms: Option<i64>,
    last_signature: Option<(u64, u64, u64, u64, u64)>,
}

impl Default for CodexScanState {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexScanState {
    pub fn new() -> Self {
        Self {
            model: None,
            session_id: String::new(),
            cwd: String::new(),
            forked: false,
            suppressing_copied_history: false,
            last_copied_event_ms: None,
            last_signature: None,
        }
    }
}

fn codex_meta_is_forked(payload: &Value) -> bool {
    let non_empty = |v: Option<&Value>| v.and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    non_empty(payload.get("forked_from_id"))
        || non_empty(payload.pointer("/source/subagent/thread_spawn/parent_thread_id"))
}

pub(super) fn parse_codex_line(line: &str, state: &mut CodexScanState) -> Option<UsageRecord> {
    let record: Value = serde_json::from_str(line).ok()?;
    let kind = record.get("type")?.as_str()?;
    let payload = record.get("payload")?;
    match kind {
        "session_meta" => {
            let meta_id = payload.get("id").and_then(Value::as_str).unwrap_or_default();
            if state.session_id.is_empty() {
                if !meta_id.is_empty() {
                    state.session_id = meta_id.to_owned();
                }
                if let Some(cwd) =
                    payload.get("cwd").and_then(Value::as_str).filter(|cwd| !cwd.is_empty())
                {
                    state.cwd = cwd.to_owned();
                }
                state.forked = codex_meta_is_forked(payload);
            } else if meta_id == state.session_id {
                state.suppressing_copied_history = false;
                state.last_copied_event_ms = None;
            } else if state.forked && !meta_id.is_empty() {
                state.suppressing_copied_history = true;
                state.last_copied_event_ms =
                    record.get("timestamp").and_then(Value::as_str).and_then(parse_rfc3339_ms);
            }
            None
        }
        "turn_context" => {
            if let Some(model) = payload
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|m| !m.is_empty())
            {
                state.model = Some(model.to_owned());
            }
            if state.cwd.is_empty()
                && let Some(cwd) =
                    payload.get("cwd").and_then(Value::as_str).filter(|cwd| !cwd.is_empty())
            {
                state.cwd = cwd.to_owned();
            }
            None
        }
        "event_msg" => {
            if payload.get("type")?.as_str()? != "token_count" {
                return None;
            }
            let timestamp_ms = parse_rfc3339_ms(record.get("timestamp")?.as_str()?)?;
            if state.suppressing_copied_history {
                match state.last_copied_event_ms {
                    Some(previous_ms)
                        if timestamp_ms.saturating_sub(previous_ms) < CODEX_FORK_COPY_GAP_MS =>
                    {
                        state.last_copied_event_ms = Some(timestamp_ms);
                        return None;
                    }
                    _ => {
                        state.suppressing_copied_history = false;
                        state.last_copied_event_ms = None;
                    }
                }
            }
            let model = state.model.clone()?;
            let usage = payload.get("info")?.get("last_token_usage")?;
            let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
            let input_tokens = count("input_tokens");
            let cached_input = count("cached_input_tokens");
            let cache_creation = count("cache_write_input_tokens");
            let output = count("output_tokens");
            let reasoning = count("reasoning_output_tokens");

            let signature = (input_tokens, cached_input, cache_creation, output, reasoning);
            if state.last_signature == Some(signature) {
                return None;
            }
            state.last_signature = Some(signature);

            let totals = TokenTotals {
                uncached_input: input_tokens.saturating_sub(cached_input + cache_creation),
                cached_input,
                cache_creation,
                output,
                reasoning,
            };
            if totals.total() == 0 {
                return None;
            }
            Some(UsageRecord {
                provider: UsageProvider::Codex,
                timestamp_ms,
                model,
                session_id: state.session_id.clone(),
                project: state.cwd.clone(),
                totals,
                reported_cost_usd: None,
                dedupe_key: None,
            })
        }
        _ => None,
    }
}

pub(super) fn parse_pi_records(
    reader: impl BufRead,
    fallback_session_id: &str,
) -> Vec<UsageRecord> {
    let mut session_id = fallback_session_id.to_owned();
    let mut cwd = String::new();
    let mut active_model = String::new();
    let mut records = Vec::new();
    let mut seen = HashSet::new();

    for line in reader.lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let kind = v.get("type").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "session" => {
                if let Some(id) = v.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    session_id = id.to_owned();
                }
                if let Some(dir) = v.get("cwd").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    cwd = dir.to_owned();
                }
            }
            "model_change" => {
                if let Some(model) = v
                    .get("modelId")
                    .or_else(|| v.get("model"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    active_model = model.to_owned();
                }
            }
            "message" => {
                let Some(msg) = v.get("message") else {
                    continue;
                };
                if msg.get("role").and_then(Value::as_str) != Some("assistant") {
                    continue;
                }
                let Some(usage) = msg.get("usage") else {
                    continue;
                };
                let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
                let totals = TokenTotals {
                    uncached_input: count("input"),
                    cached_input: count("cacheRead"),
                    cache_creation: count("cacheWrite"),
                    output: count("output"),
                    reasoning: count("reasoning"),
                };
                if totals.total() == 0 {
                    continue;
                }
                let Some(timestamp_ms) = msg
                    .get("timestamp")
                    .and_then(parse_any_timestamp_ms)
                    .or_else(|| v.get("timestamp").and_then(parse_any_timestamp_ms))
                else {
                    continue;
                };
                let model = msg
                    .get("model")
                    .or_else(|| msg.get("modelId"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        if active_model.is_empty() {
                            "pi-default".to_owned()
                        } else {
                            active_model.clone()
                        }
                    });
                let reported_cost_usd = usage
                    .pointer("/cost/total")
                    .and_then(Value::as_f64)
                    .or_else(|| usage.get("cost").and_then(Value::as_f64))
                    .filter(|c| c.is_finite() && *c > 0.0);
                let dedupe_key = v
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(|id| format!("pi:{session_id}:{id}"));
                if let Some(key) = &dedupe_key
                    && !seen.insert(key.clone())
                {
                    continue;
                }
                records.push(UsageRecord {
                    provider: UsageProvider::Pi,
                    timestamp_ms,
                    model,
                    session_id: session_id.clone(),
                    project: cwd.clone(),
                    totals,
                    reported_cost_usd,
                    dedupe_key,
                });
            }
            _ => {}
        }
    }
    records
}

fn normalize_antigravity_model(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('.').split('(').next().unwrap_or(raw).trim();
    if trimmed.is_empty() {
        return "gemini-2.5-pro".to_owned();
    }
    let lower = trimmed.to_ascii_lowercase().replace(' ', "-");
    if lower.starts_with("gemini-4-") || lower.starts_with("gemini-3") {
        // Map internal codenames (e.g. Gemini 4 Argon) to a priceable Gemini Pro tier
        // while keeping recognizable model names intact.
        if lower.contains("flash") {
            return "gemini-2.5-flash".to_owned();
        }
        return "gemini-2.5-pro".to_owned();
    }
    lower
}

fn extract_antigravity_model_change(content: &str) -> Option<String> {
    let marker = "changed setting `Model Selection` from ";
    let idx = content.find(marker)?;
    let after = &content[idx + marker.len()..];
    let to_idx = after.find(" to ")?;
    let rest = &after[to_idx + 4..];
    let end = [rest.find(". "), rest.find('\n'), rest.find('<')]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(rest.len());
    let model = rest[..end].trim().trim_end_matches('.');
    (!model.is_empty() && !model.eq_ignore_ascii_case("none"))
        .then(|| normalize_antigravity_model(model))
}

fn unquote_arg_string(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(parsed) = serde_json::from_str::<String>(trimmed) {
        return parsed;
    }
    trimmed.trim_matches('"').to_owned()
}

pub(super) fn parse_antigravity_records(
    reader: impl BufRead,
    session_id: &str,
) -> Vec<UsageRecord> {
    let mut active_model = "gemini-2.5-pro".to_owned();
    let mut discovered_cwd = String::new();
    let mut records = Vec::new();
    let mut seen = HashSet::new();

    for line in reader.lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let step_type = v.get("type").and_then(Value::as_str).unwrap_or_default();
        if step_type == "USER_INPUT" {
            if let Some(content) = v.get("content").and_then(Value::as_str)
                && let Some(model) = extract_antigravity_model_change(content)
            {
                active_model = model;
            }
            continue;
        }
        if step_type != "PLANNER_RESPONSE" {
            continue;
        }

        if discovered_cwd.is_empty()
            && let Some(calls) = v.get("tool_calls").and_then(Value::as_array)
        {
            for call in calls {
                if let Some(cwd_val) = call
                    .pointer("/args/Cwd")
                    .or_else(|| call.pointer("/args/DirectoryPath"))
                    .and_then(Value::as_str)
                {
                    let unquoted = unquote_arg_string(cwd_val);
                    if unquoted.starts_with('/') {
                        discovered_cwd = unquoted;
                        break;
                    }
                }
            }
        }

        let count = |key: &str| v.get(key).and_then(Value::as_u64).unwrap_or(0);
        let totals = TokenTotals {
            uncached_input: count("input_tokens"),
            cached_input: count("cache_read_tokens"),
            cache_creation: count("cache_creation_tokens"),
            output: count("output_tokens"),
            reasoning: count("thinking_tokens"),
        };
        if totals.total() == 0 {
            continue;
        }
        let Some(timestamp_ms) =
            v.get("created_at").and_then(Value::as_str).and_then(parse_rfc3339_ms)
        else {
            continue;
        };
        let step_index = v.get("step_index").and_then(Value::as_u64).unwrap_or(0);
        let dedupe_key = Some(format!("agy:{session_id}:{step_index}"));
        if let Some(key) = &dedupe_key
            && !seen.insert(key.clone())
        {
            continue;
        }
        let model = v
            .get("model")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(normalize_antigravity_model)
            .unwrap_or_else(|| active_model.clone());
        records.push(UsageRecord {
            provider: UsageProvider::Antigravity,
            timestamp_ms,
            model,
            session_id: session_id.to_owned(),
            project: discovered_cwd.clone(),
            totals,
            reported_cost_usd: None,
            dedupe_key,
        });
    }

    if !discovered_cwd.is_empty() {
        for record in &mut records {
            if record.project.is_empty() {
                record.project = discovered_cwd.clone();
            }
        }
    }
    records
}

pub(super) fn parse_opencode_message_json(
    text: &str,
    session_dirs: &HashMap<String, String>,
) -> Option<UsageRecord> {
    let v: Value = serde_json::from_str(text).ok()?;
    let role = v.get("role").or_else(|| v.get("type")).and_then(Value::as_str)?;
    if role != "assistant" {
        return None;
    }
    let tokens = v.get("tokens")?;
    let count = |val: Option<&Value>| val.and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as u64;
    let totals = TokenTotals {
        uncached_input: count(tokens.get("input")),
        cached_input: count(tokens.pointer("/cache/read")),
        cache_creation: count(tokens.pointer("/cache/write")),
        output: count(tokens.get("output")),
        reasoning: count(tokens.get("reasoning")),
    };
    if totals.total() == 0 {
        return None;
    }
    let timestamp_ms = v
        .pointer("/time/created")
        .or_else(|| v.pointer("/time/completed"))
        .and_then(parse_any_timestamp_ms)?;
    let session_id = v.get("sessionID").and_then(Value::as_str).unwrap_or_default().to_owned();
    let model = v
        .get("modelID")
        .or_else(|| v.pointer("/model/id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("opencode")
        .to_owned();
    let project = v
        .pointer("/path/cwd")
        .or_else(|| v.pointer("/location/directory"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| session_dirs.get(&session_id).cloned())
        .unwrap_or_default();
    let reported_cost_usd =
        v.get("cost").and_then(Value::as_f64).filter(|c| c.is_finite() && *c > 0.0);
    let msg_id = v.get("id").and_then(Value::as_str).unwrap_or_default();
    let dedupe_key = (!msg_id.is_empty()).then(|| format!("oc:{session_id}:{msg_id}"));
    Some(UsageRecord {
        provider: UsageProvider::OpenCode,
        timestamp_ms,
        model,
        session_id,
        project,
        totals,
        reported_cost_usd,
        dedupe_key,
    })
}

pub fn collect_saved_chat_records(chats: &[SavedChat]) -> Vec<UsageRecord> {
    let mut records = Vec::new();
    for chat in chats {
        let provider = UsageProvider::from_aether(chat.provider);
        let session_id = if !chat.cli_session.is_empty() {
            chat.cli_session.clone()
        } else {
            format!("aether-chat-{}", chat.id)
        };
        let project =
            if !chat.project.is_empty() { chat.project.clone() } else { chat.cwd.clone() };
        for (ix, message) in chat.messages.iter().enumerate() {
            if message.role != Role::Assistant {
                continue;
            }
            let total_io = message.prompt_tokens + message.completion_tokens;
            if total_io == 0 {
                continue;
            }
            let uncached_input = message.prompt_tokens.saturating_sub(message.cached_tokens);
            let totals = TokenTotals {
                uncached_input,
                cached_input: message.cached_tokens,
                cache_creation: 0,
                output: message.completion_tokens,
                reasoning: 0,
            };
            let timestamp_ms = message.ended_at_ms.filter(|ms| *ms > 0).unwrap_or(chat.id);
            let model = message
                .model
                .as_deref()
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| {
                    if chat.provider_model.is_empty() {
                        chat.provider.key()
                    } else {
                        chat.provider_model.as_str()
                    }
                })
                .to_owned();
            records.push(UsageRecord {
                provider,
                timestamp_ms,
                model,
                session_id: session_id.clone(),
                project: project.clone(),
                totals,
                reported_cost_usd: None,
                dedupe_key: Some(format!("aether:{}:{ix}", chat.id)),
            });
        }
    }
    records
}

#[cfg(test)]
#[path = "../../tests/unit/usage_history/parsers.rs"]
mod tests;
