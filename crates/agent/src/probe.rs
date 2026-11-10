//! Asks a provider's CLI what it loads for a session, without a model call, and overlays the
//! answer on the disk scan. Blocking; run off the UI thread.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use aether_core::{Provider, Settings, find_binary, home_dir};
use aether_driver::DriverStartOptions;
use serde_json::{Value, json};

use crate::inventory::{Inventory, Item, Status};

const DEADLINE: Duration = Duration::from_secs(25);

/// Replaces the disk estimates in `inv` with what the CLI reports, where it can.
/// Claude Code gives exact parts; Codex and OpenCode give live MCP servers (and OpenCode agents).
pub fn probe(provider: Provider, settings: &Settings, cwd: &Path, inv: &mut Inventory) {
    let binary = || {
        let opts = DriverStartOptions::for_provider(settings, provider, home_dir());
        let name = match provider {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            _ => return None,
        };
        find_binary(name, &opts.binary)
    };
    match provider {
        Provider::Claude => {
            if let Some((ctx, mcp)) = binary().and_then(|b| claude(&b, cwd)) {
                *inv = claude_inventory(&ctx, &mcp, std::mem::take(&mut inv.mcp));
            }
        }
        Provider::Codex => {
            if let Some(list) = binary().and_then(|b| codex(&b, cwd)) {
                inv.mcp = codex_servers(&list);
            }
        }
        Provider::OpenCode => {
            if let Some((mcp, agents)) = opencode(cwd) {
                inv.mcp = opencode_servers(&mcp);
                inv.agents = opencode_agents(&agents);
            }
        }
        _ => {}
    }
}

struct Cli {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Value>,
    deadline: Instant,
}

impl Cli {
    fn spawn(binary: &Path, args: &[&str], cwd: &Path) -> Option<Self> {
        let mut child = aether_core::command(binary)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line)
                    && tx.send(v).is_err()
                {
                    return;
                }
            }
        });
        Some(Self { child, stdin, lines, deadline: Instant::now() + DEADLINE })
    }

    fn send(&mut self, v: Value) -> Option<()> {
        writeln!(self.stdin, "{v}").ok()?;
        self.stdin.flush().ok()
    }

    /// The first line `pick` accepts, skipping the rest, before the deadline.
    fn wait(&self, pick: impl Fn(&Value) -> Option<Value>) -> Option<Value> {
        loop {
            let left = self.deadline.checked_duration_since(Instant::now())?;
            if let Some(v) = pick(&self.lines.recv_timeout(left).ok()?) {
                return Some(v);
            }
        }
    }
}

impl Drop for Cli {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn claude_request(id: &str, subtype: &str) -> Value {
    json!({"type": "control_request", "request_id": id, "request": {"subtype": subtype}})
}

fn claude_reply(id: &'static str) -> impl Fn(&Value) -> Option<Value> {
    move |v| {
        let r = &v["response"];
        (v["type"] == "control_response" && r["request_id"] == id && r["subtype"] == "success")
            .then(|| r["response"].clone())
    }
}

/// `get_context_usage` and `mcp_status` from a fresh headless Claude Code in `cwd`. Waits for
/// pending MCP servers (claude.ai connectors arrive late) so their tools count.
fn claude(binary: &Path, cwd: &Path) -> Option<(Value, Value)> {
    let args = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
        "--settings",
        r#"{"disableAllHooks":true}"#,
    ];
    let mut cli = Cli::spawn(binary, &args, cwd)?;
    cli.send(claude_request("init", "initialize"))?;
    cli.wait(claude_reply("init"))?;
    // claude.ai connectors join the list ~1 s after initialize; asking sooner misses them
    std::thread::sleep(Duration::from_secs(2));
    let mut mcp;
    loop {
        cli.send(claude_request("mcp", "mcp_status"))?;
        mcp = cli.wait(claude_reply("mcp"))?;
        let pending = mcp["mcpServers"]
            .as_array()
            .is_some_and(|s| s.iter().any(|s| s["status"] == "pending"));
        let room = cli.deadline.saturating_duration_since(Instant::now()) > Duration::from_secs(5);
        if !pending || !room {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    cli.send(claude_request("ctx", "get_context_usage"))?;
    Some((cli.wait(claude_reply("ctx"))?, mcp))
}

fn claude_status(status: &str) -> Option<Status> {
    Some(match status {
        "connected" => Status::Connected,
        "pending" => Status::Pending,
        "needs-auth" => Status::NeedsAuth,
        "failed" => Status::Failed,
        "disabled" => Status::Disabled,
        _ => return None,
    })
}

/// MCP tool names carry the server name with every non-alphanumeric char as `_`.
fn tool_server(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

/// Builds the inventory from Claude Code's own report. `disk_mcp` lends config paths to servers.
pub(crate) fn claude_inventory(ctx: &Value, mcp: &Value, disk_mcp: Vec<Item>) -> Inventory {
    let items = |list: &Value, name: &str, path: &str| -> Vec<Item> {
        list.as_array()
            .into_iter()
            .flatten()
            .map(|v| Item {
                name: v[name].as_str().unwrap_or_default().to_string(),
                path: v[path].as_str().unwrap_or_default().into(),
                tokens: v["tokens"].as_u64().unwrap_or(0),
                status: None,
            })
            .collect()
    };
    let mut tools: HashMap<String, u64> = HashMap::new();
    for tool in ctx["mcpTools"].as_array().into_iter().flatten() {
        if tool["isLoaded"] != false {
            let server = tool["serverName"].as_str().unwrap_or_default().to_string();
            *tools.entry(server).or_default() += tool["tokens"].as_u64().unwrap_or(0);
        }
    }
    let disk: HashMap<String, Item> = disk_mcp.into_iter().map(|i| (i.name.clone(), i)).collect();
    let servers = mcp["mcpServers"].as_array().into_iter().flatten().map(|s| {
        let name = s["name"].as_str().unwrap_or_default().to_string();
        Item {
            tokens: tools.get(&tool_server(&name)).copied().unwrap_or(0),
            status: s["status"].as_str().and_then(claude_status),
            path: disk.get(&name).map(|i| i.path.clone()).unwrap_or_default(),
            name,
        }
    });
    let categories = ctx["categories"].as_array().into_iter().flatten();
    let kind = |c: &Value, k: &str| c["kind"] == k;
    Inventory {
        memory: items(&ctx["memoryFiles"], "path", "path"),
        mcp: servers.collect(),
        agents: items(&ctx["agents"], "agentType", "path"),
        skills: items(&ctx["skills"]["skillFrontmatter"], "name", "path"),
        parts: categories
            .clone()
            .filter(|c| kind(c, "used") && c["id"] != "messages")
            .map(|c| {
                let name = c["name"].as_str().unwrap_or_default().to_string();
                (name, c["tokens"].as_u64().unwrap_or(0))
            })
            .collect(),
        buffer: categories.filter(|c| kind(c, "buffer")).filter_map(|c| c["tokens"].as_u64()).sum(),
    }
}

/// `mcpServerStatus/list` from a fresh `codex app-server` in `cwd`.
fn codex(binary: &Path, cwd: &Path) -> Option<Value> {
    let mut cli = Cli::spawn(binary, &["app-server"], cwd)?;
    let client = json!({"name": "aether", "title": "Aether", "version": env!("CARGO_PKG_VERSION")});
    let init = json!({"clientInfo": client, "capabilities": {"experimentalApi": true}});
    cli.send(json!({"id": 1, "method": "initialize", "params": init}))?;
    cli.wait(|v| (v["id"] == 1).then(Value::default))?;
    cli.send(json!({"method": "initialized"}))?;
    let params = json!({"detail": "toolsAndAuthOnly"});
    cli.send(json!({"id": 2, "method": "mcpServerStatus/list", "params": params}))?;
    cli.wait(|v| (v["id"] == 2).then(|| v["result"].clone()))
}

pub(crate) fn codex_servers(list: &Value) -> Vec<Item> {
    list["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let status = match (s["runtimeStatus"].as_str(), s["authStatus"].as_str()) {
                (_, Some("notLoggedIn")) | (Some("authenticationRequired"), _) => {
                    Some(Status::NeedsAuth)
                }
                (Some("failed"), _) => Some(Status::Failed),
                (Some("disabled"), _) => Some(Status::Disabled),
                (Some("starting" | "notStarted"), _) => Some(Status::Pending),
                _ if s["toolsError"].is_string() => Some(Status::Failed),
                _ => Some(Status::Connected),
            };
            let schema = s["tools"]
                .as_object()
                .map_or(0, |t| t.values().map(|tool| tool.to_string().len() as u64).sum::<u64>());
            Item {
                name: s["name"].as_str().unwrap_or_default().to_string(),
                tokens: schema / 4,
                status,
                ..Item::default()
            }
        })
        .collect()
}

/// `/api/mcp` and `/api/agent` from Aether's running OpenCode service, for `cwd`.
fn opencode(cwd: &Path) -> Option<(Value, Value)> {
    let api = aether_driver::opencode::service_api()?;
    let get = |path: &str| -> Option<Value> {
        let path = aether_driver::opencode::located(path, &cwd.to_string_lossy());
        let (status, text) =
            api.send(reqwest::Method::GET, &path, None, Duration::from_secs(10)).ok()?;
        (200..300).contains(&status).then(|| serde_json::from_str(&text).ok())?
    };
    Some((get("/api/mcp")?, get("/api/agent")?))
}

pub(crate) fn opencode_servers(mcp: &Value) -> Vec<Item> {
    mcp["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| Item {
            name: s["name"].as_str().unwrap_or_default().to_string(),
            status: match s["status"]["status"].as_str() {
                Some("needs_auth") => Some(Status::NeedsAuth),
                Some(other) => claude_status(other),
                None => None,
            },
            ..Item::default()
        })
        .collect()
}

/// Subagents OpenCode offers the model; primary and hidden agents aren't in its context.
pub(crate) fn opencode_agents(agents: &Value) -> Vec<Item> {
    agents["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["mode"] != "primary" && a["hidden"] != true)
        .map(|a| {
            let name = a["id"].as_str().unwrap_or_default().to_string();
            let description = a["description"].as_str().unwrap_or_default();
            Item { tokens: (name.len() + description.len()) as u64 / 4, name, ..Item::default() }
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/probe.rs"]
mod tests;
