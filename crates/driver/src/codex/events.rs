use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use aether_core::{
    Attachment, Notice, NoticeKind, NoticeState, ToolCall, ToolDetail, ToolStatus, Update,
    without_attached_files,
};

use crate::spawn::log_unknown;
use crate::tool::{
    apply_patch_text, child_activity, finish, halted, now_ms, result_text, save_images, set_output,
    start,
};

use super::approve::server_request;
use super::spawn::{
    fork_turn_params, side_thread_params, side_turn_params, start_turn, steer_refused, turn_started,
};
use super::{Call, Cites, Naming, Side, State, refuse_queued, side_turn_end};

pub(crate) fn map_event(s: &mut State, v: &Value) -> Vec<Update> {
    // server ids count from 0 like ours; only a line without a method answers us
    let Some(method) = v["method"].as_str() else { return response(s, v) };
    if v.get("id").is_some() {
        return server_request(s, method, v);
    }
    let p = &v["params"];
    if method == "serverRequest/resolved" {
        let found = s.asks.iter().find(|(_, (rpc, _))| *rpc == p["requestId"]).map(|(id, _)| *id);
        let Some(id) = found else { return vec![] };
        s.asks.remove(&id);
        return [Some(Update::AskResolved(id)), side_turn_end(s)].into_iter().flatten().collect();
    }
    if let Some(thread) = p["threadId"].as_str().filter(|t| !s.thread.is_empty() && *t != s.thread)
    {
        if s.agents.contains_key(thread) {
            return agent_event(s, thread, method, p);
        }
        if s.sides.contains_key(thread) {
            fork_event(s, thread, method, p);
            return vec![];
        }
        side_event(s, thread, method, p);
        return vec![];
    }
    let mut out = close_retry(s, method, p);
    out.extend(main_event(s, method, p));
    out
}

pub(crate) fn close_retry(s: &mut State, method: &str, p: &Value) -> Vec<Update> {
    let closes = match method {
        "item/agentMessage/delta"
        | "item/plan/delta"
        | "item/reasoning/summaryTextDelta"
        | "item/reasoning/textDelta"
        | "item/commandExecution/outputDelta"
        | "item/started"
        | "item/completed"
        | "turn/completed" => true,
        "error" => p["willRetry"] != true,
        _ => false,
    };
    if !closes {
        return vec![];
    }
    let state = if method == "error" { NoticeState::Failed } else { NoticeState::Done };
    let done = |n: Notice| Update::Notice(Notice { state, ..n });
    s.retry.take().map(done).into_iter().collect()
}

pub(crate) fn main_event(s: &mut State, method: &str, p: &Value) -> Vec<Update> {
    let text = |key: &str| p[key].as_str().unwrap_or_default();
    match method {
        "item/agentMessage/delta" | "item/plan/delta" => {
            *s.streamed.entry(text("itemId").to_string()).or_default() += text("delta").len();
            say(s, text("itemId"), text("delta"))
        }
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta"
            if !text("delta").is_empty() =>
        {
            let summary = method == "item/reasoning/summaryTextDelta";
            let seen = s.reasoning.entry(text("itemId").to_string()).or_default();
            *seen |= summary;
            if !summary && *seen {
                return vec![];
            }
            s.streamed.insert(text("itemId").to_string(), 0);
            let index = p["summaryIndex"].as_u64().or(p["contentIndex"].as_u64()).unwrap_or(0);
            think(s, text("itemId"), index, text("delta"))
        }
        "item/commandExecution/outputDelta" => output_delta(s, p),
        "item/started" => {
            let item = &p["item"];
            if let (Some(id), Some(phase)) = (item["id"].as_str(), item["phase"].as_str())
                && item["type"] == "agentMessage"
            {
                s.phases.insert(id.to_string(), phase.to_string());
            }
            let mut out = item_started(s, None, item);
            for u in &out {
                if let Update::Tool(call) = u
                    && is_collab(call)
                {
                    s.main_collab.insert(call.id.clone());
                }
            }
            out.extend(item_notice(item).map(Update::Notice));
            if p["item"]["type"] == "commandExecution"
                && let (Some(id), Some(process)) =
                    (p["item"]["id"].as_str(), p["item"]["processId"].as_str())
            {
                s.terminals.insert(id.to_string(), process.to_string());
            }
            s.gap &= out.is_empty();
            out
        }
        "item/completed" => completed(s, &p["item"]),
        "turn/started" if s.active && s.turn_id.is_none() => {
            turn_started(s, p["turn"]["id"].as_str().unwrap_or_default())
        }
        "turn/started" if !s.active => server_turn(s, p["turn"]["id"].as_str().unwrap_or_default()),
        "thread/queue/changed" => {
            s.wake_reason = Some("a queued message".into());
            vec![]
        }
        "thread/goal/updated" => {
            if let Some(goal) = p["goal"]["objective"].as_str() {
                s.wake_reason = Some(format!("goal: {goal}"));
            }
            vec![]
        }
        "turn/plan/updated" => {
            let steps = p["plan"].as_array().into_iter().flatten();
            let plan: Vec<Value> = steps
                .map(|step| {
                    let status = match step["status"].as_str() {
                        Some("inProgress") => "in_progress",
                        status => status.unwrap_or_default(),
                    };
                    json!({"step": step["step"], "status": status})
                })
                .collect();
            let id = format!("plan-{}", text("turnId"));
            let call = start(id, "update_plan", &json!({"plan": plan}));
            vec![Update::Tool(ToolCall { status: ToolStatus::Done, ..call })]
        }
        "thread/tokenUsage/updated" => usage(s, &p["tokenUsage"]),
        "turn/completed" => turn_completed(s, &p["turn"]),
        "error" if p["willRetry"] == true => {
            let notice = Notice {
                state: NoticeState::Running,
                ..Notice::new(
                    format!("retry-{}", text("turnId")),
                    NoticeKind::Retry,
                    format!("Retrying: {}", error_text(&p["error"])),
                )
            };
            s.retry = Some(notice.clone());
            vec![Update::Notice(notice)]
        }
        "error" => {
            let error = error_text(&p["error"]);
            s.error = Some(error.clone());
            let limited = matches!(
                p["error"]["codexErrorInfo"].as_str(),
                Some("usageLimitExceeded" | "rateLimitExceeded")
            );
            let notice = Notice {
                state: NoticeState::Failed,
                ..Notice::new("ratelimit-default", NoticeKind::RateLimit, error.clone())
            };
            let mut out: Vec<Update> =
                limited.then_some(Update::Notice(notice)).into_iter().collect();
            out.push(Update::Error(error));
            out
        }
        "turn/diff/updated" => {
            s.turn_diff = text("diff").to_string();
            vec![]
        }
        "account/rateLimits/updated" => rate_limits(&p["rateLimits"]),
        "model/rerouted" => {
            let label = format!("Codex switched from {} to {}", text("fromModel"), text("toModel"));
            let id = format!("model-reroute-{}", text("turnId"));
            let notice = Notice::new(id, NoticeKind::ModelChange, label);
            vec![Update::Model(text("toModel").to_string()), Update::Notice(notice)]
        }
        "warning" | "guardianWarning" => vec![Update::Warning(text("message").to_string())],
        "configWarning" | "deprecationNotice" => {
            let warning = match p["details"].as_str().filter(|d| !d.is_empty()) {
                Some(details) => format!("{}: {details}", text("summary")),
                None => text("summary").to_string(),
            };
            vec![Update::Warning(warning)]
        }
        "hook/started" | "hook/completed" => vec![Update::Notice(hook_notice(method, &p["run"]))],
        "item/autoApprovalReview/started" | "item/autoApprovalReview/completed" => {
            vec![Update::Notice(auto_review(p))]
        }
        "autoApprovalReview/strictReviewRequired" => {
            let text = "Codex needs a stricter review for the next action";
            vec![Update::Notice(Notice::new("info-strict-review", NoticeKind::Info, text))]
        }
        "item/commandExecution/terminalInteraction" => {
            let Some(call) = s.tools.get_mut(text("itemId")).filter(|_| !text("stdin").is_empty())
            else {
                return vec![];
            };
            let output =
                s.live.entry(call.id.clone()).or_default().push(&format!("\n> {}", text("stdin")));
            set_output(&mut call.detail, &output);
            vec![Update::Tool(call.clone())]
        }
        "item/fileChange/patchUpdated" => {
            let item = json!({"type": "fileChange", "id": text("itemId"), "changes": p["changes"]});
            item_started(s, None, &item)
        }
        "item/mcpToolCall/progress" => {
            let Some(call) = s.tools.get_mut(text("itemId")) else { return vec![] };
            set_output(&mut call.detail, text("message"));
            vec![Update::Tool(call.clone())]
        }
        "mcpServer/oauthLogin/completed" => {
            let name = text("name");
            let (state, text) = match (p["success"] == true, p["error"].as_str()) {
                (true, _) => (NoticeState::Done, format!("Signed in to {name}")),
                (false, Some(e)) => {
                    (NoticeState::Failed, format!("Couldn't sign in to {name}: {e}"))
                }
                (false, None) => (NoticeState::Failed, format!("Couldn't sign in to {name}")),
            };
            let notice = Notice::new(format!("mcp-{name}"), NoticeKind::McpAuth, text);
            vec![Update::Notice(Notice { state, ..notice })]
        }
        method if ignored(method) => vec![],
        "thread/name/updated" if !text("threadName").is_empty() => {
            vec![Update::Title(text("threadName").to_string())]
        }
        "mcpServer/startupStatus/updated" if p["status"] == "failed" => {
            let name = text("name");
            let detail = ToolDetail::Other {
                name: name.to_string(),
                input: String::new(),
                output: String::new(),
            };
            let mut call =
                ToolCall::new(format!("mcp-{name}"), format!("Start MCP server {name}"), detail);
            finish(&mut call, p["error"].as_str().unwrap_or("unknown error"), true);
            vec![Update::Tool(call)]
        }
        method => {
            log_unknown("codex", method);
            vec![]
        }
    }
}

/// Notifications Codex sends that the chat has no use for.
pub(crate) fn ignored(method: &str) -> bool {
    method.starts_with("rawResponse")
        || matches!(
            method,
            "thread/status/changed"
                | "thread/started"
                | "thread/compacted"
                | "skills/changed"
                | "fs/changed"
                | "account/updated"
                | "turn/moderationMetadata"
                | "model/verification"
                | "thread/settings/updated"
                | "thread/readState/changed"
        )
}

pub(crate) fn rate_limits(snapshot: &Value) -> Vec<Update> {
    let id = format!("ratelimit-{}", snapshot["limitId"].as_str().unwrap_or("default"));
    let notice = |state, text: String| Notice {
        state,
        ..Notice::new(id.clone(), NoticeKind::RateLimit, text)
    };
    if let Some(kind) = snapshot["rateLimitReachedType"].as_str() {
        let text = format!("Codex usage limit reached ({kind})");
        return vec![Update::Notice(notice(NoticeState::Failed, text))];
    }
    let primary = &snapshot["primary"];
    let used = primary["usedPercent"].as_u64().unwrap_or(0);
    if used < 80 {
        return vec![];
    }
    let mut n = notice(NoticeState::Running, format!("Codex usage at {used}%"));
    n.until_ms = primary["resetsAt"].as_i64().map(|secs| secs * 1000);
    vec![Update::Notice(n)]
}

pub(crate) fn hook_notice(method: &str, run: &Value) -> Notice {
    let id = format!("hook-{}", run["id"].as_str().unwrap_or_default());
    let mut text = format!("{} hook", run["eventName"].as_str().unwrap_or_default());
    if let Some(message) = run["statusMessage"].as_str().filter(|m| !m.is_empty()) {
        text = format!("{text}: {message}");
    }
    let state = match (method, run["status"].as_str()) {
        ("hook/started", _) => NoticeState::Running,
        (_, Some("failed" | "blocked")) => NoticeState::Failed,
        _ => NoticeState::Done,
    };
    let entries = run["entries"].as_array().into_iter().flatten();
    let detail: Vec<&str> = entries.filter_map(|e| e["text"].as_str()).collect();
    let detail = (!detail.is_empty()).then(|| detail.join("\n"));
    Notice { state, detail, ..Notice::new(id, NoticeKind::Hook, text) }
}

pub(crate) fn auto_review(p: &Value) -> Notice {
    let action = &p["action"];
    let files = action["files"].as_array().into_iter().flatten().filter_map(Value::as_str);
    let summary = match action["type"].as_str().unwrap_or_default() {
        "command" => action["command"].as_str().unwrap_or_default().to_string(),
        "execve" => action["program"].as_str().unwrap_or_default().to_string(),
        "applyPatch" => format!("edit {}", files.collect::<Vec<_>>().join(", ")),
        "writeStdin" => "send input to a command".to_string(),
        "networkAccess" => action["host"].as_str().unwrap_or_default().to_string(),
        "mcpToolCall" => format!(
            "{}.{}",
            action["server"].as_str().unwrap_or_default(),
            action["toolName"].as_str().unwrap_or_default()
        ),
        "requestPermissions" => "request permissions".to_string(),
        other => other.to_string(),
    };
    let review = &p["review"];
    let status = review["status"].as_str().unwrap_or_default();
    let state = match status {
        "inProgress" => NoticeState::Running,
        "approved" => NoticeState::Done,
        _ => NoticeState::Failed,
    };
    let id = format!("info-review-{}", p["reviewId"].as_str().unwrap_or_default());
    let text = format!("Auto-review {status}: {summary}");
    let detail = review["rationale"].as_str().map(str::to_string);
    Notice { state, detail, ..Notice::new(id, NoticeKind::Info, text) }
}

pub(crate) fn server_turn(s: &mut State, id: &str) -> Vec<Update> {
    s.active = true;
    s.cancelled = false;
    s.turn += 1;
    s.turn_id = Some(id.to_string());
    s.turn_diff.clear();
    s.phases.clear();
    s.reasoning.clear();
    s.error = None;
    s.gap = false;
    s.part = None;
    s.cites = Cites::default();
    s.wakes += 1;
    let text = match s.wake_reason.take() {
        Some(reason) => format!("Codex started a turn on its own: {reason}"),
        None => "Codex started a turn on its own".into(),
    };
    let notice = Notice::new(format!("wake-{}", s.wakes), NoticeKind::Wake, text);
    vec![Update::Wake, Update::Notice(notice)]
}

pub(crate) fn turn_completed(s: &mut State, turn: &Value) -> Vec<Update> {
    let id = turn["id"].as_str().unwrap_or_default();
    let ours = s.turn_id.as_deref() == Some(id);
    // an interrupted turn's completion must not end the turn started after it
    if s.stale.remove(id) && !ours {
        return vec![];
    }
    if !s.active || !(ours || s.turn_id.is_none()) {
        log_unknown("codex", "turn/completed for another turn");
        return vec![];
    }
    let turn_diff = std::mem::take(&mut s.turn_diff);
    s.phases.clear();
    s.reasoning.clear();
    let rows: Vec<ToolCall> = s.tools.drain().map(|(_, call)| call).collect();
    let alive: HashSet<String> = rows
        .iter()
        .filter(|c| matches!(c.detail, ToolDetail::Task { .. }))
        .filter(|c| matches!(c.status, ToolStatus::Running | ToolStatus::Background))
        .map(|c| c.id.clone())
        .collect();
    let mut out = vec![];
    for mut call in rows {
        let child_of_live = call.parent.as_ref().is_some_and(|p| alive.contains(p));
        match call.status {
            ToolStatus::Background => {}
            ToolStatus::Running
                if matches!(call.detail, ToolDetail::Task { .. })
                    || s.terminals.contains_key(&call.id) =>
            {
                call.status = ToolStatus::Background;
                out.push(Update::Tool(call.clone()));
            }
            ToolStatus::Running if child_of_live && !s.main_collab.contains(&call.id) => {}
            ToolStatus::Running => {
                call.status = ToolStatus::Stopped;
                out.push(Update::Tool(call));
                continue;
            }
            ToolStatus::Done => {
                if fill_diff(&mut call, &turn_diff) {
                    out.push(Update::Tool(call));
                }
                continue;
            }
            _ => continue,
        }
        s.tools.insert(call.id.clone(), call);
    }
    let kept = &s.tools;
    s.terminals.retain(|id, _| kept.contains_key(id));
    s.live.retain(|id, _| kept.contains_key(id));
    s.collab.clear();
    s.main_collab.clear();
    s.turn_rows.clear();
    s.cancelled = false;
    if turn["status"] == "interrupted" && !s.terminals.is_empty() {
        let snapshot: Vec<String> = s.terminals.keys().cloned().collect();
        let line = s.rpc(
            "thread/backgroundTerminals/list",
            json!({"threadId": s.thread}),
            Some(Call::Reconcile(snapshot)),
        );
        s.outbox.push(line);
    }
    if turn["status"] == "failed" {
        let error = error_text(&turn["error"]);
        if s.error.as_ref() != Some(&error) {
            out.push(Update::Error(error));
        }
    }
    out.extend(refuse_queued(s));
    out.push(Update::Final(String::new()));
    s.active = false;
    s.turn_id = None;
    s.streamed.clear();
    if let Some((text, images)) = s.next.take() {
        let line = start_turn(s, &text, &images);
        s.outbox.push(line);
    }
    out
}

/// Gives a finished edit row without a diff its file's section of the turn's diff.
pub(crate) fn fill_diff(call: &mut ToolCall, turn_diff: &str) -> bool {
    let ToolDetail::Edit { path, diff, .. } = &call.detail else { return false };
    if !diff.is_empty() || turn_diff.is_empty() {
        return false;
    }
    let file = path.rsplit(" → ").next().unwrap_or_default().to_string();
    let section = format!("\n{turn_diff}")
        .split("\ndiff --git ")
        .skip(1)
        .filter(|sec| {
            sec.lines()
                .next()
                .and_then(|h| h.rsplit_once(" b/"))
                .is_some_and(|(_, f)| file == f || file.ends_with(&format!("/{f}")))
        })
        .map(|sec| sec.lines().skip_while(|l| !l.starts_with("@@")).collect::<Vec<_>>().join("\n"))
        .next();
    let Some(section) = section else { return false };
    apply_patch_text(&mut call.detail, &section);
    matches!(&call.detail, ToolDetail::Edit { diff, .. } if !diff.is_empty())
}

pub(crate) fn response(s: &mut State, v: &Value) -> Vec<Update> {
    let Some(call) = v["id"].as_u64().and_then(|id| s.calls.remove(&id)) else { return vec![] };
    let failed = v.get("error").is_some();
    match call {
        Call::Wait(tx) => {
            let _ = tx.send(v.clone());
            vec![]
        }
        Call::Turn if !s.active => vec![],
        Call::Turn if failed => {
            s.active = false;
            s.cancelled = false;
            let mut out = refuse_queued(s);
            out.push(Update::Error(error_text(&v["error"])));
            out.push(Update::Final(String::new()));
            if let Some((text, images)) = s.next.take() {
                let line = start_turn(s, &text, &images);
                s.outbox.push(line);
            }
            out
        }
        Call::Turn => {
            // thread/compact/start replies {}; its turn id comes with turn/started
            match v["result"]["turn"]["id"].as_str() {
                Some(id) => turn_started(s, id),
                None => vec![],
            }
        }
        Call::Steer(text) => {
            let text = without_attached_files(&text).to_string();
            if !failed {
                return vec![Update::Accepted, Update::Steered(text)];
            }
            let error = &v["error"];
            let mut out = vec![Update::SteerRefused(text)];
            match steer_refused(error) {
                Some(kind @ ("review" | "compact")) => {
                    let text =
                        format!("Codex can't take messages during /{kind}. Yours stays queued.");
                    out.push(Update::Notice(Notice::new("info-steer", NoticeKind::Info, text)));
                }
                Some(_) => {}
                None => out.push(Update::Warning(error_text(error))),
            }
            out
        }
        Call::Note if failed => {
            vec![Update::Warning(format!(
                "Codex didn't take your note: {}",
                error_text(&v["error"])
            ))]
        }
        Call::Note => vec![],
        Call::Terminate(command) => match failed {
            true => vec![Update::Warning(format!(
                "Couldn't stop `{command}`: {}",
                error_text(&v["error"])
            ))],
            false => vec![],
        },
        Call::Reconcile(_) if failed => vec![],
        Call::Reconcile(snapshot) => {
            let listed: HashSet<&str> = v["result"]["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["itemId"].as_str())
                .collect();
            let gone: Vec<String> = snapshot
                .into_iter()
                .filter(|id| s.terminals.contains_key(id) && !listed.contains(id.as_str()))
                .collect();
            let mut out = vec![];
            for id in gone {
                s.terminals.remove(&id);
                if let Some(mut call) = s.tools.remove(&id) {
                    call.status = ToolStatus::Stopped;
                    out.push(Update::Tool(call));
                }
            }
            out
        }
        Call::Models(prompt) => {
            let models = v["result"]["data"].as_array().into_iter().flatten();
            let luna = models.filter_map(|m| m["model"].as_str()).find(|m| m.contains("luna"));
            let model = luna.map_or_else(|| s.model.clone(), str::to_string);
            let line = s.rpc(
                "thread/start",
                side_thread_params(&s.cwd, &model),
                Some(Call::SideThread(prompt)),
            );
            s.outbox.push(line);
            vec![]
        }
        Call::Fork(question, reply) => {
            let Some(id) = v["result"]["thread"]["id"].as_str().filter(|_| !failed) else {
                let _ = reply.unbounded_send(Update::Error(error_text(&v["error"])));
                return vec![];
            };
            s.sides.insert(id.to_string(), Side { reply, text: String::new() });
            let line = s.rpc(
                "turn/start",
                fork_turn_params(id, &question),
                Some(Call::ForkTurn(id.to_string())),
            );
            s.outbox.push(line);
            vec![]
        }
        Call::ForkTurn(id) => {
            if failed {
                end_fork(s, &id, Err(error_text(&v["error"])));
            }
            vec![]
        }
        Call::SideThread(prompt) => {
            if let Some(id) = v["result"]["thread"]["id"].as_str() {
                s.naming = Naming::Side(id.to_string());
                let line = s.rpc("turn/start", side_turn_params(id, &prompt), None);
                s.outbox.push(line);
            }
            vec![]
        }
    }
}

/// A `/btw` fork's traffic answers its side reply only; the main turn never sees it.
pub(crate) fn fork_event(s: &mut State, thread: &str, method: &str, p: &Value) {
    match method {
        "item/completed" if p["item"]["type"] == "agentMessage" => {
            let Some(text) = p["item"]["text"].as_str() else { return };
            if let Some(side) = s.sides.get_mut(thread) {
                if !side.text.is_empty() {
                    side.text.push_str("\n\n");
                }
                side.text.push_str(text);
            }
        }
        "turn/completed" => {
            let turn = &p["turn"];
            let end = match turn["status"].as_str() {
                Some("failed") => Err(error_text(&turn["error"])),
                Some("interrupted") => Err("side question interrupted".into()),
                _ => Ok(()),
            };
            end_fork(s, thread, end);
        }
        _ => {}
    }
}

fn end_fork(s: &mut State, thread: &str, end: Result<(), String>) {
    let Some(side) = s.sides.remove(thread) else { return };
    let text = side.text.trim();
    let update = match end {
        Err(why) => Update::Error(why),
        Ok(()) if text.is_empty() => Update::Error("side question got no answer".into()),
        Ok(()) => Update::Final(text.to_string()),
    };
    let _ = side.reply.unbounded_send(update);
    let line = s.rpc("thread/unsubscribe", json!({"threadId": thread}), None);
    s.outbox.push(line);
}

/// Side-thread traffic never reaches the chat; its title renames the main thread.
pub(crate) fn side_event(s: &mut State, thread: &str, method: &str, p: &Value) {
    if !matches!(&s.naming, Naming::Side(side) if side == thread) {
        return;
    }
    match method {
        "item/completed" => {
            let Some(name) = side_title(&p["item"]) else { return };
            let params = json!({"threadId": s.thread, "name": name});
            let line = s.rpc("thread/name/set", params, None);
            s.outbox.push(line);
        }
        "turn/completed" => {}
        _ => return,
    }
    s.naming = Naming::Off;
    let line = s.rpc("thread/unsubscribe", json!({"threadId": thread}), None);
    s.outbox.push(line);
}

pub(crate) fn side_title(item: &Value) -> Option<String> {
    let text = item["text"].as_str().filter(|_| item["type"] == "agentMessage")?;
    let reply = serde_json::from_str::<Value>(text).ok()?;
    let title = reply["title"].as_str()?.trim();
    (!title.is_empty()).then(|| title.to_string())
}

pub(crate) fn output_delta(s: &mut State, p: &Value) -> Vec<Update> {
    let Some(call) = p["itemId"].as_str().and_then(|id| s.tools.get_mut(id)) else {
        return vec![];
    };
    let output =
        s.live.entry(call.id.clone()).or_default().push(p["delta"].as_str().unwrap_or_default());
    set_output(&mut call.detail, &output);
    vec![Update::Tool(call.clone())]
}

pub(crate) const COLLAB_TOOLS: [&str; 7] = [
    "wait",
    "sendMessage",
    "sendInput",
    "followupTask",
    "resumeAgent",
    "closeAgent",
    "interruptAgent",
];

pub(crate) fn is_collab(call: &ToolCall) -> bool {
    matches!(&call.detail, ToolDetail::Other { name, .. } if COLLAB_TOOLS.contains(&name.as_str()) || name == "listAgents")
}

pub(crate) fn is_spawn(item: &Value) -> bool {
    item["tool"] == "spawnAgent"
        || (item["type"] == "subAgentActivity" && item["kind"] == "started")
}

/// Routes the threads a spawn row's item names to that row.
pub(crate) fn adopt(s: &mut State, item: &Value) {
    let id = item["id"].as_str().unwrap_or_default();
    if !is_spawn(item) || !s.tools.contains_key(id) {
        return;
    }
    let receivers = item["receiverThreadIds"].as_array().into_iter().flatten();
    for thread in receivers.chain([&item["agentThreadId"]]).filter_map(Value::as_str) {
        s.agents.insert(thread.to_string(), id.to_string());
    }
}

/// Opens an item's rows; `parent` is the spawn row of the thread it ran in.
pub(crate) fn item_started(s: &mut State, parent: Option<&str>, item: &Value) -> Vec<Update> {
    if item["type"] == "subAgentActivity" && item["kind"] != "started" {
        return sub_activity(s, item);
    }
    let mut out = vec![];
    for mut call in item_calls(item) {
        call.parent = parent.map(str::to_string);
        let receivers = item["receiverThreadIds"].as_array().map(Vec::as_slice).unwrap_or_default();
        if let ("collabAgentToolCall", false, [one]) =
            (item["type"].as_str().unwrap_or_default(), is_spawn(item), receivers)
        {
            call.parent = one.as_str().and_then(|t| s.agents.get(t)).cloned().or(call.parent);
        }
        if is_collab(&call) {
            for thread in receivers.iter().filter_map(Value::as_str) {
                s.collab.insert(thread.to_string(), call.id.clone());
            }
        }
        if item["type"] == "imageView" {
            call.status = ToolStatus::Done;
            call.images =
                copy_image(item["path"].as_str().unwrap_or_default(), s.attachments.as_deref())
                    .into_iter()
                    .collect();
        }
        s.tools.insert(call.id.clone(), call.clone());
        s.turn_rows.extend(item["id"].as_str().map(str::to_string));
        out.push(Update::Tool(call.clone()));
        if let Some(task) = parent.and_then(|p| s.tools.get_mut(p)) {
            child_activity(task, &call, true);
            out.push(Update::Tool(task.clone()));
        }
    }
    adopt(s, item);
    out
}

pub(crate) fn sub_activity(s: &mut State, item: &Value) -> Vec<Update> {
    let row = item["agentThreadId"].as_str().and_then(|t| s.agents.get(t));
    let Some(call) = row.and_then(|id| s.tools.get_mut(id)) else { return vec![] };
    match item["kind"].as_str().unwrap_or_default() {
        "interacted" => {
            if let ToolDetail::Task { activity, .. } = &mut call.detail {
                *activity = "Working".into();
            }
        }
        "interrupted" => call.status = ToolStatus::Stopped,
        "completed" => call.status = ToolStatus::Done,
        _ => {
            log_unknown("codex", "subAgentActivity kind");
            return vec![];
        }
    }
    vec![Update::Tool(call.clone())]
}

/// A sub-agent's thread feeds its row: its calls nest under it, its reply becomes the output.
/// With the spawn row gone, calls nest under the latest collab row aimed at the thread.
pub(crate) fn agent_event(s: &mut State, thread: &str, method: &str, p: &Value) -> Vec<Update> {
    let Some(row) = s.agents.get(thread).cloned() else { return vec![] };
    let item = &p["item"];
    let kind = item["type"].as_str().unwrap_or_default();
    let live = s.tools.contains_key(&row);
    let parent = if live { Some(row.clone()) } else { s.collab.get(thread).cloned() };
    match (method, kind) {
        ("item/started", _) => return item_started(s, parent.as_deref(), item),
        ("item/commandExecution/outputDelta", _) => return output_delta(s, p),
        (
            "item/completed",
            "commandExecution"
            | "fileChange"
            | "webSearch"
            | "mcpToolCall"
            | "contextCompaction"
            | "collabAgentToolCall"
            | "subAgentActivity"
            | "sleep"
            | "imageView"
            | "imageGeneration"
            | "dynamicToolCall",
        ) => {
            let mut out = completed(s, item);
            if kind == "fileChange" {
                for u in &out {
                    if let Update::Tool(c) = u {
                        s.tools.remove(&c.id);
                    }
                }
            }
            for u in &mut out {
                if let Update::Tool(c) = u {
                    c.parent = c.parent.take().or_else(|| parent.clone().filter(|p| *p != c.id));
                }
            }
            return out;
        }
        _ => {}
    }
    let Some(call) = s.tools.get_mut(&row) else {
        log_unknown("codex", "child event for a finished spawn row");
        return vec![];
    };
    match (method, kind) {
        ("item/completed", "agentMessage") => {
            let text = Cites::default().rewrite(item["text"].as_str().unwrap_or_default());
            set_output(&mut call.detail, &text);
        }
        ("thread/tokenUsage/updated", _) => {
            if let ToolDetail::Task { tokens, .. } = &mut call.detail {
                *tokens = p["tokenUsage"]["total"]["totalTokens"].as_u64().unwrap_or(0);
            }
        }
        ("error", _) if p["willRetry"] != true => {
            call.error = Some(error_text(&p["error"]));
        }
        ("turn/completed", _) => {
            let turn = &p["turn"];
            call.status = match turn["status"].as_str() {
                Some("completed") => ToolStatus::Done,
                Some("interrupted") => ToolStatus::Stopped,
                _ => ToolStatus::Failed,
            };
            call.error = (call.status == ToolStatus::Failed).then(|| error_text(&turn["error"]));
            call.ms = turn["durationMs"].as_u64();
        }
        _ if ignored(method) => return vec![],
        _ => {
            log_unknown("codex", method);
            return vec![];
        }
    }
    vec![Update::Tool(call.clone())]
}

pub(crate) fn completed(s: &mut State, item: &Value) -> Vec<Update> {
    let id = item["id"].as_str().unwrap_or_default();
    let status = item["status"].as_str().unwrap_or_default();
    let cut = s.cancelled && s.turn_rows.contains(id);
    match item["type"].as_str().unwrap_or_default() {
        kind @ ("agentMessage" | "plan") => {
            let sent = s.streamed.remove(id).unwrap_or(0);
            if let Some(phase) = item["phase"].as_str() {
                s.phases.entry(id.to_string()).or_insert_with(|| phase.to_string());
            }
            let mut full = item["text"].as_str().unwrap_or_default().to_string();
            if kind == "agentMessage" {
                full.push_str(&question_prose(&item["questions"]));
            }
            let out = say(s, id, full.get(sent..).unwrap_or_default());
            s.phases.remove(id);
            s.cites.buf.clear();
            out
        }
        "enteredReviewMode" | "hookPrompt" => {
            item_notice(item).map(Update::Notice).into_iter().collect()
        }
        "exitedReviewMode" => {
            s.gap = true;
            vec![Update::Delta(format!("\n\n{}", item["review"].as_str().unwrap_or_default()))]
        }
        "reasoning" => {
            s.reasoning.remove(id);
            let parts = item["summary"].as_array().into_iter().flatten();
            let summary: Vec<&str> = parts.filter_map(Value::as_str).collect();
            if s.streamed.remove(id).is_some() || summary.is_empty() {
                return vec![];
            }
            think(s, id, 0, &summary.join("\n\n"))
        }
        "commandExecution" => {
            let mut call = s.tools.remove(id).unwrap_or_else(|| command_call(item));
            s.terminals.remove(id);
            let streamed = s.live.remove(id).map(|mut live| live.push("")).unwrap_or_default();
            let code = item["exitCode"].as_i64();
            let failed = matches!(status, "failed" | "declined") || code.is_some_and(|c| c != 0);
            finish(&mut call, item["aggregatedOutput"].as_str().unwrap_or(&streamed), failed);
            if let (ToolDetail::Command { exit_code, .. }, Some(code)) = (&mut call.detail, code) {
                *exit_code = Some(code as i32);
            }
            halted(&mut call, status == "declined", cut);
            call.ms = item["durationMs"].as_u64();
            vec![Update::Tool(call)]
        }
        "fileChange" => {
            let mut out = vec![];
            for mut call in item_calls(item) {
                s.tools.remove(&call.id);
                finish(&mut call, "", status != "completed");
                halted(&mut call, status == "declined", cut);
                let bare = matches!(&call.detail, ToolDetail::Edit { diff, .. } if diff.is_empty());
                if bare && call.status == ToolStatus::Done {
                    s.tools.insert(call.id.clone(), call.clone());
                }
                out.push(Update::Tool(call));
            }
            out
        }
        kind @ ("sleep" | "imageView" | "imageGeneration" | "dynamicToolCall") => {
            let prior = s.tools.remove(id);
            let Some(mut call) = item_calls(item).pop() else { return vec![] };
            call.images = prior.map(|p| p.images).unwrap_or_default();
            finish_extra(s, kind, item, &mut call);
            halted(&mut call, false, cut);
            vec![Update::Tool(call)]
        }
        "collabAgentToolCall" | "subAgentActivity" => {
            adopt(s, item);
            let spawn = is_spawn(item);
            if item["type"] == "subAgentActivity" && !spawn || spawn && status != "failed" {
                return vec![];
            }
            let rebuilt = || item_calls(item).pop().filter(|_| !spawn);
            let Some(mut call) = s.tools.remove(id).or_else(rebuilt) else { return vec![] };
            finish(&mut call, "", status == "failed");
            vec![Update::Tool(call)]
        }
        kind @ ("webSearch" | "mcpToolCall" | "contextCompaction") => {
            s.tools.remove(id);
            for r in item["results"].as_array().into_iter().flatten() {
                if let (Some(rid), Some(url)) = (r["ref_id"].as_str(), r["url"].as_str()) {
                    s.cites.refs.insert(rid.to_string(), url.to_string());
                }
            }
            let Some(mut call) = item_calls(item).pop() else { return vec![] };
            let (text, failed) = match item["error"]["message"].as_str() {
                Some(error) => (error.to_string(), true),
                None if kind == "webSearch" => (search_results(item), false),
                None => (mcp_text(&item["result"]), status == "failed"),
            };
            finish(&mut call, &text, failed);
            if kind == "mcpToolCall" {
                call.images = save_images(&item["result"]["content"], s.attachments.as_deref());
            }
            halted(&mut call, false, cut);
            call.ms = item["durationMs"].as_u64();
            vec![Update::Tool(call)]
        }
        "userMessage" => vec![],
        other => {
            log_unknown("codex", &format!("item {other}"));
            vec![]
        }
    }
}

pub(crate) fn item_calls(item: &Value) -> Vec<ToolCall> {
    let id = item["id"].as_str().unwrap_or_default();
    let text = |key: &str| item[key].as_str().unwrap_or_default();
    match text("type") {
        "commandExecution" => vec![command_call(item)],
        "fileChange" => {
            let changes = item["changes"].as_array().into_iter().flatten().enumerate();
            changes
                .map(|(i, c)| {
                    let id = format!("{id}#{i}");
                    let path = &c["path"];
                    match c["kind"]["type"].as_str() {
                        Some("update") if c["kind"]["move_path"].is_string() => {
                            let to = &c["kind"]["move_path"];
                            let path = format!(
                                "{} → {}",
                                path.as_str().unwrap_or_default(),
                                to.as_str().unwrap_or_default()
                            );
                            let call = start(
                                id,
                                "apply_patch",
                                &json!({"file_path": path, "diff": c["diff"]}),
                            );
                            ToolCall { title: "Rename file".into(), ..call }
                        }
                        Some("add") => {
                            start(id, "write", &json!({"file_path": path, "content": c["diff"]}))
                        }
                        Some("delete") => ToolCall {
                            title: "Delete file".into(),
                            ..start(id, "write", &json!({"file_path": path, "content": ""}))
                        },
                        _ => {
                            start(id, "apply_patch", &json!({"file_path": path, "diff": c["diff"]}))
                        }
                    }
                })
                .collect()
        }
        "webSearch" => {
            let input = json!({"query": item["query"], "url": item["action"]["url"]});
            vec![start(id, "web_search", &input)]
        }
        "mcpToolCall" => {
            let name = format!("mcp__{}__{}", text("server"), text("tool"));
            vec![start(id, &name, &item["arguments"])]
        }
        "contextCompaction" => vec![start(id, "compact_context", &Value::Null)],
        "dynamicToolCall" => vec![start(id, text("tool"), &item["arguments"])],
        "imageView" => {
            let call = start(id, "read", &json!({"file_path": item["path"]}));
            vec![ToolCall { title: "View image".into(), ..call }]
        }
        "imageGeneration" => {
            let detail = ToolDetail::Other {
                name: "imageGeneration".into(),
                input: text("revisedPrompt").to_string(),
                output: String::new(),
            };
            vec![ToolCall::new(id, "Generate image", detail)]
        }
        "sleep" => {
            let secs = item["durationMs"].as_u64().unwrap_or(0).div_ceil(1000);
            let detail = ToolDetail::Other {
                name: "sleep".into(),
                input: String::new(),
                output: String::new(),
            };
            vec![ToolCall::new(id, format!("Wait {secs} s"), detail)]
        }
        "collabAgentToolCall" if text("tool") == "spawnAgent" => {
            vec![start(id, "agent", &json!({"prompt": item["prompt"]}))]
        }
        "collabAgentToolCall" => {
            let title = match text("tool") {
                "wait" => "Wait for agents",
                "sendMessage" => "Message agent",
                "sendInput" => "Send input to agent",
                "followupTask" => "Follow up with agent",
                "resumeAgent" => "Resume agent",
                "closeAgent" => "Close agent",
                "interruptAgent" => "Interrupt agent",
                "listAgents" => "List agents",
                _ => {
                    log_unknown("codex", "collabAgentToolCall tool");
                    return vec![];
                }
            };
            let receivers = item["receiverThreadIds"].as_array().into_iter().flatten();
            let receivers: Vec<&str> = receivers.filter_map(Value::as_str).collect();
            let input = match text("prompt") {
                "" => receivers.join(", "),
                prompt => prompt.to_string(),
            };
            let detail =
                ToolDetail::Other { name: text("tool").to_string(), input, output: String::new() };
            vec![ToolCall::new(id, title, detail)]
        }
        "subAgentActivity" if text("kind") == "started" => {
            let name = text("agentPath").rsplit('/').next().unwrap_or_default();
            vec![start(id, "agent", &json!({"prompt": name, "agent": name}))]
        }
        "agentMessage" | "userMessage" | "reasoning" | "plan" | "enteredReviewMode"
        | "exitedReviewMode" | "hookPrompt" | "subAgentActivity" => vec![],
        other => {
            log_unknown("codex", &format!("item {other}"));
            vec![]
        }
    }
}

pub(crate) fn command_call(item: &Value) -> ToolCall {
    let id = item["id"].as_str().unwrap_or_default();
    match item["commandActions"].as_array().map(Vec::as_slice).unwrap_or_default() {
        [a] if a["type"] == "read" => start(id, "read", &json!({"file_path": a["path"]})),
        [a] if a["type"] == "listFiles" => start(id, "ls", &json!({"path": a["path"]})),
        [a] if a["type"] == "search" => {
            start(id, "grep", &json!({"pattern": a["query"], "path": a["path"]}))
        }
        _ => start(id, "bash", &json!({"command": command_text(item), "cwd": item["cwd"]})),
    }
}

pub(crate) fn command_text(v: &Value) -> String {
    let actions = v["commandActions"].as_array().into_iter().flatten();
    let parts: Vec<String> =
        actions.filter_map(|a| a["command"].as_str()).map(unwrap_shell).collect();
    if parts.is_empty() {
        unwrap_shell(v["command"].as_str().unwrap_or_default())
    } else {
        parts.join(" && ")
    }
}

pub(crate) fn unwrap_shell(cmd: &str) -> String {
    let mut words = cmd.splitn(3, ' ');
    let (Some(shell), Some("-lc" | "-c"), Some(body)) = (words.next(), words.next(), words.next())
    else {
        return cmd.to_string();
    };
    if !shell.ends_with("sh") {
        return cmd.to_string();
    }
    if let Some(body) = body.strip_prefix('\'').and_then(|b| b.strip_suffix('\'')) {
        return body.replace(r"'\''", "'");
    }
    let Some(body) = body.strip_prefix('"').and_then(|b| b.strip_suffix('"')) else {
        return body.to_string();
    };
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match (c, chars.clone().next()) {
            ('\\', Some(next @ ('\\' | '"' | '$' | '`'))) => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

pub(crate) fn search_results(item: &Value) -> String {
    let results = item["results"].as_array().into_iter().flatten();
    let lines: Vec<String> = results
        .map(|r| ["title", "url"].iter().filter_map(|k| r[*k].as_str()).collect::<Vec<_>>())
        .filter(|parts| !parts.is_empty())
        .map(|parts| parts.join(" "))
        .collect();
    lines.join("\n")
}

pub(crate) fn say(s: &mut State, item: &str, raw: &str) -> Vec<Update> {
    let text = s.cites.rewrite(raw);
    if text.is_empty() {
        return vec![];
    }
    if s.phases.get(item).is_some_and(|p| p == "commentary") {
        return think(s, item, 0, &text);
    }
    let mut out = vec![];
    if s.item != item {
        if s.gap {
            out.push(Update::Delta("\n\n".into()));
        }
        s.item = item.to_string();
    }
    s.gap = true;
    out.push(Update::Delta(text));
    out
}

pub(crate) fn think(s: &mut State, item: &str, index: u64, text: &str) -> Vec<Update> {
    let part = (item.to_string(), index);
    let mut out = vec![];
    if s.part.as_ref().is_some_and(|p| *p != part) {
        out.push(Update::Thinking("\n\n".into()));
    }
    s.part = Some(part);
    out.push(Update::Thinking(text.to_string()));
    out
}

pub(crate) fn usage(s: &mut State, u: &Value) -> Vec<Update> {
    let mut out: Vec<Update> = u["last"]["totalTokens"]
        .as_u64()
        .filter(|t| *t > 0)
        .map(Update::Usage)
        .into_iter()
        .collect();
    out.extend(u["modelContextWindow"].as_u64().map(Update::Window));
    let n = |key: &str| u["total"][key].as_u64().unwrap_or(0);
    let total = [n("inputTokens"), n("outputTokens"), n("cachedInputTokens")];
    if let Some([prompt, completion, cached]) = s.totals.replace(total) {
        let (prompt, completion, cached) = (
            total[0].saturating_sub(prompt),
            total[1].saturating_sub(completion),
            total[2].saturating_sub(cached),
        );
        if prompt + completion + cached > 0 {
            out.push(Update::Count { prompt, completion, cached });
        }
    }
    out
}

/// API errors arrive as JSON with the reason nested inside.
pub(crate) fn error_text(e: &Value) -> String {
    let message = e["message"].as_str().unwrap_or("codex run failed");
    serde_json::from_str::<Value>(message)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| message.to_string())
}

pub(crate) fn item_notice(item: &Value) -> Option<Notice> {
    let id = item["id"].as_str().unwrap_or_default();
    match item["type"].as_str()? {
        "enteredReviewMode" => {
            let text = format!("Reviewing {}", item["review"].as_str().unwrap_or_default());
            Some(Notice::new(format!("info-review-{id}"), NoticeKind::Info, text))
        }
        "hookPrompt" => {
            let fragments = item["fragments"].as_array().into_iter().flatten();
            let texts: Vec<&str> = fragments.filter_map(|f| f["text"].as_str()).collect();
            Some(Notice::new(format!("hook-prompt-{id}"), NoticeKind::Hook, texts.join("\n")))
        }
        _ => None,
    }
}

pub(crate) fn question_prose(questions: &Value) -> String {
    let mut out = String::new();
    for q in questions.as_array().into_iter().flatten() {
        out.push_str(&format!("\n\n**{}**", q["title"].as_str().unwrap_or_default()));
        for option in q["options"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            out.push_str(&format!("\n- {option}"));
        }
    }
    out
}

pub(crate) fn mcp_text(result: &Value) -> String {
    let text = result_text(&result["content"]);
    match &result["structuredContent"] {
        Value::Null => text,
        structured if text.is_empty() => {
            serde_json::to_string_pretty(structured).unwrap_or_default()
        }
        _ => text,
    }
}

/// Settles a sleep, image or dynamic tool row from its completed item.
pub(crate) fn finish_extra(s: &State, kind: &str, item: &Value, call: &mut ToolCall) {
    let dir = s.attachments.as_deref();
    match kind {
        "imageView" => {
            finish(call, "", false);
            if call.images.is_empty() {
                call.images = copy_image(item["path"].as_str().unwrap_or_default(), dir)
                    .into_iter()
                    .collect();
            }
        }
        "imageGeneration" => {
            let failure = &item["failure"];
            let failed = !failure.is_null() || item["status"] == "failed";
            let why = failure["type"].as_str().unwrap_or("Image generation failed");
            finish(call, if failed { why } else { "" }, failed);
            call.images = item["savedPath"]
                .as_str()
                .and_then(|path| copy_image(path, dir))
                .into_iter()
                .collect();
        }
        "dynamicToolCall" => {
            let items = item["contentItems"].as_array().map(Vec::as_slice).unwrap_or_default();
            let text: Vec<&str> = items.iter().filter_map(|i| i["text"].as_str()).collect();
            let failed = item["success"] == false || item["status"] == "failed";
            finish(call, &text.join("\n"), failed);
            call.ms = item["durationMs"].as_u64();
            let blocks: Vec<Value> = items.iter().filter_map(data_url_image).collect();
            call.images = save_images(&Value::Array(blocks), dir);
        }
        _ => finish(call, "", item["status"] == "failed"),
    }
}

pub(crate) fn data_url_image(item: &Value) -> Option<Value> {
    let (mime, data) = item["imageUrl"].as_str()?.strip_prefix("data:")?.split_once(";base64,")?;
    Some(json!({"type": "image", "data": data, "mimeType": mime}))
}

/// Copies a local image into the chat's attachment folder (ADR 0001).
pub(crate) fn copy_image(path: &str, dir: Option<&Path>) -> Option<Attachment> {
    let (dir, from) = (dir?, Path::new(path));
    let mime = match from.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let name = format!("{}-{seq}-{}", now_ms(), from.file_name()?.to_str()?);
    let to = dir.join(&name);
    std::fs::create_dir_all(dir).ok()?;
    let size = std::fs::copy(from, &to).ok()?;
    Some(Attachment { name, path: to, mime: mime.to_string(), size })
}
