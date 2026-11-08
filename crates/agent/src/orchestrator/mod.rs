pub mod mcp;
pub(crate) mod server;

use std::io::{BufRead, BufReader, Read, Write};
#[cfg(windows)]
use std::net::TcpStream as Stream;
#[cfg(unix)]
use std::os::unix::net::UnixStream as Stream;
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

// Windows has no unix sockets: the "socket" file holds the loopback address instead.
pub(crate) fn connect(sock: &Path) -> std::io::Result<Stream> {
    #[cfg(unix)]
    return Stream::connect(sock);
    #[cfg(windows)]
    return Stream::connect(std::fs::read_to_string(sock)?.trim());
}

pub fn socket_path() -> Option<PathBuf> {
    Some(aether_core::config_dir()?.join("orchestrator.sock"))
}

fn chat_filter(about: &str) -> Value {
    json!({
        "type": "object",
        "description": about,
        "properties": {
            "ids": {"type": "array", "items": {"type": "integer"}, "description": "Only these chat ids (from list_chats); then any project unless project is set"},
            "project": {"type": "string", "description": "Project folder name or path; \"all\" for every project. Default: your own project"},
            "older_than": {"type": "string", "description": "Last activity at least this long ago: a number and m, h, d or w, like \"30m\", \"12h\", \"3d\""},
            "state": {"type": "string", "description": "One of: running, done (last reply succeeded), failed (last reply errored), empty, not_running"},
            "provider": {"type": "string", "description": "One of: claude, codex, pi, opencode, antigravity, openrouter"},
            "archived": {"type": "boolean", "description": "Match archived chats instead of active ones"},
            "include_pinned": {"type": "boolean", "description": "Also act on pinned chats (skipped by default)"}
        }
    })
}

fn chat_id() -> Value {
    json!({"type": "integer", "description": "Chat id from the list_chats tool"})
}

pub fn tool_specs() -> Value {
    let bulk_note = "Never touches you, running chats, or pinned chats unless include_pinned. The user must confirm in Aether; this returns at once and the outcome arrives as your next turn, so don't poll.";
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
            "description": "List this project's open task-board cards, one line each.",
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
        },
        {
            "name": "delete_cards",
            "description": "Permanently delete task-board cards matching a filter (linked chats stay). The user must confirm in Aether; this returns at once and the outcome arrives as your next turn, so don't poll.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "filter": {
                        "type": "object",
                        "description": "Which cards; all fields optional",
                        "properties": {
                            "ids": {"type": "array", "items": {"type": "integer"}, "description": "Only these card ids; then any project unless project is set"},
                            "col": {"type": "string", "description": "One of: backlog, todo, doing, review, done"},
                            "project": {"type": "string", "description": "Project folder name or path; \"all\" for every project. Default: your own project"},
                            "added_by": {"type": "string", "description": "agent or user"},
                            "search": {"type": "string", "description": "Words that must all appear in the title or body"}
                        }
                    }
                },
                "required": ["filter"]
            }
        },
        {
            "name": "read_history",
            "description": "Read earlier messages of this chat in full, with the tool calls and output a context handoff leaves out. Messages are numbered from 0, oldest first, as in the handed-over context; this still works after your own context was compacted.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "offset": {"type": "integer", "description": "First message number (default 0)"},
                    "limit": {"type": "integer", "description": "How many messages, 1 to 20 (default 5)"},
                    "max_chars": {"type": "integer", "description": "Cap per message, 500 to 20000 (default 4000)"}
                }
            }
        },
        {
            "name": "list_chats",
            "description": "List Aether chats matching a filter, with id, title, project, provider, state and last activity.",
            "inputSchema": {"type": "object", "properties": {"filter": chat_filter("Which chats; all fields optional")}}
        },
        {
            "name": "archive_chats",
            "description": format!("Archive the chats matching a filter (undo with unarchive_chats). {bulk_note}"),
            "inputSchema": {"type": "object", "properties": {"filter": chat_filter("Which chats to archive")}, "required": ["filter"]}
        },
        {
            "name": "delete_chats",
            "description": format!("Permanently delete the chats matching a filter, with their history. {bulk_note}"),
            "inputSchema": {"type": "object", "properties": {"filter": chat_filter("Which chats to delete")}, "required": ["filter"]}
        },
        {
            "name": "unarchive_chats",
            "description": "Move archived chats matching a filter back to the sidebar.",
            "inputSchema": {"type": "object", "properties": {"filter": chat_filter("Which archived chats; archived defaults to true here")}, "required": ["filter"]}
        },
        {
            "name": "update_chat",
            "description": "Rename a chat or pin/unpin it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": chat_id(),
                    "title": {"type": "string", "description": "New title"},
                    "pinned": {"type": "boolean", "description": "Pin (true) or unpin (false)"}
                },
                "required": ["id"]
            }
        },
        {
            "name": "stop_chat",
            "description": "Stop any chat's current turn (not your own).",
            "inputSchema": {"type": "object", "properties": {"id": chat_id()}, "required": ["id"]}
        },
        {
            "name": "open_chat",
            "description": "Show a chat to the user in Aether.",
            "inputSchema": {"type": "object", "properties": {"id": chat_id()}, "required": ["id"]}
        }
    ])
}

/// What a chat that isn't a lead gets: only read_history, for its handed-over context.
pub fn history_specs() -> Value {
    let specs = tool_specs();
    Value::Array(
        specs
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["name"] == "read_history")
            .cloned()
            .collect(),
    )
}

pub fn forward(sock: &Path, token: &str, tool: &str, args: Value) -> Result<Value, String> {
    let mut stream = connect(sock).map_err(|_| "Aether isn't running".to_string())?;
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
