use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::{Child, ChildStdin};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

use aether_core::{
    AccessMode, Answer, Ask, Attachment, MutexExt, Notice, PlanItem, ToolCall, ToolStatus, Update,
    without_attached_files,
};

use super::Driver;
use super::record::Recorder;
use super::spawn::StderrTail;

#[cfg(test)]
use super::DriverStartOptions;
#[cfg(test)]
#[allow(unused_imports)]
use super::record;
#[cfg(test)]
#[allow(unused_imports)]
use super::spawn::{command, log_unknown, spawn_logged};
#[cfg(test)]
#[allow(unused_imports)]
use super::tool::{
    apply_hunks, child_activity, classify, finish, halted, now_ms, result_text, save_images, start,
};
#[cfg(test)]
#[allow(unused_imports)]
use super::version::gate_binary;
#[cfg(test)]
#[allow(unused_imports)]
use aether_core::{
    Approval, ApprovalKind, ApprovalOption, Choice, Field, FieldKind, Form, NoticeKind,
    NoticeState, Plan, PlanStatus, Provider, Question, ToolDetail, home_dir,
};
#[cfg(test)]
#[allow(unused_imports)]
use base64::Engine;
#[cfg(test)]
#[allow(unused_imports)]
use base64::engine::general_purpose::STANDARD;
#[cfg(test)]
#[allow(unused_imports)]
use serde_json::Map;
#[cfg(test)]
#[allow(unused_imports)]
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(test)]
#[allow(unused_imports)]
use std::process::{ExitStatus, Stdio};
#[cfg(test)]
#[allow(unused_imports)]
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};

mod approve;
mod events;
mod spawn;

pub(crate) use approve::*;
pub(crate) use events::*;
pub use spawn::config_dir;
pub(crate) use spawn::*;

pub struct ClaudeDriver {
    child: Arc<Mutex<Option<Child>>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    tx: UnboundedSender<Update>,
    state: Arc<Mutex<State>>,
    resumed: bool,
    titled: bool,
    next_request: u64,
    effort: String,
    rec: Option<Arc<Recorder>>,
}

#[derive(Clone, Debug)]
struct ClaudeAsk {
    request_id: String,
    input: Value,
    kind: Kind,
    /// Task id of the subagent that asked.
    agent: Option<String>,
    card: Ask,
}

#[derive(Clone, Debug)]
pub(crate) enum Kind {
    /// `shown` is the summary the card offered for editing.
    Approval {
        tool: String,
        shown: String,
        suggestions: Vec<Value>,
    },
    /// Question texts of one AskUserQuestion, in order.
    Questions(Vec<String>),
    Plan {
        suggestions: Vec<Value>,
    },
    /// Field key and JSON type.
    Elicitation(Vec<(String, String)>),
}

#[derive(Default)]
pub(crate) struct State {
    tools: HashMap<String, (ToolCall, Instant)>,
    asks: HashMap<u64, ClaudeAsk>,
    next_ask: u64,
    active: bool,
    /// The interrupted turn, until its result lands.
    cancelled: Option<u64>,
    /// Interrupt requests sent; the number of the latest.
    interrupts: u64,
    turn: u64,
    dead: Option<String>,
    /// Prompts (`None`) and steers (`Some(text)`) whose `isReplay` echo hasn't arrived.
    echoes: VecDeque<Option<String>>,
    /// (uuid, text) of this turn's steers, so a Stop's `still_queued` can name them.
    steer_ids: Vec<(String, String)>,
    /// Steers a Stop left queued; they run next in the same reply.
    held: usize,
    /// Rows started this turn; only these can be cut by its Stop.
    turn_rows: HashSet<String>,
    /// A turn an idle ask opened, with no main output yet.
    quiet_wake: bool,
    /// Model Aether set and the CLI confirmed, and one asked for but not yet confirmed.
    chosen: String,
    choosing: Option<String>,
    /// From the latest `init`; keys `modelUsage`.
    model: String,
    session: String,
    stderr: StderrTail,
    /// What `opts.access` asked for.
    requested: AccessMode,
    /// Last `permissionMode` reported.
    mode: Option<AccessMode>,
    modes: u32,
    /// Lines for the CLI, written after the stdout line that queued them.
    outbox: Vec<Value>,
    /// Claude task id to (tool row id, description).
    tasks: HashMap<String, (String, String)>,
    /// `agent_id` to subagent type.
    agents: HashMap<String, String>,
    /// Rows whose tool result said the work went to the background; their notification settles them.
    background: HashSet<String>,
    /// Forwarded subagent text deltas per parent row, until its assistant message lands.
    sub_text: HashMap<String, String>,
    wake_reason: Option<String>,
    wakes: u32,
    attachments: Option<PathBuf>,
    retry: Option<Notice>,
    compactions: u32,
    /// The compaction notice still waiting for its boundary.
    compacting: Option<Notice>,
    models: u32,
    /// Rate-limit notice ids shown, so `allowed` only clears a warning that was there.
    limits: HashSet<String>,
    /// Last `total_cost_usd`; it is a process-wide running total.
    cost_seen: f64,
    /// `modelUsage` sums (prompt, completion, cached); also a running total.
    usage_seen: [u64; 3],
    todos: Vec<(String, PlanItem)>,
    /// TaskCreate tool_use id to its item, until the result names the task id.
    todo_uses: HashMap<String, PlanItem>,
    /// Tool uses that get no row (TaskUpdate, Skill); their results are dropped.
    quiet: HashSet<String>,
    /// Message id from the latest main `message_start`.
    streaming: String,
    /// Assistant message ids whose text already arrived as deltas.
    texted: HashSet<String>,
    /// (message uuid, text) of the current text segment, for refusal retractions.
    turn_texts: Vec<(String, String)>,
}

const TODOS_ID: &str = "todos";

const INIT_REQUEST: &str = "aether-init";

/// The SDK publishes no payload schema for any dialog kind; an undeclared kind makes the CLI
/// fall back to its no-dialog behavior.
const DIALOG_KINDS: &[&str] = &[];

const TITLE_REQUEST: &str = "title";

const MODEL_REQUEST: &str = "aether-model-";

const STOP_REQUEST: &str = "aether-stop-";

const INTERRUPT_REQUEST: &str = "aether-interrupt-";

const ACTIVITY_CHARS: usize = 120;

fn on_prompt(s: &mut State, text: &str) {
    s.active = true;
    s.quiet_wake = false;
    s.turn += 1;
    s.wake_reason = None;
    s.echoes.clear();
    s.steer_ids.clear();
    s.turn_rows.clear();
    // slash commands run without an echo
    if !text.starts_with('/') {
        s.echoes.push_back(None);
    }
}

/// `None` when no turn is running; else the updates to send now.
fn on_steer(s: &mut State, text: &str) -> Option<Vec<Update>> {
    if !s.active {
        return None;
    }
    if !text.starts_with('/') {
        let text = without_attached_files(text).to_string();
        s.steer_ids.push((new_uuid(), text.clone()));
        s.echoes.push_back(Some(text));
    }
    // slash commands get no echo, so nothing else would report them
    Some(if text.starts_with('/') {
        vec![Update::Steered(without_attached_files(text).to_string())]
    } else {
        vec![]
    })
}

enum Stop {
    Idle,
    /// Send interrupt request `n`.
    Interrupt(u64),
    /// Closed without the CLI; the outbox holds the replies to send.
    Closed(Vec<Update>),
}

fn on_cancel(s: &mut State) -> Stop {
    if !s.active {
        return Stop::Idle;
    }
    if s.quiet_wake && s.echoes.is_empty() {
        // only a background agent's ask opened this turn: the CLI runs no turn to interrupt
        s.active = false;
        s.quiet_wake = false;
        let mut ids: Vec<u64> = s.asks.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let line = reply(s, id, &Answer::Deny);
            s.outbox.extend(line);
        }
        return Stop::Closed(vec![Update::Final(String::new())]);
    }
    s.cancelled = Some(s.turn);
    s.interrupts += 1;
    Stop::Interrupt(s.interrupts)
}

/// Interrupt `n` is the latest and its turn still hasn't ended: the CLI is stuck.
fn overdue(s: &State, n: u64) -> bool {
    s.active && s.cancelled.is_some() && s.interrupts == n && s.dead.is_none()
}

/// A turn an idle ask opened ends once nothing waits on the user: no result will come for it.
fn quiet_turn_end(s: &mut State) -> Option<Update> {
    if !s.quiet_wake || !s.active || !s.asks.is_empty() || !s.echoes.is_empty() {
        return None;
    }
    s.active = false;
    s.quiet_wake = false;
    Some(Update::Final(String::new()))
}

/// Stops background rows and the live rows under them, children first so they still route
/// to their agent's reply: nothing reports them once the process is gone.
fn settle_background(s: &mut State) -> Vec<Update> {
    let mut gone: Vec<String> = std::mem::take(&mut s.background)
        .into_iter()
        .filter(|id| s.tools.get(id).is_some_and(|(c, _)| c.status == ToolStatus::Background))
        .collect();
    gone.sort();
    let mut at = 0;
    while at < gone.len() {
        let mut kids: Vec<String> = s
            .tools
            .values()
            .filter(|(c, _)| {
                matches!(c.status, ToolStatus::Running | ToolStatus::Waiting)
                    && c.parent.as_ref() == Some(&gone[at])
            })
            .map(|(c, _)| c.id.clone())
            .collect();
        kids.sort();
        gone.extend(kids);
        at += 1;
    }
    s.tasks.retain(|_, (row, _)| !gone.contains(row));
    gone.iter()
        .rev()
        .filter_map(|id| s.tools.remove(id))
        .map(|(mut call, _)| {
            call.status = ToolStatus::Stopped;
            Update::Tool(call)
        })
        .collect()
}

/// The process is gone: settles background work, refuses steers it never took, reports the exit.
fn on_exit(s: &mut State, why: String) -> Vec<Update> {
    let mut out = settle_background(s);
    if s.active {
        s.active = false;
        out.extend(s.echoes.drain(..).flatten().map(Update::SteerRefused));
        out.push(Update::Error(why.clone()));
        out.push(Update::Final(String::new()));
    } else {
        out.push(Update::Warning(why.clone()));
    }
    s.dead = Some(why);
    out
}

/// True when `model` needs a `set_model` request.
fn choose_model(s: &mut State, model: &str) -> bool {
    if s.choosing.as_deref().unwrap_or(&s.chosen) == model {
        return false;
    }
    s.choosing = Some(model.to_string());
    true
}

/// The interrupt's reply: steers in `still_queued` run next; the rest were dropped.
fn on_interrupted(s: &mut State, reply: &Value) -> Vec<Update> {
    let Some(kept) = reply["still_queued"].as_array() else { return vec![] };
    let kept = |id: &str| kept.iter().filter_map(Value::as_str).any(|k| k.eq_ignore_ascii_case(id));
    let mut out = vec![];
    for (id, text) in std::mem::take(&mut s.steer_ids) {
        // echoed already: the turn took it in before the Stop
        let Some(at) = s.echoes.iter().position(|e| e.as_deref() == Some(text.as_str())) else {
            continue;
        };
        if kept(&id) {
            s.held += 1;
        } else {
            s.echoes.remove(at);
            out.push(Update::SteerRefused(text));
        }
    }
    out
}

impl Driver for ClaudeDriver {
    fn prompt(&mut self, text: String, images: Vec<Attachment>) {
        let mut s = self.state.guard();
        if let Some(why) = &s.dead {
            let _ = self.tx.unbounded_send(Update::Error(why.clone()));
            let _ = self.tx.unbounded_send(Update::Final(String::new()));
            return;
        }
        on_prompt(&mut s, &text);
        drop(s);
        self.mark(json!({"aether": "prompt", "text": text}));
        self.write(user_message(&text, &images));
        if !self.titled {
            self.titled = true;
            let description: String = without_attached_files(&text).chars().take(1000).collect();
            self.write(json!({
                "type": "control_request",
                "request_id": TITLE_REQUEST,
                "request": {
                    "subtype": "generate_session_title",
                    "description": description,
                    "persist": true,
                },
            }));
        }
    }

    fn steer(&mut self, text: String, images: Vec<Attachment>) -> bool {
        let mut s = self.state.guard();
        let Some(now) = on_steer(&mut s, &text) else {
            return false;
        };
        let uuid = s.steer_ids.last().map(|(id, _)| id.clone()).filter(|_| !text.starts_with('/'));
        drop(s);
        for u in now {
            let _ = self.tx.unbounded_send(u);
        }
        let mut message = user_message(&text, &images);
        if let Some(uuid) = uuid {
            message["uuid"] = uuid.clone().into();
            self.mark(json!({"aether": "steer", "text": text, "uuid": uuid}));
        } else {
            self.mark(json!({"aether": "steer", "text": text}));
        }
        self.write(message);
        true
    }

    fn reports_steered(&self) -> bool {
        true
    }

    fn set_options(&mut self, model: &str, effort: &str) -> bool {
        if effort != self.effort {
            return false;
        }
        if choose_model(&mut self.state.guard(), model) {
            self.next_request += 1;
            let model_name = if model.is_empty() { "default" } else { model };
            self.write(json!({
                "type": "control_request",
                "request_id": format!("{MODEL_REQUEST}{}", self.next_request),
                "request": {"subtype": "set_model", "model": model_name},
            }));
        }
        true
    }

    fn resumed(&self) -> bool {
        self.resumed
    }

    fn alive(&self) -> bool {
        self.state.guard().dead.is_none()
    }

    fn pid(&self) -> Option<u32> {
        self.child.guard().as_ref().map(Child::id)
    }

    fn stop_tool(&mut self, id: &str) -> bool {
        let line = stop_request(&self.state.guard(), id, self.next_request + 1);
        let Some(line) = line else { return false };
        self.next_request += 1;
        self.write(line);
        true
    }

    fn cancel(&mut self) {
        let mut s = self.state.guard();
        let stop = on_cancel(&mut s);
        let outbox = std::mem::take(&mut s.outbox);
        drop(s);
        let n = match stop {
            Stop::Idle => return,
            Stop::Closed(out) => {
                self.mark(json!({"aether": "cancel"}));
                for v in outbox {
                    self.write(v);
                }
                for u in out {
                    let _ = self.tx.unbounded_send(u);
                }
                return;
            }
            Stop::Interrupt(n) => n,
        };
        self.mark(json!({"aether": "cancel"}));
        self.write(json!({
            "type": "control_request",
            "request_id": format!("{INTERRUPT_REQUEST}{n}"),
            "request": {"subtype": "interrupt"},
        }));
        let (state, tx, child) = (self.state.clone(), self.tx.clone(), self.child.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            let mut s = state.guard();
            if overdue(&s, n) {
                let why = s.stderr.fatal("claude did not stop after cancel");
                let out = on_exit(&mut s, why);
                drop(s);
                if let Some(child) = child.guard().as_mut() {
                    let _ = child.kill();
                }
                for u in out {
                    let _ = tx.unbounded_send(u);
                }
            }
        });
    }

    fn answer(&mut self, id: u64, answer: Answer) {
        self.mark(json!({"aether": "answer", "id": id, "answer": answer}));
        let mut s = self.state.guard();
        let line = reply(&mut s, id, &answer);
        let end = quiet_turn_end(&mut s);
        drop(s);
        if let Some(line) = line {
            self.write(line);
        }
        if let Some(end) = end {
            let _ = self.tx.unbounded_send(end);
        }
    }
}

impl Drop for ClaudeDriver {
    fn drop(&mut self) {
        if let Ok(mut s) = self.state.lock()
            && s.dead.is_none()
        {
            for u in settle_background(&mut s) {
                let _ = self.tx.unbounded_send(u);
            }
            s.dead = Some("claude session closed".into());
        }
        if let Ok(mut stdin) = self.stdin.lock() {
            stdin.take();
        }
        if let Some(mut child) = self.child.lock().ok().and_then(|mut c| c.take()) {
            std::thread::spawn(move || {
                for _ in 0..30 {
                    if !matches!(child.try_wait(), Ok(None)) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                let _ = child.kill();
                let _ = child.wait();
            });
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/claude.rs"]
mod tests;
