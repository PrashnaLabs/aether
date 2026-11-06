# Agent orchestration, phases 0 and 1: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Claude Code lead chat can call `list_agents` and `spawn_agent` and get a real child chat (any CLI harness) that runs its task, unfocused, with a saved parent link. Phase 0 records the exact per-harness flags every later adapter needs.

**Architecture:** The app listens on a Unix socket and answers one JSON line per tool call. `aether mcp` (the app binary in shim mode) speaks MCP on stdio and forwards calls to the socket with a per-lead token. A lead's Claude driver gets that shim through `--mcp-config`. Tool calls reach `ChatApp` over a channel, the same way `SideUpdate`s do, and the app creates and submits the child chat.

**Tech Stack:** Rust 2024, gpui (`gpui-kit`), `std::os::unix::net`, `serde_json`, `sqlx` sqlite. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-04-agent-orchestration-design.md`

## Global Constraints

- No new crates in any `Cargo.toml`.
- Socket: `<aether_core::config_dir()>/orchestrator.sock`, file mode `0600`, stale file removed before bind.
- MCP server name `aether`; Claude tool names are therefore `mcp__aether__list_agents` and `mcp__aether__spawn_agent`.
- Shim env vars: `AETHER_ORCH_SOCK`, `AETHER_ORCH_TOKEN`. A shim without a token lists zero tools.
- Child task suffix, exact copy: `\n\nWhen you finish, end your reply with a short summary of what you did and which files you changed.`
- Children start with only the task text. No lead history.
- Children never get the orchestration tools (depth 1).
- A child's access never exceeds its lead's (ceiling in Task 6, so phase 4 only adds worktrees and the cap).
- Tests live in `crates/<crate>/tests/unit/...` and are included with `#[cfg(test)] #[path = ...] mod tests;`, matching the repo.
- Every new `DriverStartOptions` literal needs `lead: None`.
- Baseline already red on main: `cargo fmt --check` and the `hackernews::client` flake. Don't chase them.
- Parallel worktrees need separate `CARGO_TARGET_DIR`s.
- Commits: Conventional Commits, no `Co-Authored-By` lines, and only once the user has approved.

## Review Focus

1. **Socket path too long** (macOS `sun_path` is 104 bytes, and a custom config dir can be long). Expected: bind fails, the error is logged once, the app still starts with orchestration off. Test in Task 4.
2. **Stale socket from a crashed run.** Expected: the next launch removes the file and binds. Test in Task 4.
3. **Shim run with no app listening, or without a token.** Expected: `tools/list` is empty with no token. A `tools/call` with no app running returns `isError: true` with text `Aether isn't running`. The shim never exits on a bad line. Tests in Task 3.
4. **Lead chat gone while a call is in flight** (deleted, or an unknown key). Expected: the error `lead chat not found`, never a hang or panic. The socket side times out after 30 s with `Aether didn't answer`. Tests in Tasks 4 and 6.
5. **Bad model arguments:** missing `task`, an unknown provider such as `"openrouter"` or `"gpt"`, or a model not in the probe's list. Expected: an error that names the fix, e.g. `unknown model "x" for codex; call list_agents`. No child is created. Tests in Task 6.

---

## File map

| File | Change |
|------|--------|
| `docs/wayfinder/research/orchestration-adapters.md` | new: phase 0 findings |
| `crates/driver/src/lib.rs` | `LeadTools`, `DriverStartOptions.lead` |
| `crates/driver/src/claude.rs` | MCP config file and launch flags when `lead` is set |
| `crates/agent/src/orchestrator/mod.rs` | new: `OrchCall`, `tool_specs`, socket path |
| `crates/agent/src/orchestrator/mcp.rs` | new: MCP line handler and stdio shim |
| `crates/agent/src/orchestrator/server.rs` | new: socket listener and token check |
| `crates/agent/src/agent.rs` | token map, listener start, `orchestrator_calls()`, `ChatSession.lead`, `SessionKey.lead` |
| `crates/agent/src/store.rs` | `parent`, `lead` columns and `SavedChat` fields |
| `crates/app/src/main.rs` | `mcp` first-argument branch |
| `crates/app/src/app/mod.rs` | `Chat.parent`, `Chat.lead`; `mod orchestrate;` |
| `crates/app/src/app/chat/{mod,create,send}.rs` | persist, restore and `cli_request` carry the new fields |
| `crates/app/src/app/orchestrate.rs` | new: `list_agents`, `spawn_agent`, call dispatch |
| `crates/app/src/app/new.rs` | loop that feeds `OrchCall`s to `ChatApp` |
| `crates/app/src/palette.rs` | "Lead: toggle for this chat" command |
| tests | `crates/driver/tests/unit/claude.rs`, `crates/agent/tests/unit/orchestrator/{mcp,server}.rs`, `crates/agent/tests/unit/store.rs`, `crates/app/tests/unit/app/orchestrate.rs` |

Order: Task 1 first. Tasks 2, 3 and 5 can then run in parallel. Task 4 needs 2 and 3, Task 6 needs 4 and 5, and Task 7 comes last.

---

### Task 1: Phase 0 spike, per-harness flags

Throwaway code goes under `/tmp/aether-orch-spike/` and is never committed. Only the research doc is committed. The five harness steps are independent, so one subagent per harness is fine. Each appends its own section.

**Files:**
- Create: `docs/wayfinder/research/orchestration-adapters.md`

**Interfaces:**
- Produces: for each harness, the exact launch args and env that make a stdio MCP server (or the Pi extension) visible in Aether's real launch mode; the exact tool names the model sees; and the approval behavior for those tools under each `AccessMode` the harness allows (`settings.rs:425`). Tasks 2 and phase 2b consume this.

- [ ] **Step 1: Build a dummy MCP server.** `/tmp/aether-orch-spike/echo_mcp.py`: a stdio JSON-RPC server answering `initialize`, `tools/list` (one tool `echo{text}`), and `tools/call`. It appends every request to `/tmp/aether-orch-spike/calls.log` and logs its env var `SPIKE_TOKEN`.
- [ ] **Step 2: Claude 2.1.318.** Use the args `launch_args` builds (`claude.rs:802`) plus `--mcp-config <file>` and `--allowedTools mcp__spike`. Send one stream-json user message asking it to call echo. Record whether the call happened, whether the server-level allow (`mcp__spike`) worked or per-tool names were needed, whether `env` reached the server, and whether all of this still holds after `--resume <id>`. Repeat for each permission mode: manual, acceptEdits, auto, plan, bypassPermissions.
- [ ] **Step 3: Codex 0.166.0.** `codex app-server -c 'mcp_servers.spike.command="python3"' -c 'mcp_servers.spike.args=["/tmp/aether-orch-spike/echo_mcp.py"]' -c 'mcp_servers.spike.env={SPIKE_TOKEN="t"}'`, then drive one `thread/start` and `turn/start` over JSON-RPC the way `codex.rs` does. Record the tool name the model sees, approval requests per approvalPolicy/sandbox row (`codex.rs:514`), and any per-tool approval config key.
- [ ] **Step 4: Pi.** Write a dummy extension `/tmp/aether-orch-spike/echo.ts` that registers `aether_echo` and logs `process.env.SPIKE_TOKEN`. Run `pi --mode rpc -e /tmp/aether-orch-spike/echo.ts` with and without the Read-only allowlist `--tools read,grep,find,ls,aether_echo`. Record whether the extension can open a Unix socket (`node:net`) and whether any approval appears.
- [ ] **Step 5: OpenCode 2.2.5.** Start a second `opencode serve --port <free>` (not `--service`) with `OPENCODE_CONFIG_CONTENT='{"mcp":{"spike":{"type":"local","command":["python3","/tmp/aether-orch-spike/echo_mcp.py"],"environment":{"SPIKE_TOKEN":"t"}}}}'`. Create a session over HTTP the way `opencode/session.rs` does and prompt it to call echo. Record the tool name, whether the shared `--service` instance is unaffected, and the permission events.
- [ ] **Step 6: Antigravity.** Try `agy -p --input-format stream-json --output-format stream-json --settings '<json with an MCP servers entry>'`, and look up the settings key in agy's docs or settings.json. If no session-only form works, try `agy mcp add spike python3 /tmp/aether-orch-spike/echo_mcp.py` (then `agy mcp remove spike` afterwards) and record whether the server inherits `SPIKE_TOKEN` from the agy process env. Record approvals per `--mode`.
- [ ] **Step 7: Write the doc.** One section per harness: working args and env (copy-pasteable), tool name seen, approval table, and caveats. Finish with a "Decision changes" list for anything that contradicts the spec's adapter table.
- [ ] **Step 8: Commit** `docs: record orchestration adapter flags per harness`.

---

### Task 2: Lead tools on the driver launch, Claude adapter

**Files:**
- Modify: `crates/driver/src/lib.rs:23-71` (struct and both `for_provider` returns)
- Modify: `crates/driver/src/claude.rs:802` (`launch_args`)
- Modify: every `DriverStartOptions {` literal (35 sites; `grep -rn "DriverStartOptions {" crates`) to add `lead: None`
- Test: `crates/driver/tests/unit/claude.rs`

**Interfaces:**
- Consumes: Task 1 Claude section. If it found the server-level allow doesn't work, use the per-tool `--allowedTools` list.
- Produces:
  - `pub struct LeadTools { pub exe: PathBuf, pub sock: PathBuf, pub token: String }` (`Clone, Debug, PartialEq`) in `aether_driver`.
  - `DriverStartOptions.lead: Option<LeadTools>`.
  - `pub fn mcp_config_json(lead: &LeadTools) -> String` in `aether_driver` (Codex and others reuse it in 2b).

- [ ] **Step 1: Write the failing tests** in `crates/driver/tests/unit/claude.rs`:

```rust
#[test]
fn lead_launch_adds_aether_mcp_server() {
    let lead = LeadTools { exe: "/x/aether".into(), sock: "/s/o.sock".into(), token: "tok123".into() };
    let opts = DriverStartOptions { lead: Some(lead), ..test_opts() };
    let (args, ..) = launch_args(&opts);
    let at = args.iter().position(|a| a == "--mcp-config").expect("mcp flag");
    let body = std::fs::read_to_string(&args[at + 1]).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let s = &v["mcpServers"]["aether"];
    assert_eq!(s["command"], "/x/aether");
    assert_eq!(s["args"], serde_json::json!(["mcp"]));
    assert_eq!(s["env"]["AETHER_ORCH_TOKEN"], "tok123");
    assert_eq!(s["env"]["AETHER_ORCH_SOCK"], "/s/o.sock");
    assert!(args.windows(2).any(|w| w[0] == "--allowedTools" && w[1].contains("mcp__aether")));
    assert!(!args.iter().any(|a| a.contains("tok123")), "token stays out of argv");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&args[at + 1]).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn non_lead_launch_has_no_mcp_flags() {
    let (args, ..) = launch_args(&test_opts());
    assert!(!args.iter().any(|a| a == "--mcp-config" || a == "--allowedTools"));
}
```

`test_opts()` is a local helper returning the literal used in `launch_args_let_claude_read_attachments` (`claude.rs` test :904). Add the helper if none exists.

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p aether-driver lead_launch` should fail to compile (`LeadTools` missing).
- [ ] **Step 3: Implement.**
  - Add `LeadTools` and the field, plus `lead: None` in both `for_provider` returns and all literals.
  - `mcp_config_json` builds `{"mcpServers":{"aether":{"command":exe,"args":["mcp"],"env":{"AETHER_ORCH_SOCK":sock,"AETHER_ORCH_TOKEN":token}}}}` with `serde_json::json!`.
  - In `launch_args`, when `opts.lead` is set, write that JSON to `std::env::temp_dir().join(format!("aether-mcp-{}.json", &token[..8]))`. Create it with `OpenOptions` plus `std::os::unix::fs::OpenOptionsExt::mode(0o600)` and truncate on rewrite. Then push `--mcp-config <path>` and `--allowedTools mcp__aether`.
  - A write failure logs `aether: lead tools unavailable: {e}` and skips both flags; the chat still runs.
- [ ] **Step 4: Run the tests and confirm they pass.** `cargo test -p aether-driver` should be green (the whole crate, since 35 literals changed).
- [ ] **Step 5: Commit** `feat(driver): pass aether mcp server to claude leads`.

---

### Task 3: Orchestrator protocol, MCP handler and shim

**Files:**
- Create: `crates/agent/src/orchestrator/mod.rs`, `crates/agent/src/orchestrator/mcp.rs`
- Modify: `crates/agent/src/lib.rs` (`pub mod orchestrator;`)
- Modify: `crates/app/src/main.rs:40` (the shim branch is the first statement of `main`, before `relaunch_in_dev_bundle`)
- Test: `crates/agent/tests/unit/orchestrator/mcp.rs`

**Interfaces:**
- Produces, in `aether_agent::orchestrator`:
  - `pub const SOCK_ENV: &str = "AETHER_ORCH_SOCK";` and `pub const TOKEN_ENV: &str = "AETHER_ORCH_TOKEN";`
  - `pub fn socket_path() -> Option<PathBuf>`, which is `config_dir()?.join("orchestrator.sock")`.
  - `pub fn tool_specs() -> serde_json::Value`, an array of MCP tool objects `{name, description, inputSchema}` for `list_agents` (no args) and `spawn_agent`. `spawn_agent` takes required `provider`, `model`, `title` and `task` (strings) and optional `effort`. Keep descriptions terse. The `provider` description lists the keys `claude, codex, pi, opencode, antigravity`.
  - Wire format, socket request line: `{"token":String,"tool":String,"args":Value}`. Response line: `{"ok":true,"result":Value}` or `{"ok":false,"error":String}`. One request per connection.
  - `pub fn forward(sock: &Path, token: &str, tool: &str, args: Value) -> Result<Value, String>`. It connects, writes one line, and reads one line. A connect failure returns `Err("Aether isn't running")`.
  - In `mcp`: `pub fn handle(line: &str, token: Option<&str>, call: &mut dyn FnMut(&str, Value) -> Result<Value, String>) -> Option<String>`, which returns the response line, or `None` for notifications. Also `pub fn run_stdio() -> i32`.

- [ ] **Step 1: Write the failing tests** in `crates/agent/tests/unit/orchestrator/mcp.rs`:

```rust
fn rpc(line: &str, token: Option<&str>, call: &mut dyn FnMut(&str, Value) -> Result<Value, String>) -> Value {
    serde_json::from_str(&handle(line, token, call).expect("a response")).unwrap()
}
fn never(_: &str, _: Value) -> Result<Value, String> { panic!("no call expected") }

#[test]
fn initialize_echoes_protocol_version_and_names_server() {
    let v = rpc(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#, Some("t"), &mut never);
    assert_eq!(v["id"], 1);
    assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(v["result"]["serverInfo"]["name"], "aether");
    assert!(v["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn notifications_get_no_reply() {
    assert!(handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, Some("t"), &mut never).is_none());
}

#[test]
fn tools_list_is_empty_without_token() {
    let v = rpc(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, None, &mut never);
    assert_eq!(v["result"]["tools"], serde_json::json!([]));
    let v = rpc(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#, Some("t"), &mut never);
    let names: Vec<_> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["list_agents", "spawn_agent"]);
}

#[test]
fn tools_call_forwards_and_wraps_result_as_text() {
    let mut seen = None;
    let v = rpc(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"spawn_agent","arguments":{"task":"x"}}}"#,
        Some("t"), &mut |tool, args| { seen = Some((tool.to_string(), args)); Ok(serde_json::json!({"id": 7})) });
    assert_eq!(seen.unwrap().0, "spawn_agent");
    assert_eq!(v["result"]["isError"], false);
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<Value>(text).unwrap()["id"], 7);
}

#[test]
fn tools_call_error_is_tool_error_not_rpc_error() {
    let v = rpc(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"list_agents","arguments":{}}}"#,
        Some("t"), &mut |_, _| Err("Aether isn't running".into()));
    assert_eq!(v["result"]["isError"], true);
    assert_eq!(v["result"]["content"][0]["text"], "Aether isn't running");
}

#[test]
fn garbage_and_unknown_methods_answer_with_rpc_errors() {
    let v = rpc("not json", Some("t"), &mut never);
    assert_eq!(v["error"]["code"], -32700);
    let v = rpc(r#"{"jsonrpc":"2.0","id":6,"method":"nope"}"#, Some("t"), &mut never);
    assert_eq!(v["error"]["code"], -32601);
    let v = rpc(r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#, Some("t"), &mut never);
    assert_eq!(v["result"], serde_json::json!({}));
}

#[test]
fn forward_without_listener_says_aether_isnt_running() {
    let p = std::env::temp_dir().join(format!("aether-orch-none-{}.sock", std::process::id()));
    assert_eq!(forward(&p, "t", "list_agents", serde_json::json!({})).unwrap_err(), "Aether isn't running");
}
```

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p aether-agent orchestrator::mcp` should not compile (module missing).
- [ ] **Step 3: Implement `orchestrator/mod.rs` and `orchestrator/mcp.rs`.**
  - `handle`: when a token is present, `tools/call` calls `call(name, arguments)`. A tool call with no token gets `isError: true` and the text `not an Aether lead`.
  - `run_stdio`: reads `TOKEN_ENV` and `SOCK_ENV`, then loops over stdin lines, calling `handle` with `call = |t, a| forward(&sock, &token, t, a)`. It writes each response line and flushes. It returns `0` at EOF.
- [ ] **Step 4: Add the shim branch to `main.rs`.** The first statement of `main` becomes:
  `if std::env::args().nth(1).as_deref() == Some("mcp") { std::process::exit(aether_agent::orchestrator::mcp::run_stdio()); }`
- [ ] **Step 5: Run the tests and confirm they pass.** `cargo test -p aether-agent orchestrator` should be green. Then smoke-test the shim: run `printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | cargo run -p aether -- mcp`. The binary name comes from `crates/app/Cargo.toml`. It should print `"tools":[]`, because no token was given.
- [ ] **Step 6: Commit** `feat(agent): add orchestrator mcp shim`.

---

### Task 4: Socket server, lead tokens, Agent wiring

**Files:**
- Create: `crates/agent/src/orchestrator/server.rs`
- Modify: `crates/agent/src/agent.rs`: the `Agent` struct (:107), `with_factory` (:165), `ChatSession` (:51), `SessionKey` (:62), `session_options` (:886)
- Test: `crates/agent/tests/unit/orchestrator/server.rs`, `crates/agent/tests/unit/agent.rs`

**Interfaces:**
- Consumes: `LeadTools` and `DriverStartOptions.lead` (Task 2); the wire format and `forward` (Task 3).
- Produces:
  - `pub struct OrchCall { pub lead: String, pub tool: String, pub args: Value, pub reply: std::sync::mpsc::Sender<Result<Value, String>> }` in `orchestrator`. `lead` is the chat key, which is `chat.id.to_string()`.
  - `pub(crate) type Leads = Arc<Mutex<HashMap<String, String>>>`, mapping token to chat key.
  - `pub(crate) fn listen(path: &Path, leads: Leads, calls: UnboundedSender<OrchCall>) -> std::io::Result<()>` in `server`.
  - `ChatSession.lead: bool` (the app sets it) and `SessionKey.lead: bool`, so toggling respawns.
  - `Agent::orchestrator_calls(&self) -> Option<UnboundedReceiver<OrchCall>>`, which can be taken once, like `side_updates`.
  - `Agent::lead_token(&self, chat_key: &str) -> String`, which mints a token on first use and returns the same one for the app's lifetime.

- [ ] **Step 1: Write the failing tests** in `crates/agent/tests/unit/orchestrator/server.rs`:

```rust
fn sock(name: &str) -> PathBuf { std::env::temp_dir().join(format!("ao-{name}-{}.sock", std::process::id())) }

#[test]
fn known_token_reaches_app_with_lead_key_and_reply_returns() {
    let p = sock("ok");
    let leads: Leads = Default::default();
    leads.lock().unwrap().insert("tok".into(), "42".into());
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    listen(&p, leads, tx).unwrap();
    let app = std::thread::spawn(move || {
        let call = futures::executor::block_on(rx.next()).unwrap();
        assert_eq!((call.lead.as_str(), call.tool.as_str()), ("42", "list_agents"));
        call.reply.send(Ok(serde_json::json!({"agents": []}))).unwrap();
    });
    assert_eq!(forward(&p, "tok", "list_agents", serde_json::json!({})).unwrap()["agents"], serde_json::json!([]));
    app.join().unwrap();
}

#[test]
fn unknown_token_is_refused_without_reaching_app() {
    let p = sock("bad");
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    listen(&p, Default::default(), tx).unwrap();
    assert_eq!(forward(&p, "nope", "list_agents", serde_json::json!({})).unwrap_err(), "not an Aether lead");
    assert!(rx.try_next().is_err(), "nothing queued");
}

#[test]
fn stale_socket_file_is_replaced_and_mode_is_0600() {
    use std::os::unix::fs::PermissionsExt;
    let p = sock("stale");
    std::fs::write(&p, b"").unwrap();
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    listen(&p, Default::default(), tx).unwrap();
    assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn too_long_path_is_an_error_not_a_panic() {
    let p = std::env::temp_dir().join("x".repeat(200)).join("o.sock");
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    assert!(listen(&p, Default::default(), tx).is_err());
}

#[test]
fn dropped_reply_reports_app_gone() {
    let p = sock("gone");
    let leads: Leads = Default::default();
    leads.lock().unwrap().insert("tok".into(), "1".into());
    let (tx, rx) = futures::channel::mpsc::unbounded();
    listen(&p, leads, tx).unwrap();
    drop(rx);
    assert_eq!(forward(&p, "tok", "list_agents", serde_json::json!({})).unwrap_err(), "Aether didn't answer");
}
```

Add to `crates/agent/tests/unit/agent.rs`:

```rust
#[test]
fn lead_session_gets_lead_tools_and_stable_token() { /* agent.lead_token("9") twice -> equal, 32 hex chars;
   session_options with ChatSession{lead:true,..} -> opts.lead == Some(LeadTools{token: that, sock: socket_path, exe: current_exe});
   lead:false -> opts.lead == None; SessionKey differs between the two */ }
```

Write that test's body from the comment. `session_options` takes the token from `Agent`, so give it a `&Agent` parameter or a closure, whichever reads best at its two call sites.

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p aether-agent orchestrator::server` should not compile.
- [ ] **Step 3: Implement `listen`.**
  - Remove any existing file at `path`, bind a `UnixListener`, and `set_permissions(0o600)`. Spawn an accept thread, plus one thread per connection.
  - Each connection reads one line (`BufRead::read_line`). An unknown token gets `not an Aether lead`. Otherwise it sends an `OrchCall` and waits on the reply with `recv_timeout(30 s)`. A timeout, or a dropped sender or receiver, gets `Aether didn't answer`. The thread writes one response line.
- [ ] **Step 4: Wire it into `Agent`.**
  - Add `leads: Leads` and `orch_rx: Mutex<Option<UnboundedReceiver<OrchCall>>>` to `Agent`.
  - In `with_factory`, call `listen(&socket_path()?, ...)`. On `Err`, run `eprintln!("aether: orchestration off: {e}")` and carry on.
  - Tests that build many `Agent`s shouldn't fight over one socket. Skip the listener under `#[cfg(test)]`; the server tests call `listen` directly.
  - The token is 16 bytes from `/dev/urandom`, hex-encoded.
  - `session_options` sets `lead` when `chat.lead`, and `SessionKey.lead = chat.lead`.
- [ ] **Step 5: Run the tests and confirm they pass.** `cargo test -p aether-agent` should be green.
- [ ] **Step 6: Commit** `feat(agent): serve orchestrator socket for lead chats`.

---

### Task 5: Persist `parent` and `lead`, toggle from the palette

**Files:**
- Modify: `crates/agent/src/store.rs` (`SavedChat`, migrations at :68, save, load)
- Modify: `crates/app/src/app/mod.rs:108` (`Chat`), the six `Chat {` literals (`grep -rn "restarted_ms: 0" crates/app`), `chat/mod.rs:402` (`persist`), `chat/create.rs:343` (`restore_chats`), and `chat/send.rs:363` (`cli_request` sets `lead: chat.lead`)
- Modify: `crates/app/src/palette.rs` (one command)
- Test: `crates/agent/tests/unit/store.rs`, `crates/app/tests/unit/app/orchestrate.rs` (new, registered in `tests/unit/app/mod.rs`)

**Interfaces:**
- Consumes: `ChatSession.lead` (Task 4). If Task 4 hasn't landed, add the field here and let Task 4 use it.
- Produces:
  - `SavedChat { parent: Option<i64>, lead: bool, .. }`.
  - `Chat { pub(crate) parent: Option<i64>, pub(crate) lead: bool, .. }`.
  - `ChatApp::toggle_lead(&mut self, ix: usize, cx: &mut Context<Self>)`.

- [ ] **Step 1: Write the failing tests.**
  - In `store.rs` tests: `parent_and_lead_round_trip`. Save a chat with `parent: Some(5), lead: true`, flush, reopen, and assert both come back. Also `old_file_gains_parent_and_lead_as_none_false`: create the table with the old `SCHEMA` minus the new columns, insert one row, open with `Store::open`, and assert `parent == None` and `lead == false`. Model it on the existing migration test, if `store.rs` tests have one.
  - In `app/orchestrate.rs`: `toggle_lead_flips_and_persists_flag`. With `ChatApp::with_state` (see `watch.rs:161`), push a chat, call `toggle_lead(0, cx)` twice, and assert `true` then `false`. Also assert that `cli_request(0).1.lead` matches.
- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p aether-agent store` and `cargo test -p aether orchestrate` should not compile.
- [ ] **Step 3: Implement.**
  - Columns: `parent INTEGER` (nullable) and `lead INTEGER NOT NULL DEFAULT 0`, added through the `for (col, add)` loop.
  - Thread both fields through `persist`, `restore_chats` and every `Chat` literal (`None`, `false`).
  - `toggle_lead` flips the flag and calls `persist`.
  - Palette command label: `Lead: toggle for this chat`. Show it only for CLI chats, and only on Claude until phase 2b adds the other adapters.
- [ ] **Step 4: Run the tests and confirm they pass.** `cargo test -p aether-agent store` and `cargo test -p aether orchestrate` should be green.
- [ ] **Step 5: Commit** `feat(app): save lead flag and parent link on chats`.

---

### Task 6: `list_agents` and `spawn_agent` in the app

**Files:**
- Create: `crates/app/src/app/orchestrate.rs` (plus `mod orchestrate;` in `app/mod.rs`)
- Modify: `crates/app/src/app/new.rs:731` (a second loop next to the side-update loop)
- Test: `crates/app/tests/unit/app/orchestrate.rs`

**Interfaces:**
- Consumes: `OrchCall` and `Agent::orchestrator_calls()` (Task 4); `Chat.parent` and `Chat.lead` (Task 5); `ChatApp::submit` (`send.rs:262`); `self.probes: HashMap<Provider, ProviderProbe>` (`ProviderProbe.installed`, `.models: Vec<CliModel{id, efforts, ..}>`); `CLIS`; `AccessMode::for_provider` (`settings.rs:425`); `Provider::key()`.
- Produces (phase 2 builds on these):
  - `pub(super) fn orchestrate(&mut self, call: OrchCall, window: &mut Window, cx: &mut Context<Self>)`. It finds the lead by key, dispatches on `call.tool`, and always sends exactly one reply. Unknown tool: `unknown tool "x"`. Lead not found: `lead chat not found`.
  - `pub(super) fn list_agents(&self, lead_ix: usize) -> Value` returns `{"agents":[{"provider":"codex","label":"Codex","models":[{"id":"...","efforts":[...]}]}],"children":[{"id":i64,"title":..,"provider":..,"model":..,"status":..}]}`. `agents` covers installed probes of `CLIS` only.
  - `pub(super) struct SpawnArgs { provider: Provider, model: String, effort: String, title: String, task: String }`, with `fn parse(args: &Value, probes: &HashMap<Provider, ProviderProbe>) -> Result<SpawnArgs, String>`.
  - `pub(super) fn create_child(&mut self, lead_ix: usize, a: &SpawnArgs) -> Result<usize, String>` (Err only from the access ceiling). It pushes the child `Chat` and does **not** call `select`.
  - `pub(super) fn child_status(chat: &Chat) -> &'static str`, one of `running`, `needs_you`, `failed`, `idle`.

- [ ] **Step 1: Write the failing tests** in `crates/app/tests/unit/app/orchestrate.rs`, using `ChatApp::with_state` with one lead chat (provider Claude, `lead: true`, cwd `/tmp/proj`, access `AutoEdit`). Also insert a fake installed Codex probe with models `gpt-5-mini` and `gpt-5`.

```rust
#[test] fn parse_rejects_missing_task()           // Err contains "task"
#[test] fn parse_rejects_unknown_provider()       // "openrouter" and "gpt" -> Err contains "claude, codex, pi, opencode, antigravity"
#[test] fn parse_rejects_model_not_in_probe()     // Err == r#"unknown model "x" for codex; call list_agents"#
#[test] fn parse_accepts_any_model_when_probe_lists_none()
#[test] fn parse_rejects_uninstalled_provider()   // Err contains "not installed"

#[gpui_kit::test]
fn create_child_links_parent_and_keeps_focus(cx: &mut TestAppContext) {
    // active stays 0; new chat has parent == Some(lead.id), lead == false, provider Codex,
    // provider_model "gpt-5-mini", cwd == lead cwd, project == lead project, title from args,
    // access == AutoEdit (Codex allows it, lead is AutoEdit)
}

#[test] fn child_access_never_exceeds_lead()
// lead AutoEdit: pi -> ReadOnly, claude -> AutoEdit; lead Ask: antigravity -> Err
// "antigravity can't run as restricted as this lead (Ask)"; lead Full: codex -> Full

#[gpui_kit::test]
fn list_agents_shows_installed_clis_and_own_children_only(cx: &mut TestAppContext) {
    // a second lead's child is absent from "children"; uninstalled probes absent from "agents"
}

#[gpui_kit::test]
fn call_for_missing_lead_replies_error(cx: &mut TestAppContext) {
    // orchestrate(OrchCall{lead:"999",..}) -> reply Err("lead chat not found"); chats.len() unchanged
}
```

The full task text goes to `submit`. Test it as `child_prompt(task) == task + SUFFIX`, with the suffix exactly as in Global Constraints. Make `child_prompt(task: &str) -> String` a free function so the test needs no driver.

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p aether orchestrate` should not compile.
- [ ] **Step 3: Implement `orchestrate.rs`.**
  - `spawn_agent` = `parse`, then `create_child`, `persist`, `submit(child_ix, child_prompt(&a.task), vec![], window, cx)`, then reply `{"id", "title", "provider", "model"}`.
  - Build the child `Chat` the way `add_external_chat` does (`chat/create.rs:467`), using `allocate_chat_id` and `next_session_id`.
  - Access ceiling: rank modes with `ReadOnly 0 < Ask 1 < AutoEdit 2 < Auto 3 < Full 4`. The child gets the highest-ranked mode in `AccessMode::for_provider(child.provider)` whose rank is ≤ the lead's.
  - When no allowed mode is that narrow, `create_child` fails with `antigravity can't run as restricted as this lead (Ask)`. Example: Antigravity under an Ask lead. Use the provider key and the mode's `Debug` name.
  - `child_status`: running if `busy()`; needs_you if the last message has an unanswered request; failed if the last reply `failed`; idle otherwise.
- [ ] **Step 4: Wire the loop in `new.rs`.** Next to the side-update loop: `if let Some(mut calls) = app.agent.orchestrator_calls() { cx.spawn_in(window, async move |this, cx| { while let Some(call) = calls.next().await { ... this.update_in(cx, |this, window, cx| this.orchestrate(call, window, cx)) ... } }).detach(); }`. If `update_in` fails, the app is gone: break. The dropped `reply` then makes the socket answer `Aether didn't answer`.
- [ ] **Step 5: Run the tests and confirm they pass.** `cargo test -p aether` should be green, apart from the known `hackernews::client` flake.
- [ ] **Step 6: Commit** `feat(app): let lead chats list and spawn child agents`.

---

### Task 7: End-to-end check

- [ ] **Step 1:** `cargo build -p aether`, then launch the app.
- [ ] **Step 2:** Open a Claude chat in this repo and run `Lead: toggle for this chat` from the palette. Send: `Call list_agents, then spawn a codex child with the cheapest model to add a one-line comment-free README section "Orchestration" in a scratch file /tmp/orch-check.md. Report the child id.`
- [ ] **Step 3:** Confirm:
  - The lead's tool row shows `mcp__aether__spawn_agent` and its result.
  - A new Codex chat appears in the sidebar and runs without taking focus.
  - The file exists afterwards.
  - Restarting Aether still shows the child, and its parent link is intact (`sqlite3 "<config_dir>/aether.db" 'select id,parent,lead from chats order by id desc limit 2'`).
- [ ] **Step 4:** Toggle lead off and send another message. Confirm the respawned session no longer lists the Aether tools.
- [ ] **Step 5:** Record any GUI items still pending (child nesting, the crew strip) as phase 3 inputs. No commit unless something changed.
