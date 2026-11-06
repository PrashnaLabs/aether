use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

use aether_core::{
    AccessMode, Answer, Approval, ApprovalKind, Ask, Attachment, MutexExt, Notice, NoticeKind,
    NoticeState, Provider, ToolCall, ToolDetail, ToolStatus, Update, find_binary, home_dir,
    repaired_path, without_attached_files,
};

use super::record::{self, Recorder};
use super::spawn::{StderrTail, command, log_unknown, spawn_logged};
use super::tool::{finish, halted, set_output, start};
use super::version::gate_binary;
use super::{Driver, DriverStartOptions};

/// Slash commands agy answers itself or only runs in its TUI; on the stream they end the process.
const TERMINAL_ONLY: &[&str] = &[
    "agents",
    "changelog",
    "config",
    "settings",
    "credits",
    "effort",
    "help",
    "hooks",
    "model",
    "permissions",
    "plugin",
    "skills",
    "usage",
    "quota",
    "add-dir",
    "artifact",
    "branch",
    "btw",
    "clear",
    "codesearch",
    "context",
    "conversation",
    "copy",
    "diff",
    "exit",
    "feedback",
    "fork",
    "keybindings",
    "logout",
    "mcp",
    "new",
    "open",
    "quit",
    "remote-control",
    "rename",
    "resume",
    "rewind",
    "search",
    "statusline",
    "switch",
    "tasks",
    "title",
    "undo",
    "voice",
];

pub struct AntigravityDriver {
    opts: DriverStartOptions,
    tx: UnboundedSender<Update>,
    state: Arc<Mutex<State>>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    resumed: bool,
    rec: Option<Arc<Recorder>>,
}

#[derive(Default)]
struct State {
    /// Process generation; output of older processes is ignored.
    epoch: u64,
    live: bool,
    full: bool,
    conversation: String,
    stderr: StderrTail,
    active: bool,
    cancelled: bool,
    turn: u64,
    /// Inputs written whose result has not arrived.
    inputs: VecDeque<Input>,
    /// Inputs whose `user_input` step has arrived.
    taken: usize,
    wakes: u32,
    /// The mode the launch asked for; `init` says what agy is really in.
    requested: AccessMode,
    tools: HashMap<String, ToolCall>,
    /// Finished rows agy gave no output for, with the agy tool name that made them.
    blocked: Vec<(String, ToolCall)>,
    /// agy background task id to the row that started it; the row outlives its turn.
    background: HashMap<String, String>,
    model: String,
    /// Message of the `AGY_ERROR:` stderr line, the headline of a fatal exit.
    agy_error: Option<String>,
    text_step: Option<u64>,
    texted: bool,
    ask: Option<(u64, Vec<String>)>,
    next_ask: u64,
    /// Last title sent; agy writes its own to an annotations file.
    title: String,
    dead: Option<String>,
    /// Subagent rows dispatched this turn; agy's next system message carries their report.
    dispatched: Vec<String>,
    /// The turn's denials so far, as the rerun card and its lines.
    rerun: Option<(Approval, Vec<String>)>,
}

struct Input {
    text: String,
    steer: bool,
}

impl AntigravityDriver {
    pub fn spawn(opts: DriverStartOptions, tx: UnboundedSender<Update>) -> Self {
        let state = State { conversation: opts.resume.clone(), ..State::default() };
        let rec = record::from_env("agy", &opts.cwd);
        let mut me = Self {
            opts,
            tx,
            state: Arc::new(Mutex::new(state)),
            child: None,
            stdin: None,
            resumed: false,
            rec,
        };
        if let Err(why) = gate_binary(Provider::Antigravity, "agy", &me.opts, &me.tx) {
            me.die(why);
            return me;
        }
        match me.start(me.opts.access == AccessMode::Full, true) {
            Ok(id) => {
                me.resumed = !me.opts.resume.is_empty() && id == me.opts.resume;
                if me.resumed {
                    send_title(&me.state, &me.tx);
                }
            }
            Err(e) => me.die(e),
        }
        me
    }

    fn die(&self, why: String) {
        self.state.guard().dead = Some(why);
    }

    /// Replaces the process. With `wait`, blocks for its `init` and returns the conversation id;
    /// without, the reader handles `init` and the new process is usable at once.
    fn start(&mut self, full: bool, wait: bool) -> Result<String, String> {
        // before close: the old reader's EOF must see a stale epoch, not end the new turn
        let gone = {
            let mut s = self.state.guard();
            s.epoch += 1;
            stop_background(&mut s)
        };
        for u in gone {
            let _ = self.tx.unbounded_send(u);
        }
        if let Some(child) = self.close() {
            std::thread::spawn(move || reap(child));
        }
        let mut cmd = command(&self.opts, "agy").ok_or_else(|| {
            "agy is not installed; install it or set an explicit binary path".to_string()
        })?;
        let conversation = self.state.guard().conversation.clone();
        cmd.args(launch_args(&self.opts, &conversation, full))
            .envs(spawn_env(&self.opts))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = spawn_logged(&mut cmd).map_err(|e| format!("agy failed to start: {e}"))?;
        let epoch = {
            let mut s = self.state.guard();
            s.epoch += 1;
            s.live = true;
            s.full = full;
            s.requested = requested_mode(self.opts.access);
            s.model.clone_from(&self.opts.model);
            s.stderr = StderrTail::default();
            s.agy_error = None;
            s.epoch
        };
        let _ = self.tx.unbounded_send(spawn_notice(NoticeState::Running));
        if let Some(stderr) = child.stderr.take() {
            let (state, tx) = (self.state.clone(), self.tx.clone());
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let mut s = state.guard();
                    if s.epoch != epoch {
                        continue;
                    }
                    for u in on_stderr(&mut s, &line) {
                        let _ = tx.unbounded_send(u);
                    }
                }
            });
        }
        let (init_tx, init_rx) = std::sync::mpsc::channel();
        if let Some(stdout) = child.stdout.take() {
            let (state, tx, rec) = (self.state.clone(), self.tx.clone(), self.rec.clone());
            let waiter = wait.then_some(init_tx);
            std::thread::spawn(move || read(stdout, epoch, &state, &tx, rec, waiter));
        }
        if !wait {
            self.stdin = child.stdin.take();
            self.child = Some(child);
            return Ok(String::new());
        }
        let init = init_rx
            .recv_timeout(Duration::from_secs(45))
            .unwrap_or_else(|_| Err("agy did not start within 45 s".into()));
        match init {
            Ok(id) => {
                self.stdin = child.stdin.take();
                self.child = Some(child);
                Ok(id)
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = self.tx.unbounded_send(spawn_notice(NoticeState::Failed));
                Err(e)
            }
        }
    }

    fn mark(&self, v: Value) {
        if let Some(rec) = &self.rec {
            rec.line(&v.to_string());
        }
    }

    fn close(&mut self) -> Option<Child> {
        self.stdin.take();
        self.child.take()
    }

    fn send(&mut self, text: &str) {
        let line = format!("{}\n", json!({"event": "user", "message": {"content": text}}));
        if !self.stdin.as_mut().is_some_and(|w| w.write_all(line.as_bytes()).is_ok()) {
            self.settle(Some("agy is not running".into()));
        }
    }

    /// Ends the running turn, once.
    fn settle(&self, error: Option<String>) {
        let out = end_turn(&mut self.state.guard(), error);
        for u in out {
            let _ = self.tx.unbounded_send(u);
        }
    }
}

impl Driver for AntigravityDriver {
    fn prompt(&mut self, text: String, _images: Vec<Attachment>) {
        let full = self.opts.access == AccessMode::Full;
        let mut s = self.state.guard();
        let resolved = on_prompt(&mut s, &text);
        let (dead, respawn) = (s.dead.clone(), !s.live || s.full != full);
        s.epoch += u64::from(respawn);
        drop(s);
        for u in resolved {
            let _ = self.tx.unbounded_send(u);
        }
        self.mark(json!({"aether": "prompt", "text": text}));
        if let Some(why) = dead {
            return self.settle(Some(why));
        }
        if let Some(c) = terminal_only(&text) {
            let why = format!("`/{c}` only works in the agy terminal");
            let _ = self.tx.unbounded_send(Update::Warning(why));
            return self.settle(None);
        }
        if respawn && let Err(e) = self.start(full, false) {
            self.die(e.clone());
            return self.settle(Some(e));
        }
        self.send(&text);
    }

    fn steer(&mut self, text: String, _images: Vec<Attachment>) -> bool {
        let mut s = self.state.guard();
        let ask = s.ask.as_ref().map(|(id, _)| *id);
        if !on_steer(&mut s, &text) {
            return false;
        }
        let terminal = terminal_only(&text);
        drop(s);
        self.mark(json!({"aether": "steer", "text": text}));
        if let Some(c) = terminal {
            let own = without_attached_files(&text).to_string();
            let why = format!("`/{c}` only works in the agy terminal");
            let _ = self.tx.unbounded_send(Update::SteerRefused(own));
            let _ = self.tx.unbounded_send(Update::Warning(why));
            return true;
        }
        if let Some(id) = ask {
            let _ = self.tx.unbounded_send(Update::AskResolved(id));
        }
        self.send(&text);
        true
    }

    fn reports_steered(&self) -> bool {
        true
    }

    fn cancel(&mut self) {
        let mut s = self.state.guard();
        if !on_cancel(&mut s) {
            return;
        }
        // behind a rerun card agy idles: nothing runs to stop
        let pid = self.child.as_ref().map(Child::id).filter(|_| s.ask.is_none());
        let Some(pid) = pid else {
            drop(s);
            self.mark(json!({"aether": "cancel"}));
            return self.settle(None);
        };
        // SIGINT always ends agy; the next prompt respawns
        s.live = false;
        let (turn, epoch) = (s.turn, s.epoch);
        drop(s);
        self.mark(json!({"aether": "cancel"}));
        signal("-INT", pid);
        let (state, tx) = (self.state.clone(), self.tx.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            let mut s = state.guard();
            if s.active && s.turn == turn && s.epoch == epoch {
                signal("-KILL", pid);
                for u in killed(&mut s) {
                    let _ = tx.unbounded_send(u);
                }
            }
        });
    }

    fn answer(&mut self, id: u64, answer: Answer) {
        let mut s = self.state.guard();
        let Some((_, blocked)) = s.ask.take_if(|(ask, _)| *ask == id) else { return };
        drop(s);
        self.mark(json!({"aether": "answer", "id": id, "answer": answer}));
        if !matches!(answer, Answer::Approve { .. } | Answer::Reply(_) | Answer::Pick(_)) {
            return self.settle(None);
        }
        let _ = self.tx.unbounded_send(Update::Notice(Notice::new(
            "info-rerun",
            NoticeKind::Info,
            "Restarting agy with full permissions for this turn",
        )));
        if let Err(e) = self.start(true, false) {
            self.die(e.clone());
            return self.settle(Some(e));
        }
        let retry = retry_prompt(&blocked);
        self.state.guard().inputs.push_back(Input { text: retry.clone(), steer: false });
        self.send(&retry);
    }

    fn resumed(&self) -> bool {
        self.resumed
    }

    fn alive(&self) -> bool {
        self.state.guard().dead.is_none()
    }

    fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }
}

impl Drop for AntigravityDriver {
    fn drop(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            s.epoch += 1;
            s.dead.get_or_insert_with(|| "agy session closed".into());
        }
        if let Some(child) = self.close() {
            std::thread::spawn(move || reap(child));
        }
    }
}

fn begin(s: &mut State) {
    s.active = true;
    s.cancelled = false;
    s.turn += 1;
    s.tools.retain(|id, _| s.background.values().any(|row| row == id));
    s.blocked.clear();
    s.text_step = None;
    s.texted = false;
    s.dispatched.clear();
    s.rerun = None;
}

/// Steers agy never took; they go back to the user's queue with the turn's inputs.
fn refuse_inputs(s: &mut State) -> Vec<Update> {
    let untaken = s.inputs.iter().skip(s.taken).filter(|i| i.steer);
    let out = untaken.map(|i| Update::SteerRefused(i.text.clone())).collect();
    s.inputs.clear();
    s.taken = 0;
    out
}

fn end_turn(s: &mut State, error: Option<String>) -> Vec<Update> {
    if !s.active {
        return vec![];
    }
    s.active = false;
    s.cancelled = false;
    s.ask = None;
    let mut out = refuse_inputs(s);
    out.extend(error.map(Update::Error));
    out.push(Update::Final(String::new()));
    out
}

fn on_prompt(s: &mut State, text: &str) -> Vec<Update> {
    begin(s);
    s.inputs.clear();
    s.taken = 0;
    s.inputs.push_back(Input { text: text.into(), steer: false });
    s.ask.take().map(|(id, _)| Update::AskResolved(id)).into_iter().collect()
}

fn on_steer(s: &mut State, text: &str) -> bool {
    if !s.active {
        return false;
    }
    s.text_step = None;
    if terminal_only(text).is_none() {
        s.ask = None;
        s.inputs.push_back(Input { text: without_attached_files(text).into(), steer: true });
    }
    true
}

fn on_cancel(s: &mut State) -> bool {
    if !s.active {
        return false;
    }
    s.cancelled = true;
    true
}

fn reap(mut child: Child) {
    for _ in 0..30 {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn signal(sig: &str, pid: u32) {
    let _ = Command::new("/bin/kill")
        .args([sig, &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn read(
    stdout: ChildStdout,
    epoch: u64,
    state: &Arc<Mutex<State>>,
    tx: &UnboundedSender<Update>,
    rec: Option<Arc<Recorder>>,
    waiter: Option<Sender<Result<String, String>>>,
) {
    let mut up = false;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(rec) = &rec {
            rec.line(&line);
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if !up {
            let Some(r) = startup(&v) else { continue };
            up = true;
            let mut s = state.guard();
            match &r {
                Ok(_) if s.epoch == epoch => {
                    for u in init(&mut s, &v) {
                        let _ = tx.unbounded_send(u);
                    }
                }
                Err(why) if waiter.is_none() && s.epoch == epoch => fail(&mut s, tx, why.clone()),
                _ => {}
            }
            if let Some(w) = &waiter {
                let _ = w.send(r);
            }
            continue;
        }
        let mut s = state.guard();
        if s.epoch == epoch {
            for u in map_event(&mut s, &v) {
                let _ = tx.unbounded_send(u);
            }
            if v["event"] == "result" {
                watch_title(state.clone(), tx.clone());
            }
        }
    }
    let mut s = state.guard();
    let started = up || waiter.is_some();
    if !up && let Some(w) = waiter {
        let _ = w.send(Err(fatal_text(&s)));
    }
    if s.epoch != epoch {
        return;
    }
    for u in closed(&mut s, started) {
        let _ = tx.unbounded_send(u);
    }
}

fn spawn_notice(state: NoticeState) -> Update {
    Update::Notice(Notice {
        state,
        ..Notice::new("startup-spawn", NoticeKind::Startup, "Starting agy")
    })
}

/// Routes one stderr line: the `AGY_ERROR:` message is kept apart from the tail, which
/// carries the rest; the `--mode auto` refusal downgrades the mode.
fn on_stderr(s: &mut State, line: &str) -> Vec<Update> {
    if line.starts_with("warning:") || line.starts_with("jetski:") {
        return Vec::new();
    }
    if let Some(message) = agy_error(line) {
        s.agy_error = Some(message);
        return Vec::new();
    }
    s.stderr.push(line);
    if s.requested == AccessMode::Auto
        && line.contains("--mode auto")
        && (line.contains("unavailable") || line.contains("not available"))
    {
        return vec![
            Update::Mode(AccessMode::AutoEdit),
            mode_notice(s.epoch, AccessMode::AutoEdit, s.requested),
        ];
    }
    Vec::new()
}

fn agy_error(line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line.trim().strip_prefix("AGY_ERROR:")?.trim()).ok()?;
    v["message"].as_str().or(v["error"].as_str()).map(String::from)
}

fn fatal_text(s: &State) -> String {
    s.stderr.fatal(s.agy_error.as_deref().unwrap_or("agy exited"))
}

/// The cancel grace ran out and agy was killed; the epoch bump keeps `closed()` from running.
fn killed(s: &mut State) -> Vec<Update> {
    s.active = false;
    s.epoch += 1;
    let mut out = stop_background(s);
    out.extend(refuse_inputs(s));
    out.push(Update::Final(String::new()));
    out
}

/// Rows of background tasks that died with their process.
fn stop_background(s: &mut State) -> Vec<Update> {
    let rows: Vec<String> = s.background.drain().map(|(_, row)| row).collect();
    rows.iter()
        .filter_map(|row| s.tools.remove(row))
        .map(|mut call| {
            call.status = ToolStatus::Stopped;
            Update::Tool(call)
        })
        .collect()
}

/// stdout closed: the updates that end the startup and the turn it leaves open.
/// `started` is false for a respawn that never reached `init` and has no waiter.
fn closed(s: &mut State, started: bool) -> Vec<Update> {
    let why = fatal_text(s);
    let mut out = stop_background(s);
    s.live = false;
    if !started {
        out.push(spawn_notice(NoticeState::Failed));
    }
    if s.active && s.ask.is_none() {
        s.active = false;
        out.extend(refuse_inputs(s));
        if !s.cancelled {
            out.push(Update::Error(why.clone()));
        }
        out.push(Update::Final(String::new()));
    }
    out
}

/// A respawned process that never reached `init`: nothing will answer the turn.
fn fail(s: &mut State, tx: &UnboundedSender<Update>, why: String) {
    s.live = false;
    s.dead = Some(why.clone());
    let _ = tx.unbounded_send(spawn_notice(NoticeState::Failed));
    if s.active {
        s.active = false;
        for u in refuse_inputs(s) {
            let _ = tx.unbounded_send(u);
        }
        let _ = tx.unbounded_send(Update::Error(why));
        let _ = tx.unbounded_send(Update::Final(String::new()));
    }
}

// ponytail: static table, agy reports no window; wrong when agy changes limits. read agy's context_window once print mode exposes it
fn window(model: &str) -> Option<u64> {
    if model.starts_with("gemini-") {
        Some(1_048_576)
    } else if model.starts_with("claude-") {
        Some(200_000)
    } else {
        None
    }
}

fn requested_mode(access: AccessMode) -> AccessMode {
    if access == AccessMode::Auto { AccessMode::Auto } else { AccessMode::AutoEdit }
}

fn agy_mode(permission_mode: &str) -> Option<AccessMode> {
    let mode = match permission_mode {
        "accept-edits" | "request-review" => Some(AccessMode::AutoEdit),
        "auto" => Some(AccessMode::Auto),
        "plan" => Some(AccessMode::ReadOnly),
        _ => None,
    };
    if mode.is_none() {
        log_unknown("agy", &format!("permission_mode {permission_mode}"));
    }
    mode
}

fn mode_notice(n: u64, mode: AccessMode, requested: AccessMode) -> Update {
    Update::Notice(Notice::new(
        format!("mode-{n}"),
        NoticeKind::ModeChange,
        format!(
            "agy is in {} mode. {} isn't available for this account.",
            mode.label(),
            requested.label()
        ),
    ))
}

/// The updates for agy's `init` line.
fn init(s: &mut State, v: &Value) -> Vec<Update> {
    let id = v["conversation_id"].as_str().unwrap_or_default().to_string();
    let mut out =
        vec![Update::Notice(Notice::new("startup-spawn", NoticeKind::Startup, "Starting agy"))];
    if !s.conversation.is_empty() && s.conversation != id {
        out.push(Update::Notice(Notice::new(
            "info-resume",
            NoticeKind::Info,
            "Couldn't find the earlier agy conversation, so this is a new one.",
        )));
    }
    s.conversation.clone_from(&id);
    out.push(Update::Session(id));
    out.extend(window(&s.model).map(Update::Window));
    let mode = if s.full {
        Some(AccessMode::Full)
    } else {
        agy_mode(v["init"]["permission_mode"].as_str().unwrap_or_default())
    };
    if let Some(mode) = mode {
        out.push(Update::Mode(mode));
        if !s.full && mode != s.requested {
            out.push(mode_notice(s.epoch, mode, s.requested));
        }
    }
    out
}

/// agy titles a conversation shortly after its first result.
fn watch_title(state: Arc<Mutex<State>>, tx: UnboundedSender<Update>) {
    std::thread::spawn(move || {
        for _ in 0..30 {
            let dead = state.guard().dead.is_some();
            if dead || send_title(&state, &tx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

fn send_title(state: &Mutex<State>, tx: &UnboundedSender<Update>) -> bool {
    let file = format!(".gemini/antigravity-cli/annotations/{}.pbtxt", state.guard().conversation);
    let Ok(pbtxt) = std::fs::read_to_string(home_dir().join(file)) else { return false };
    let Some(title) = new_title(&mut state.guard(), &pbtxt) else { return false };
    let _ = tx.unbounded_send(Update::Title(title));
    true
}

fn new_title(s: &mut State, pbtxt: &str) -> Option<String> {
    let title = pbtxt_title(pbtxt).filter(|t| *t != s.title)?;
    s.title.clone_from(&title);
    Some(title)
}

/// The `title` field of agy's protobuf text annotation.
fn pbtxt_title(pbtxt: &str) -> Option<String> {
    let line = pbtxt.lines().find_map(|l| l.trim().strip_prefix("title:"))?;
    let mut chars = line.trim_start().strip_prefix('"')?.chars();
    let mut title = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(title).filter(|t| !t.trim().is_empty()),
            '\\' => title.push(chars.next()?),
            c => title.push(c),
        }
    }
    None
}

fn launch_args(opts: &DriverStartOptions, conversation: &str, full: bool) -> Vec<String> {
    let mut args: Vec<String> = ["--input-format", "stream-json", "--output-format", "stream-json"]
        .map(String::from)
        .into();
    if !conversation.is_empty() {
        args.extend(["--conversation".into(), conversation.into()]);
    }
    // `agy models` ids carry an effort suffix, which conflicts with --effort
    let model = match opts.effort.as_str() {
        "" => opts.model.as_str(),
        _ => ["-low", "-medium", "-high", "-xhigh", "-max"]
            .iter()
            .find_map(|s| opts.model.strip_suffix(s))
            .unwrap_or(&opts.model),
    };
    for (flag, value) in [("--model", model), ("--effort", &opts.effort)] {
        if !value.is_empty() {
            args.extend([flag.to_string(), value.to_string()]);
        }
    }
    let mode: &[&str] = match opts.access {
        _ if full => &["--dangerously-skip-permissions"],
        AccessMode::Auto => &["--mode", "auto"],
        _ => &["--mode", "accept-edits"],
    };
    args.extend(mode.iter().map(|s| s.to_string()));
    if opts.lead.is_some() && !full {
        args.extend(["--settings".into(), r#"{"permissions":{"allow":["mcp(aether/*)"]}}"#.into()]);
    }
    if let Some(dir) = &opts.attachments {
        let _ = std::fs::create_dir_all(dir);
        args.extend(["--add-dir".into(), dir.display().to_string()]);
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
            ]
        })
        .collect()
}

fn agy(binary: &str) -> io::Result<Command> {
    let bin = find_binary("agy", binary)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "agy is not installed"))?;
    let mut cmd = Command::new(bin);
    cmd.env("PATH", repaired_path()).stdin(Stdio::null());
    Ok(cmd)
}

#[derive(Debug, PartialEq)]
pub struct AgyEntry {
    pub command: String,
    pub disabled: bool,
}

pub fn agy_mcp_config() -> std::path::PathBuf {
    home_dir().join(".gemini/config/mcp_config.json")
}

/// `mcpServers.aether` from agy's MCP config; a missing or 0-byte file means none.
pub fn agy_aether_entry(config: &Path) -> Option<AgyEntry> {
    let config: Value = serde_json::from_slice(&std::fs::read(config).ok()?).ok()?;
    let entry = &config["mcpServers"]["aether"];
    Some(AgyEntry {
        command: entry["command"].as_str()?.to_string(),
        disabled: entry["disabled"].as_bool().unwrap_or(false),
    })
}

/// One-time global registration; the token reaches the shim through agy's process env only.
pub fn register_agy_aether(binary: &str, exe: &Path) -> io::Result<()> {
    let out = agy(binary)?.args(["mcp", "add", "aether"]).arg(exe).arg("mcp").output()?;
    if out.status.success() {
        return Ok(());
    }
    let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let why = if why.is_empty() { format!("agy mcp add failed: {}", out.status) } else { why };
    Err(io::Error::other(why))
}

fn terminal_only(text: &str) -> Option<&str> {
    let cmd = text.trim_start().strip_prefix('/')?.split_whitespace().next()?;
    TERMINAL_ONLY.contains(&cmd).then_some(cmd)
}

/// What a pre-`init` line says about startup: the conversation id, or why it failed.
fn startup(v: &Value) -> Option<Result<String, String>> {
    match v["event"].as_str()? {
        "init" => Some(Ok(v["conversation_id"].as_str().unwrap_or_default().to_string())),
        "result" => {
            Some(Err(v["result"]["error"].as_str().unwrap_or("agy failed to start").to_string()))
        }
        _ => None,
    }
}

fn map_event(s: &mut State, v: &Value) -> Vec<Update> {
    let event = v["event"].as_str().unwrap_or_default();
    if !matches!(event, "step_update" | "result") {
        log_unknown("agy", if event.is_empty() { "event without a name" } else { event });
        return Vec::new();
    }
    let mut out = Vec::new();
    if !s.active {
        if s.cancelled {
            return out;
        }
        begin(s);
        s.wakes += 1;
        out.push(Update::Wake);
        out.push(Update::Notice(Notice::new(
            format!("wake-{}", s.wakes),
            NoticeKind::Wake,
            "agy started a turn on its own",
        )));
    }
    out.extend(if event == "step_update" {
        step(s, &v["step_update"])
    } else {
        result(s, &v["result"])
    });
    out
}

fn step(s: &mut State, st: &Value) -> Vec<Update> {
    match st["step_type"].as_str() {
        Some("agent_response") => {
            let mut out = Vec::new();
            if let Some(text) = st["text_delta"].as_str().filter(|t| !t.is_empty()) {
                let at = st["step_index"].as_u64();
                let sep = if s.text_step.is_some_and(|i| Some(i) != at) { "\n\n" } else { "" };
                out.push(Update::Delta(format!("{sep}{text}")));
                s.text_step = at;
                s.texted = true;
            }
            if let Some(u) = st.get("usage") {
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                let (prompt, completion) = (n("input_tokens"), n("output_tokens"));
                let total = u["total_tokens"].as_u64().unwrap_or(prompt + completion);
                out.push(Update::Usage(total));
                out.push(Update::Count { prompt, completion, cached: n("cache_read_tokens") });
            }
            out
        }
        Some("tool") => tool(s, st).into_iter().map(Update::Tool).collect(),
        Some("subagent") => {
            let id = format!("{}:{}", s.conversation, st["step_index"]);
            let first = &st["subagent_info"]["subagents"][0];
            let mut call = s.tools.remove(&id).unwrap_or_else(|| {
                start(
                    id.clone(),
                    "agent",
                    &json!({"description": first["role"], "subagent_type": first["type_name"]}),
                )
            });
            if let (ToolDetail::Task { prompt, .. }, Some(p)) =
                (&mut call.detail, first["initial_prompt"].as_str())
            {
                *prompt = p.into();
            }
            s.text_step = None;
            match st["state"].as_str() {
                // dispatched: it reports back in a later system message
                Some("DONE") => {
                    s.dispatched.push(id.clone());
                    s.tools.insert(id, call.clone());
                }
                Some(state) if state != "ACTIVE" => {
                    finish(&mut call, "", true);
                    halted(&mut call, false, s.cancelled);
                    call.ms = st["duration_seconds"].as_f64().map(|d| (d * 1000.0) as u64);
                }
                _ => {
                    s.tools.insert(id, call.clone());
                }
            }
            vec![Update::Tool(call)]
        }
        Some("system_message") => reported(s),
        Some("user_input") => {
            if st["state"] != "DONE" {
                return Vec::new();
            }
            let Some(input) = s.inputs.get(s.taken) else { return Vec::new() };
            let steered = input.steer.then(|| Update::Steered(input.text.clone()));
            s.taken += 1;
            steered.into_iter().collect()
        }
        other => {
            log_unknown("agy", other.unwrap_or("step_update without a type"));
            Vec::new()
        }
    }
}

/// Subagents dispatched this turn have reported back.
fn reported(s: &mut State) -> Vec<Update> {
    let rows: Vec<String> = std::mem::take(&mut s.dispatched);
    rows.iter()
        .filter_map(|id| s.tools.remove(id))
        .map(|mut call| {
            finish(&mut call, "", false);
            halted(&mut call, false, s.cancelled);
            Update::Tool(call)
        })
        .collect()
}

/// What a `run_command` result says when the command went to the background, then its task id.
/// Neither phrase has appeared in a recorded stream: print mode ran `sleep 15` to completion
/// both times (fixtures/agy/background-run). Both come from the agy 1.4.3 binary.
const BACKGROUND_MARKS: [&str; 2] =
    ["Background command ID: ", "Tool is running as a background task with task id: "];

fn background_task(output: &str) -> Option<&str> {
    let head = output.split("Command output:").next()?;
    BACKGROUND_MARKS.iter().find_map(|m| head.split_once(m)?.1.split_whitespace().next())
}

/// The row, then a background row this step settled.
fn tool(s: &mut State, st: &Value) -> Vec<ToolCall> {
    let id = format!("{}:{}", s.conversation, st["step_index"]);
    let info = &st["tool_info"];
    let name = st["tool_name"].as_str().or(info["name"].as_str()).unwrap_or_default();
    let mut call = s.tools.remove(&id).unwrap_or_else(|| {
        let p = &info["parameters"];
        let (name, input) = match (name, p["ToolName"].as_str()) {
            ("call_mcp_tool", Some(tool)) if p["ServerName"] == "aether" => {
                (tool, p["Arguments"].clone())
            }
            _ => tool_input(name, p),
        };
        start(id.clone(), name, &input)
    });
    s.text_step = None;
    let output = info["output"].as_str().filter(|o| !o.is_empty());
    let mut out = Vec::new();
    match st["state"].as_str() {
        Some("ACTIVE") | None => {
            if let Some(output) = output {
                set_output(&mut call.detail, output);
            }
            s.tools.insert(id, call.clone());
        }
        Some(state) => {
            finish(&mut call, output.or(st["error"].as_str()).unwrap_or_default(), state != "DONE");
            halted(&mut call, false, s.cancelled);
            call.ms = st["duration_seconds"].as_f64().map(|d| (d * 1000.0) as u64);
            let managing = matches!(name, "manage_task" | "command_status" | "send_command_input");
            if output.is_none() && !managing {
                s.blocked.push((name.to_string(), call.clone()));
            }
            if state == "DONE" {
                let task =
                    (name == "run_command").then(|| output.and_then(background_task)).flatten();
                if let Some(task) = task {
                    call.status = ToolStatus::Background;
                    s.background.insert(task.to_string(), id.clone());
                    s.tools.insert(id, call.clone());
                } else if managing {
                    out.extend(settle_background(s, &info["parameters"], output.unwrap_or("")));
                }
            }
        }
    }
    out.insert(0, call);
    out
}

/// A status, input or kill step updates the row of the background task it names.
fn settle_background(s: &mut State, p: &Value, output: &str) -> Option<ToolCall> {
    let task = p["TaskId"].as_str().or(p["CommandId"].as_str())?;
    let row = s.background.get(task)?.clone();
    let call = s.tools.get_mut(&row)?;
    if !output.is_empty() {
        set_output(&mut call.detail, output);
    }
    let code = output
        .split_once("exited with code ")
        .and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit() && c != '-').next())
        .and_then(|n| n.parse::<i32>().ok());
    let status = if p["Action"] == "kill" || p["Terminate"] == true {
        Some(ToolStatus::Stopped)
    } else if output
        .split("Log output:")
        .next()
        .is_some_and(|h| h.lines().any(|l| l == "Status: DONE"))
    {
        Some(ToolStatus::Done)
    } else {
        code.map(|c| if c == 0 { ToolStatus::Done } else { ToolStatus::Failed })
    };
    if let (ToolDetail::Command { exit_code, .. }, Some(c)) = (&mut call.detail, code) {
        *exit_code = Some(c);
    }
    let Some(status) = status else { return Some(call.clone()) };
    call.status = status;
    s.background.remove(task);
    s.tools.remove(&row)
}

/// agy tool name and parameters as the shared names `tool::classify` types.
fn tool_input<'a>(name: &'a str, p: &Value) -> (&'a str, Value) {
    match name {
        "run_command" => ("bash", json!({"command": p["CommandLine"], "cwd": p["Cwd"]})),
        "view_file" | "read_file" => {
            let limit = p["StartLine"]
                .as_u64()
                .zip(p["EndLine"].as_u64())
                .map(|(a, b)| (b + 1).saturating_sub(a));
            (
                "read",
                json!({"file_path": p["AbsolutePath"].as_str().or(p["Path"].as_str()), "offset": p["StartLine"], "limit": limit}),
            )
        }
        "invoke_subagent" => {
            let subs = p["Subagents"].as_array().map_or(std::slice::from_ref(p), Vec::as_slice);
            let text = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
            let names: Vec<String> = subs.iter().map(|a| text(a, "TypeName")).collect();
            let first = subs.first().unwrap_or(&Value::Null);
            let description = match subs.len() {
                0 | 1 => Some(text(first, "Role"))
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| names.concat()),
                n => format!("{n} subagents: {}", names.join(", ")),
            };
            (
                "agent",
                json!({"description": description, "prompt": first["Prompt"], "subagent_type": first["TypeName"]}),
            )
        }
        "browser_subagent" => (
            "agent",
            json!({
                "description": p["TaskName"].as_str().filter(|n| !n.is_empty()).or(p["TaskSummary"].as_str()),
                "prompt": p["Task"],
                "subagent_type": "browser",
            }),
        ),
        "write_to_file" | "write_file" => (
            "write",
            json!({"file_path": p["TargetFile"].as_str().or(p["Path"].as_str()), "content": p["CodeContent"]}),
        ),
        "replace_file_content" => (
            "edit",
            json!({
                "file_path": p["TargetFile"],
                "old_string": p["TargetContent"],
                "new_string": p["ReplacementContent"],
            }),
        ),
        "multi_replace_file_content" => {
            let edits: Vec<Value> = p["ReplacementChunks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| {
                    json!({"old_string": c["TargetContent"], "new_string": c["ReplacementContent"]})
                })
                .collect();
            ("edit", json!({"file_path": p["TargetFile"], "edits": edits}))
        }
        "grep_search" => ("grep", json!({"pattern": p["Query"], "path": p["SearchPath"]})),
        "find_by_name" => ("glob", json!({"pattern": p["Pattern"], "path": p["SearchDirectory"]})),
        "list_dir" => ("ls", json!({"path": p["DirectoryPath"]})),
        "read_url_content" => ("webfetch", json!({"url": p["Url"]})),
        "search_web" => {
            ("websearch", json!({"query": p["query"].as_str().or(p["Query"].as_str())}))
        }
        _ => (name, p.clone()),
    }
}

/// `run_command` to the `RunCommand` agy shows in `denied_actions`.
fn pascal(tool: &str) -> String {
    tool.split('_')
        .map(|w| {
            w.chars()
                .next()
                .map_or_else(String::new, |c| c.to_uppercase().chain(w.chars().skip(1)).collect())
        })
        .collect()
}

fn retry_prompt(blocked: &[String]) -> String {
    format!(
        "The user approved the blocked actions:\n{}\nRetry them and continue.",
        blocked.join("\n")
    )
}

fn result(s: &mut State, r: &Value) -> Vec<Update> {
    let mut out = reported(s);
    let response = r["response"].as_str().unwrap_or_default();
    if !std::mem::take(&mut s.texted) && !response.is_empty() {
        out.push(Update::Delta(response.to_string()));
    }
    s.inputs.pop_front();
    s.taken = s.taken.saturating_sub(1);
    let denied = r["denied_actions"].as_array().filter(|d| !d.is_empty());
    let mut lines = Vec::new();
    let mut approval = Approval {
        tool: "Antigravity".into(),
        title: "Rerun with full access?".into(),
        reason: "agy blocked these actions. Allowing reruns this turn with full permissions: every action in it runs without asking, not only these.".into(),
        ..Approval::default()
    };
    if let Some(denied) = denied {
        let names: Vec<&str> =
            denied.iter().map(|a| a["display_name"].as_str().unwrap_or_default()).collect();
        let mut hit: Vec<Option<usize>> = vec![None; names.len()];
        let free = |hit: &[Option<usize>], i: usize| !hit.contains(&Some(i));
        for (n, name) in names.iter().enumerate() {
            hit[n] = (0..s.blocked.len())
                .rev()
                .find(|&i| free(&hit, i) && pascal(&s.blocked[i].0) == *name);
        }
        for n in 0..names.len() {
            if hit[n].is_none() {
                hit[n] = (0..s.blocked.len()).rev().find(|&i| free(&hit, i));
            }
        }
        let mut rows: Vec<usize> = hit.iter().flatten().copied().collect();
        rows.sort_unstable();
        let mut marked: Vec<ToolCall> = rows.iter().rev().map(|&i| s.blocked.remove(i).1).collect();
        marked.reverse();
        approval.tool_id = marked.first().map(|c| c.id.clone());
        let command = |c: &ToolCall| matches!(c.detail, ToolDetail::Command { .. });
        if !marked.is_empty() && marked.iter().all(command) {
            approval.kind = ApprovalKind::Command;
        }
        for mut call in marked {
            call.status = ToolStatus::Denied;
            lines.push(match &call.detail {
                ToolDetail::Command { command, .. } => command.clone(),
                ToolDetail::Edit { path, .. } => path.clone(),
                d => d.summary(),
            });
            out.push(Update::Tool(call));
        }
        lines.extend(
            names.iter().zip(&hit).filter(|(_, h)| h.is_none()).map(|(n, _)| n.to_string()),
        );
        // a steer's input may still be pending: the card waits for the turn's last result
        match &mut s.rerun {
            Some((first, all)) => {
                if first.kind != approval.kind {
                    first.kind = ApprovalKind::default();
                }
                all.extend(lines);
            }
            None => s.rerun = Some((approval, lines)),
        }
    }
    if r["status"] == "ERROR" && !s.cancelled {
        let error = r["error"].as_str().filter(|e| !e.is_empty()).unwrap_or("agy run failed");
        out.push(Update::Error(error.to_string()));
    }
    if s.cancelled || s.inputs.is_empty() {
        if let Some((approval, lines)) = s.rerun.take().filter(|_| !s.full && !s.cancelled) {
            s.next_ask += 1;
            out.push(Update::Ask {
                id: s.next_ask,
                ask: Ask::Approval(Approval { input: lines.join("\n"), ..approval }),
            });
            s.ask = Some((s.next_ask, lines));
            return out;
        }
        s.active = false;
        out.push(Update::Final(String::new()));
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/antigravity.rs"]
mod tests;
