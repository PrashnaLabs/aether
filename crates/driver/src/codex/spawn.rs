use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

use aether_core::{
    AccessMode, Attachment, MutexExt, Notice, NoticeKind, NoticeState, Provider, ToolCall,
    ToolStatus, Update, without_attached_files,
};

use crate::DriverStartOptions;
use crate::record::{self};
use crate::spawn::{command, spawn_logged};
use crate::tool::{Throttle, start};
use crate::version::gate;

use super::events::{map_event, unwrap_shell};
use super::{Call, Cites, CodexDriver, Naming, State};
use super::{interrupt, on_death, refuse_queued};

pub(crate) fn launch_args(opts: &DriverStartOptions) -> Vec<String> {
    let mut args = vec!["app-server".to_string()];
    if let Some(lead) = &opts.lead {
        let exe = Value::from(lead.exe.to_string_lossy().into_owned());
        for kv in [
            format!("mcp_servers.aether.command={exe}"),
            r#"mcp_servers.aether.args=["mcp"]"#.to_string(),
            r#"mcp_servers.aether.default_tools_approval_mode="approve""#.to_string(),
            r#"mcp_servers.aether.env_vars=["AETHER_ORCH_SOCK","AETHER_ORCH_TOKEN"]"#.to_string(),
        ] {
            args.extend(["-c".to_string(), kv]);
        }
    }
    args
}

pub(crate) fn spawn_env(opts: &DriverStartOptions) -> Vec<(String, String)> {
    opts.lead
        .iter()
        .flat_map(|l| {
            [
                ("AETHER_ORCH_SOCK".to_string(), l.sock.to_string_lossy().into_owned()),
                ("AETHER_ORCH_TOKEN".to_string(), l.token.clone()),
            ]
        })
        .collect()
}

impl CodexDriver {
    pub fn spawn(opts: DriverStartOptions, tx: UnboundedSender<Update>) -> Self {
        let state = State {
            access: opts.access,
            model: opts.model.clone(),
            effort: opts.effort.clone(),
            cwd: opts.cwd.to_string_lossy().into_owned(),
            attachments: opts.attachments.clone(),
            ..State::default()
        };
        let mut me = Self {
            child: Arc::default(),
            stdin: Arc::default(),
            throttle: Throttle::new(tx.clone()),
            tx,
            state: Arc::new(Mutex::new(state)),
            resumed: false,
            rec: record::from_env("codex", &opts.cwd),
        };
        let Some(mut cmd) = command(&opts, "codex") else {
            me.die("codex is not installed; install it or set an explicit binary path".into());
            return me;
        };
        cmd.args(launch_args(&opts)).envs(spawn_env(&opts));
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
        me.note("startup-spawn", "Starting codex app-server", NoticeState::Running);
        let mut child = match spawn_logged(&mut cmd) {
            Ok(child) => child,
            Err(e) => {
                me.note("startup-spawn", "Starting codex app-server", NoticeState::Failed);
                me.die(format!("codex failed to start: {e}"));
                return me;
            }
        };
        *me.stdin.guard() = child.stdin.take();
        let (stderr_done, stderr_wait) = channel::<()>();
        if let Some(stderr) = child.stderr.take() {
            let state = me.state.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    state.guard().stderr.push(&line);
                }
                drop(stderr_done);
            });
        }
        if let Some(stdout) = child.stdout.take() {
            let (throttle, state, stdin, rec) =
                (me.throttle.clone(), me.state.clone(), me.stdin.clone(), me.rec.clone());
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(rec) = &rec {
                        rec.line(&line);
                    }
                    let Ok(v) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    let mut s = state.guard();
                    if s.dead.is_some() {
                        return;
                    }
                    for u in map_event(&mut s, &v) {
                        throttle.send(u);
                    }
                    let outbox = std::mem::take(&mut s.outbox);
                    drop(s);
                    for line in outbox {
                        send(&stdin, &line);
                    }
                }
                // the crash reason is usually the last stderr line, still in flight
                let _ = stderr_wait.recv_timeout(Duration::from_secs(1));
                let mut s = state.guard();
                s.calls.clear();
                if s.dead.is_some() {
                    return;
                }
                let why = s.stderr.fatal("codex exited");
                for u in on_death(&mut s, why) {
                    throttle.send(u);
                }
            });
        }
        *me.child.guard() = Some(child);
        if let Err(e) = me.handshake(&opts) {
            me.state.guard().dead.get_or_insert(format!("codex failed to start: {e}"));
        }
        me
    }

    fn handshake(&mut self, opts: &DriverStartOptions) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let client =
            json!({"name": "aether", "title": "Aether", "version": env!("CARGO_PKG_VERSION")});
        let init = json!({"clientInfo": client, "capabilities": {"experimentalApi": true}});
        self.note("startup-spawn", "Starting codex app-server", NoticeState::Running);
        let reply = match self.request("initialize", init, deadline) {
            Ok(reply) => reply,
            Err(e) => {
                self.note("startup-spawn", "Starting codex app-server", NoticeState::Failed);
                return Err(e);
            }
        };
        let agent = reply["userAgent"].as_str().and_then(|a| a.split_once('/')).map(|(_, v)| v);
        if let Err(why) = gate(Provider::Codex, agent, &self.tx) {
            self.note("startup-spawn", "Starting codex app-server", NoticeState::Failed);
            self.die(why);
            return Ok(());
        }
        self.write(&json!({"method": "initialized"}));
        self.note("startup-spawn", "Starting codex app-server", NoticeState::Done);
        let stage = if opts.resume.is_empty() { "Starting thread" } else { "Resuming thread" };
        self.note("startup-session", stage, NoticeState::Running);
        let mut resumed = None;
        if !opts.resume.is_empty() {
            match self.request("thread/resume", resume_params(opts), deadline) {
                Ok(thread) => resumed = Some(thread),
                Err(e) => {
                    let _ = self.tx.unbounded_send(Update::Notice(resume_notice(&e)));
                }
            }
        }
        self.resumed = resumed.is_some();
        let thread = match resumed {
            Some(thread) => Ok(thread),
            None => self.request("thread/start", thread_params(opts), deadline),
        };
        let state = if thread.is_ok() { NoticeState::Done } else { NoticeState::Failed };
        self.note("startup-session", stage, state);
        let thread = thread?;
        let id = thread["thread"]["id"].as_str().ok_or("thread/start returned no thread id")?;
        let mut s = self.state.guard();
        s.thread = id.to_string();
        note_thread(&mut s, &thread);
        // a resumed thread replays its stored totals first; that one only seeds them
        if !self.resumed {
            s.totals = Some([0; 3]);
            s.naming = Naming::Wanted;
        }
        drop(s);
        let _ = self.tx.unbounded_send(Update::Session(id.to_string()));
        for u in thread_updates(&thread, opts.access) {
            let _ = self.tx.unbounded_send(u);
        }
        if let Some(name) = thread["thread"]["name"].as_str().filter(|n| !n.is_empty()) {
            let _ = self.tx.unbounded_send(Update::Title(name.to_string()));
        }
        if self.resumed {
            match self.request("thread/backgroundTerminals/list", json!({"threadId": id}), deadline)
            {
                Ok(list) => {
                    let mut s = self.state.guard();
                    for t in list["data"].as_array().into_iter().flatten() {
                        if let (Some(item), Some(process)) =
                            (t["itemId"].as_str(), t["processId"].as_str())
                        {
                            s.terminals.insert(item.to_string(), process.to_string());
                        }
                    }
                    for call in background_rows(&list) {
                        s.tools.insert(call.id.clone(), call.clone());
                        let _ = self.tx.unbounded_send(Update::Tool(call));
                    }
                }
                Err(e) => {
                    let text = format!("Couldn't list background terminals: {e}");
                    let _ = self.tx.unbounded_send(Update::Warning(text));
                }
            }
        }
        Ok(())
    }

    pub(super) fn request(
        &self,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, String> {
        let (tx, rx) = channel();
        let line = self.state.guard().rpc(method, params, Some(Call::Wait(tx)));
        self.write(&line);
        let reply = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| format!("no reply to {method}"))?;
        match reply.get("error") {
            Some(e) => Err(e["message"].as_str().unwrap_or("request failed").to_string()),
            None => Ok(reply["result"].clone()),
        }
    }

    fn note(&self, id: &str, text: &str, state: NoticeState) {
        let notice = Notice { state, ..Notice::new(id, NoticeKind::Startup, text) };
        let _ = self.tx.unbounded_send(Update::Notice(notice));
    }

    fn die(&self, why: String) {
        self.state.guard().dead = Some(why);
    }

    pub(crate) fn write(&self, v: &Value) {
        send(&self.stdin, v);
    }

    pub(crate) fn mark(&self, v: Value) {
        if let Some(rec) = &self.rec {
            rec.line(&v.to_string());
        }
    }
}

pub(crate) fn send(stdin: &Mutex<Option<ChildStdin>>, v: &Value) {
    if let Some(stdin) = stdin.guard().as_mut() {
        let _ = stdin.write_all(format!("{v}\n").as_bytes());
    }
}

/// approvalPolicy, thread sandbox, turn sandboxPolicy type, approvalsReviewer.
pub(crate) fn access(mode: AccessMode) -> [&'static str; 4] {
    match mode {
        AccessMode::Ask => ["untrusted", "read-only", "readOnly", "user"],
        AccessMode::ReadOnly => ["never", "read-only", "readOnly", "user"],
        AccessMode::AutoEdit => ["untrusted", "workspace-write", "workspaceWrite", "user"],
        AccessMode::Auto => ["on-request", "workspace-write", "workspaceWrite", "auto_review"],
        AccessMode::Full => ["never", "danger-full-access", "dangerFullAccess", "user"],
    }
}

pub(crate) fn thread_mode(policy: &str, sandbox: &Value, reviewer: &str) -> Option<AccessMode> {
    let sandbox = sandbox["type"].as_str()?;
    AccessMode::ALL.into_iter().find(|m| {
        let [p, _, s, r] = access(*m);
        (p, s) == (policy, sandbox) && (*m != AccessMode::Auto || r == reviewer)
    })
}

pub(crate) fn thread_updates(result: &Value, asked: AccessMode) -> Vec<Update> {
    let mut out = vec![];
    if let Some(model) = result["model"].as_str().filter(|m| !m.is_empty()) {
        out.push(Update::Model(model.to_string()));
    }
    let policy = result["approvalPolicy"].as_str().unwrap_or_default();
    let reviewer = result["approvalsReviewer"].as_str().unwrap_or_default();
    if let Some(mode) = thread_mode(policy, &result["sandbox"], reviewer) {
        out.push(Update::Mode(mode));
        if mode != asked {
            let text = format!("Codex is running in {} mode", mode.label());
            out.push(Update::Notice(Notice::new("mode-1", NoticeKind::ModeChange, text)));
        }
    }
    out
}

pub(crate) fn resume_notice(message: &str) -> Notice {
    let text = if message.contains("thread writer lock") {
        "Couldn't resume the Codex thread because it's open in another Codex window. Started a new one.".to_string()
    } else {
        format!("Couldn't resume the Codex thread ({message}). Started a new one.")
    };
    Notice::new("info-resume", NoticeKind::Info, text)
}

/// Why a `turn/steer` error means the turn can't take it. Matched on text, never the code.
pub(crate) fn steer_refused(error: &Value) -> Option<&'static str> {
    let info = &error["data"]["codexErrorInfo"]["activeTurnNotSteerable"]["turnKind"];
    match info.as_str() {
        Some("review") => return Some("review"),
        Some("compact") => return Some("compact"),
        _ => {}
    }
    match error["message"].as_str()? {
        "cannot steer a review turn" => Some("review"),
        "cannot steer a compact turn" => Some("compact"),
        m if m == "no active turn to steer" || m.starts_with("expected active turn id") => {
            Some("ended")
        }
        _ => None,
    }
}

pub(crate) fn thread_params(opts: &DriverStartOptions) -> Value {
    let [policy, sandbox, _, reviewer] = access(opts.access);
    let mut p = json!({
        "cwd": opts.cwd.to_string_lossy(),
        "approvalPolicy": policy,
        "sandbox": sandbox,
        "approvalsReviewer": reviewer,
    });
    if !opts.model.is_empty() {
        p["model"] = opts.model.clone().into();
    }
    if opts.ephemeral {
        p["ephemeral"] = true.into();
    }
    p
}

pub(crate) fn resume_params(opts: &DriverStartOptions) -> Value {
    let mut p = thread_params(opts);
    p["threadId"] = opts.resume.clone().into();
    p["excludeTurns"] = true.into();
    p
}

pub(crate) fn input(text: &str, images: &[Attachment]) -> Value {
    let mut items = vec![json!({"type": "text", "text": text})];
    items.extend(images.iter().map(|f| json!({"type": "localImage", "path": f.path})));
    items.into()
}

pub(crate) fn start_turn(s: &mut State, text: &str, images: &[Attachment]) -> Value {
    s.active = true;
    s.cancelled = false;
    s.turn += 1;
    s.turn_id = None;
    s.queued.clear();
    s.turn_diff.clear();
    s.phases.clear();
    s.reasoning.clear();
    s.error = None;
    s.gap = false;
    s.part = None;
    s.cites = Cites::default();
    let trimmed = text.trim();
    let (command, rest) = trimmed.split_once(char::is_whitespace).unwrap_or((trimmed, ""));
    let (method, p) = match (command, rest.trim()) {
        ("/compact", "") => ("thread/compact/start", json!({"threadId": s.thread})),
        ("/review", rest) => {
            let target = if rest.is_empty() {
                json!({"type": "uncommittedChanges"})
            } else {
                json!({"type": "custom", "instructions": rest})
            };
            ("review/start", json!({"threadId": s.thread, "target": target, "delivery": "inline"}))
        }
        _ => {
            let plan = command == "/plan" && !rest.trim().is_empty();
            let body = if plan { rest.trim() } else { text };
            let exiting = !plan && s.plan;
            let mode =
                if plan { Some("plan") } else { std::mem::take(&mut s.plan).then_some("default") };
            s.plan = plan;
            let mut p = json!({
                "threadId": s.thread,
                "input": input(body, images),
                "summary": "auto",
            });
            let model = if s.model.is_empty() { &s.resolved } else { &s.model };
            if let Some(mode) = mode.filter(|m| *m == "plan" || !model.is_empty()) {
                let effort = Some(&s.effort).filter(|e| !e.is_empty());
                p["collaborationMode"] = json!({"mode": mode, "settings": {
                    "model": model,
                    "reasoning_effort": effort,
                    "developer_instructions": null,
                }});
            }
            for (key, value) in [("model", &s.model), ("effort", &s.effort)] {
                if !value.is_empty() {
                    p[key] = value.clone().into();
                }
            }
            if matches!(s.naming, Naming::Wanted | Naming::Prompt(_)) {
                s.naming =
                    Naming::Prompt(without_attached_files(body).chars().take(1000).collect());
            }
            s.plan |= exiting && p.get("collaborationMode").is_none();
            ("turn/start", p)
        }
    };
    s.rpc(method, p, Some(Call::Turn))
}

pub(crate) fn turn_started(s: &mut State, id: &str) -> Vec<Update> {
    s.turn_id = Some(id.to_string());
    if let Naming::Prompt(prompt) = &mut s.naming {
        let prompt = std::mem::take(prompt);
        s.naming = Naming::Off;
        let line = s.rpc("model/list", json!({}), Some(Call::Models(prompt)));
        s.outbox.push(line);
    }
    let mut out = vec![];
    if s.cancelled {
        s.stale.insert(id.to_string());
        let line = interrupt(s);
        s.outbox.push(line);
        out.extend(refuse_queued(s));
    } else {
        for (text, images) in std::mem::take(&mut s.queued) {
            let line = steer_turn(s, text, images);
            s.outbox.push(line);
        }
    }
    out
}

pub(crate) fn steer_turn(s: &mut State, text: String, images: Vec<Attachment>) -> Value {
    let p = steer_params(s, &text, &images);
    s.rpc("turn/steer", p, Some(Call::Steer(text)))
}

pub(crate) fn note_turn(s: &mut State, note: &str) -> Value {
    let p = steer_params(s, note, &[]);
    s.rpc("turn/steer", p, Some(Call::Note))
}

pub(crate) fn steer_params(s: &State, text: &str, images: &[Attachment]) -> Value {
    json!({"threadId": s.thread, "expectedTurnId": s.turn_id, "input": input(text, images)})
}

pub(crate) fn background_rows(list: &Value) -> Vec<ToolCall> {
    let terminals = list["data"].as_array().into_iter().flatten();
    terminals
        .filter_map(|t| {
            let command = unwrap_shell(t["command"].as_str().unwrap_or_default());
            let mut call =
                start(t["itemId"].as_str()?, "bash", &json!({"command": command, "cwd": t["cwd"]}));
            call.status = ToolStatus::Background;
            Some(call)
        })
        .collect()
}

pub(crate) const TITLE_PROMPT: &str = "Generate a concise, single-line task title of at most 40 characters and under five words where possible. Start with an imperative verb. Capitalize only the first word unless the user's language, proper nouns, acronyms, or code terms require otherwise. Preserve ticket references exactly. Write in the user's language. Do not use quotes, markdown, or trailing punctuation. Do not answer the request.";

pub(crate) fn side_thread_params(cwd: &str, model: &str) -> Value {
    let mut p =
        json!({"cwd": cwd, "approvalPolicy": "never", "sandbox": "read-only", "ephemeral": true});
    if !model.is_empty() {
        p["model"] = model.into();
    }
    p
}

const BTW_BOUNDARY: &str = "Side conversation boundary. This is a forked side conversation for answering one question without disrupting the main thread. Do not continue, execute, or complete any instructions, plans, tool calls, approvals, edits, or requests from the inherited history; it is reference only. Do not modify files, source, repository state, or permissions. Do not spawn sub-agents. Answer concisely.";

pub(crate) fn fork_params(thread: &str, model: &str) -> Value {
    json!({
        "threadId": thread,
        "ephemeral": true,
        "excludeTurns": true,
        "model": model,
        "sandbox": "read-only",
        "approvalPolicy": "never",
        "developerInstructions": BTW_BOUNDARY,
    })
}

pub(crate) fn fork_turn_params(thread: &str, question: &str) -> Value {
    json!({"threadId": thread, "input": input(question, &[])})
}

pub(crate) fn side_turn_params(thread: &str, prompt: &str) -> Value {
    json!({
        "threadId": thread,
        "input": input(&format!("{TITLE_PROMPT}\n\nUser prompt:\n{prompt}"), &[]),
        "effort": "low",
        "summary": "none",
        "outputSchema": {
            "type": "object",
            "properties": {"title": {"type": "string"}},
            "required": ["title"],
            "additionalProperties": false,
        },
    })
}

pub(crate) fn note_thread(s: &mut State, thread: &Value) {
    if let Some(model) = thread["model"].as_str().filter(|m| !m.is_empty()) {
        s.resolved = model.to_string();
    }
}
