use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::{Child, ChildStdin};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

use aether_core::{
    AccessMode, Answer, Attachment, MutexExt, Notice, NoticeState, ToolCall, ToolStatus, Update,
    without_attached_files,
};

use super::Driver;
use super::record::Recorder;
use super::spawn::StderrTail;
use super::tool::{Throttle, clean};

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
    apply_patch_text, child_activity, finish, halted, now_ms, result_text, save_images, set_output,
    start,
};
#[cfg(test)]
#[allow(unused_imports)]
use super::version::gate;
#[cfg(test)]
#[allow(unused_imports)]
use aether_core::{
    Approval, ApprovalKind, ApprovalOption, Ask, Choice, Field, FieldKind, Form, NoticeKind,
    Provider, Question, ToolDetail,
};
#[cfg(test)]
#[allow(unused_imports)]
use std::io::{BufRead, BufReader, Write};
#[cfg(test)]
#[allow(unused_imports)]
use std::path::Path;
#[cfg(test)]
#[allow(unused_imports)]
use std::process::Stdio;
#[cfg(test)]
#[allow(unused_imports)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
#[allow(unused_imports)]
use std::sync::mpsc::channel;
#[cfg(test)]
#[allow(unused_imports)]
use std::time::Instant;

mod approve;
mod events;
mod spawn;

pub(crate) use approve::*;
#[cfg(test)]
pub(crate) use events::*;
pub(crate) use spawn::*;

pub struct CodexDriver {
    child: Arc<Mutex<Option<Child>>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    tx: UnboundedSender<Update>,
    /// Shared with the reader: anything sent around it can overtake a held snapshot.
    throttle: Throttle,
    state: Arc<Mutex<State>>,
    resumed: bool,
    rec: Option<Arc<Recorder>>,
}

enum Call {
    Wait(Sender<Value>),
    Turn,
    Steer(String),
    /// A deny note sent into the turn; not one of the user's steers.
    Note,
    Models(String),
    SideThread(String),
    Terminate(String),
    Reconcile(Vec<String>),
}

/// The app-server never names threads; like Codex's TUI, a fresh one gets a title from a side turn.
#[derive(Default, Debug, PartialEq)]
enum Naming {
    #[default]
    Off,
    Wanted,
    Prompt(String),
    Side(String),
}

#[derive(Default)]
pub(crate) struct State {
    next_id: u64,
    calls: HashMap<u64, Call>,
    outbox: Vec<Value>,
    access: AccessMode,
    model: String,
    effort: String,
    cwd: String,
    thread: String,
    naming: Naming,
    turn_id: Option<String>,
    queued: Vec<(String, Vec<Attachment>)>,
    active: bool,
    cancelled: bool,
    turn: u64,
    tools: HashMap<String, ToolCall>,
    live: HashMap<String, LiveOutput>,
    streamed: HashMap<String, usize>,
    agents: HashMap<String, String>,
    terminals: HashMap<String, String>,
    collab: HashMap<String, String>,
    main_collab: HashSet<String>,
    cites: Cites,
    item: String,
    gap: bool,
    part: Option<(String, u64)>,
    totals: Option<[u64; 3]>,
    error: Option<String>,
    asks: HashMap<u64, (Value, CodexAsk)>,
    next_ask: u64,
    stderr: StderrTail,
    dead: Option<String>,
    stale: HashSet<String>,
    next: Option<(String, Vec<Attachment>)>,
    wake_reason: Option<String>,
    wakes: u32,
    attachments: Option<PathBuf>,
    phases: HashMap<String, String>,
    reasoning: HashMap<String, bool>,
    turn_diff: String,
    retry: Option<Notice>,
    plan: bool,
    resolved: String,
    /// Items started this turn; only these can be cut by its Stop.
    turn_rows: HashSet<String>,
}

const LIVE_OUTPUT_CAP: usize = 64 * 1024;

/// A command's streamed output. Finished lines are cleaned once; the open last line stays
/// raw so escapes and `\r` redraws split across deltas still clean right.
#[derive(Default)]
pub(crate) struct LiveOutput {
    done: String,
    partial: String,
}

impl LiveOutput {
    fn push(&mut self, delta: &str) -> String {
        self.partial.push_str(delta);
        if self.partial.len() > LIVE_OUTPUT_CAP {
            let mut cut = self.partial.len() - LIVE_OUTPUT_CAP;
            while !self.partial.is_char_boundary(cut) {
                cut += 1;
            }
            self.partial.drain(..cut);
        }
        if let Some(at) = self.partial.rfind('\n') {
            let lines: String = self.partial.drain(..=at).collect();
            self.done.push_str(&clean(&lines));
            while self.done.len() > LIVE_OUTPUT_CAP {
                match self.done.find('\n') {
                    Some(i) => self.done.drain(..=i),
                    None => self.done.drain(..),
                };
            }
        }
        format!("{}{}", self.done, clean(&self.partial))
    }
}

/// Rewrites Codex's private `\u{e200}cite\u{e202}ref\u{e201}` markers into `[n](url)`
/// links to the turn's search results, holding a partial one in `buf` until its end
/// arrives. Other markers and unknown refs are dropped.
#[derive(Default)]
struct Cites {
    buf: String,
    refs: HashMap<String, String>,
    urls: Vec<String>,
}

impl Cites {
    fn rewrite(&mut self, delta: &str) -> String {
        self.buf.push_str(delta);
        let mut out = String::new();
        while let Some(start) = self.buf.find('\u{e200}') {
            out.push_str(&self.buf[..start]);
            let Some(end) = self.buf[start..].find('\u{e201}') else {
                self.buf.drain(..start);
                return out;
            };
            let marker = self.buf[start + '\u{e200}'.len_utf8()..start + end].to_string();
            self.buf.drain(..start + end + '\u{e201}'.len_utf8());
            let links = self.links(&marker);
            if !links.is_empty() {
                if !out.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
                out.push_str(&links);
            }
        }
        out.push_str(&self.buf);
        self.buf.clear();
        out
    }

    fn links(&mut self, marker: &str) -> String {
        let mut parts = marker.split('\u{e202}');
        if parts.next() != Some("cite") {
            return String::new();
        }
        let mut links: Vec<String> = vec![];
        for url in parts.filter_map(|r| self.refs.get(r)) {
            let n = match self.urls.iter().position(|u| u == url) {
                Some(i) => i + 1,
                None => {
                    self.urls.push(url.clone());
                    self.urls.len()
                }
            };
            let link = format!("[{n}]({})", url.replace('(', "%28").replace(')', "%29"));
            if !links.contains(&link) {
                links.push(link);
            }
        }
        links.join(" ")
    }
}

impl State {
    fn rpc(&mut self, method: &str, params: Value, call: Option<Call>) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        if let Some(call) = call {
            self.calls.insert(id, call);
        }
        json!({"method": method, "id": id, "params": params})
    }
}

impl Driver for CodexDriver {
    fn prompt(&mut self, text: String, images: Vec<Attachment>) {
        let mut s = self.state.guard();
        if let Some(why) = &s.dead {
            let _ = self.tx.unbounded_send(Update::Error(why.clone()));
            let _ = self.tx.unbounded_send(Update::Final(String::new()));
            return;
        }
        let mut refused = vec![];
        let line = if s.active && s.cancelled {
            refused = queue_next(&mut s, text.clone(), images);
            None
        } else {
            Some(start_turn(&mut s, &text, &images))
        };
        drop(s);
        for u in refused {
            let _ = self.tx.unbounded_send(u);
        }
        self.mark(json!({"aether": "prompt", "text": text}));
        if let Some(line) = line {
            self.write(&line);
        }
    }

    fn steer(&mut self, text: String, images: Vec<Attachment>) -> bool {
        let mut s = self.state.guard();
        if !s.active {
            return false;
        }
        let mark = json!({"aether": "steer", "text": text});
        if s.turn_id.is_none() {
            s.queued.push((text, images));
            drop(s);
            self.mark(mark);
            return true;
        }
        let line = steer_turn(&mut s, text, images);
        drop(s);
        self.mark(mark);
        self.write(&line);
        true
    }

    fn reports_steered(&self) -> bool {
        true
    }

    fn set_options(&mut self, model: &str, effort: &str) -> bool {
        let mut s = self.state.guard();
        (s.model, s.effort) = (model.to_string(), effort.to_string());
        true
    }

    fn cancel(&mut self) {
        let mut s = self.state.guard();
        if !on_cancel(&mut s) {
            return;
        }
        let turn = s.turn;
        let line = s.turn_id.is_some().then(|| interrupt(&mut s));
        drop(s);
        self.mark(json!({"aether": "cancel"}));
        if let Some(line) = line {
            self.write(&line);
        }
        let (state, throttle, child) =
            (self.state.clone(), self.throttle.clone(), self.child.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            let mut s = state.guard();
            if s.active && s.turn == turn && s.dead.is_none() {
                let why = s.stderr.fatal("codex did not stop after cancel");
                let out = on_death(&mut s, why);
                drop(s);
                if let Some(child) = child.guard().as_mut() {
                    let _ = child.kill();
                }
                for u in out {
                    throttle.send(u);
                }
            }
        });
    }

    fn answer(&mut self, id: u64, answer: Answer) {
        self.mark(json!({"aether": "answer", "id": id, "answer": answer}));
        let mut s = self.state.guard();
        let line = reply(&mut s, id, &answer);
        let end = line.as_ref().and_then(|_| side_turn_end(&s));
        let rest = std::mem::take(&mut s.outbox);
        drop(s);
        for line in line.into_iter().chain(rest) {
            self.write(&line);
        }
        if let Some(end) = end {
            self.throttle.send(end);
        }
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
        let line = terminate(&mut self.state.guard(), id);
        let stopped = line.is_some();
        if let Some(line) = line {
            self.write(&line);
        }
        stopped
    }
}

impl Drop for CodexDriver {
    fn drop(&mut self) {
        if let Ok(mut s) = self.state.lock()
            && s.dead.is_none()
        {
            for u in settle_background(&mut s) {
                self.throttle.send(u);
            }
            s.dead = Some("codex session closed".into());
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

fn on_cancel(s: &mut State) -> bool {
    if !s.active {
        return false;
    }
    s.cancelled = true;
    s.next = None;
    if let Some(id) = &s.turn_id {
        s.stale.insert(id.clone());
    }
    true
}

fn queue_next(s: &mut State, text: String, images: Vec<Attachment>) -> Vec<Update> {
    if s.next.is_none() {
        s.next = Some((text, images));
        return vec![];
    }
    let warning =
        "Codex is still stopping and already holds a queued message. Send this one again.";
    vec![Update::Warning(warning.into())]
}

/// Ends the side turn an idle ask opened once nothing waits on the user.
fn side_turn_end(s: &State) -> Option<Update> {
    (!s.active && s.asks.is_empty()).then(|| Update::Final(String::new()))
}

fn refuse_queued(s: &mut State) -> Vec<Update> {
    let queued = std::mem::take(&mut s.queued);
    let refuse = |(text, _): (String, Vec<Attachment>)| {
        Update::SteerRefused(without_attached_files(&text).to_string())
    };
    queued.into_iter().map(refuse).collect()
}

fn refuse_next(s: &mut State) -> Vec<Update> {
    if s.next.take().is_none() {
        return vec![];
    }
    vec![Update::Warning("Codex stopped before your message was sent. Send it again.".into())]
}

/// Stops background rows and the live rows under them, children first so they still route
/// to their agent's reply.
fn settle_background(s: &mut State) -> Vec<Update> {
    s.terminals.clear();
    let mut ids: Vec<String> = s
        .tools
        .values()
        .filter(|c| c.status == ToolStatus::Background)
        .map(|c| c.id.clone())
        .collect();
    ids.sort();
    let mut at = 0;
    while at < ids.len() {
        let mut kids: Vec<String> = s
            .tools
            .values()
            .filter(|c| c.status == ToolStatus::Running && c.parent.as_ref() == Some(&ids[at]))
            .map(|c| c.id.clone())
            .collect();
        kids.sort();
        ids.extend(kids);
        at += 1;
    }
    ids.iter()
        .rev()
        .filter_map(|id| s.tools.remove(id))
        .map(|mut call| {
            call.status = ToolStatus::Stopped;
            Update::Tool(call)
        })
        .collect()
}

/// Ends the session: stops background rows, returns held input, then reports the failure.
fn on_death(s: &mut State, why: String) -> Vec<Update> {
    let mut out = settle_background(s);
    out.extend(s.retry.take().map(|n| Update::Notice(Notice { state: NoticeState::Failed, ..n })));
    if s.active {
        s.active = false;
        out.extend(refuse_queued(s));
        out.extend(refuse_next(s));
        out.push(Update::Error(why.clone()));
        out.push(Update::Final(String::new()));
    } else {
        out.push(Update::Warning(why.clone()));
    }
    s.dead = Some(why);
    out
}

fn terminate(s: &mut State, id: &str) -> Option<Value> {
    s.dead.is_none().then_some(())?;
    let process = s.terminals.get(id)?.clone();
    let command = s.tools.get(id).map(|c| c.detail.summary()).unwrap_or_default();
    let p = json!({"threadId": s.thread, "processId": process});
    Some(s.rpc("thread/backgroundTerminals/terminate", p, Some(Call::Terminate(command))))
}

fn interrupt(s: &mut State) -> Value {
    let p = json!({"threadId": s.thread, "turnId": s.turn_id});
    s.rpc("turn/interrupt", p, None)
}

#[cfg(test)]
#[path = "../../tests/unit/codex.rs"]
mod tests;
