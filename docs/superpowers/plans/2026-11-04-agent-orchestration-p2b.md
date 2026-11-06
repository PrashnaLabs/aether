# Agent orchestration, phase 2b: every harness can lead, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Codex, Pi, OpenCode, Antigravity and native (OpenRouter) chats can be leads, with the same five tools and result delivery a Claude lead already has.

**Architecture:** `Agent::lead_tools` already puts `LeadTools { exe, sock, token }` on `DriverStartOptions.lead` for any lead chat. Each driver turns that into its own launch form, using the flags recorded in `docs/wayfinder/research/orchestration-adapters.md`. The native agent skips the socket: it gets `FunctionTool`s that send `OrchCall`s straight onto the orchestrator channel. Last, the palette offers **Make lead** for every harness.

**Tech Stack:** Rust 2024, gpui, adk-rust `FunctionTool`, `serde_json`, Pi's TypeScript extension API (loaded by jiti). No new crates.

**Spec:** `docs/superpowers/specs/2026-11-04-agent-orchestration-design.md` (decision 3 table)

## Global Constraints

- No new crates in any `Cargo.toml`.
- MCP server name is `aether`. The shim is `<exe> mcp`, and its env vars are `AETHER_ORCH_SOCK` and `AETHER_ORCH_TOKEN`. A shim with no token lists zero tools; that is already built.
- The token never appears in argv or logs. Pass it through process env or a 0600 file only.
- With `lead: None`, each driver's launch args and env are byte-for-byte unchanged. Every adapter test asserts this.
- Aether's own tools are pre-approved for the lead in every access mode. The children's own actions stay gated.
- Children never become leads (depth 1).
- Exact flags come from `docs/wayfinder/research/orchestration-adapters.md`. Read your harness's section before coding.
- Tests live in `crates/<crate>/tests/unit/...` and are included with `#[cfg(test)] #[path]`. No test spawns a real CLI. Manual real-CLI checks go in the task report.
- Baseline: `cargo fmt --check` is red on main; `hackernews::client` is flaky (`--skip hackernews::client`).
- Commits: Conventional Commits. No `Co-Authored-By` or AI attribution.

## Review Focus

1. **Token in argv.** Codex `-c` and Antigravity `--settings` are argv. Expected: the token reaches the server through env forwarding, never a `-c ...env=` or a `--settings` value. Tests in Tasks 1 and 4.
2. **Resume or respawn of a lead.** Expected: the same lead flags are re-passed on every spawn (Codex config isn't stored in the thread; OpenCode's dedicated server must be relaunched with the same config). Tests in Tasks 1 and 3.
3. **Read-only Pi lead.** Expected: the Aether tool names are appended to the `--tools` allowlist, or the tools load but stay silently inactive. Test in Task 2.
4. **OpenCode first prompt before MCP is connected.** Expected: the lead driver waits for `GET /api/mcp` to report `aether` as `connected`, up to a timeout, before its first prompt. On timeout it logs and prompts anyway. Test in Task 3.
5. **Antigravity outside Aether after registration.** Expected: the globally registered shim lists zero tools when agy runs without the env vars (already true of the shim). Registration happens only after the user confirms, never silently. Test in Task 4.

---

## File map

| Task | Files |
|------|-------|
| 1 Codex | `crates/driver/src/codex.rs` (spawn args/env), `crates/driver/tests/unit/codex.rs` |
| 2 Pi | `crates/driver/src/pi.rs`, new `crates/driver/src/pi_lead.ts` (embedded via `include_str!`), `crates/driver/src/lib.rs` (`LeadTools.specs`), `crates/agent/src/agent.rs` (`lead_tools` fills specs), `crates/driver/tests/unit/pi.rs` |
| 3 OpenCode | `crates/driver/src/opencode/{mod,service,session}.rs`, `crates/driver/tests/unit/opencode/*` |
| 4 Antigravity | `crates/driver/src/antigravity.rs` (launch args + `call_mcp_tool` row), `crates/app/src/app/orchestrate.rs` or `manage.rs` (register-once confirm), tests in `crates/driver/tests/unit/antigravity.rs` and `crates/app/tests/unit/app/orchestrate.rs` |
| 5 Native | `crates/agent/src/tools/{mod.rs,lead.rs}`, `crates/agent/src/agent.rs` (native `send` path), `crates/agent/tests/unit/tools/lead.rs` |
| 6 Palette | `crates/app/src/palette.rs`, app test |

Order: tasks run one at a time, in the order 1, 2, 3, 4, 5, 6. Tasks 1, 3 and 4 touch disjoint driver files. Task 2 adds `LeadTools.specs` and must update every `LeadTools {` literal. Task 6 goes last.

---

### Task 1: Codex lead

**Interfaces:**
- Consumes: `DriverStartOptions.lead: Option<LeadTools>`, plus the Codex section of the research doc.
- Produces: `fn lead_config_args(lead: &LeadTools) -> Vec<String>` in `codex.rs`. It returns `-c` pairs for:
  - `mcp_servers.aether.command` set to `lead.exe`
  - `mcp_servers.aether.args = ["mcp"]`
  - `mcp_servers.aether.default_tools_approval_mode = "approve"`
  - the token, forwarded from the codex process env, never as a literal

  Use Codex's env-forwarding key: `mcp_servers.aether.env_vars = ["AETHER_ORCH_SOCK","AETHER_ORCH_TOKEN"]`. First confirm against the installed `codex` 0.166.0 that `env_vars` forwards both vars, using `codex app-server generate-json-schema` or a quick real run, and record the evidence in the report. If `env_vars` does not exist, stop and report NEEDS_CONTEXT. Do not fall back to `env={...}` in argv.

  The codex process env gets `AETHER_ORCH_SOCK` and `AETHER_ORCH_TOKEN` when `lead` is set. Both are added on every spawn, which covers resume and respawn.

- [ ] **Step 1: Tests**
  - `lead_spawn_adds_aether_mcp_config`: the args contain each `-c` pair, `default_tools_approval_mode="approve"` among them, and no arg contains the token. The spawn env contains both vars. If env is built inline, extract a pure helper `spawn_env(opts) -> Vec<(String,String)>` so this is testable.
  - `non_lead_spawn_is_unchanged`: same args and env as before, with no `mcp_servers.aether` entries.
- [ ] **Step 2:** Run `cargo test -p aether-driver codex` and confirm RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p aether-driver` and confirm GREEN. Then do one manual real run: start `codex app-server` with the args and env, have it call `list_agents` against a running dev Aether or a test listener, and record the result in the report.
- [ ] **Step 5: Commit** `feat(driver): pass aether mcp server to codex leads`.

### Task 2: Pi lead

**Interfaces:**
- Consumes: the Pi section of the research doc, and `aether_agent::orchestrator::tool_specs()`.
- Produces:
  - `LeadTools.specs: serde_json::Value`, the MCP tool array. `Agent::lead_tools` fills it with `orchestrator::tool_specs()`. Update every `LeadTools {` literal.
  - `crates/driver/src/pi_lead.ts`, embedded via `include_str!`. At load it reads `process.env.AETHER_ORCH_SPECS` (a JSON array), `AETHER_ORCH_SOCK` and `AETHER_ORCH_TOKEN`. For each spec it registers a Pi tool named `aether_<name>`, with `parameters` set to the spec's `inputSchema` (verify that Pi accepts plain JSON Schema; if it needs TypeBox, convert minimally). `execute` sends `{"token","tool":<name>,"args":params}` as one line over `node:net` and returns the result text, or the error as an error result. With no token or no socket, it registers nothing.
  - `fn lead_extension(lead: &LeadTools) -> io::Result<PathBuf>` writes the TS file to `<config_dir>/pi-lead.ts` with mode 0600.
  - `launch_args` adds `-e <path>` when `lead` is set. In Read-only mode, the `--tools` allowlist gains `aether_<name>` for every spec.
  - The pi process env gets `AETHER_ORCH_SOCK`, `AETHER_ORCH_TOKEN` and `AETHER_ORCH_SPECS`.
- [ ] **Step 1: Tests**
  - `lead_launch_loads_aether_extension`: `-e` points to an existing 0600 file, and the env has all three vars.
  - `read_only_lead_allowlists_aether_tools`: `--tools` includes `aether_list_agents` and `aether_spawn_agent`.
  - `non_lead_launch_is_unchanged`.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN for `cargo test -p aether-driver` and `cargo test -p aether-agent`. Then one manual `pi --mode rpc -e <file>` run that calls `aether_list_agents` against a test listener, recorded in the report.
- [ ] **Step 5: Commit** `feat(driver): give pi leads aether tools via extension`.

### Task 3: OpenCode lead

**Interfaces:**
- Consumes: the OpenCode section of the research doc, `opencode/service.rs` (shared `--service` instance) and `opencode/session.rs`.
- Produces:
  - When `lead` is set, the driver uses a dedicated server and leaves the shared service alone. The dedicated server is started as `opencode serve --port 0`, with these env vars:
    - `OPENCODE_SERVER_PASSWORD`, a random value that Aether generates.
    - `OPENCODE_CONFIG_CONTENT = {"mcp":{"aether":{"type":"local","command":[exe,"mcp"],"environment":{"AETHER_ORCH_SOCK":..,"AETHER_ORCH_TOKEN":..},"codemode":false}}}`. This is process env, not argv, so the token stays out of `ps`.
  - The driver parses `server listening on http://127.0.0.1:<port>` from stdout, then builds its HTTP API from that URL and the password. Add the constructor it needs next to the shared-service one, so a lead never touches `service.json`.
  - Before the first prompt, poll `GET /api/mcp` until `aether` is `connected`, up to 15 s (a constant with a `ponytail:` comment). On timeout, log `aether: opencode lead tools not ready` and prompt anyway.
  - The dedicated server process is owned by the driver and killed on driver drop. Resume relaunches it with the same env.
- [ ] **Step 1: Tests (pure helpers, no real opencode)**
  - `lead_config_content_has_aether_entry_without_codemode`: exact JSON, with `codemode:false`.
  - `parses_listening_port_line`.
  - `mcp_ready_waits_for_connected`: feed the poller a fake sequence (`[]`, `pending`, `connected`) through a closure or trait the driver already uses for HTTP. Expect it to return ready, and return timeout when `connected` never arrives (use a short timeout in the test).
  - `non_lead_uses_shared_service`.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN for `cargo test -p aether-driver`. Then one manual real run (dedicated server, tool call to a test listener), recorded in the report. Confirm the shared `--service` instance was not touched.
- [ ] **Step 5: Commit** `feat(driver): run opencode leads on a dedicated server with aether tools`.

### Task 4: Antigravity lead

**Interfaces:**
- Consumes: the Antigravity section of the research doc and `antigravity.rs` launch args (~740-765) and tool mapping (~989-1050).
- Produces:
  - When `lead` is set:
    - The agy process env gets `AETHER_ORCH_SOCK` and `AETHER_ORCH_TOKEN`.
    - Every mode except `Full` appends `--settings '{"permissions":{"allow":["mcp(aether/*)"]}}'`. If Aether already passes `--settings`, merge the allow rule into that JSON instead of adding a second flag.
    - The token never appears in `--settings`.
  - The tool mapping renders a `call_mcp_tool` row whose `ServerName == "aether"` as its `ToolName` (e.g. `spawn_agent`), with `Arguments` as input, instead of an unknown tool.
  - `pub fn agy_has_aether(binary) -> bool` (it runs `agy mcp list` and looks for `aether`). `pub fn register_agy_aether(binary, exe) -> io::Result<()>` runs `agy mcp add aether <exe> mcp` with no `--env`.
  - App: making an Antigravity chat a lead first checks `agy_has_aether`. If the server is missing, show a confirm prompt with this exact text:
    `Antigravity can only load Aether's tools from its global MCP config (~/.gemini/config/mcp_config.json). Register Aether there? Outside Aether the entry stays inactive.`
    Buttons are `Register` and `Cancel`. On Register, call `register_agy_aether` and then set lead. On Cancel, or on a registration error (show it), lead stays off.
- [ ] **Step 1: Tests**
  - `lead_launch_allows_aether_mcp_without_token_in_args`: for each non-Full mode, `--settings` holds `mcp(aether/*)`, no arg contains the token, and the env has both vars.
  - `full_mode_lead_adds_no_settings`.
  - `non_lead_launch_is_unchanged`.
  - `call_mcp_tool_for_aether_renders_inner_tool`.
  - The app confirm flow, as far as testable without a real agy (e.g. a pure decision fn `needs_agy_registration(provider, registered) -> bool`).
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN. Do not run `agy mcp add` against the user's config during the task. Note in the report that the real registration is a desktop check.
- [ ] **Step 5: Commit** `feat: let antigravity chats lead via registered aether mcp`.

### Task 5: Native (OpenRouter) lead

**Interfaces:**
- Consumes:
  - the `OrchCall` channel sender held by `Agent` (`orch_tx` or whatever Task 4 of phase 1 named it)
  - `ChatSession.lead` and `ChatSession.key`
  - `Tools::enabled` (`crates/agent/src/tools/mod.rs:23`)
  - `orchestrator::tool_specs()`
  - the `FunctionTool` pattern in `tools/ask_user.rs`
- Produces:
  - `crates/agent/src/tools/lead.rs`: `pub(crate) fn lead_tools(key: String, calls: UnboundedSender<OrchCall>) -> Vec<Arc<dyn Tool>>`, one `FunctionTool` per spec, using the same names and descriptions. The parameter schema comes from the spec's `inputSchema`; check how `FunctionTool` takes a raw JSON schema, and if it only takes `schemars` types, write `Args` structs for the five tools. Each tool:
    1. sends `OrchCall { lead: key, tool, args, reply }`
    2. awaits the reply with `tokio::task::spawn_blocking(move || rx.recv_timeout(30 s))`
    3. returns the result JSON, or the error text as the tool's error
  - The native `send` path adds these tools when `chat.lead`. No `Gate` approval, because they are pre-approved.
- [ ] **Step 1: Tests**
  - `lead_tools_cover_all_specs`: names match `tool_specs()` order.
  - `lead_tool_call_round_trips`: invoke one tool's closure with a test channel; a test thread answers `Ok(json!({"x":1}))`, and the tool returns it.
  - `lead_tool_reports_dropped_reply`: the reply sender is dropped and the call returns `Aether didn't answer`.
  - `non_lead_chat_gets_no_lead_tools`.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN for `cargo test -p aether-agent`.
- [ ] **Step 5: Commit** `feat(agent): give native lead chats orchestration tools`.

### Task 6: Make lead for every harness

**Interfaces:**
- Consumes: the Task 4 confirm flow.
- Produces: the palette shows **Make lead** / **Stop leading** for every provider in `CLIS` plus `Provider::OpenRouter`, still only when `parent.is_none()`. For Antigravity it goes through Task 4's confirm. Use a single fn `lead_capable(provider) -> bool` (true for all six), and remove the Claude-only gate.
- [ ] **Step 1: Test**
  - `lead_capable_covers_every_harness`.
  - An existing palette test, if one is reachable, showing the item for a Codex chat and not for a child.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN for `cargo test -p aether -- --skip hackernews::client`.
- [ ] **Step 5: Commit** `feat(app): offer make lead for every harness`.

### Task 7: Desktop checks (user)

- [ ] For each harness: make a chat a lead, ask it to `list_agents`, then `spawn_agent` a cheap child, and check that the result is delivered back.
- [ ] Antigravity: the confirm appears once, and after registration agy in a plain terminal shows no Aether tools.
