use std::time::Instant;

use serde_json::{Map, Value, json};

use aether_core::{
    AccessMode, Notice, NoticeKind, NoticeState, PlanItem, PlanStatus, ToolCall, ToolDetail,
    ToolStatus, Update, without_attached_files,
};

use crate::spawn::log_unknown;
use crate::tool::{
    apply_hunks, child_activity, finish, halted, now_ms, result_text, save_images, start,
};

use super::approve::{DENIED, control_request};
use super::{
    ACTIVITY_CHARS, BTW_REQUEST, ClaudeAsk, INTERRUPT_REQUEST, MODEL_REQUEST, STOP_REQUEST, State,
    TITLE_REQUEST, TODOS_ID, on_interrupted, quiet_turn_end, side_answer,
};

pub(crate) fn claude_mode(mode: &str) -> Option<AccessMode> {
    let found = match mode {
        "default" | "manual" => AccessMode::Ask,
        "acceptEdits" => AccessMode::AutoEdit,
        "auto" => AccessMode::Auto,
        "bypassPermissions" => AccessMode::Full,
        "plan" | "dontAsk" => AccessMode::ReadOnly,
        _ => {
            log_unknown("claude", &format!("permissionMode {mode}"));
            return None;
        }
    };
    Some(found)
}

/// `Mode`, plus a notice when Claude runs in a mode other than the one asked for.
pub(crate) fn mode_updates(s: &mut State, raw: &str) -> Vec<Update> {
    let Some(mode) = claude_mode(raw) else { return vec![] };
    if s.mode == Some(mode) {
        return vec![];
    }
    s.mode = Some(mode);
    let mut out = vec![Update::Mode(mode)];
    if mode != s.requested {
        s.modes += 1;
        let text = format!(
            "Claude is in {raw} mode. {} isn't available right now (plan, model or fast mode).",
            s.requested.label()
        );
        out.push(Update::Notice(Notice::new(
            format!("mode-{}", s.modes),
            NoticeKind::ModeChange,
            text,
        )));
    }
    out
}

pub(crate) fn mcp_updates(servers: &Value) -> Vec<Update> {
    let mut out = vec![];
    for server in servers.as_array().into_iter().flatten() {
        let name = server["name"].as_str().unwrap_or_default();
        match server["status"].as_str() {
            Some("failed") => {
                out.push(Update::Warning(format!("MCP server {name} failed to start")));
            }
            Some("needs-auth") => {
                let text =
                    format!("{name} needs you to sign in. Run /mcp in Claude Code's terminal.");
                out.push(Update::Notice(Notice::new(
                    format!("mcp-{name}"),
                    NoticeKind::McpAuth,
                    text,
                )));
            }
            _ => {}
        }
    }
    out
}

pub(crate) fn map_line(s: &mut State, line: &str) -> Vec<Update> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![];
    };
    let woke = wake(s, &v);
    let mut out = map_value(s, &v);
    if matches!(v["type"].as_str(), Some("stream_event" | "assistant"))
        && let Some(retry) = s.retry.take()
    {
        out.insert(0, Update::Notice(Notice { state: NoticeState::Done, until_ms: None, ..retry }));
    }
    if woke {
        s.wakes += 1;
        let text = match s.wake_reason.take() {
            Some(why) => format!("Claude started a turn on its own: {why}"),
            None => "Claude started a turn on its own".into(),
        };
        let id = format!("wake-{}", s.wakes);
        out.splice(0..0, [Update::Wake, Update::Notice(Notice::new(id, NoticeKind::Wake, text))]);
    }
    out
}

pub(crate) fn stop_request(s: &State, row: &str, n: u64) -> Option<Value> {
    let (task, _) = s.tasks.iter().find(|(_, (id, _))| id == row)?;
    Some(json!({
        "type": "control_request",
        "request_id": format!("{STOP_REQUEST}{n}"),
        "request": {"subtype": "stop_task", "task_id": task},
    }))
}

/// Splits the harness trailer (agent id, usage block, hand-back frame) off a subagent's result.
pub(crate) fn strip_agent_trailer(text: &str) -> (String, Option<(u64, u32)>) {
    let body = match text.strip_prefix("[Subagent hand-back]") {
        Some(rest) => rest.split_once("The report follows:\n").map_or(rest, |(_, r)| r),
        None => text,
    };
    let framed = text.starts_with("[Subagent hand-back]");
    let mut usage = None;
    let mut kept = Vec::new();
    let mut block = String::new();
    let lines: Vec<&str> = body.lines().collect();
    let usage_at =
        lines.iter().rposition(|l| l.trim_start().starts_with("<usage>")).unwrap_or(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i >= usage_at {
            block.push_str(&line.replace("<usage>", "").replace("</usage>", ""));
            block.push('\n');
            continue;
        }
        let trailer = line.starts_with("agentId: ")
            && (line.contains("(internal ID") || line.contains("SendMessage"));
        if !trailer {
            kept.push(if framed { line.strip_prefix("  ").unwrap_or(line) } else { line });
        }
    }
    let field = |keys: &[&str]| {
        block.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            keys.contains(&k.trim()).then(|| v.trim().parse::<u64>().ok()).flatten()
        })
    };
    if let Some(tokens) = field(&["total_tokens", "subagent_tokens"]) {
        usage = Some((tokens, field(&["tool_uses"]).unwrap_or(0) as u32));
    }
    (kept.join("\n").trim().to_string(), usage)
}

pub(crate) fn touch(s: &mut State, id: &str, f: impl FnOnce(&mut ToolCall) -> bool) -> Vec<Update> {
    let Some((call, _)) = s.tools.get_mut(id) else { return vec![] };
    if f(call) { vec![Update::Tool(call.clone())] } else { vec![] }
}

pub(crate) fn set_activity(s: &mut State, parent: &str, text: &str) -> Vec<Update> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let line: String = line.chars().take(ACTIVITY_CHARS).collect();
    touch(s, parent, |call| match &mut call.detail {
        ToolDetail::Task { activity, .. } => {
            *activity = line;
            true
        }
        _ => false,
    })
}

pub(crate) fn append_output(detail: &mut ToolDetail, extra: &str) {
    if let ToolDetail::Command { output, .. }
    | ToolDetail::Search { output, .. }
    | ToolDetail::Web { output, .. }
    | ToolDetail::Task { output, .. }
    | ToolDetail::Other { output, .. } = detail
    {
        output.push_str(extra);
        if output.starts_with('\n') {
            *output = output.trim_start().to_string();
        }
    }
}

pub(crate) fn task_line(s: &mut State, subtype: &str, v: &Value) -> Vec<Update> {
    let task = v["task_id"].as_str().unwrap_or_default().to_string();
    let text = |k: &str| v[k].as_str().filter(|t| !t.is_empty()).map(str::to_string);
    if subtype == "task_started" {
        if let Some(row) = text("tool_use_id") {
            s.tasks.insert(task.clone(), (row, text("description").unwrap_or_default()));
        }
        if let Some(kind) = text("subagent_type") {
            s.agents.insert(task.clone(), kind);
        }
    }
    let row =
        s.tasks.get(&task).map(|t| t.0.clone()).or_else(|| text("tool_use_id")).unwrap_or_default();
    match subtype {
        "task_started" => touch(s, &row, |call| match &mut call.detail {
            ToolDetail::Task { agent, prompt, .. } if agent.is_empty() || prompt.is_empty() => {
                if agent.is_empty() {
                    *agent = text("subagent_type").unwrap_or_default();
                }
                if prompt.is_empty() {
                    *prompt = text("prompt").unwrap_or_default();
                }
                true
            }
            _ => false,
        }),
        "task_progress" => {
            let said = text("summary").or_else(|| text("description"));
            touch(s, &row, |call| match &mut call.detail {
                ToolDetail::Task { tokens, tools, activity, .. } => {
                    if let Some(n) = v["usage"]["total_tokens"].as_u64() {
                        *tokens = n;
                    }
                    if let Some(n) = v["usage"]["tool_uses"].as_u64() {
                        *tools = n as u32;
                    }
                    if let Some(said) = said {
                        *activity = said;
                    }
                    true
                }
                _ => false,
            })
        }
        "task_updated" => {
            let patch = &v["patch"];
            let status = match patch["status"].as_str() {
                Some("failed") => ToolStatus::Failed,
                Some("killed") => ToolStatus::Stopped,
                Some("completed") => ToolStatus::Done,
                _ if patch["is_backgrounded"] == true => ToolStatus::Background,
                _ => return vec![],
            };
            if status == ToolStatus::Background {
                s.background.insert(row.clone());
            } else {
                s.tasks.remove(&task);
            }
            let error = patch["error"].as_str().map(str::to_string);
            touch(s, &row, |call| {
                call.status = status;
                call.error = error.filter(|_| status == ToolStatus::Failed);
                true
            })
        }
        _ => {
            let status = v["status"].as_str().unwrap_or_default();
            let summary = text("summary").unwrap_or_default();
            if !s.background.remove(&row) {
                return vec![];
            }
            let described = s.tasks.remove(&task).map(|t| t.1).filter(|d| !d.is_empty());
            let Some((mut call, at)) = s.tools.remove(&row) else { return vec![] };
            let name = described.unwrap_or_else(|| summary.clone());
            s.wake_reason = Some(format!("background task \"{name}\" {status}"));
            call.status = match status {
                "completed" => ToolStatus::Done,
                "failed" => ToolStatus::Failed,
                "stopped" | "killed" => ToolStatus::Stopped,
                other => {
                    log_unknown("claude", &format!("task status {other}"));
                    ToolStatus::Done
                }
            };
            call.error = (call.status == ToolStatus::Failed).then(|| summary.clone());
            let mut extra = format!("\n\n{summary}");
            if let Some(file) = text("output_file") {
                extra.push_str(&format!("\n\nFull output: {file}"));
            }
            append_output(&mut call.detail, &extra);
            if let ToolDetail::Task { tokens, tools, .. } = &mut call.detail {
                if let Some(n) = v["usage"]["total_tokens"].as_u64() {
                    *tokens = n;
                }
                if let Some(n) = v["usage"]["tool_uses"].as_u64() {
                    *tools = n as u32;
                }
            }
            call.ms = Some(at.elapsed().as_millis() as u64);
            vec![Update::Tool(call)]
        }
    }
}

/// Opens a turn for model output or a permission ask that arrives while idle: the CLI
/// starts one itself when background work it was told about finishes.
pub(crate) fn wake(s: &mut State, v: &Value) -> bool {
    let main = !v["parent_tool_use_id"].is_string();
    let output = main && matches!(v["type"].as_str(), Some("stream_event" | "assistant"));
    let ask = v["type"] == "control_request"
        && matches!(v["request"]["subtype"].as_str(), Some("can_use_tool" | "elicitation"));
    if output {
        s.quiet_wake = false;
    }
    if s.active || s.dead.is_some() || !(output || ask) {
        return false;
    }
    open_turn(s);
    s.quiet_wake = !output;
    true
}

pub(crate) fn open_turn(s: &mut State) {
    s.active = true;
    s.turn += 1;
    s.echoes.clear();
    s.steer_ids.clear();
    s.turn_rows.clear();
}

/// The ended reply's cards expire, but a background agent's ask still waits in the CLI: it
/// goes out again under a fresh id, opening a side turn like an idle ask.
pub(crate) fn background_asks(s: &mut State) -> Vec<Update> {
    let mut kept: Vec<(u64, ClaudeAsk)> = std::mem::take(&mut s.asks)
        .into_iter()
        .filter(|(_, a)| a.agent.as_deref().is_some_and(|agent| backgrounded(s, agent)))
        .collect();
    if kept.is_empty() {
        return vec![];
    }
    kept.sort_unstable_by_key(|(id, _)| *id);
    open_turn(s);
    s.quiet_wake = true;
    kept.into_iter()
        .map(|(_, ask)| {
            s.next_ask += 1;
            let card = ask.card.clone();
            s.asks.insert(s.next_ask, ask);
            Update::Ask { id: s.next_ask, ask: card }
        })
        .collect()
}

/// Task `agent`'s row is background work, or runs under it.
pub(crate) fn backgrounded(s: &State, agent: &str) -> bool {
    let mut row = s.tasks.get(agent).map(|t| t.0.clone());
    for _ in 0..=s.tools.len() {
        let Some(id) = row else { return false };
        if s.background.contains(&id) {
            return true;
        }
        row = s.tools.get(&id).and_then(|(c, _)| c.parent.clone());
    }
    false
}

pub(crate) fn map_value(s: &mut State, v: &Value) -> Vec<Update> {
    let sub = v["parent_tool_use_id"].is_string();
    let blocks = |kind: &'static str| {
        v["message"]["content"].as_array().into_iter().flatten().filter(move |b| b["type"] == kind)
    };
    match v["type"].as_str() {
        Some("system") => match v["subtype"].as_str() {
            Some("init") => {
                s.model = v["model"].as_str().unwrap_or_default().to_string();
                let mut out = vec![];
                if let Some(id) = v["session_id"].as_str().filter(|id| *id != s.session) {
                    s.session = id.to_string();
                    out.push(Update::Session(id.to_string()));
                }
                if !s.model.is_empty() {
                    out.push(Update::Model(s.model.clone()));
                }
                out.extend(
                    v["permissionMode"].as_str().into_iter().flat_map(|m| mode_updates(s, m)),
                );
                out.extend(mcp_updates(&v["mcp_servers"]));
                out
            }
            Some("status") => system_status(s, v),
            Some("compact_boundary") => {
                let meta = &v["compact_metadata"];
                let (pre, post) = (meta["pre_tokens"].as_u64(), meta["post_tokens"].as_u64());
                let text = match (pre, post) {
                    (Some(pre), Some(post)) => format!("Compacted {pre} → {post} tokens"),
                    (Some(pre), None) => format!("Compacted {pre} tokens"),
                    _ => "Compacted context".into(),
                };
                let open = open_compaction(s);
                let done = Notice { text, state: NoticeState::Done, ..open };
                let mut out = vec![Update::Notice(done)];
                out.extend(post.filter(|n| *n > 0).map(Update::Usage));
                out
            }
            Some("api_retry") => {
                let n = |k: &str| v[k].as_u64().unwrap_or(0);
                let why = [
                    v["error_status"].as_u64().map(|c| c.to_string()),
                    v["error"].as_str().map(str::to_string),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
                let retry = Notice {
                    state: NoticeState::Running,
                    until_ms: Some(now_ms() + n("retry_delay_ms") as i64),
                    ..Notice::new(
                        format!("retry-{}", s.turn),
                        NoticeKind::Retry,
                        format!("Retrying ({}/{}): {why}", n("attempt"), n("max_retries")),
                    )
                };
                s.retry = Some(retry.clone());
                vec![Update::Notice(retry)]
            }
            Some("permission_denied") => {
                let id = v["tool_use_id"].as_str().unwrap_or_default();
                let why = ["decision_reason", "message"]
                    .iter()
                    .find_map(|k| v[*k].as_str().filter(|t| !t.is_empty()))
                    .map(str::to_string);
                touch(s, id, |call| {
                    call.status = ToolStatus::Denied;
                    call.error = why;
                    true
                })
            }
            Some("informational") => {
                let content = v["content"].as_str().unwrap_or_default().to_string();
                if v["level"] == "warning" {
                    return vec![Update::Warning(content)];
                }
                let id = v["uuid"].as_str().map_or_else(|| s.turn.to_string(), str::to_string);
                vec![Update::Notice(Notice::new(format!("info-{id}"), NoticeKind::Info, content))]
            }
            Some("notification") => {
                let id = v["key"].as_str().unwrap_or_default();
                let text = v["text"].as_str().unwrap_or_default();
                vec![Update::Notice(Notice::new(format!("info-{id}"), NoticeKind::Info, text))]
            }
            Some(kind @ ("model_fallback" | "model_refusal_fallback")) => {
                let model = |k: &str| v[k].as_str().unwrap_or("unknown").to_string();
                let (from, to) = (model("original_model"), model("fallback_model"));
                s.models += 1;
                let mut out = vec![Update::Notice(Notice::new(
                    format!("model-{}", s.models),
                    NoticeKind::ModelChange,
                    format!("Switched from {from} to {to}"),
                ))];
                if to != "unknown" {
                    s.model = to.clone();
                    out.push(Update::Model(to));
                }
                let gone: Vec<&str> = v["retracted_message_uuids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                if kind == "model_refusal_fallback" && !gone.is_empty() {
                    s.turn_texts.retain(|(id, _)| !gone.contains(&id.as_str()));
                    let kept: Vec<&str> = s.turn_texts.iter().map(|(_, t)| t.as_str()).collect();
                    out.push(Update::Replace(kept.join("\n\n")));
                }
                out
            }
            Some("model_refusal_no_fallback") => {
                vec![Update::Warning(v["content"].as_str().unwrap_or_default().to_string())]
            }
            Some("hook_started") => {
                vec![Update::Notice(Notice { state: NoticeState::Running, ..hook_notice(v) })]
            }
            Some("hook_response") => {
                let failed = v["outcome"] == "error";
                vec![Update::Notice(Notice {
                    state: if failed { NoticeState::Failed } else { NoticeState::Done },
                    detail: v["stderr"]
                        .as_str()
                        .filter(|e| failed && !e.is_empty())
                        .map(str::to_string),
                    ..hook_notice(v)
                })]
            }
            Some(t @ ("task_started" | "task_progress" | "task_updated" | "task_notification")) => {
                task_line(s, t, v)
            }
            // the tasks it lists already arrive as task_* lines
            Some(
                "background_tasks_changed"
                | "hook_progress"
                | "local_command"
                | "memory_recall"
                | "commands_changed"
                | "control_request_progress"
                | "thinking_tokens",
            ) => vec![],
            other => {
                log_unknown("claude", &format!("system/{}", other.unwrap_or_default()));
                vec![]
            }
        },
        Some("rate_limit_event") => rate_limit(s, &v["rate_limit_info"]),
        Some("stream_event") if sub => {
            let d = &v["event"]["delta"];
            let (Some("text_delta"), Some(delta)) = (d["type"].as_str(), d["text"].as_str()) else {
                return vec![];
            };
            let parent = v["parent_tool_use_id"].as_str().unwrap_or_default();
            let buf = s.sub_text.entry(parent.to_string()).or_default();
            if buf.trim_start().contains('\n') {
                return vec![];
            }
            buf.push_str(delta);
            let text: String = buf
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or_default()
                .chars()
                .take(ACTIVITY_CHARS)
                .collect();
            set_activity(s, parent, &text)
        }
        Some("stream_event") => {
            let d = &v["event"]["delta"];
            let text = |k: &str| d[k].as_str().filter(|t| !t.is_empty()).map(str::to_string);
            if v["event"]["type"] == "message_start" {
                s.streaming = v["event"]["message"]["id"].as_str().unwrap_or_default().to_string();
            }
            match d["type"].as_str() {
                Some("text_delta") => {
                    if !s.streaming.is_empty() {
                        s.texted.insert(s.streaming.clone());
                    }
                    text("text").map(Update::Delta).into_iter().collect()
                }
                Some("thinking_delta") => {
                    text("thinking").map(Update::Thinking).into_iter().collect()
                }
                _ => vec![],
            }
        }
        Some("assistant") => {
            let parent = v["parent_tool_use_id"].as_str();
            let mut updates: Vec<Update> = context(&v["message"]["usage"])
                .filter(|_| !sub)
                .map(Update::Usage)
                .into_iter()
                .collect();
            if let Some(p) = parent {
                s.sub_text.remove(p);
                for b in blocks("text") {
                    updates.extend(set_activity(s, p, b["text"].as_str().unwrap_or_default()));
                }
            }
            let message = &v["message"];
            if !sub && message["model"] != "<synthetic>" {
                let streamed = message["id"].as_str().is_some_and(|id| s.texted.contains(id));
                for b in blocks("text") {
                    let text = b["text"].as_str().filter(|t| !t.is_empty());
                    let Some(text) = text else { continue };
                    let uuid = v["uuid"].as_str().unwrap_or_default().to_string();
                    s.turn_texts.push((uuid, text.to_string()));
                    if !streamed {
                        updates.push(Update::Delta(text.to_string()));
                    }
                }
            }
            for b in blocks("tool_use") {
                let id = b["id"].as_str().unwrap_or_default();
                let input = &b["input"];
                match b["name"].as_str().unwrap_or_default() {
                    "TaskCreate" => {
                        let text = input["subject"].as_str().unwrap_or_default().to_string();
                        s.todo_uses
                            .insert(id.to_string(), PlanItem { text, status: PlanStatus::Pending });
                        continue;
                    }
                    "TaskUpdate" => {
                        s.quiet.insert(id.to_string());
                        updates.extend(update_todo(s, input));
                        continue;
                    }
                    "Skill" => {
                        s.quiet.insert(id.to_string());
                        updates
                            .extend(input["skill"].as_str().map(|n| Update::Skill(n.to_string())));
                        continue;
                    }
                    _ => {}
                }
                let mut call = start(id, b["name"].as_str().unwrap_or_default(), &b["input"]);
                call.parent = parent.map(str::to_string);
                s.turn_rows.insert(call.id.clone());
                s.tools.insert(call.id.clone(), (call.clone(), Instant::now()));
                updates.push(Update::Tool(call.clone()));
                if let Some((row, _)) = parent.and_then(|p| s.tools.get_mut(p)) {
                    child_activity(row, &call, true);
                    updates.push(Update::Tool(row.clone()));
                }
            }
            updates
        }
        Some("user") if !sub && v["isReplay"] == true => {
            // set_model's confirmation is replayed too, but no prompt of ours caused it
            if result_text(&v["message"]["content"]).starts_with("<local-command-stdout>") {
                return vec![];
            }
            let text = without_attached_files(&result_text(&v["message"]["content"])).to_string();
            let at =
                s.echoes.iter().position(|e| e.as_deref().is_some_and(|q| q.trim() == text.trim()));
            // the CLI may normalize the text, so an unmatched echo still retires the front
            let entry = match at {
                Some(i) => s.echoes.remove(i),
                None => s.echoes.pop_front(),
            };
            entry.flatten().map(Update::Steered).into_iter().collect()
        }
        Some("user") => {
            let parent = v["parent_tool_use_id"].as_str();
            let results: Vec<&Value> = blocks("tool_result").collect();
            // the structured patch sits beside `message`, so it only names a lone result
            let patch = (results.len() == 1).then(|| &v["tool_use_result"]["structuredPatch"]);
            if !sub {
                s.turn_texts.clear();
            }
            results
                .into_iter()
                .filter_map(|b| {
                    let id = b["tool_use_id"].as_str().unwrap_or_default();
                    if s.quiet.remove(id) {
                        return None;
                    }
                    if let Some(item) = s.todo_uses.remove(id) {
                        let text = result_text(&b["content"]);
                        let n: String = text
                            .split_once("Task #")
                            .map(|(_, n)| n.chars().take_while(char::is_ascii_digit).collect())
                            .unwrap_or_default();
                        if n.is_empty() {
                            log_unknown("claude", "TaskCreate result without a task id");
                            return None;
                        }
                        s.todos.push((n, item));
                        return Some(Update::Tool(todos_row(s)));
                    }
                    let (mut call, at) = s.tools.remove(id).unwrap_or_else(|| {
                        (ToolCall::new(id, "Tool", ToolDetail::default()), Instant::now())
                    });
                    call.parent = call.parent.take().or_else(|| parent.map(str::to_string));
                    let mut text = result_text(&b["content"]);
                    if text.starts_with("Command running in background with ID:")
                        || text.starts_with("Async agent launched")
                        || text.contains("\"resumedAgentId\"")
                    {
                        call.status = ToolStatus::Background;
                        s.background.insert(id.to_string());
                        s.tools.insert(id.to_string(), (call.clone(), at));
                        return Some(Update::Tool(call));
                    }
                    s.tasks.retain(|_, (row, _)| row != id);
                    if let ToolDetail::Task { tokens, tools, .. } = &mut call.detail {
                        let (clean, usage) = strip_agent_trailer(&text);
                        text = clean;
                        if let Some((t, n)) = usage {
                            (*tokens, *tools) = (t, n);
                        }
                    }
                    let was_denied = call.status == ToolStatus::Denied;
                    let reason = call.error.take();
                    finish(&mut call, &text, b["is_error"] == true);
                    let denied = was_denied
                        || text.contains(DENIED)
                        || text.contains("doesn't want to proceed with this tool use");
                    let cut = s.cancelled == Some(s.turn) && s.turn_rows.contains(id);
                    halted(&mut call, denied, cut || text.contains("interrupted by user"));
                    if was_denied {
                        call.status = ToolStatus::Denied;
                        call.error = reason.or(call.error.take());
                    }
                    if let Some(hunks) = patch {
                        apply_hunks(&mut call.detail, hunks);
                    }
                    call.images = save_images(&b["content"], s.attachments.as_deref());
                    call.ms = Some(at.elapsed().as_millis() as u64);
                    Some(Update::Tool(call))
                })
                .collect()
        }
        Some("control_request") => control_request(s, v),
        Some("control_cancel_request") => {
            let id = v["request_id"].as_str().unwrap_or_default();
            let mut gone: Vec<u64> =
                s.asks.iter().filter(|(_, a)| a.request_id == id).map(|(k, _)| *k).collect();
            gone.sort_unstable();
            let mut out: Vec<Update> = gone
                .into_iter()
                .map(|k| {
                    s.asks.remove(&k);
                    Update::AskResolved(k)
                })
                .collect();
            out.extend(quiet_turn_end(s));
            out
        }
        Some("control_response")
            if v["response"]["request_id"]
                .as_str()
                .is_some_and(|id| id.starts_with(BTW_REQUEST)) =>
        {
            let id = v["response"]["request_id"].as_str().unwrap_or_default();
            if let Some(reply) = s.sides.remove(id) {
                let _ = reply.unbounded_send(side_answer(v));
            }
            vec![]
        }
        Some("control_response")
            if v["response"]["subtype"] == "error"
                && v["response"]["request_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(STOP_REQUEST)) =>
        {
            let why = v["response"]["error"].as_str().unwrap_or("claude couldn't stop the task");
            vec![Update::Warning(why.to_string())]
        }
        Some("control_response")
            if v["response"]["subtype"] == "error"
                && v["response"]["request_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(MODEL_REQUEST)) =>
        {
            s.choosing = None;
            let why = v["response"]["error"].as_str().unwrap_or("claude couldn't switch models");
            vec![Update::Warning(why.to_string())]
        }
        Some("control_response")
            if v["response"]["request_id"]
                .as_str()
                .is_some_and(|id| id.starts_with(MODEL_REQUEST)) =>
        {
            if let Some(model) = s.choosing.take() {
                s.chosen = model;
            }
            vec![]
        }
        Some("control_response")
            if v["response"]["request_id"]
                .as_str()
                .is_some_and(|id| id.starts_with(INTERRUPT_REQUEST)) =>
        {
            on_interrupted(s, &v["response"]["response"])
        }
        Some("control_response") if v["response"]["request_id"] == TITLE_REQUEST => {
            let title = v["response"]["response"]["title"].as_str().filter(|t| !t.is_empty());
            title.map(|t| Update::Title(t.to_string())).into_iter().collect()
        }
        Some("result") if s.active => {
            s.quiet_wake = false;
            let u = &v["usage"];
            let last = match &u["iterations"] {
                Value::Array(calls) => calls.last(),
                _ => Some(u),
            };
            let mut out: Vec<Update> =
                last.and_then(context).map(Update::Usage).into_iter().collect();
            out.extend(window(&v["modelUsage"], &s.model).map(Update::Window));
            let total = v["total_cost_usd"].as_f64().unwrap_or(0.0);
            if total > s.cost_seen {
                out.push(Update::Cost(((total - s.cost_seen) * 1e6).round() as u64));
            }
            s.cost_seen = s.cost_seen.max(total);
            out.extend(usage_delta(&v["modelUsage"], &mut s.usage_seen));
            let text = v["result"].as_str().unwrap_or_default().to_string();
            let subtype = v["subtype"].as_str().unwrap_or_default();
            let errors: Vec<&str> =
                v["errors"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let failed = v["is_error"] == true || subtype.starts_with("error");
            let cancelled = s.cancelled.take();
            // the interrupted turn's own result, landing after a new prompt
            if cancelled.is_some_and(|turn| turn != s.turn) {
                return out;
            }
            let cancelled = cancelled.is_some();
            if failed && !cancelled {
                out.push(Update::Error(if !errors.is_empty() {
                    errors.join("\n")
                } else if !text.is_empty() {
                    text.clone()
                } else if let Some(why) = subtype.strip_prefix("error_") {
                    let why = match why {
                        "max_turns" => "max turns reached".into(),
                        "max_budget_usd" => "budget reached".into(),
                        other => other.replace('_', " "),
                    };
                    format!("Claude stopped: {why}")
                } else {
                    "claude run failed".into()
                }));
            }
            s.turn_texts.clear();
            s.texted.clear();
            if let Some(retry) = s.retry.take() {
                let state = if failed { NoticeState::Failed } else { NoticeState::Done };
                out.push(Update::Notice(Notice { state, until_ms: None, ..retry }));
            }
            // a steer the turn's last model call missed, or one a Stop left queued, runs next
            let held = std::mem::take(&mut s.held) > 0;
            if !s.echoes.is_empty() && (!cancelled || held) {
                return out;
            }
            s.active = false;
            out.push(Update::Final(if failed { String::new() } else { text }));
            out.extend(background_asks(s));
            out
        }
        Some("result" | "control_response" | "command_lifecycle") => vec![],
        other => {
            log_unknown("claude", other.unwrap_or("untyped line"));
            vec![]
        }
    }
}

pub(crate) fn hook_notice(v: &Value) -> Notice {
    let text = |k: &str| v[k].as_str().unwrap_or_default();
    Notice::new(
        format!("hook-{}", text("hook_id")),
        NoticeKind::Hook,
        format!("{} hook: {}", text("hook_event"), text("hook_name")),
    )
}

pub(crate) fn open_compaction(s: &mut State) -> Notice {
    s.compacting.take().unwrap_or_else(|| {
        s.compactions += 1;
        Notice::new(format!("compact-{}", s.compactions), NoticeKind::Compaction, "")
    })
}

pub(crate) fn system_status(s: &mut State, v: &Value) -> Vec<Update> {
    let mut out: Vec<Update> =
        v["permissionMode"].as_str().into_iter().flat_map(|m| mode_updates(s, m)).collect();
    if v["status"] == "compacting" {
        // CLI can repeat this status within one compaction; only its end closes the notice
        let n = Notice {
            text: "Compacting context".into(),
            state: NoticeState::Running,
            ..open_compaction(s)
        };
        s.compacting = Some(n.clone());
        out.push(Update::Notice(n));
    }
    if v["compact_result"] == "failed" {
        let why = v["compact_error"].as_str().filter(|e| !e.is_empty());
        let text =
            why.map_or("Compaction failed".to_string(), |e| format!("Compaction failed: {e}"));
        let open = open_compaction(s);
        out.push(Update::Notice(Notice { text, state: NoticeState::Failed, ..open }));
    }
    out
}

pub(crate) fn rate_limit(s: &mut State, info: &Value) -> Vec<Update> {
    let kind = info["rateLimitType"].as_str().unwrap_or("usage");
    let id = format!("ratelimit-{kind}");
    let until_ms = info["resetsAt"].as_i64().map(|t| t * 1000);
    let (state, text) = match info["status"].as_str() {
        Some("allowed_warning") => {
            let pct = (info["utilization"].as_f64().unwrap_or(0.0) * 100.0).round();
            (NoticeState::Running, format!("Claude usage at {pct}% of the {kind} limit"))
        }
        Some("rejected") => (NoticeState::Failed, "Claude usage limit reached".to_string()),
        Some("allowed") if s.limits.contains(&id) => {
            (NoticeState::Done, format!("Claude usage is back under the {kind} limit"))
        }
        Some("allowed") => return vec![],
        other => {
            log_unknown("claude", &format!("rate limit status {}", other.unwrap_or_default()));
            return vec![];
        }
    };
    if state == NoticeState::Done {
        s.limits.remove(&id);
    } else {
        s.limits.insert(id.clone());
    }
    let until_ms = until_ms.filter(|_| state != NoticeState::Done);
    let notice = Notice { state, until_ms, ..Notice::new(id, NoticeKind::RateLimit, text) };
    vec![Update::Notice(notice)]
}

pub(crate) fn todos_row(s: &State) -> ToolCall {
    let items = s.todos.iter().map(|(_, i)| i.clone()).collect();
    let mut call = ToolCall::new(TODOS_ID, "Update to-dos", ToolDetail::Plan { items });
    call.status = ToolStatus::Done;
    call
}

pub(crate) fn update_todo(s: &mut State, input: &Value) -> Option<Update> {
    let id = input["taskId"].as_str()?;
    let at = s.todos.iter().position(|(n, _)| n == id)?;
    match input["status"].as_str() {
        Some("deleted") => {
            s.todos.remove(at);
        }
        Some("pending") => s.todos[at].1.status = PlanStatus::Pending,
        Some("in_progress") => s.todos[at].1.status = PlanStatus::Active,
        Some("completed") => s.todos[at].1.status = PlanStatus::Done,
        _ => {}
    }
    Some(Update::Tool(todos_row(s)))
}

/// Counts every model, subagents included; `modelUsage` only ever grows, so a drop means a new process.
pub(crate) fn usage_delta(model_usage: &Value, seen: &mut [u64; 3]) -> Option<Update> {
    let mut now = [0u64; 3];
    for m in model_usage.as_object().into_iter().flat_map(Map::values) {
        let n = |k: &str| m[k].as_u64().unwrap_or(0);
        now[0] += n("inputTokens") + n("cacheReadInputTokens") + n("cacheCreationInputTokens");
        now[1] += n("outputTokens");
        now[2] += n("cacheReadInputTokens");
    }
    let delta: [u64; 3] = std::array::from_fn(|i| now[i].checked_sub(seen[i]).unwrap_or(now[i]));
    *seen = now;
    (delta[0] + delta[1] > 0).then_some(Update::Count {
        prompt: delta[0],
        completion: delta[1],
        cached: delta[2],
    })
}

/// Prompt tokens of one call: fresh, cache-written and cache-read input.
pub(crate) fn context(u: &Value) -> Option<u64> {
    let keys = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"];
    let n: u64 = keys.iter().map(|k| u[*k].as_u64().unwrap_or(0)).sum();
    (n > 0).then_some(n)
}

/// `model`'s window, else the largest; subagent models share the map.
pub(crate) fn window(models: &Value, model: &str) -> Option<u64> {
    let models = models.as_object()?;
    let size = |m: &Value| m["contextWindow"].as_u64();
    models.get(model).and_then(size).or_else(|| models.values().filter_map(size).max())
}
