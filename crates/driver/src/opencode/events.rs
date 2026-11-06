use std::collections::HashSet;
use std::time::Instant;

use serde_json::{Map, Value, json};

use aether_core::{
    AccessMode, Answer, Approval, ApprovalKind, ApprovalOption, Ask, Choice, Field, FieldKind,
    Form, Notice, NoticeKind, NoticeState, ToolCall, ToolDetail, ToolStatus, Update, When,
};

use super::{Child, OpencodeAsk, State};
use crate::spawn::log_unknown;
use crate::tool::{
    apply_patch_text, child_activity, classify, finish, halted, now_ms, result_text, set_output,
    start as start_call,
};

pub(super) fn decision(answer: &Answer) -> (&'static str, Option<String>) {
    match answer {
        Answer::Pick(id) if id == "always" => ("always", None),
        Answer::Approve { .. } | Answer::Reply(_) | Answer::Pick(_) => ("once", None),
        Answer::DenyWith(note) => ("reject", Some(note.clone())),
        _ => ("reject", None),
    }
}

// a bare "*" would allow every later write; an empty save makes the server treat "always" as once
fn scoped(save: &[String]) -> bool {
    !save.is_empty() && !save.iter().any(|p| p == "*")
}

fn notice(id: String, kind: NoticeKind, state: NoticeState, text: impl Into<String>) -> Notice {
    Notice { state, ..Notice::new(id, kind, text) }
}

fn settled(mut n: Notice) -> Update {
    n.state = NoticeState::Done;
    n.until_ms = None;
    Update::Notice(n)
}

fn cost(data: &Value) -> Option<Update> {
    let micros = (data["cost"].as_f64().unwrap_or(0.0) * 1e6).round() as u64;
    (micros > 0).then_some(Update::Cost(micros))
}

pub(super) fn same_file(a: &str, b: &str) -> bool {
    a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}"))
}

fn auto_approves(access: AccessMode, action: &str) -> bool {
    match access {
        AccessMode::Full => true,
        AccessMode::AutoEdit => matches!(action, "edit" | "write" | "patch"),
        AccessMode::Ask | AccessMode::Auto | AccessMode::ReadOnly => false,
    }
}

pub(super) fn settle(s: &mut State) -> Vec<Update> {
    // a background child's ask outlives the turn; the root's own asks end with it
    let root = s.root.clone();
    s.asks.retain(|_, a| a.session() != root);
    let asks = &s.asks;
    s.cards.retain(|id, _| asks.contains_key(id));
    s.calls.retain(|key, _| asks.values().any(|a| a.key() == key));
    let tools = &s.tools;
    s.denied.retain(|call| tools.contains_key(call));
    let background = &s.background;
    let live = |task: &String| background.contains_key(task) || tools.contains_key(task);
    s.children.retain(|sid, c| live(&c.task) || c.task == *sid);
    s.step_error = None;
    s.dismissed = false;
    s.streamed.clear();
    s.edited.clear();
    s.changed.clear();
    s.inbox_seen.clear();
    let mut out: Vec<Update> = s.retry.take().map(settled).into_iter().collect();
    if !std::mem::take(&mut s.active) {
        return out;
    }
    // every word already went out as a Delta
    out.push(Update::Final(String::new()));
    // fresh ids after the Final: the ended reply's cards expire, these open a side turn
    let mut kept: Vec<u64> = s.cards.keys().copied().collect();
    kept.sort_unstable();
    for old in kept {
        let (Some(ask), Some(card)) = (s.asks.remove(&old), s.cards.remove(&old)) else {
            continue;
        };
        s.next_ask += 1;
        s.asks.insert(s.next_ask, ask);
        s.cards.insert(s.next_ask, card.clone());
        out.push(Update::Ask { id: s.next_ask, ask: card });
    }
    out
}

/// Stops Background rows and the child rows under them, children first so they still
/// route to their agent's reply.
fn stop_background(s: &mut State) -> Vec<Update> {
    let mut tasks: Vec<String> = s.background.keys().cloned().collect();
    tasks.sort();
    let mut kids: Vec<String> = s
        .tools
        .iter()
        .filter(|(_, (_, c, _))| c.parent.as_ref().is_some_and(|p| tasks.contains(p)))
        .map(|(id, _)| id.clone())
        .collect();
    kids.sort();
    let kids = kids.iter().filter_map(|id| s.tools.remove(id)).map(|(_, call, _)| call);
    let rows: Vec<ToolCall> =
        kids.chain(tasks.iter().filter_map(|id| s.background.remove(id))).collect();
    rows.into_iter()
        .map(|mut call| {
            call.status = ToolStatus::Stopped;
            Update::Tool(call)
        })
        .collect()
}

/// The session is gone for good: stops background work, refuses steers it never took.
fn end_session(s: &mut State, why: String) -> Vec<Update> {
    s.dead = Some(why.clone());
    s.asks.clear();
    let mut out = stop_background(s);
    let mut steers: Vec<(String, String)> = s.steers.drain().collect();
    steers.sort();
    out.extend(steers.into_iter().map(|(_, text)| Update::SteerRefused(text)));
    out.push(Update::Error(why));
    out.extend(settle(s));
    out
}

pub(super) fn lost_service(s: &mut State, log_tail: &str) -> Vec<Update> {
    if !s.active {
        return vec![];
    }
    let msg = format!("OpenCode service stopped responding\n{log_tail}");
    end_session(s, msg.trim_end().to_string())
}

pub(super) fn disconnected(s: &mut State, log_tail: impl FnOnce() -> String) -> Vec<Update> {
    if s.active {
        return lost_service(s, &log_tail());
    }
    s.lost = true;
    vec![]
}

fn retry_over(s: &mut State) -> Vec<Update> {
    s.retry.take().map(settled).into_iter().collect()
}

fn remember_model(s: &mut State, model: &Value) {
    let mut m = json!({"providerID": model["providerID"], "id": model["id"]});
    if let Some(v) = model["variant"].as_str() {
        m["variant"] = v.into();
    }
    s.model = Some(m);
}

pub(super) fn map_event(s: &mut State, ty: &str, data: &Value) -> Vec<Update> {
    let text = |k: &str| data[k].as_str().filter(|t| !t.is_empty()).map(str::to_string);
    let sid = data["sessionID"].as_str().unwrap_or_default();
    if !ty.starts_with("permission.")
        && !ty.starts_with("form.")
        && let Some(child) = s.children.get(sid).cloned()
    {
        return child_event(s, &child, ty, data);
    }
    match ty {
        "session.text.delta" => {
            let Some(d) = text("delta") else { return vec![] };
            let key = (
                data["assistantMessageID"].as_str().unwrap_or_default().to_string(),
                data["ordinal"].as_u64().unwrap_or(0),
            );
            s.streamed.entry(key).or_default().push_str(&d);
            [retry_over(s), vec![Update::Delta(d)]].concat()
        }
        "session.text.ended" => {
            let key = (
                data["assistantMessageID"].as_str().unwrap_or_default().to_string(),
                data["ordinal"].as_u64().unwrap_or(0),
            );
            let Some(full) = data["text"].as_str() else { return vec![] };
            let seen = s.streamed.entry(key).or_default();
            // a gap in the middle or start can't be patched without garbling; skip it
            let rest = full.strip_prefix(seen.as_str()).filter(|r| !r.is_empty()).map(String::from);
            if rest.is_some() {
                *seen = full.into();
            }
            match rest {
                Some(rest) => vec![Update::Delta(rest)],
                None => vec![],
            }
        }
        "session.reasoning.delta" => text("delta").map(Update::Thinking).into_iter().collect(),
        "session.tool.input.started" | "session.tool.called" => tool(s, data, ToolStatus::Running),
        "session.tool.progress" => progress(s, data),
        "session.tool.success" => tool(s, data, ToolStatus::Done),
        "session.tool.failed" => tool(s, data, ToolStatus::Failed),
        "session.step.started" => {
            s.step_error = None;
            if data["model"].is_object() {
                remember_model(s, &data["model"]);
            }
            retry_over(s)
        }
        "session.step.ended" => {
            s.changed.extend(data["files"].as_array().into_iter().flatten().filter_map(|f| {
                f.as_str().or(f["file"].as_str()).or(f["path"].as_str()).map(String::from)
            }));
            let mut out = usage(&data["tokens"]);
            out.extend(cost(data));
            if data["finish"] == "length" {
                out.push(Update::Warning("The reply hit the model's output limit".into()));
            }
            out
        }
        "session.step.failed" => {
            let kind = data["error"]["type"].as_str().unwrap_or_default();
            let mut out = usage(&data["tokens"]);
            out.extend(cost(data));
            match kind {
                "aborted" => {}
                "provider.content-filter" => out.push(Update::Warning(
                    "The provider's content filter stopped this reply".into(),
                )),
                _ => s.step_error = Some(error_message(&data["error"])),
            }
            out
        }
        "session.retry.scheduled" => {
            let text = format!(
                "Retrying (attempt {}): {}",
                data["attempt"].as_u64().unwrap_or(1),
                error_message(&data["error"])
            );
            let n = Notice {
                until_ms: data["at"].as_i64(),
                ..notice(format!("retry-{}", s.turn), NoticeKind::Retry, NoticeState::Running, text)
            };
            s.retry = Some(n.clone());
            vec![Update::Notice(n)]
        }
        "session.model.selected" => {
            let m = &data["model"];
            let (provider, id) = (m["providerID"].as_str(), m["id"].as_str());
            let Some(id) = id else { return vec![] };
            remember_model(s, m);
            let mut out = vec![Update::Model(format!("{}/{id}", provider.unwrap_or_default()))];
            if !data["previous"].is_null() {
                s.notices += 1;
                let id = format!("model-{}", s.notices);
                let text = format!("Model changed to {}", m["id"].as_str().unwrap_or_default());
                out.push(Update::Notice(notice(
                    id,
                    NoticeKind::ModelChange,
                    NoticeState::Done,
                    text,
                )));
            }
            out
        }
        "session.synthetic" => {
            let first = data["text"].as_str().and_then(|t| t.lines().next()).unwrap_or_default();
            if first.is_empty() {
                return vec![];
            }
            s.notices += 1;
            let id = format!("info-synthetic-{}", s.notices);
            vec![Update::Notice(notice(id, NoticeKind::Info, NoticeState::Done, first))]
        }
        "aether.diff" => diff_rows(s, data),
        "session.execution.started" if !s.active => {
            s.active = true;
            s.turn += 1;
            s.wakes += 1;
            s.started = now_ms() as u64;
            let id = format!("wake-{}", s.wakes);
            let text = match s.wake_reason.take() {
                Some(why) => format!("OpenCode resumed this session on its own: {why}"),
                None => "OpenCode resumed this session on its own".into(),
            };
            vec![Update::Wake, Update::Notice(Notice::new(id, NoticeKind::Wake, text))]
        }
        "session.execution.started" => vec![],
        "session.skill.activated" => text("name").map(Update::Skill).into_iter().collect(),
        "session.execution.succeeded" => {
            [s.step_error.take().map(Update::Error).into_iter().collect(), settle(s)].concat()
        }
        // a real service stop suspends the run, which resumes on boot; a dismissed form doesn't
        "session.execution.interrupted" => match data["reason"].as_str().unwrap_or_default() {
            "shutdown" if !s.dismissed => {
                let text = "OpenCode's service stopped. This turn resumes when it restarts.";
                vec![Update::Notice(Notice::new("info-service", NoticeKind::Info, text))]
            }
            reason @ ("inactivity" | "superseded") if s.active => {
                let warning = Update::Warning(format!("OpenCode stopped this turn ({reason})"));
                [vec![warning], settle(s)].concat()
            }
            _ => settle(s),
        },
        "session.execution.failed" if s.active => {
            let retry = s.retry.take().map(|mut n| {
                n.state = NoticeState::Failed;
                n.until_ms = None;
                Update::Notice(n)
            });
            let why = s.step_error.take().unwrap_or_else(|| error_message(&data["error"]));
            let why = if why.contains("User not found") {
                "OpenCode error: User not found. The selected model requires an authenticated provider. Switch to OpenCode's Default model or run 'opencode auth login' in terminal.".into()
            } else {
                why
            };
            [retry.into_iter().collect(), vec![Update::Error(why)], settle(s)].concat()
        }
        "session.deleted" => end_session(s, "OpenCode session was deleted".into()),
        "session.inbox.enqueued" if data["item"]["type"] == "synthetic" => {
            shell_done(s, &data["item"]["payload"])
        }
        "session.inbox.delivered" if s.steers.contains_key(inbox(data)) => {
            vec![Update::Steered(s.steers.remove(inbox(data)).unwrap_or_default())]
        }
        "session.inbox.cancelled" if s.steers.contains_key(inbox(data)) => {
            vec![Update::SteerRefused(s.steers.remove(inbox(data)).unwrap_or_default())]
        }
        "session.inbox.cancelled" if s.active && data["inboxID"] == s.prompt.as_str() => {
            [vec![Update::Error("OpenCode cancelled the prompt".into())], settle(s)].concat()
        }
        "session.inbox.delivered" | "session.inbox.cancelled" => {
            if let Some(id) = data["inboxID"].as_str() {
                s.inbox_seen.insert(id.into(), ty.ends_with("delivered"));
            }
            vec![]
        }
        "session.compaction.started" => {
            s.compactions += 1;
            let id = format!("compact-{}", s.compactions);
            vec![Update::Notice(notice(
                id,
                NoticeKind::Compaction,
                NoticeState::Running,
                "Compacting context",
            ))]
        }
        "session.compaction.ended" | "session.compaction.failed" => {
            s.compactions = s.compactions.max(1);
            let id = format!("compact-{}", s.compactions);
            let failed = ty.ends_with("failed");
            let (state, text) = if failed {
                (NoticeState::Failed, error_message(&data["error"]))
            } else {
                (NoticeState::Done, "Compacted context".into())
            };
            let detail = if failed { None } else { text_of(data, "text") };
            vec![Update::Notice(Notice {
                detail,
                ..notice(id, NoticeKind::Compaction, state, text)
            })]
        }
        "form.created" => {
            let form = &data["form"];
            let id = form["id"].as_str().unwrap_or_default();
            let known =
                s.asks.values().any(|a| matches!(a, OpencodeAsk::Form { id: f, .. } if f == id));
            if id.is_empty() || form["fields"].as_array().is_none_or(Vec::is_empty) || known {
                return vec![];
            }
            if let Some(call) = form["metadata"]["tool"]["id"].as_str() {
                s.calls.insert(id.into(), call.into());
            }
            let session = form["sessionID"].as_str().unwrap_or_default().to_string();
            let source = source_of(s, &session);
            let (ask, fields) = form_ask(form, source);
            s.next_ask += 1;
            s.asks.insert(s.next_ask, OpencodeAsk::Form { id: id.into(), fields, session });
            s.cards.insert(s.next_ask, Ask::Form(ask.clone()));
            vec![Update::Ask { id: s.next_ask, ask: Ask::Form(ask) }]
        }
        "form.replied" | "form.cancelled" => {
            s.dismissed |= ty == "form.cancelled" && data["sessionID"] == s.root.as_str();
            let form = data["id"].as_str().unwrap_or_default();
            resolve(s, |a| matches!(a, OpencodeAsk::Form { id, .. } if id == form))
        }
        "permission.asked" => {
            let request = data["id"].as_str().unwrap_or_default().to_string();
            let action = data["action"].as_str().unwrap_or_default();
            let session = data["sessionID"].as_str().unwrap_or_default().to_string();
            if s.asks
                .values()
                .any(|a| matches!(a, OpencodeAsk::Permission { request: r, .. } if *r == request))
            {
                return vec![];
            }
            if auto_approves(s.access, action) {
                s.auto.push((request, session));
                return vec![];
            }
            let resources: Vec<&str> = data["resources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let save: Vec<String> = data["save"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p.as_str().map(String::from))
                .collect();
            let meta = &data["metadata"];
            let mut input = resources.join("\n");
            let patches: Vec<&str> = meta["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f["patch"].as_str())
                .collect();
            let mut diff = (!patches.is_empty()).then(|| patches.join("\n"));
            if meta["files"].is_null() {
                diff = meta["diff"].as_str().map(String::from);
                if diff.is_none() && !meta["input"].is_null() {
                    let params = serde_json::to_string_pretty(&meta["input"]).unwrap_or_default();
                    input = format!("{input}\n{params}");
                }
            }
            if let Some(call) = data["source"]["id"].as_str() {
                s.calls.insert(request.clone(), call.into());
            }
            s.next_ask += 1;
            s.asks
                .insert(s.next_ask, OpencodeAsk::Permission { request, session: session.clone() });
            let ask = Ask::Approval(Approval {
                tool: action.into(),
                title: resources.first().copied().unwrap_or(action).into(),
                input,
                reason: {
                    let message = data["message"].as_str().unwrap_or_default();
                    if scoped(&save) {
                        let scope = "Always allows for this whole project, in every chat, until removed in OpenCode:";
                        format!("{message}\n{scope} {}", save.join(", "))
                            .trim_start_matches('\n')
                            .into()
                    } else {
                        message.into()
                    }
                },
                tool_id: data["source"]["id"].as_str().map(String::from),
                kind: match action {
                    "shell" | "bash" => ApprovalKind::Command,
                    "edit" | "write" | "patch" | "apply_patch" => ApprovalKind::Edit,
                    "webfetch" => ApprovalKind::Fetch,
                    _ => ApprovalKind::Other,
                },
                diff,
                source: source_of(s, &session),
                options: if scoped(&save) {
                    vec![ApprovalOption {
                        id: "always".into(),
                        label: format!("Always allow {}", save.join(", ")),
                    }]
                } else {
                    vec![]
                },
                ..Approval::default()
            });
            s.cards.insert(s.next_ask, ask.clone());
            vec![Update::Ask { id: s.next_ask, ask }]
        }
        "permission.replied" | "permission.rejected" => {
            let request = data["requestID"].as_str().unwrap_or_default();
            if !matches!(data["reply"].as_str(), Some("once" | "always")) {
                deny(s, request);
            }
            resolve(s, |a| matches!(a, OpencodeAsk::Permission { request: r, .. } if r == request))
        }
        "session.renamed" => text("title")
            .filter(|t| !is_placeholder_title(t))
            .map(Update::Title)
            .into_iter()
            .collect(),
        // known events with nothing to show
        "session.created"
        | "session.usage.updated"
        | "session.step.streamed"
        | "session.text.started"
        | "session.reasoning.started"
        | "session.reasoning.ended"
        | "session.tool.input.delta"
        | "session.tool.input.ended"
        | "session.inbox.enqueued"
        | "session.agent.selected"
        | "server.connected"
        | "session.instructions.updated"
        | "session.compaction.delta"
        | "session.permissions" => vec![],
        _ => {
            log_unknown("opencode", ty);
            vec![]
        }
    }
}

/// A background shell finished: its synthetic inbox entry carries the exit and output.
fn shell_done(s: &mut State, payload: &Value) -> Vec<Update> {
    let meta = &payload["metadata"];
    let call = meta["shellID"].as_str().and_then(|id| s.shells.remove(id));
    let Some(mut row) = call.and_then(|call| s.background.remove(&call)) else { return vec![] };
    let exit = meta["exit"].as_i64();
    let state = meta["state"].as_str().unwrap_or("finished");
    let text = payload["text"].as_str().unwrap_or_default();
    let body = text.split_once('\n').map_or("", |(_, rest)| rest);
    let body = body.trim_end().strip_suffix("</shell>").unwrap_or(body);
    finish(&mut row, body.trim_end(), state != "completed" || exit.is_some_and(|c| c != 0));
    if let (ToolDetail::Command { exit_code, .. }, Some(code)) = (&mut row.detail, exit) {
        *exit_code = Some(code as i32);
    }
    let command =
        payload["description"].as_str().map_or_else(|| row.detail.summary(), String::from);
    let code = exit.map_or(String::new(), |c| format!(" (exit {c})"));
    s.wake_reason = Some(format!("`{command}` {state}{code}"));
    vec![Update::Tool(row)]
}

fn inbox(data: &Value) -> &str {
    data["inboxID"].as_str().unwrap_or_default()
}

fn text_of(data: &Value, key: &str) -> Option<String> {
    data[key].as_str().filter(|t| !t.is_empty()).map(String::from)
}

fn source_of(s: &State, session: &str) -> Option<String> {
    if session == s.root {
        return None;
    }
    let agent = s.children.get(session).map(|c| c.agent.as_str()).unwrap_or_default();
    Some(if agent.is_empty() { "Subagent".into() } else { agent.to_string() })
}

fn resolve(s: &mut State, hit: impl Fn(&OpencodeAsk) -> bool) -> Vec<Update> {
    let mut ids: Vec<u64> = s.asks.iter().filter(|(_, a)| hit(a)).map(|(id, _)| *id).collect();
    ids.sort_unstable();
    let mut out: Vec<Update> = ids
        .into_iter()
        .map(|id| {
            s.asks.remove(&id);
            Update::AskResolved(id)
        })
        .collect();
    if !out.is_empty() {
        out.extend(super::side_turn_end(s));
    }
    out
}

fn diff_rows(s: &mut State, data: &Value) -> Vec<Update> {
    let mut out = Vec::new();
    for f in data["files"].as_array().into_iter().flatten() {
        let (Some(file), Some(patch)) = (f["file"].as_str(), f["patch"].as_str()) else { continue };
        let changed = s.changed.iter().any(|c| same_file(c, file));
        if !changed || s.edited.iter().any(|e| same_file(e, file)) {
            continue;
        }
        let mut row =
            start_call(format!("diff-{}-{file}", s.turn), "write", &json!({"filePath": file}));
        row.title = "Changed by command".into();
        apply_patch_text(&mut row.detail, patch);
        row.status = ToolStatus::Done;
        out.push(Update::Tool(row));
    }
    out
}

/// Choice and text fields of a form, plus the raw fields `form_reply` needs for hidden ones.
pub(super) fn form_ask(form: &Value, source: Option<String>) -> (Form, Vec<Value>) {
    let raw = form["fields"].as_array().cloned().unwrap_or_default();
    let mut out = Form {
        title: form["title"].as_str().unwrap_or_default().into(),
        message: form["description"]
            .as_str()
            .or(form["message"].as_str())
            .unwrap_or_default()
            .into(),
        source,
        ..Form::default()
    };
    for f in raw.iter().filter(|f| f["hidden"] != true) {
        let ty = f["type"].as_str().unwrap_or("string");
        if ty == "external" {
            out.url = f["url"].as_str().map(String::from);
            continue;
        }
        let options: Vec<Choice> = f["options"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| {
                let value = o["value"].as_str().unwrap_or_default();
                Choice {
                    label: o["label"].as_str().unwrap_or(value).into(),
                    description: o["description"].as_str().unwrap_or_default().into(),
                    value: value.into(),
                }
            })
            .collect();
        let kind = match ty {
            "number" | "integer" => FieldKind::Number,
            "boolean" => FieldKind::Bool,
            "multiselect" => FieldKind::Multi,
            _ if !options.is_empty() => FieldKind::Choice,
            _ => FieldKind::Text,
        };
        out.fields.push(Field {
            key: f["key"].as_str().unwrap_or_default().into(),
            label: f["title"].as_str().unwrap_or_default().into(),
            description: f["description"].as_str().unwrap_or_default().into(),
            kind,
            options,
            required: f["required"] == true,
            default: strings(&f["default"]),
            secret: f["secret"] == true,
            other: f["custom"] == true,
            when: f["when"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| When {
                    key: r["key"].as_str().unwrap_or_default().into(),
                    not: r["op"] == "neq",
                    value: plain_string(&r["value"]),
                })
                .collect(),
        });
    }
    (out, raw)
}

fn plain_string(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), String::from)
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::Null => vec![],
        Value::Array(a) => a.iter().map(plain_string).collect(),
        v => vec![plain_string(v)],
    }
}

/// The `/form/{id}/reply` body: each answered field typed by its raw definition.
pub(super) fn form_reply(fields: &[Value], answer: &Answer) -> Value {
    let given: &[(String, Vec<String>)] = match answer {
        Answer::Form(f) => f,
        _ => &[],
    };
    let answered = |key: &str| given.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    let first = |key: &str| {
        answered(key).and_then(|v| v.first().cloned()).or_else(|| {
            let field = fields.iter().find(|f| f["key"] == key)?;
            strings(&field["default"]).into_iter().next()
        })
    };
    let mut out = Map::new();
    for f in fields {
        let (Some(key), ty) = (f["key"].as_str(), f["type"].as_str().unwrap_or("string")) else {
            continue;
        };
        if ty == "external" {
            continue;
        }
        if f["hidden"] == true {
            if !f["default"].is_null() {
                out.insert(key.into(), f["default"].clone());
            }
            continue;
        }
        let shown = f["when"].as_array().into_iter().flatten().all(|r| {
            let hit = first(r["key"].as_str().unwrap_or_default()).as_deref()
                == Some(plain_string(&r["value"]).as_str());
            hit != (r["op"] == "neq")
        });
        let Some(values) = answered(key).filter(|v| !v.is_empty()).filter(|_| shown) else {
            continue;
        };
        let value = match ty {
            "number" | "integer" => values[0]
                .trim()
                .parse::<i64>()
                .map(|n| json!(n))
                .or_else(|_| values[0].trim().parse::<f64>().map(|n| json!(n)))
                .unwrap_or_else(|_| json!(values[0])),
            "boolean" => json!(values[0] == "true"),
            "multiselect" => json!(values),
            _ => json!(values[0]),
        };
        out.insert(key.into(), value);
    }
    json!({"answer": out})
}

fn token_sum(tokens: &Value) -> u64 {
    ["input", "output", "reasoning"].iter().map(|k| tokens[*k].as_f64().unwrap_or(0.0) as u64).sum()
}

fn child_event(s: &mut State, child: &Child, ty: &str, data: &Value) -> Vec<Update> {
    match ty {
        "session.tool.input.started" => nested(s, child, data, ToolStatus::Running, true),
        "session.tool.called" => nested(s, child, data, ToolStatus::Running, false),
        "session.tool.success" => nested(s, child, data, ToolStatus::Done, false),
        "session.tool.failed" => nested(s, child, data, ToolStatus::Failed, false),
        "session.tool.progress" => progress(s, data),
        "session.step.ended" | "session.step.failed" => {
            let total = token_sum(&data["tokens"]);
            let mut out = task_update(s, &child.task, |t| {
                if let ToolDetail::Task { tokens, .. } = &mut t.detail {
                    *tokens += total;
                }
            });
            out.extend(cost(data));
            out
        }
        "session.text.ended" => {
            let first = data["text"].as_str().and_then(|t| t.lines().next()).unwrap_or_default();
            let first: String = first.chars().take(120).collect();
            task_update(s, &child.task, |t| {
                if let ToolDetail::Task { activity, .. } = &mut t.detail {
                    *activity = first;
                }
            })
        }
        "session.execution.failed" => {
            let why = error_message(&data["error"]);
            let out = task_update(s, &child.task, |t| {
                t.status = ToolStatus::Failed;
                t.error = Some(why);
            });
            s.background.remove(&child.task);
            out
        }
        "session.execution.succeeded" => match s.background.remove(&child.task) {
            Some(mut row) => {
                row.status = ToolStatus::Done;
                vec![Update::Tool(row)]
            }
            None => vec![],
        },
        "session.execution.interrupted" if data["reason"] != "shutdown" => {
            match s.background.remove(&child.task) {
                Some(mut row) => {
                    row.status = ToolStatus::Stopped;
                    vec![Update::Tool(row)]
                }
                None => vec![],
            }
        }
        "session.text.delta"
        | "session.text.started"
        | "session.reasoning.delta"
        | "session.reasoning.started"
        | "session.reasoning.ended"
        | "session.step.started"
        | "session.step.streamed"
        | "session.usage.updated"
        | "session.tool.input.delta"
        | "session.tool.input.ended"
        | "session.inbox.enqueued"
        | "session.inbox.delivered"
        | "session.inbox.cancelled"
        | "session.instructions.updated"
        | "session.retry.scheduled"
        | "session.compaction.started"
        | "session.compaction.ended"
        | "session.compaction.failed"
        | "session.compaction.delta"
        | "session.model.selected"
        | "session.agent.selected"
        | "session.synthetic"
        | "session.created"
        | "session.renamed"
        | "session.permissions"
        | "session.execution.started"
        | "session.execution.interrupted"
        | "session.skill.activated" => vec![],
        _ => {
            log_unknown("opencode", ty);
            vec![]
        }
    }
}

/// A child's tool row, plus its subagent's row showing the latest activity.
fn nested(
    s: &mut State,
    child: &Child,
    data: &Value,
    status: ToolStatus,
    new: bool,
) -> Vec<Update> {
    let mut out = tool(s, data, status);
    if let Some(Update::Tool(call)) = out.first().cloned() {
        out.extend(task_update(s, &child.task, |t| child_activity(t, &call, new)));
    }
    out
}

fn task_update(s: &mut State, id: &str, f: impl FnOnce(&mut ToolCall)) -> Vec<Update> {
    let row = match s.tools.get_mut(id) {
        Some((_, call, _)) => call,
        None => match s.background.get_mut(id) {
            Some(call) => call,
            None => return vec![],
        },
    };
    f(row);
    vec![Update::Tool(row.clone())]
}

fn error_message(e: &Value) -> String {
    e["message"].as_str().or(e.as_str()).unwrap_or("opencode run failed").to_string()
}

pub(super) fn deny(s: &mut State, request: &str) {
    if let Some(call) = s.calls.remove(request) {
        s.denied.insert(call);
    }
}

fn tool(s: &mut State, data: &Value, status: ToolStatus) -> Vec<Update> {
    let id = data["id"].as_str().unwrap_or_default().to_string();
    let (denied, cancelled) = (status != ToolStatus::Running && s.denied.remove(&id), s.cancelled);
    let mut name = data["name"].as_str().unwrap_or_default().to_string();
    if name.is_empty() {
        if let Some((saved_name, _, _)) = s.tools.get(&id) {
            name = saved_name.clone();
        } else {
            let content = result_text(&data["content"]);
            if content.contains("User has answered your question") {
                name = "question".to_string();
            }
        }
    }
    if name == "skill" {
        if status == ToolStatus::Running {
            s.tools.entry(id.clone()).or_insert_with(|| {
                (name.clone(), start_call(id.clone(), &name, &Value::Null), Instant::now())
            });
            return vec![];
        }
        s.tools.remove(&id);
        let skill = data["metadata"]["name"].as_str().or(data["input"]["id"].as_str());
        return skill
            .filter(|_| status == ToolStatus::Done)
            .map(|n| Update::Skill(n.into()))
            .into_iter()
            .collect();
    }
    let sid = data["sessionID"].as_str().unwrap_or_default();
    let parent = s.children.get(sid).map(|c| c.task.clone());
    let (name, call, at) = s.tools.entry(id.clone()).or_insert_with(|| {
        (name.clone(), start_call(id.clone(), &name, &Value::Null), Instant::now())
    });
    call.parent = parent.clone();
    if !data["input"].is_null() {
        (call.title, call.detail) = classify(name, &data["input"]);
    } else if call.title.is_empty() {
        (call.title, call.detail) = classify(name, &Value::Null);
    }
    let prior = (call.status, call.error.clone());
    let mut extra = Vec::new();
    if status != ToolStatus::Running {
        let content = result_text(&data["content"]);
        let text = match data["error"]["message"].as_str() {
            Some(why) if status == ToolStatus::Failed => {
                [why, &content].join("\n").trim_end().to_string()
            }
            _ => content,
        };
        finish(call, &text, status == ToolStatus::Failed);
        halted(call, denied, cancelled);
        let meta = &data["metadata"];
        if let Some(diff) = meta["diff"].as_str() {
            apply_patch_text(&mut call.detail, diff);
        }
        if status == ToolStatus::Done {
            extra = file_rows(&mut s.edited, call, name, &meta["files"]);
            if let ToolDetail::Edit { path, .. } = &call.detail
                && !path.is_empty()
            {
                s.edited.insert(path.clone());
            }
        }
        if let (ToolDetail::Command { exit_code, .. }, Some(code)) =
            (&mut call.detail, meta["exit"].as_i64())
        {
            *exit_code = Some(code as i32);
        }
        call.ms = Some(at.elapsed().as_millis() as u64);
        if let Some(session) = meta["sessionID"].as_str() {
            adopt(&mut s.children, call, session);
        }
        // a child that already failed owns the row's outcome
        if prior.0 == ToolStatus::Failed && matches!(call.detail, ToolDetail::Task { .. }) {
            (call.status, call.error) = prior;
        }
        let shell = meta["shellID"].as_str();
        if call.status == ToolStatus::Done
            && meta["status"] == "running"
            && (matches!(call.detail, ToolDetail::Task { .. }) || shell.is_some())
        {
            call.status = ToolStatus::Background;
            if let Some(shell) = shell {
                s.shells.insert(shell.to_string(), id.clone());
            }
        }
    }
    let mut out = vec![Update::Tool(call.clone())];
    out.extend(extra.into_iter().map(Update::Tool));
    let (done, is_execute) = (call.clone(), name == "execute");
    if status != ToolStatus::Running {
        s.tools.remove(&id);
        if done.status == ToolStatus::Background {
            s.background.insert(id.clone(), done);
        }
        if is_execute {
            out.extend(execute_rows(&id, &data["metadata"]["toolCalls"]));
        }
    }
    out
}

fn adopt(children: &mut std::collections::HashMap<String, Child>, call: &ToolCall, session: &str) {
    if let ToolDetail::Task { agent, .. } = &call.detail {
        let child = Child { task: call.id.clone(), agent: agent.clone() };
        // a child rebuilt after a relaunch is keyed by its own id until its call shows up
        if children.get(session).is_none_or(|c| c.task == session) {
            children.insert(session.to_string(), child);
        }
    }
}

/// One Edit row per file of a multi-file edit; the first keeps the call's id.
fn file_rows(
    edited: &mut HashSet<String>,
    call: &mut ToolCall,
    name: &str,
    files: &Value,
) -> Vec<ToolCall> {
    let mut rows = Vec::new();
    let files = files.as_array().into_iter().flatten();
    for (i, f) in files.enumerate() {
        let (Some(file), ToolDetail::Edit { .. }) = (f["file"].as_str(), &call.detail) else {
            continue;
        };
        edited.insert(file.into());
        let mut row = call.clone();
        if i > 0 {
            row.id = format!("{}#{i}", call.id);
            row.detail = classify("write", &json!({"filePath": file})).1;
        }
        if let ToolDetail::Edit { path, .. } = &mut row.detail
            && path.is_empty()
        {
            *path = file.into();
        }
        if let Some(patch) = f["patch"].as_str() {
            apply_patch_text(&mut row.detail, patch);
        }
        if name == "write" && f["status"] == "modified" {
            row.title = "Overwrite file".into();
        }
        if i == 0 {
            *call = row;
        } else {
            rows.push(row);
        }
    }
    rows
}

fn execute_rows(call: &str, calls: &Value) -> Vec<Update> {
    let rows = calls.as_array().into_iter().flatten().enumerate();
    rows.map(|(i, c)| {
        let name = c["tool"].as_str().unwrap_or_default();
        let name = name.strip_prefix("opencode.").unwrap_or(name);
        let mut row = start_call(format!("{call}#{i}"), name, &Value::Null);
        (row.title, row.detail) = classify(name, &c["input"]);
        row.parent = Some(call.to_string());
        row.status = if c["status"] == "completed" { ToolStatus::Done } else { ToolStatus::Failed };
        Update::Tool(row)
    })
    .collect()
}

fn progress(s: &mut State, data: &Value) -> Vec<Update> {
    let Some((_, call, _)) = s.tools.get_mut(data["id"].as_str().unwrap_or_default()) else {
        return vec![];
    };
    if let Some(session) = data["metadata"]["sessionID"].as_str() {
        adopt(&mut s.children, call, session);
    }
    let Some(output) = data["metadata"]["output"].as_str() else {
        return vec![];
    };
    set_output(&mut call.detail, output);
    vec![Update::Tool(call.clone())]
}

fn usage(tokens: &Value) -> Vec<Update> {
    if tokens.is_null() {
        return vec![];
    }
    let n = |v: &Value| v.as_f64().unwrap_or(0.0) as u64;
    let (input, read, write) =
        (n(&tokens["input"]), n(&tokens["cache"]["read"]), n(&tokens["cache"]["write"]));
    let completion = n(&tokens["output"]) + n(&tokens["reasoning"]);
    let prompt = input + read + write;
    vec![Update::Usage(prompt + completion), Update::Count { prompt, completion, cached: read }]
}

pub fn is_placeholder_title(title: &str) -> bool {
    let t = title.trim();
    t.is_empty()
        || t.starts_with("New session - ")
        || t.eq_ignore_ascii_case("new session")
        || t.eq_ignore_ascii_case("untitled")
}

#[cfg(test)]
#[path = "../../tests/unit/opencode/events.rs"]
mod tests;

/// Records a steer by inbox id; an ack that beat the POST response lands now.
pub(super) fn register_steer(s: &mut State, id: &str, text: String) -> Vec<Update> {
    match s.inbox_seen.remove(id) {
        Some(true) => vec![Update::Steered(text)],
        Some(false) => vec![Update::SteerRefused(text)],
        None => {
            s.steers.insert(id.into(), text);
            vec![]
        }
    }
}
