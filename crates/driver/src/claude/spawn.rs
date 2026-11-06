use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

use aether_core::{
    AccessMode, Attachment, MutexExt, Notice, NoticeKind, NoticeState, Provider, Update, home_dir,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::DriverStartOptions;
use crate::record::{self};
use crate::spawn::{StderrTail, command, spawn_logged};
use crate::version::gate_binary;

use super::events::map_line;
use super::{ClaudeDriver, DIALOG_KINDS, INIT_REQUEST, State, on_exit};

impl ClaudeDriver {
    pub fn spawn(opts: DriverStartOptions, tx: UnboundedSender<Update>) -> Self {
        let mut me = Self {
            child: Arc::default(),
            stdin: Arc::default(),
            tx,
            state: Arc::default(),
            resumed: false,
            titled: false,
            next_request: 0,
            effort: opts.effort.clone(),
            rec: record::from_env("claude", &opts.cwd),
        };
        if let Err(why) = gate_binary(Provider::Claude, "claude", &opts, &me.tx) {
            me.die(why);
            return me;
        }
        let (mut args, mut id, mut resumed, note) = launch_args(&opts);
        let mut note = note;
        for attempt in 0..2 {
            me.notice(Notice {
                state: NoticeState::Running,
                ..Notice::new("startup-spawn", NoticeKind::Startup, "Starting claude")
            });
            let session = if resumed { "Resuming session" } else { "Starting session" };
            me.notice(Notice {
                state: NoticeState::Running,
                ..Notice::new("startup-session", NoticeKind::Startup, session)
            });
            let Some(mut cmd) = command(&opts, "claude") else {
                me.die("claude is not installed; install it or set an explicit binary path".into());
                return me;
            };
            cmd.args(&args).stdin(Stdio::piped()).stdout(Stdio::piped());
            let child = match spawn_logged(&mut cmd) {
                Ok(child) => child,
                Err(e) => {
                    me.die(format!("claude failed to start: {e}"));
                    return me;
                }
            };
            me.resumed = resumed;
            me.titled = resumed;
            // fresh Arc: the first attempt's reader thread still holds the old one
            me.state = Arc::new(Mutex::new(State {
                requested: opts.access,
                attachments: opts.attachments.clone(),
                session: id.clone(),
                chosen: opts.model.clone(),
                ..State::default()
            }));
            let _ = me.tx.unbounded_send(Update::Session(id.clone()));
            if let Some(n) = note.take() {
                me.notice(n);
            }
            let reply = me.launch(child);
            me.write(initialize_request());
            match reply.recv_timeout(Duration::from_secs(30)) {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    let why = format!(
                        "Claude Code didn't accept Aether's handshake ({e}). Subagent activity and progress are off."
                    );
                    let _ = me.tx.unbounded_send(Update::Warning(why));
                }
                Err(RecvTimeoutError::Timeout) => {
                    let why = "Claude Code didn't answer Aether's handshake. Subagent activity and progress are off.";
                    let _ = me.tx.unbounded_send(Update::Warning(why.into()));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let fix = resume_fix(&me.state.guard().stderr);
                    match fix {
                        Some(fix) if attempt == 0 => {
                            me.stop_child();
                            (args, id, resumed, note) = apply_fix(args, fix);
                            continue;
                        }
                        _ => {
                            me.startup(NoticeState::Failed, "Claude didn't start", session);
                            return me;
                        }
                    }
                }
            }
            me.startup(NoticeState::Done, "Started claude", session);
            break;
        }
        me
    }

    fn startup(&self, state: NoticeState, spawn: &str, session: &str) {
        for (id, text) in [("startup-spawn", spawn), ("startup-session", session)] {
            self.notice(Notice { state, ..Notice::new(id, NoticeKind::Startup, text) });
        }
    }

    fn notice(&self, n: Notice) {
        let _ = self.tx.unbounded_send(Update::Notice(n));
    }

    fn stop_child(&self) {
        self.stdin.guard().take();
        if let Some(mut child) = self.child.guard().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Wires the child's pipes; the result carries the `initialize` reply, and is dropped
    /// once stdout has closed without one.
    fn launch(&mut self, mut child: Child) -> Receiver<Result<(), String>> {
        *self.stdin.guard() = child.stdin.take();
        let (stderr_done, stderr_wait) = channel::<()>();
        if let Some(stderr) = child.stderr.take() {
            let state = self.state.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    state.guard().stderr.push(&line);
                }
                drop(stderr_done);
            });
        }
        let (init_tx, init_rx) = channel();
        if let Some(stdout) = child.stdout.take() {
            let (tx, state) = (self.tx.clone(), self.state.clone());
            let (stdin, child, rec) = (self.stdin.clone(), self.child.clone(), self.rec.clone());
            let mut init_tx: Option<Sender<Result<(), String>>> = Some(init_tx);
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(rec) = &rec {
                        rec.line(&line);
                    }
                    if init_tx.is_some()
                        && let Ok(v) = serde_json::from_str::<Value>(&line)
                        && v["response"]["request_id"] == INIT_REQUEST
                    {
                        let r = &v["response"];
                        let reply = match r["subtype"].as_str() {
                            Some("error") => {
                                Err(r["error"].as_str().unwrap_or("error").to_string())
                            }
                            _ => Ok(()),
                        };
                        let _ = init_tx.take().unwrap().send(reply);
                        continue;
                    }
                    let mut s = state.guard();
                    if s.dead.is_some() {
                        return;
                    }
                    for u in map_line(&mut s, &line) {
                        let _ = tx.unbounded_send(u);
                    }
                    let outbox = std::mem::take(&mut s.outbox);
                    drop(s);
                    for v in outbox {
                        send(&stdin, &v);
                    }
                }
                let _ = stderr_wait.recv_timeout(Duration::from_secs(1));
                // spawn reads the stderr tail once this drops, so it has to come after the wait
                drop(init_tx);
                let status = (0..10).find_map(|_| {
                    let status = child.guard().as_mut()?.try_wait().ok().flatten();
                    if status.is_none() {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    status
                });
                let mut s = state.guard();
                if s.dead.is_some() {
                    return;
                }
                let why = exit_error(status, &s.stderr);
                for u in on_exit(&mut s, why) {
                    let _ = tx.unbounded_send(u);
                }
            });
        }
        *self.child.guard() = Some(child);
        init_rx
    }

    fn die(&self, why: String) {
        self.state.guard().dead = Some(why);
    }

    pub(crate) fn write(&self, v: Value) {
        send(&self.stdin, &v);
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

pub(crate) fn exit_error(status: Option<ExitStatus>, tail: &StderrTail) -> String {
    tail.fatal(&match status {
        Some(status) => format!("claude exited ({status})"),
        None => "claude exited".to_string(),
    })
}

pub(crate) fn initialize_request() -> Value {
    json!({
        "type": "control_request",
        "request_id": INIT_REQUEST,
        "request": {
            "subtype": "initialize",
            "sdkForwardNestedSubagentToolCalls": true,
            "agentProgressSummaries": true,
            "forwardSubagentText": true,
            "supportedDialogKinds": DIALOG_KINDS,
        },
    })
}

pub(crate) enum ResumeFix {
    Fresh(String),
    Fork,
}

pub(crate) fn resume_fix(stderr: &StderrTail) -> Option<ResumeFix> {
    let tail = stderr.fatal("");
    let has = |what: &str| tail.contains(what);
    if has("No conversation found") {
        Some(ResumeFix::Fresh("it no longer exists".into()))
    } else if has("--fork-session") || has("running in the background") {
        Some(ResumeFix::Fork)
    } else {
        None
    }
}

/// Args for the one respawn, the session id, whether it resumes, and the notice to show.
pub(crate) fn apply_fix(
    mut args: Vec<String>,
    fix: ResumeFix,
) -> (Vec<String>, String, bool, Option<Notice>) {
    let at = args.iter().position(|a| a == "--resume").unwrap_or(args.len() - 2);
    match fix {
        ResumeFix::Fresh(reason) => {
            let id = new_uuid();
            args.splice(at..at + 2, ["--session-id".to_string(), id.clone()]);
            let text =
                format!("Couldn't resume the Claude session because {reason}. Started a new one.");
            (args, id, false, Some(Notice::new("info-resume", NoticeKind::Info, text)))
        }
        ResumeFix::Fork => {
            let id = args[at + 1].clone();
            args.push("--fork-session".into());
            let text = "This Claude session was open elsewhere, so the chat continues in a fork.";
            (args, id, true, Some(Notice::new("info-resume", NoticeKind::Info, text)))
        }
    }
}

pub(crate) fn lost_resume(requested: &str, resumed: bool) -> Option<Notice> {
    if !requested.is_empty() && !resumed {
        Some(Notice::new(
            "info-resume",
            NoticeKind::Info,
            "Couldn't find the earlier Claude session, so this is a new one.",
        ))
    } else {
        None
    }
}

/// CLI args, session id, whether that id resumes a stored session, and any lost-resume notice.
pub(crate) fn launch_args(
    opts: &DriverStartOptions,
) -> (Vec<String>, String, bool, Option<Notice>) {
    let mode = match opts.access {
        AccessMode::AutoEdit => "acceptEdits",
        AccessMode::Auto => "auto",
        AccessMode::Full => "bypassPermissions",
        AccessMode::Ask => "manual",
        AccessMode::ReadOnly => "plan",
    };
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--replay-user-messages",
        "--thinking-display",
        "summarized",
        "--include-hook-events",
        "--permission-prompts",
        "host",
        "--permission-mode",
        mode,
    ]
    .map(String::from)
    .into();
    if opts.access == AccessMode::Full {
        args.push("--dangerously-skip-permissions".into());
    }
    for (flag, value) in [("--model", &opts.model), ("--effort", &opts.effort)] {
        if !value.is_empty() {
            args.extend([flag.to_string(), value.clone()]);
        }
    }
    if let Some(dir) = aether_core::attachments_dir() {
        args.extend(["--add-dir".into(), dir.display().to_string()]);
    }
    if let Some(lead) = &opts.lead {
        match write_mcp_config(lead) {
            Ok(path) => args.extend([
                "--mcp-config".into(),
                path.display().to_string(),
                "--allowedTools".into(),
                "mcp__aether".into(),
            ]),
            Err(e) => eprintln!("aether: lead tools unavailable: {e}"),
        }
    }
    let resumed = !opts.resume.is_empty() && session_exists(&opts.resume);
    let note = lost_resume(&opts.resume, resumed);
    let id = if resumed { opts.resume.clone() } else { new_uuid() };
    args.extend([if resumed { "--resume" } else { "--session-id" }.to_string(), id.clone()]);
    (args, id, resumed, note)
}

pub(crate) fn write_mcp_config(lead: &crate::LeadTools) -> std::io::Result<PathBuf> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    lead.token.hash(&mut h);
    let path = std::env::temp_dir().join(format!("aether-mcp-{:016x}.json", h.finish()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    opts.open(&path)?.write_all(crate::mcp_config_json(lead).as_bytes())?;
    Ok(path)
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude"))
}

pub(crate) fn session_exists(id: &str) -> bool {
    std::fs::read_dir(config_dir().join("projects"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|d| d.path().join(format!("{id}.jsonl")).is_file())
}

pub(crate) fn new_uuid() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    b[6] = b[6] & 0x0f | 0x40;
    b[8] = b[8] & 0x3f | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

pub(crate) fn user_message(text: &str, images: &[Attachment]) -> Value {
    let mut content: Vec<Value> = images
        .iter()
        .filter_map(|f| {
            let data = STANDARD.encode(std::fs::read(&f.path).ok()?);
            let source = json!({"type": "base64", "media_type": f.mime, "data": data});
            Some(json!({"type": "image", "source": source}))
        })
        .collect();
    // the API rejects an empty text block, which an image-only message would send
    if !text.is_empty() || content.is_empty() {
        content.insert(0, json!({"type": "text", "text": text}));
    }
    json!({
        "type": "user",
        "message": {"role": "user", "content": content},
        "parent_tool_use_id": null,
    })
}
