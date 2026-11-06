use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use reqwest::Method;
use serde_json::{Value, json};

use aether_core::{
    AccessMode, Answer, Attachment, MutexExt, Notice, ToolCall, Update, without_attached_files,
};

use super::record::{self, Recorder};
use super::{Driver, DriverStartOptions};

mod events;
mod service;
mod session;

pub use events::is_placeholder_title;
pub use service::{Api, kill_lead_servers, service_api, service_reachable};

use events::{decision, deny, form_reply, register_steer, settle};
use session::{
    cancel_form, model_ref, model_switch, prompt_body, reply, reply_form, start, submit,
};

pub struct OpencodeDriver {
    conn: Option<Conn>,
    tx: UnboundedSender<Update>,
    state: Arc<Mutex<State>>,
    rec: Option<Arc<Recorder>>,
}

struct Conn {
    api: Arc<Api>,
    session: String,
    resumed: bool,
    _stop: oneshot::Sender<()>,
    server: Option<service::LeadServer>,
}

impl Conn {
    fn path(&self, tail: &str) -> String {
        format!("/api/session/{}{tail}", self.session)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum OpencodeAsk {
    Permission { request: String, session: String },
    Form { id: String, fields: Vec<Value>, session: String },
}

impl OpencodeAsk {
    fn session(&self) -> &str {
        match self {
            Self::Permission { session, .. } | Self::Form { session, .. } => session,
        }
    }

    /// The request or form id the service knows it by.
    fn key(&self) -> &str {
        match self {
            Self::Permission { request, .. } => request,
            Self::Form { id, .. } => id,
        }
    }
}

/// A subagent's own session: the call that owns it and the agent's name.
#[derive(Clone, Debug)]
struct Child {
    task: String,
    agent: String,
}

#[derive(Default)]
struct State {
    access: AccessMode,
    tools: HashMap<String, (String, ToolCall, Instant)>,
    asks: HashMap<u64, OpencodeAsk>,
    next_ask: u64,
    /// (request, session) pairs to approve once the lock is released.
    auto: Vec<(String, String)>,
    /// The chat's own session id; asks from other sessions are subagents'.
    root: String,
    active: bool,
    cancelled: bool,
    /// Call each asked permission or form is for, and calls the user declined.
    calls: HashMap<String, String>,
    denied: HashSet<String>,
    turn: u64,
    /// Inbox id of the turn's prompt.
    prompt: String,
    /// Unix ms the turn started at.
    started: u64,
    dead: Option<String>,
    step_error: Option<String>,
    /// The last `{providerID, id, variant}` the session ran or switched to.
    model: Option<Value>,
    /// Delta text seen per (assistantMessageID, ordinal), to repair missed ones.
    streamed: HashMap<(String, u64), String>,
    /// Steer text by inbox id, until it is delivered or cancelled.
    steers: HashMap<String, String>,
    /// Inbox acks with no steer registered yet (delivered?), for acks that beat the POST reply.
    inbox_seen: HashMap<String, bool>,
    /// Files with an Edit row this turn.
    edited: HashSet<String>,
    /// Files steps reported as changed this turn.
    changed: Vec<String>,
    retry: Option<Notice>,
    compactions: u32,
    /// Counter for one-off notice ids.
    notices: u32,
    /// A question form was just dismissed; its interrupt reads "shutdown" like a service stop.
    dismissed: bool,
    /// Subagent sessions by id; their tools nest under the owning call.
    children: HashMap<String, Child>,
    /// Task rows that outlive their turn, by call id.
    background: HashMap<String, ToolCall>,
    wakes: u32,
    /// The service stayed unreachable past the deadline while idle; the next prompt respawns.
    lost: bool,
    /// Background shell ids to their call's row id.
    shells: HashMap<String, String>,
    /// What finished in the background, for the next wake notice.
    wake_reason: Option<String>,
    /// The card each ask showed, so a background child's ask can outlive its turn.
    cards: HashMap<u64, aether_core::Ask>,
    /// Background row ids of the reattached reply, until the first relink settles them.
    restored: Vec<String>,
}

impl State {
    fn usable(&self) -> bool {
        self.dead.is_none() && !self.lost
    }
}

impl OpencodeDriver {
    pub fn spawn(opts: DriverStartOptions, tx: UnboundedSender<Update>) -> Self {
        let state = Arc::new(Mutex::new(State {
            access: opts.access,
            restored: opts.background.clone(),
            ..State::default()
        }));
        let rec = record::from_env("opencode", &opts.cwd);
        let conn = start(&opts, &state, &tx, &rec).map_err(|e| state.guard().dead = Some(e)).ok();
        Self { conn, tx, state, rec }
    }

    fn mark(&self, v: Value) {
        if let Some(rec) = &self.rec {
            rec.line(&v.to_string());
        }
    }
}

fn on_prompt(s: &mut State) {
    s.active = true;
    s.cancelled = false;
    s.turn += 1;
    s.started = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    s.wake_reason = None;
}

fn on_steer(s: &State) -> bool {
    s.active
}

/// Ends the side turn an idle ask opened once nothing waits on the user.
fn side_turn_end(s: &State) -> Option<Update> {
    (!s.active && s.asks.is_empty()).then(|| Update::Final(String::new()))
}

fn side_request(session: &str, question: &str) -> (String, Value) {
    let prompt = format!(
        "Answer this side question briefly from the conversation so far. Do not use tools.\n\n{question}"
    );
    (format!("/api/session/{session}/generate"), json!({"prompt": prompt}))
}

fn side_reply(res: Result<Value, String>) -> Update {
    match res {
        Ok(v) => match v["data"]["text"].as_str() {
            Some(t) => Update::Final(t.to_string()),
            None => Update::Error("opencode side question returned no text".into()),
        },
        Err(e) => Update::Error(e),
    }
}

fn on_cancel(s: &mut State) -> bool {
    if !s.active {
        return false;
    }
    s.cancelled = true;
    true
}

impl Driver for OpencodeDriver {
    fn prompt(&mut self, text: String, images: Vec<Attachment>) {
        let Some(c) = &self.conn else {
            let why = self.state.guard().dead.clone().unwrap_or_default();
            let _ = self.tx.unbounded_send(Update::Error(why));
            let _ = self.tx.unbounded_send(Update::Final(String::new()));
            return;
        };
        let mut s = self.state.guard();
        on_prompt(&mut s);
        let turn = s.turn;
        drop(s);
        self.mark(json!({"aether": "prompt", "text": text}));
        let sent = submit(c, &text, &images);
        let mut s = self.state.guard();
        match sent {
            Ok(id) => s.prompt = id,
            Err(e) if s.active && s.turn == turn => {
                let _ = self.tx.unbounded_send(Update::Error(e));
                for u in settle(&mut s) {
                    let _ = self.tx.unbounded_send(u);
                }
            }
            Err(_) => {}
        }
    }

    fn steer(&mut self, text: String, images: Vec<Attachment>) -> bool {
        let s = self.state.guard();
        let Some(c) = self.conn.as_ref().filter(|_| on_steer(&s)) else {
            return false;
        };
        drop(s);
        self.mark(json!({"aether": "steer", "text": text}));
        let steered = c
            .api
            .call(
                Method::POST,
                &c.path("/prompt"),
                Some(prompt_body(&text, &images, Some("steer"))),
            )
            .and_then(|v| {
                let id = v["data"]["id"].as_str().unwrap_or_default();
                let own = without_attached_files(&text).to_string();
                for u in register_steer(&mut self.state.guard(), id, own) {
                    let _ = self.tx.unbounded_send(u);
                }
                match v["data"]["delivery"].as_str() {
                    Some("steer") => Ok(()),
                    _ => {
                        let inbox = c.path(&format!("/inbox/{id}"));
                        c.api
                            .call(Method::PATCH, &inbox, Some(json!({"delivery": "steer"})))
                            .map(drop)
                            .inspect_err(|_| drop(self.state.guard().steers.remove(id)))
                    }
                }
            });
        if let Err(e) = steered {
            let _ =
                self.tx.unbounded_send(Update::SteerRefused(without_attached_files(&text).into()));
            let _ = self.tx.unbounded_send(Update::Warning(e));
        }
        true
    }

    fn set_options(&mut self, model: &str, effort: &str) -> bool {
        let Some(c) = &self.conn else { return false };
        let default = if model_ref(model, "").is_none() {
            match c.api.call(Method::GET, "/api/model/default", None) {
                Ok(v) => v["data"].clone(),
                Err(e) => {
                    let _ = self.tx.unbounded_send(Update::Warning(e));
                    return true;
                }
            }
        } else {
            Value::Null
        };
        let current = self.state.guard().model.clone();
        if let Some(body) = model_switch(model, effort, current.as_ref(), &default) {
            match c.api.call(Method::POST, &c.path("/model"), Some(body.clone())) {
                Ok(_) => self.state.guard().model = Some(body["model"].clone()),
                Err(e) => {
                    let _ = self.tx.unbounded_send(Update::Warning(e));
                }
            }
        }
        true
    }

    fn side_question(&mut self, question: &str, reply: UnboundedSender<Update>) -> bool {
        let Some(c) = self.conn.as_ref().filter(|c| !c.session.is_empty()) else {
            return false;
        };
        if !self.state.guard().usable() {
            return false;
        }
        let api = c.api.clone();
        let (path, body) = side_request(&c.session, question);
        std::thread::spawn(move || {
            let _ = reply.unbounded_send(side_reply(api.call(Method::POST, &path, Some(body))));
        });
        true
    }

    fn cancel(&mut self) {
        let mut s = self.state.guard();
        let Some(c) = self.conn.as_ref().filter(|_| on_cancel(&mut s)) else { return };
        let turn = s.turn;
        drop(s);
        self.mark(json!({"aether": "cancel"}));
        let (state, tx) = (self.state.clone(), self.tx.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            let mut s = state.guard();
            if s.active && s.turn == turn && s.dead.is_none() {
                for u in settle(&mut s) {
                    let _ = tx.unbounded_send(u);
                }
            }
        });
        if let Err(e) = c.api.call(Method::POST, &c.path("/interrupt"), None) {
            let _ = self.tx.unbounded_send(Update::Warning(e));
        }
    }

    fn answer(&mut self, id: u64, answer: Answer) {
        let Some(c) = &self.conn else { return };
        let Some(ask) = self.state.guard().asks.remove(&id) else { return };
        self.mark(json!({"aether": "answer", "id": id, "answer": answer}));
        let sent = match ask {
            OpencodeAsk::Permission { request, session } => {
                let (decision, note) = decision(&answer);
                if decision == "reject" {
                    deny(&mut self.state.guard(), &request);
                }
                reply(&c.api, &session, &request, decision, note.as_deref())
            }
            OpencodeAsk::Form { id, fields, session } if matches!(answer, Answer::Form(_)) => {
                reply_form(&c.api, &session, &id, &form_reply(&fields, &answer))
            }
            OpencodeAsk::Form { id, session, .. } => {
                let mut s = self.state.guard();
                s.dismissed |= session == s.root;
                deny(&mut s, &id);
                drop(s);
                cancel_form(&c.api, &session, &id)
            }
        };
        if let Err(e) = sent {
            let _ = self.tx.unbounded_send(Update::Warning(e));
        }
        if let Some(end) = side_turn_end(&self.state.guard()) {
            let _ = self.tx.unbounded_send(end);
        }
    }

    fn reports_steered(&self) -> bool {
        true
    }

    fn resumed(&self) -> bool {
        self.conn.as_ref().is_some_and(|c| c.resumed)
    }

    fn alive(&self) -> bool {
        self.conn.is_some() && self.state.guard().usable()
    }
}

impl Drop for OpencodeDriver {
    fn drop(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            s.dead.get_or_insert_with(|| "opencode session closed".into());
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/opencode/mod.rs"]
mod tests;
