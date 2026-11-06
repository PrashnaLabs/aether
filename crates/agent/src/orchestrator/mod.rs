pub mod mcp;
pub(crate) mod server;

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

/// In-flight orchestrator calls awaiting the UI; past this the UI is wedged
/// and senders get a busy error instead of queueing behind a 30s wait.
pub(crate) const MAX_PENDING: usize = 16;
pub type OrchPending = Arc<AtomicUsize>;
pub(crate) const BUSY: &str = "Aether is busy; retry shortly";

pub(crate) fn try_queue(
    calls: &UnboundedSender<OrchCall>,
    pending: &OrchPending,
    call: OrchCall,
) -> Result<(), String> {
    let queued = pending
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n < MAX_PENDING).then_some(n + 1))
        .is_ok();
    if !queued {
        return Err(BUSY.to_string());
    }
    if calls.unbounded_send(call).is_err() {
        done_pending(pending);
        return Err("Aether didn't answer".to_string());
    }
    Ok(())
}

pub fn done_pending(pending: &OrchPending) {
    let _ = pending.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n > 0).then_some(n - 1));
}

pub struct OrchCall {
    pub lead: String,
    pub tool: String,
    pub args: Value,
    pub reply: std::sync::mpsc::Sender<Result<Value, String>>,
}

pub const SOCK_ENV: &str = "AETHER_ORCH_SOCK";
pub const TOKEN_ENV: &str = "AETHER_ORCH_TOKEN";

pub fn socket_path() -> Option<PathBuf> {
    Some(aether_core::config_dir()?.join("orchestrator.sock"))
}

pub fn tool_specs() -> Value {
    json!([
        {
            "name": "list_agents",
            "description": "List the agent chats in this Aether workspace.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "spawn_agent",
            "description": "Start a new agent chat on a task (limited running agents per lead, configured in Aether settings). Its result is sent to you automatically when it finishes; don't poll read_agent or sleep.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "provider": {"type": "string", "description": "One of: claude, codex, pi, opencode, antigravity"},
                    "model": {"type": "string", "description": "Model id for the provider"},
                    "title": {"type": "string", "description": "Short chat title"},
                    "task": {"type": "string", "description": "What the agent should do"},
                    "effort": {"type": "string", "description": "Optional reasoning effort"},
                    "worktree": {"type": "boolean", "description": "Own git branch + folder off the lead's current branch; lead's uncommitted changes aren't included"},
                    "nonce": {"type": "string", "description": "Idempotency key for this spawn; retrying with the same key returns the existing child instead of creating a second"},
                    "card": {"type": "integer", "description": "Optional board card id to link; omitted adds a card for this agent"}
                },
                "required": ["provider", "model", "title", "task"]
            }
        },
        {
            "name": "read_agent",
            "description": "Read a child agent's status and latest replies. Use when you need a child's output early; finished results arrive automatically.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "description": "Child agent id from the list_agents tool"},
                    "last": {"type": "integer", "description": "How many recent replies, 1 to 10 (default 1)"}
                },
                "required": ["id"]
            }
        },
        {
            "name": "message_agent",
            "description": "Send a message to a child agent; queued if it is busy.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "description": "Child agent id from the list_agents tool"},
                    "text": {"type": "string", "description": "Message to send"}
                },
                "required": ["id", "text"]
            }
        },
        {
            "name": "stop_agent",
            "description": "Stop a child agent's current turn.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "description": "Child agent id from the list_agents tool"}
                },
                "required": ["id"]
            }
        },
        {
            "name": "list_cards",
            "description": "List open task-board cards, one line each.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "add_card",
            "description": "Add a card to the task board (default column todo).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "Short card title"},
                    "body": {"type": "string", "description": "Optional details"},
                    "col": {"type": "string", "description": "One of: backlog, todo, doing, review"}
                },
                "required": ["title"]
            }
        },
        {
            "name": "move_card",
            "description": "Move a task-board card to backlog, todo, doing or review.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "description": "Card id from the list_cards tool"},
                    "col": {"type": "string", "description": "One of: backlog, todo, doing, review"}
                },
                "required": ["id", "col"]
            }
        }
    ])
}

pub fn forward(sock: &Path, token: &str, tool: &str, args: Value) -> Result<Value, String> {
    let mut stream = UnixStream::connect(sock).map_err(|_| "Aether isn't running".to_string())?;
    let timeout = Duration::from_secs(35);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let silent = || "Aether didn't answer".to_string();
    let req = json!({"token": token, "tool": tool, "args": args});
    writeln!(stream, "{req}").map_err(|_| silent())?;
    let mut line = String::new();
    // replies (list_agents) can exceed the 64KiB request cap
    BufReader::new(stream).take(1024 * 1024 + 1).read_line(&mut line).map_err(|_| silent())?;
    if line.len() > 1024 * 1024 {
        return Err(silent());
    }
    let reply: Value = serde_json::from_str(&line).map_err(|_| silent())?;
    match reply["ok"].as_bool() {
        Some(true) => Ok(reply["result"].clone()),
        Some(false) => Err(reply["error"].as_str().unwrap_or("Aether didn't answer").to_string()),
        None => Err(silent()),
    }
}
