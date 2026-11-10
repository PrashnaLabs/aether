use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

use aether_core::{
    AccessMode, Answer, Ask, Attachment, Choice, Field, FieldKind, Form, MutexExt, Notice,
    NoticeKind, NoticeState, Provider, ToolCall, ToolDetail, ToolStatus, Update, home_dir,
    without_attached_files,
};

use super::lines::{MAX_LINE, capped_lines, oversized};
use super::record::{self, Recorder};
use super::spawn::{
    StderrTail, command as spawn_command, log_unknown, spawn_logged, visible_stderr,
};
use super::tool::{
    Throttle, append_output, apply_patch_text, clean, finish, halted, now_ms, result_text,
    save_images, set_output, start,
};
use super::version::gate_binary;
use super::{Driver, DriverStartOptions};

pub struct PiDriver {
    child: Arc<Mutex<Option<Child>>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    tx: UnboundedSender<Update>,
    state: Arc<Mutex<State>>,
    resumed: bool,
    next_id: u64,
    effort: String,
    rec: Option<Arc<Recorder>>,
    opts: DriverStartOptions,
    approve: bool,
    respawn: bool,
    session: String,
}

#[derive(Default)]
struct State {
    pending: HashMap<String, Sender<Value>>,
    prompt: String,
    tools: HashMap<String, ToolCall>,
    asks: HashMap<u64, (String, bool)>,
    next_ask: u64,
    active: bool,
    running: bool,
    grace: bool,
    cancelled: bool,
    turn: u64,
    window: Option<u64>,
    saw_text: bool,
    error: Option<String>,
    /// The compaction row already shows why `/compact` failed.
    compact_failed: bool,
    dead: Option<String>,
    steers: VecDeque<String>,
    /// Leading `steers` already marked Steered by a `queue_update`.
    taken: usize,
    /// Steer texts a `queue_update` has listed. One leaving the list was taken.
    seen: HashSet<String>,
    clearing: String,
    wakes: u32,
    stderr: StderrTail,
    attachments: Option<PathBuf>,
    /// Content index to (tool call id, streamed argument JSON).
    args: HashMap<u64, (String, String)>,
    names: HashMap<String, String>,
    retry: Option<Notice>,
    compactions: u32,
    served: String,
    effort: String,
    bashes: u32,
    customs: u32,
    notifies: u32,
    /// The pending prompt is an Rpc command, settled by its own response.
    direct: bool,
    /// Extra update a command's reply owes, by request id.
    after: HashMap<String, Update>,
    /// Steer request ids to their text, until Pi acks them.
    steer_acks: HashMap<String, String>,
    /// Lines for Pi, written after the stdout line that queued them.
    outbox: Vec<Value>,
    /// The model Aether set or a `/model` switched to, as `provider/id`.
    model: String,
}

impl PiDriver {
    pub fn spawn(opts: DriverStartOptions, tx: UnboundedSender<Update>) -> Self {
        Self::launch(opts, tx, false)
    }

    fn launch(opts: DriverStartOptions, tx: UnboundedSender<Update>, approve: bool) -> Self {
        let mut me = Self {
            child: Arc::default(),
            stdin: Arc::default(),
            tx,
            state: Arc::default(),
            resumed: false,
            next_id: 0,
            effort: opts.effort.clone(),
            rec: record::from_env("pi", &opts.cwd),
            opts: opts.clone(),
            approve,
            respawn: false,
            session: String::new(),
        };
        {
            let mut s = me.state.guard();
            s.attachments = opts.attachments.clone();
            s.model = opts.model.clone();
        }
        me.startup("startup-spawn", "Starting pi", NoticeState::Running);
        if !opts.resume.is_empty() {
            me.startup("startup-session", "Loading session", NoticeState::Running);
        }
        if let Err(why) = gate_binary(Provider::Pi, "pi", &opts, &me.tx) {
            me.die(why);
            return me;
        }
        let Some(mut cmd) = spawn_command(&opts, "pi") else {
            me.die("pi is not installed; install it or set an explicit binary path".into());
            return me;
        };
        cmd.args(launch_args(&opts, approve)).envs(spawn_env(&opts));
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = match spawn_logged(&mut cmd) {
            Ok(child) => child,
            Err(e) => {
                me.die(format!("pi failed to start: {e}"));
                return me;
            }
        };
        *me.stdin.guard() = child.stdin.take();
        if let Some(stdout) = child.stdout.take() {
            let (tx, state, rec) = (me.tx.clone(), me.state.clone(), me.rec.clone());
            let stdin = me.stdin.clone();
            std::thread::spawn(move || {
                let throttle = Throttle::new(tx.clone());
                for line in capped_lines(BufReader::new(stdout), MAX_LINE) {
                    let Some(line) = line else {
                        throttle.send(oversized("pi"));
                        continue;
                    };
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
                    let armed = s.grace;
                    for u in map_event(&mut s, &v) {
                        throttle.send(u);
                    }
                    let outbox = std::mem::take(&mut s.outbox);
                    if v["type"] == "extension_ui_request"
                        && s.asks.contains_key(&s.next_ask)
                        && let Some(ms) = v["timeout"].as_u64()
                    {
                        let (state, tx, local) = (state.clone(), tx.clone(), s.next_ask);
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(ms));
                            let update = expire_dialog(&mut state.guard(), local);
                            if let Some(u) = update {
                                let _ = tx.unbounded_send(u);
                            }
                        });
                    }
                    if s.grace && !armed {
                        let (state, tx, turn) = (state.clone(), tx.clone(), s.turn);
                        std::thread::spawn(move || {
                            std::thread::sleep(HANDLED_GRACE);
                            let mut s = state.guard();
                            if s.turn == turn && std::mem::take(&mut s.grace) && s.dead.is_none() {
                                for u in settle(&mut s) {
                                    let _ = tx.unbounded_send(u);
                                }
                            }
                        });
                    }
                    drop(s);
                    for line in outbox {
                        send(&stdin, &line);
                    }
                }
                let mut s = state.guard();
                s.pending.clear();
                if s.dead.is_some() {
                    return;
                }
                let why = s.stderr.fatal("pi exited");
                for u in on_exit(&mut s, why) {
                    throttle.send(u);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let (tx, state) = (me.tx.clone(), me.state.clone());
            std::thread::spawn(move || {
                for line in capped_lines(BufReader::new(stderr), MAX_LINE).flatten() {
                    state.guard().stderr.push(&line);
                    if let Some(u) = stderr_update(&line)
                        && tx.unbounded_send(u).is_err()
                    {
                        return;
                    }
                }
            });
        }
        *me.child.guard() = Some(child);
        if let Err(e) = me.handshake(&opts) {
            let why = me.state.guard().stderr.fatal(&format!("pi didn't start: {e}"));
            me.die(why);
        }
        me
    }

    fn startup(&self, id: &str, text: &str, state: NoticeState) {
        let notice = Notice { state, ..Notice::new(id, NoticeKind::Startup, text) };
        let _ = self.tx.unbounded_send(Update::Notice(notice));
    }

    fn handshake(&mut self, opts: &DriverStartOptions) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        self.request(json!({"type": "get_state"}), deadline)?;
        self.resumed = Path::new(&opts.resume).is_file();
        self.startup("startup-spawn", "Starting pi", NoticeState::Done);
        if !opts.resume.is_empty() {
            self.startup("startup-session", "Loading session", NoticeState::Done);
        }
        let mut missing = None;
        if !opts.model.is_empty() {
            missing = self.request(set_model(&opts.model), deadline).err();
        }
        if !opts.effort.is_empty() {
            // set_model re-picks the level from Pi's per-model settings
            self.state.guard().effort = opts.effort.clone();
            self.request(json!({"type": "set_thinking_level", "level": opts.effort}), deadline)?;
        }
        let state = self.request(json!({"type": "get_state"}), deadline)?;
        self.state.guard().window = state["model"]["contextWindow"].as_u64().filter(|w| *w > 0);
        if let Some(e) = missing {
            let _ =
                self.tx.unbounded_send(Update::Warning(format!("{e}; using Pi's current model")));
        }
        if let Some(file) = state["sessionFile"].as_str() {
            self.session = file.to_string();
            let _ = self.tx.unbounded_send(Update::Session(file.to_string()));
        }
        if let Some(name) = state["sessionName"].as_str().filter(|n| !n.is_empty()) {
            let _ = self.tx.unbounded_send(Update::Title(name.to_string()));
        }
        if opts.access == AccessMode::Full {
            let _ = self.tx.unbounded_send(Update::Mode(AccessMode::Full));
        } else {
            let text = "Read only is a tool allowlist. Extension commands can still change files.";
            let _ = self.tx.unbounded_send(Update::Mode(AccessMode::ReadOnly));
            let _ = self.tx.unbounded_send(Update::Notice(Notice::new(
                "info-readonly",
                NoticeKind::Info,
                text,
            )));
        }
        let agent_dir = std::env::var_os("PI_CODING_AGENT_DIR")
            .map_or_else(|| home_dir().join(".pi/agent"), Into::into);
        let read = |name: &str| {
            let text = std::fs::read_to_string(agent_dir.join(name)).unwrap_or_default();
            serde_json::from_str::<Value>(&text).unwrap_or_default()
        };
        let default_trust =
            read("settings.json")["defaultProjectTrust"].as_str().unwrap_or("ask").to_string();
        if !self.approve && untrusted_project(&opts.cwd, &read("trust.json"), &default_trust) {
            let text = "Pi skipped this project's extensions, skills and settings because the project isn't trusted.";
            let notice = Notice {
                url: Some("aether:trust-project".into()),
                ..Notice::new("info-trust", NoticeKind::Info, text)
            };
            let _ = self.tx.unbounded_send(Update::Notice(notice));
        }
        Ok(())
    }

    fn request(&mut self, mut cmd: Value, deadline: Instant) -> Result<Value, String> {
        let id = self.next_id();
        let kind = cmd["type"].as_str().unwrap_or_default().to_string();
        cmd["id"] = id.clone().into();
        let (tx, rx) = channel();
        self.state.guard().pending.insert(id, tx);
        self.write(&cmd);
        let r = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| format!("no reply to {kind}"))?;
        if r["success"] == true {
            Ok(r["data"].clone())
        } else {
            Err(r["error"].as_str().unwrap_or("command failed").to_string())
        }
    }

    fn next_id(&mut self) -> String {
        self.next_id += 1;
        format!("aether-{}", self.next_id)
    }

    fn die(&self, why: String) {
        self.state.guard().dead = Some(why);
    }

    fn write(&self, v: &Value) {
        send(&self.stdin, v);
    }

    fn mark(&self, v: Value) {
        if let Some(rec) = &self.rec {
            rec.line(&v.to_string());
        }
    }
}

fn on_prompt(s: &mut State, id: &str) {
    s.active = true;
    s.grace = false;
    s.cancelled = false;
    s.turn += 1;
    s.prompt = id.into();
    s.saw_text = false;
    s.error = None;
    clear_steers(s);
}

/// Pi is gone: refuses the steers it never took, then ends the turn.
fn on_exit(s: &mut State, why: String) -> Vec<Update> {
    let mut out = vec![];
    if s.active {
        s.active = false;
        out.extend(refuse_steers(s));
        out.push(Update::Error(why.clone()));
        out.push(Update::Final(String::new()));
    }
    s.dead = Some(why);
    out
}

fn string_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn track_command(s: &mut State, id: &str, line: &Value) {
    s.direct = line["type"] != "prompt";
    match line["type"].as_str() {
        Some("set_thinking_level") => s.effort = line["level"].as_str().unwrap_or_default().into(),
        Some("set_session_name") => {
            let name = line["name"].as_str().unwrap_or_default();
            s.after.insert(id.into(), Update::Title(name.into()));
        }
        _ => {}
    }
}

fn clear_steers(s: &mut State) {
    s.steers.clear();
    s.taken = 0;
    s.seen.clear();
    s.steer_acks.clear();
}

/// Steers no `queue_update` reported taken; they go back to the user's queue.
fn refuse_steers(s: &mut State) -> Vec<Update> {
    let out = s.steers.iter().skip(s.taken).cloned().map(Update::SteerRefused).collect();
    clear_steers(s);
    out
}

fn on_steer(s: &mut State, text: &str) -> bool {
    if !s.active {
        return false;
    }
    s.steers.push_back(without_attached_files(text).to_string());
    s.grace = false;
    true
}

fn on_cancel(s: &mut State) -> bool {
    if !s.active {
        return false;
    }
    s.cancelled = true;
    true
}

impl Driver for PiDriver {
    fn prompt(&mut self, text: String, images: Vec<Attachment>) {
        if self.respawn && !self.state.guard().active {
            self.respawn();
        }
        let id = self.next_id();
        let mut s = self.state.guard();
        if let Some(why) = &s.dead {
            let _ = self.tx.unbounded_send(Update::Error(why.clone()));
            let _ = self.tx.unbounded_send(Update::Final(String::new()));
            return;
        }
        if let Some(PiCommand::TuiOnly(cmd)) = command(&text, &id) {
            drop(s);
            let why = format!("`/{cmd}` only works in Pi's terminal");
            let _ = self.tx.unbounded_send(Update::Warning(why));
            let _ = self.tx.unbounded_send(Update::Final(String::new()));
            return;
        }
        on_prompt(&mut s, &id);
        let line = prompt_cmd(&id, &text, &images);
        track_command(&mut s, &id, &line);
        drop(s);
        self.mark(json!({"aether": "prompt", "text": text}));
        self.write(&line);
    }

    fn steer(&mut self, text: String, images: Vec<Attachment>) -> bool {
        let id = self.next_id();
        let mut s = self.state.guard();
        if !on_steer(&mut s, &text) {
            return false;
        }
        s.steer_acks.insert(id.clone(), without_attached_files(&text).to_string());
        drop(s);
        self.mark(json!({"aether": "steer", "text": text}));
        // not the steer command: that queues even when idle, where nothing drains it
        self.write(&message_cmd(&id, &text, "steer", &images));
        true
    }

    fn set_options(&mut self, model: &str, effort: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        if !model.is_empty() && model != self.state.guard().model {
            match self.request(set_model(model), deadline) {
                Ok(data) => {
                    let mut s = self.state.guard();
                    s.model = model.to_string();
                    if let Some(window) = data["contextWindow"].as_u64() {
                        s.window = Some(window);
                    }
                }
                Err(e) => {
                    let _ = self
                        .tx
                        .unbounded_send(Update::Warning(format!("{e}; using Pi's current model")));
                }
            }
        }
        if !effort.is_empty() && effort != self.effort {
            let level = json!({"type": "set_thinking_level", "level": effort});
            self.state.guard().effort = effort.to_string();
            if self.request(level, deadline).is_ok() {
                self.effort = effort.to_string();
            }
        }
        true
    }

    fn cancel(&mut self) {
        let clear_id = self.next_id();
        let mut s = self.state.guard();
        if !on_cancel(&mut s) {
            let (lines, updates) = close_dialogs(&mut s);
            drop(s);
            self.mark(json!({"aether": "cancel"}));
            for u in updates {
                let _ = self.tx.unbounded_send(u);
            }
            for line in &lines {
                self.write(line);
            }
            return;
        }
        let turn = s.turn;
        let (lines, updates) = cancel_lines(&mut s, &clear_id);
        drop(s);
        self.mark(json!({"aether": "cancel"}));
        for u in updates {
            let _ = self.tx.unbounded_send(u);
        }
        for line in &lines {
            self.write(line);
        }
        let (state, tx, child) = (self.state.clone(), self.tx.clone(), self.child.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            let mut s = state.guard();
            if s.active && s.turn == turn && s.dead.is_none() {
                let why = s.stderr.fatal("pi did not stop after cancel");
                let out = on_exit(&mut s, why);
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
        let line = reply(&mut self.state.guard(), id, answer);
        if let Some(line) = line {
            self.write(&line);
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

    fn reports_steered(&self) -> bool {
        true
    }

    fn notice_action(&mut self, action: &str) {
        if action != "trust-project" {
            return;
        }
        self.approve = true;
        if self.state.guard().active {
            self.respawn = true;
        } else {
            self.respawn();
        }
    }
}

impl PiDriver {
    fn respawn(&mut self) {
        let mut opts = self.opts.clone();
        opts.effort = self.effort.clone();
        if !self.session.is_empty() {
            opts.resume = self.session.clone();
        }
        let mut s = self.state.guard();
        opts.model = s.model.clone();
        // the old Pi takes its open dialogs with it
        let (_, closed) = close_dialogs(&mut s);
        s.dead.get_or_insert_with(|| "pi session closed".into());
        drop(s);
        for u in closed {
            let _ = self.tx.unbounded_send(u);
        }
        shut(&self.child, &self.stdin);
        *self = Self::launch(opts, self.tx.clone(), true);
    }
}

impl Drop for PiDriver {
    fn drop(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            s.dead.get_or_insert_with(|| "pi session closed".into());
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

/// Closes the old Pi before another opens the same session file.
fn shut(child: &Mutex<Option<Child>>, stdin: &Mutex<Option<ChildStdin>>) {
    stdin.guard().take();
    let Some(mut child) = child.guard().take() else { return };
    for _ in 0..50 {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn send(stdin: &Mutex<Option<ChildStdin>>, v: &Value) {
    if let Some(stdin) = stdin.guard().as_mut() {
        let _ = stdin.write_all(format!("{v}\n").as_bytes());
    }
}

const HANDLED_GRACE: Duration = Duration::from_millis(1500);

const LEAD_EXTENSION: &str = include_str!("pi_lead.ts");

fn lead_extension() -> std::io::Result<PathBuf> {
    let dir = aether_core::config_dir().ok_or_else(|| std::io::Error::other("no config dir"))?;
    lead_extension_in(&dir)
}

fn lead_extension_in(dir: &Path) -> std::io::Result<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = dir.join("pi-lead.ts");
    if std::fs::read_to_string(&path).is_ok_and(|s| s == LEAD_EXTENSION) {
        return Ok(path);
    }
    aether_core::create_private_dir(dir)?;
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!("pi-lead.ts.{}.{n}.tmp", std::process::id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let written = opts.open(&tmp).and_then(|mut f| f.write_all(LEAD_EXTENSION.as_bytes()));
    if let Err(e) = written.and_then(|_| std::fs::rename(&tmp, &path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(path)
}

fn launch_args(opts: &DriverStartOptions, approve: bool) -> Vec<String> {
    let ext = opts.lead.as_ref().and_then(|_| {
        lead_extension().inspect_err(|e| eprintln!("aether: lead tools unavailable: {e}")).ok()
    });
    launch_args_with(opts, approve, ext.as_deref())
}

fn launch_args_with(opts: &DriverStartOptions, approve: bool, ext: Option<&Path>) -> Vec<String> {
    let mut args: Vec<String> = ["--mode", "rpc"].map(String::from).into();
    let lead = opts.lead.as_ref().zip(ext);
    if opts.access != AccessMode::Full {
        let mut tools = "read,grep,find,ls".to_string();
        let specs = lead.iter().filter_map(|(l, _)| l.specs.as_array()).flatten();
        for name in specs.filter_map(|s| s["name"].as_str()) {
            tools += &format!(",aether_{name}");
        }
        args.extend(["--tools".into(), tools, "--exclude-tools".into(), "mcp__*".into()]);
    }
    if let Some((_, path)) = lead {
        args.extend(["-e".into(), path.to_string_lossy().into_owned()]);
    }
    if Path::new(&opts.resume).is_file() {
        args.extend(["--session".into(), opts.resume.clone()]);
    }
    if approve {
        args.push("--approve".into());
    }
    if opts.ephemeral {
        args.push("--no-session".into());
    }
    args
}

fn spawn_env(opts: &DriverStartOptions) -> Vec<(String, String)> {
    opts.lead
        .iter()
        .flat_map(|l| {
            [
                ("AETHER_ORCH_SOCK".to_string(), l.sock.to_string_lossy().into_owned()),
                ("AETHER_ORCH_TOKEN".to_string(), l.token.clone()),
                ("AETHER_ORCH_SPECS".to_string(), l.specs.to_string()),
            ]
        })
        .collect()
}

fn close_dialogs(s: &mut State) -> (Vec<Value>, Vec<Update>) {
    let mut open: Vec<_> = s.asks.drain().collect();
    open.sort_by_key(|(local, _)| *local);
    let lines = open
        .iter()
        .map(|(_, (ui, _))| json!({"type": "extension_ui_response", "id": ui, "cancelled": true}))
        .collect();
    (lines, open.iter().map(|(local, _)| Update::AskResolved(*local)).collect())
}

fn cancel_lines(s: &mut State, clear_id: &str) -> (Vec<Value>, Vec<Update>) {
    let (mut lines, updates) = close_dialogs(s);
    s.clearing = clear_id.into();
    // abort alone still runs queued steers
    lines.push(json!({"id": clear_id, "type": "clear_queue"}));
    lines.push(json!({"type": "abort"}));
    (lines, updates)
}

fn expire_dialog(s: &mut State, local: u64) -> Option<Update> {
    s.asks.remove(&local).map(|_| Update::AskResolved(local))
}

fn stderr_update(line: &str) -> Option<Update> {
    let shown = line.starts_with("Warning:") || line.starts_with("Error:") || visible_stderr(line);
    shown.then(|| Update::Warning(line.into()))
}

fn untrusted_project(cwd: &Path, trust: &Value, default_trust: &str) -> bool {
    const PROTECTED: [&str; 8] = [
        ".pi/settings.json",
        ".pi/mcp.json",
        ".pi/extensions",
        ".pi/skills",
        ".pi/prompts",
        ".pi/themes",
        ".pi/SYSTEM.md",
        ".pi/APPEND_SYSTEM.md",
    ];
    let cwd = aether_core::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let protected = PROTECTED.iter().any(|p| cwd.join(p).exists())
        || cwd.ancestors().any(|d| d.join(".agents/skills").exists());
    if !protected || default_trust == "always" {
        return false;
    }
    let saved = cwd.ancestors().find_map(|d| trust[d.to_string_lossy().as_ref()].as_bool());
    saved != Some(true)
}

fn message_cmd(id: &str, text: &str, behavior: &str, images: &[Attachment]) -> Value {
    let mut cmd =
        json!({"id": id, "type": "prompt", "message": text, "streamingBehavior": behavior});
    let images: Vec<Value> = images
        .iter()
        .filter_map(|f| {
            let data = STANDARD.encode(std::fs::read(&f.path).ok()?);
            Some(json!({"type": "image", "data": data, "mimeType": f.mime}))
        })
        .collect();
    if !images.is_empty() {
        cmd["images"] = images.into();
    }
    cmd
}

fn prompt_cmd(id: &str, text: &str, images: &[Attachment]) -> Value {
    match command(text, id) {
        Some(PiCommand::Rpc(cmd)) => cmd,
        _ => message_cmd(id, text, "followUp", images),
    }
}

enum PiCommand {
    Rpc(Value),
    TuiOnly(String),
}

const TUI_ONLY: [&str; 19] = [
    "settings",
    "scoped-models",
    "login",
    "logout",
    "llama",
    "share",
    "bug",
    "trust",
    "reload",
    "hotkeys",
    "changelog",
    "tree",
    "fork",
    "clone",
    "resume",
    "new",
    "import",
    "copy",
    "quit",
];

/// `None` is a normal prompt: extension commands and skills run inside Pi.
fn command(text: &str, id: &str) -> Option<PiCommand> {
    let rest = text.strip_prefix('/')?;
    let (name, arg) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let arg = arg.trim();
    let rpc = |v: Value| Some(PiCommand::Rpc(v));
    let needs = |v: Value| {
        if arg.is_empty() { Some(PiCommand::TuiOnly(name.into())) } else { rpc(v) }
    };
    match name {
        "compact" if arg.is_empty() => rpc(json!({"id": id, "type": "compact"})),
        "compact" => rpc(json!({"id": id, "type": "compact", "customInstructions": arg})),
        "model" => {
            let mut cmd = set_model(arg);
            cmd["id"] = id.into();
            needs(cmd)
        }
        "thinking" => needs(json!({"id": id, "type": "set_thinking_level", "level": arg})),
        "name" => needs(json!({"id": id, "type": "set_session_name", "name": arg})),
        "session" => rpc(json!({"id": id, "type": "get_session_stats"})),
        "export" if arg.is_empty() => rpc(json!({"id": id, "type": "export_html"})),
        "export" => rpc(json!({"id": id, "type": "export_html", "outputPath": arg})),
        _ if TUI_ONLY.contains(&name) => Some(PiCommand::TuiOnly(name.into())),
        _ => None,
    }
}

fn set_model(model: &str) -> Value {
    let (provider, id) = model.split_once('/').unwrap_or(("", model));
    json!({"type": "set_model", "provider": provider, "modelId": id})
}

fn reply(s: &mut State, id: u64, answer: Answer) -> Option<Value> {
    let (ui, value) = s.asks.remove(&id)?;
    let first = |values: Option<&Vec<String>>| values.map(|v| v.join(", "));
    let picked = match &answer {
        Answer::Reply(text) | Answer::Approve { input: text, .. } => Some(text.clone()),
        Answer::Pick(id) => Some(id.clone()),
        Answer::Answers(a) => first(a.first()),
        Answer::Form(f) => first(f.first().map(|(_, v)| v)),
        _ => None,
    };
    Some(match picked {
        Some(text) if value => json!({"type": "extension_ui_response", "id": ui, "value": text}),
        _ if value => json!({"type": "extension_ui_response", "id": ui, "cancelled": true}),
        _ => {
            let allow = matches!(
                answer,
                Answer::Approve { .. } | Answer::Reply(_) | Answer::Pick(_) | Answer::Form(_)
            ) || matches!(answer, Answer::Plan { approved: true, .. });
            json!({"type": "extension_ui_response", "id": ui, "confirmed": allow})
        }
    })
}

fn settle(s: &mut State) -> Vec<Update> {
    if !s.active {
        return vec![];
    }
    s.active = false;
    s.direct = false;
    s.compact_failed = false;
    s.names.clear();
    s.tools.clear();
    s.args.clear();
    let mut out = vec![];
    if let Some(e) = s.error.take() {
        out.extend(refuse_steers(s));
        out.push(Update::Error(e));
    }
    clear_steers(s);
    // every word already went out as a Delta
    out.push(Update::Final(String::new()));
    out
}

fn map_event(s: &mut State, v: &Value) -> Vec<Update> {
    let string = string_of;
    match v["type"].as_str().unwrap_or_default() {
        "response" => {
            let id = v["id"].as_str().unwrap_or_default();
            if !id.is_empty() && id == s.clearing {
                s.clearing.clear();
                let cleared = v["data"]["steering"].as_array().into_iter().flatten();
                let mut out = vec![];
                for text in cleared.filter_map(Value::as_str).map(without_attached_files) {
                    if let Some(i) = s.steers.iter().position(|t| t == text) {
                        s.steers.remove(i);
                        out.push(Update::SteerRefused(text.into()));
                    }
                }
                return out;
            }
            if let Some(waiter) = s.pending.remove(id) {
                let _ = waiter.send(v.clone());
                return vec![];
            }
            if let Some(text) = s.steer_acks.remove(id) {
                if v["success"] != false {
                    // as for a prompt: idle Pi runs it, unless an extension handled it
                    s.grace = v["data"]["disposition"] == "handled" && !s.running;
                    return vec![];
                }
                if let Some(at) = s.steers.iter().skip(s.taken).position(|t| *t == text) {
                    s.steers.remove(s.taken + at);
                }
                let why = v["error"].as_str().unwrap_or("Pi didn't take the message");
                let mut out = vec![Update::SteerRefused(text), Update::Warning(why.into())];
                // the run may have settled while this ack was owed
                if !s.running && s.prompt.is_empty() && s.steer_acks.is_empty() {
                    out.extend(settle(s));
                }
                return out;
            }
            if id.is_empty() || id != s.prompt {
                return vec![];
            }
            s.prompt.clear();
            if !s.active {
                return vec![];
            }
            let error = (v["success"] == false)
                .then(|| v["error"].as_str().unwrap_or("pi rejected the prompt").to_string());
            let command = v["command"].as_str().unwrap_or_default();
            if s.direct || (error.is_some() && !s.running) {
                let shown = std::mem::take(&mut s.compact_failed);
                let mut out = if error.is_none() {
                    command_updates(s, id, command, &v["data"])
                } else {
                    vec![]
                };
                s.after.remove(id);
                s.error = error.filter(|_| !s.cancelled && !shown);
                out.extend(settle(s));
                return out;
            }
            if let Some(e) = error {
                return vec![Update::Error(e)];
            }
            // extensions start runs unawaited, so one may still follow the ack
            s.grace = v["data"]["disposition"] == "handled" && !s.running;
            vec![]
        }
        "message_start" if v["message"]["role"] == "user" => {
            let blocks = v["message"]["content"].as_array().into_iter().flatten();
            let text: String =
                blocks.filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect();
            let text = without_attached_files(&text);
            // Pi expands a `/template` or `/skill` steer, so only its place in line can match it
            let at = s
                .steers
                .iter()
                .position(|t| t == text)
                .or_else(|| s.steers.front().filter(|t| t.starts_with('/')).map(|_| 0));
            let Some(at) = at else { return vec![] };
            let text = s.steers.remove(at).unwrap_or_default();
            s.seen.remove(&text);
            if at < s.taken {
                s.taken -= 1;
                return vec![];
            }
            vec![Update::Steered(text)]
        }
        "queue_update" if !s.clearing.is_empty() || s.cancelled => vec![],
        "queue_update" => {
            let queued: Vec<&str> = v["steering"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(without_attached_files)
                .collect();
            let mut out = vec![];
            for text in s.steers.iter().skip(s.taken) {
                if queued.contains(&text.as_str()) {
                    s.seen.insert(text.clone());
                    break;
                }
                // a queue_update from before Pi listed this steer says nothing about it
                if !s.seen.contains(text) {
                    break;
                }
                out.push(Update::Steered(text.clone()));
            }
            s.taken += out.len();
            out
        }
        "message_update" => message_update(s, &v["assistantMessageEvent"]),
        "message_end" if v["message"]["role"] == "assistant" => {
            let m = &v["message"];
            let mut out = vec![];
            if !std::mem::take(&mut s.saw_text) {
                let blocks = m["content"].as_array().into_iter().flatten();
                let text: String = blocks
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect();
                if !text.is_empty() {
                    out.push(Update::Delta(text));
                }
            }
            s.error = (m["stopReason"] == "error")
                .then(|| m["errorMessage"].as_str().unwrap_or("pi run failed").into());
            // Pi never runs the tool calls of an aborted or failed message
            let failed = m["stopReason"] == "error";
            if failed || m["stopReason"] == "aborted" {
                for id in std::mem::take(&mut s.names).into_keys() {
                    let Some(mut call) = s.tools.remove(&id) else { continue };
                    call.status = if failed { ToolStatus::Failed } else { ToolStatus::Stopped };
                    out.push(Update::Tool(call));
                }
                s.args.clear();
            }
            let n = |k: &str| m["usage"][k].as_u64().unwrap_or(0);
            let prompt = n("input") + n("cacheRead") + n("cacheWrite");
            let total = Some(n("totalTokens")).filter(|t| *t > 0).unwrap_or(prompt + n("output"));
            out.extend(s.window.map(Update::Window));
            if total > 0 {
                out.push(Update::Usage(total));
            }
            out.push(Update::Count { prompt, completion: n("output"), cached: n("cacheRead") });
            let cost = m["usage"]["cost"]["total"].as_f64().unwrap_or(0.0);
            if cost > 0.0 {
                out.push(Update::Cost((cost * 1_000_000.0).round() as u64));
            }
            if m["stopReason"] == "length" {
                out.push(Update::Warning("The reply hit the model's output limit".into()));
            }
            let model = m["responseModel"].as_str().or(m["model"].as_str()).unwrap_or_default();
            if !model.is_empty() && model != s.served {
                s.served = model.into();
                out.push(Update::Model(model.into()));
            }
            out
        }
        "message_end" => match v["message"]["role"].as_str() {
            Some("bashExecution") => {
                let m = &v["message"];
                s.bashes += 1;
                let code = m["exitCode"].as_i64();
                let detail = ToolDetail::Command {
                    command: string_of(&m["command"]),
                    cwd: None,
                    output: string_of(&m["output"]),
                    exit_code: code.map(|c| c as i32),
                };
                let status = match code {
                    None => ToolStatus::Stopped,
                    Some(0) => ToolStatus::Done,
                    Some(_) => ToolStatus::Failed,
                };
                let call = ToolCall {
                    status,
                    ..ToolCall::new(format!("bash-{}", s.bashes), "Run command", detail)
                };
                vec![Update::Tool(call)]
            }
            Some("custom") if v["message"]["display"] == true => {
                s.customs += 1;
                let text = result_text(&v["message"]["content"]);
                let line = text.lines().next().unwrap_or_default();
                let id = format!("info-custom-{}", s.customs);
                vec![Update::Notice(Notice::new(id, NoticeKind::Info, line))]
            }
            Some("user" | "toolResult") => vec![],
            other => {
                log_unknown("pi", &format!("message_end {}", other.unwrap_or("without role")));
                vec![]
            }
        },
        "tool_execution_start" => {
            let mut call = start(
                string(&v["toolCallId"]),
                v["toolName"].as_str().unwrap_or_default(),
                &v["args"],
            );
            call.parent = v["parentToolCallId"].as_str().map(Into::into);
            s.names.remove(&call.id);
            s.tools.insert(call.id.clone(), call.clone());
            vec![Update::Tool(call)]
        }
        "tool_execution_update" => {
            let Some(call) = s.tools.get_mut(v["toolCallId"].as_str().unwrap_or_default()) else {
                return vec![];
            };
            set_output(&mut call.detail, &result_text(&v["partialResult"]["content"]));
            if let Some(parent) = v["parentToolCallId"].as_str() {
                call.parent = Some(parent.into());
            }
            vec![Update::Tool(call.clone())]
        }
        "tool_execution_end" => {
            let id = string_of(&v["toolCallId"]);
            s.names.remove(&id);
            let mut call = s.tools.remove(&id).unwrap_or_else(|| {
                start(id, v["toolName"].as_str().unwrap_or_default(), &Value::Null)
            });
            let result = &v["result"];
            finish(&mut call, &result_text(&result["content"]), v["isError"] == true);
            halted(&mut call, false, s.cancelled);
            call.ms = v["durationMs"].as_u64();
            if let Some(parent) = v["parentToolCallId"].as_str() {
                call.parent = Some(parent.into());
            }
            let details = &result["details"];
            if let Some(patch) = details["patch"].as_str() {
                apply_patch_text(&mut call.detail, patch);
            }
            if details["truncation"]["truncated"] == true {
                let t = &details["truncation"];
                let mut note = format!(
                    "\n… Showing {} of {} lines.",
                    t["outputLines"].as_u64().unwrap_or(0),
                    t["totalLines"].as_u64().unwrap_or(0)
                );
                if let Some(path) =
                    details["fullOutputPath"].as_str().or(t["fullOutputPath"].as_str())
                {
                    note.push_str(&format!(" Full output: {path}"));
                }
                append_output(&mut call.detail, &note);
            }
            call.images = save_images(&result["content"], s.attachments.as_deref());
            vec![Update::Tool(call)]
        }
        "extension_ui_request" => extension_ui(s, v),
        "extension_error" => {
            let (path, error) = (string(&v["extensionPath"]), string(&v["error"]));
            vec![Update::Warning(format!("{path}: {error}"))]
        }
        "session_info_changed" => v["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .map(|n| Update::Title(n.into()))
            .into_iter()
            .collect(),
        "compaction_start" => {
            s.compactions += 1;
            let text = format!("Compacting context ({})", string(&v["reason"]));
            let notice =
                Notice::new(format!("compact-{}", s.compactions), NoticeKind::Compaction, text);
            vec![Update::Notice(Notice { state: NoticeState::Running, ..notice })]
        }
        "compaction_end" => {
            let id = format!("compact-{}", s.compactions);
            let r = &v["result"];
            let mut notice = Notice::new(id, NoticeKind::Compaction, "Compaction stopped");
            if v["aborted"] != true {
                if r.is_null() {
                    notice.text = string(&v["errorMessage"]);
                    notice.state = NoticeState::Failed;
                } else {
                    let n = |k: &str| r[k].as_u64().unwrap_or(0);
                    notice.text = format!(
                        "Compacted {} → ~{} tokens",
                        n("tokensBefore"),
                        n("estimatedTokensAfter")
                    );
                    notice.detail = r["summary"].as_str().map(Into::into);
                }
            }
            s.compact_failed = notice.state == NoticeState::Failed;
            let mut out = vec![Update::Notice(notice)];
            out.extend(r["estimatedTokensAfter"].as_u64().map(Update::Usage));
            out
        }
        "auto_retry_start" => {
            let text = format!(
                "Retrying ({}/{}): {}",
                v["attempt"].as_u64().unwrap_or(0),
                v["maxAttempts"].as_u64().unwrap_or(0),
                string(&v["errorMessage"])
            );
            let until = now_ms() + v["delayMs"].as_u64().unwrap_or(0) as i64;
            retry_start(s, format!("retry-{}", s.turn), text, until)
        }
        "summarization_retry_scheduled" => {
            let text = format!(
                "Retrying summary ({}/{}): {}",
                v["attempt"].as_u64().unwrap_or(0),
                v["maxAttempts"].as_u64().unwrap_or(0),
                string(&v["errorMessage"])
            );
            let until = now_ms() + v["delayMs"].as_u64().unwrap_or(0) as i64;
            retry_start(s, format!("retry-summary-{}", s.turn), text, until)
        }
        "auto_retry_end" => {
            let Some(mut notice) = s.retry.take() else { return vec![] };
            if v["success"] != true {
                notice.state = NoticeState::Failed;
                if let Some(e) = v["finalError"].as_str() {
                    notice.text = e.into();
                }
            } else {
                notice.state = NoticeState::Done;
            }
            notice.until_ms = None;
            vec![Update::Notice(notice)]
        }
        "summarization_retry_finished" => {
            let Some(mut notice) = s.retry.take() else { return vec![] };
            notice.state = NoticeState::Done;
            notice.until_ms = None;
            vec![Update::Notice(notice)]
        }
        "thinking_level_changed" => {
            let level = string(&v["level"]);
            if s.effort.is_empty() || level == s.effort {
                return vec![];
            }
            let text = format!(
                "Pi is using thinking level {level}. {} isn't available for this model.",
                s.effort
            );
            vec![Update::Notice(Notice::new("info-thinking", NoticeKind::Info, text))]
        }
        "agent_start" => {
            s.running = true;
            s.grace = false;
            if s.active {
                return vec![];
            }
            s.active = true;
            s.turn += 1;
            s.wakes += 1;
            let notice = Notice::new(
                format!("wake-{}", s.wakes),
                NoticeKind::Wake,
                "Pi started a run on its own",
            );
            vec![Update::Wake, Update::Notice(notice)]
        }
        "agent_settled" => {
            s.running = false;
            // an unacked prompt or steer may start a fresh run after this one
            if s.prompt.is_empty() && s.steer_acks.is_empty() { settle(s) } else { vec![] }
        }
        "turn_start"
        | "turn_end"
        | "agent_end"
        | "message_start"
        | "summarization_retry_attempt_start" => vec![],
        "entry_appended" => {
            log_unknown("pi", "entry_appended");
            vec![]
        }
        other => {
            log_unknown("pi", other);
            vec![]
        }
    }
}

fn retry_start(s: &mut State, id: String, text: String, until_ms: i64) -> Vec<Update> {
    let notice = Notice {
        state: NoticeState::Running,
        until_ms: Some(until_ms),
        ..Notice::new(id, NoticeKind::Retry, text)
    };
    s.retry = Some(notice.clone());
    vec![Update::Notice(notice)]
}

fn command_updates(s: &mut State, id: &str, command: &str, data: &Value) -> Vec<Update> {
    let mut out: Vec<Update> = s.after.remove(id).into_iter().collect();
    match command {
        "set_model" => {
            out.extend(data["id"].as_str().map(|m| Update::Model(m.into())));
            if let (Some(provider), Some(id)) = (data["provider"].as_str(), data["id"].as_str()) {
                s.model = format!("{provider}/{id}");
            }
            if let Some(window) = data["contextWindow"].as_u64() {
                s.window = Some(window);
            }
        }
        "get_session_stats" => {
            let text = format!(
                "{} tokens, ${:.4}",
                data["tokens"]["total"].as_u64().unwrap_or(0),
                data["cost"].as_f64().unwrap_or(0.0)
            );
            let detail = serde_json::to_string_pretty(data).ok();
            out.push(Update::Notice(Notice {
                detail,
                ..Notice::new("info-session", NoticeKind::Info, text)
            }));
        }
        "export_html" => {
            let text = format!("Exported to {}", string_of(&data["path"]));
            out.push(Update::Notice(Notice::new("info-export", NoticeKind::Info, text)));
        }
        _ => {}
    }
    out
}

fn message_update(s: &mut State, e: &Value) -> Vec<Update> {
    let index = e["contentIndex"].as_u64().unwrap_or(0);
    match e["type"].as_str() {
        Some("text_delta") => {
            let Some(delta) = e["delta"].as_str().filter(|d| !d.is_empty()) else { return vec![] };
            s.saw_text = true;
            vec![Update::Delta(delta.into())]
        }
        Some("thinking_delta") => {
            let delta = e["delta"].as_str().filter(|d| !d.is_empty());
            delta.map(|d| Update::Thinking(d.into())).into_iter().collect()
        }
        Some("toolcall_start") => {
            let (id, name) = (string_of(&e["id"]), string_of(&e["toolName"]));
            let call = start(id.clone(), &name, &Value::Null);
            s.args.insert(index, (id.clone(), String::new()));
            s.names.insert(id.clone(), name);
            s.tools.insert(id, call.clone());
            vec![Update::Tool(call)]
        }
        Some("toolcall_delta") => {
            let Some((id, json)) = s.args.get_mut(&index) else { return vec![] };
            json.push_str(e["delta"].as_str().unwrap_or_default());
            // ponytail: re-parses the whole buffer whenever it could be complete, still quadratic
            // for args with many `}`-ending deltas
            if !json.trim_end().ends_with('}') {
                return vec![];
            }
            let Ok(input) = serde_json::from_str::<Value>(json) else { return vec![] };
            let id = id.clone();
            reclassify(s, &id, &input)
        }
        Some("toolcall_end") => {
            s.args.remove(&index);
            let call = &e["toolCall"];
            let id = string_of(&call["id"]);
            if !s.names.contains_key(&id) {
                s.names.insert(id.clone(), string_of(&call["name"]));
            }
            reclassify(s, &id, &call["arguments"])
        }
        Some(
            "start" | "text_start" | "text_end" | "thinking_start" | "thinking_end" | "done"
            | "error",
        ) => vec![],
        other => {
            log_unknown("pi", &format!("message_update {}", other.unwrap_or("without type")));
            vec![]
        }
    }
}

fn reclassify(s: &mut State, id: &str, input: &Value) -> Vec<Update> {
    let Some(name) = s.names.get(id) else { return vec![] };
    let mut call = start(id, name, input);
    if let Some(old) = s.tools.get(id) {
        call.parent.clone_from(&old.parent);
    }
    s.tools.insert(id.into(), call.clone());
    vec![Update::Tool(call)]
}

fn extension_ui(s: &mut State, v: &Value) -> Vec<Update> {
    let string = string_of;
    let notice = |id: String, text: String| Notice::new(id, NoticeKind::Info, text);
    let ask = match v["method"].as_str() {
        Some("confirm") => Ask::Form(Form {
            title: string(&v["title"]),
            message: string(&v["message"]),
            timeout_ms: v["timeout"].as_u64(),
            ..Form::default()
        }),
        Some(method @ ("select" | "input" | "editor")) => {
            let field = match method {
                "select" => {
                    let options = v["options"].as_array().into_iter().flatten();
                    Field {
                        key: "value".into(),
                        kind: FieldKind::Choice,
                        required: true,
                        options: options
                            .map(|o| Choice { label: string(o), ..Choice::default() })
                            .collect(),
                        ..Field::default()
                    }
                }
                _ => Field {
                    key: "value".into(),
                    label: string(&v["placeholder"]),
                    default: Some(string(&v["prefill"]))
                        .filter(|t| !t.is_empty())
                        .into_iter()
                        .collect(),
                    ..Field::default()
                },
            };
            Ask::Form(Form {
                title: string(&v["title"]),
                fields: vec![field],
                timeout_ms: v["timeout"].as_u64(),
                ..Form::default()
            })
        }
        Some("notify") => {
            let message = clean(v["message"].as_str().unwrap_or_default()).into_owned();
            if v["notifyType"] == "error" {
                return vec![Update::Warning(message)];
            }
            if let Some(url) = url_in(&message) {
                let id = format!("mcp-{}", url_host(&url));
                let n = Notice { url: Some(url), ..Notice::new(id, NoticeKind::McpAuth, message) };
                return vec![Update::Notice(n)];
            }
            s.notifies += 1;
            return vec![Update::Notice(notice(format!("info-notify-{}", s.notifies), message))];
        }
        Some("setStatus") => {
            let text = clean(v["statusText"].as_str().unwrap_or_default()).into_owned();
            let id = format!("info-status-{}", string(&v["statusKey"]));
            return vec![Update::Notice(notice(id, text))];
        }
        Some("setWidget") => {
            let lines: Vec<String> = v["widgetLines"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|l| clean(l).into_owned())
                .collect();
            let id = format!("info-widget-{}", string(&v["widgetKey"]));
            let n = Notice {
                detail: Some(lines.join("\n")).filter(|d| !d.is_empty()),
                ..notice(id, lines.first().cloned().unwrap_or_default())
            };
            return vec![Update::Notice(n)];
        }
        Some("setTitle") => return vec![Update::Title(string(&v["title"]))],
        Some("set_editor_text") => {
            log_unknown("pi", "set_editor_text");
            return vec![];
        }
        other => {
            let what = format!("extension_ui_request {}", other.unwrap_or("without method"));
            log_unknown("pi", &what);
            s.outbox
                .push(json!({"type": "extension_ui_response", "id": v["id"], "cancelled": true}));
            return vec![Update::Warning(format!("Pi sent a dialog Aether can't show ({what})"))];
        }
    };
    s.next_ask += 1;
    s.asks.insert(s.next_ask, (string(&v["id"]), v["method"] != "confirm"));
    vec![Update::Ask { id: s.next_ask, ask }]
}

fn url_in(text: &str) -> Option<String> {
    let word =
        text.split_whitespace().find(|w| w.starts_with("http://") || w.starts_with("https://"))?;
    Some(word.trim_end_matches(['.', ',', ')', ';']).to_string())
}

fn url_host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#', ':']).next().unwrap_or_default()
}

#[cfg(test)]
#[path = "../tests/unit/pi.rs"]
mod tests;
