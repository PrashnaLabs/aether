# Agent Manager Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** List every running/open Claude, Pi, OpenCode agent on the machine (any origin) with state, and let the user open, reveal, or stop it.

**Architecture:** `aether-agent` gets `external.rs`: pure parsers over `ps`/`lsof`/registry/transcript/API text plus a blocking `scan` and actions, exposed as `Agent` futures on `spawn_blocking`. The app polls in the background, renders an Agents page and a sidebar summary row, and wires actions.

**Tech Stack:** Rust 2024, std::process (`ps`, `lsof`, `/bin/kill`), serde_json, the OpenCode driver's service/auth helpers, gpui-kit 0.7.3.

**Spec:** `docs/superpowers/specs/2026-10-14-agent-manager-design.md` (binding). Research: `docs/wayfinder/research/external-agents.md`.

## Global Constraints

- Never block the UI thread; scans and actions only on agent threads.
- Never read `~/.claude/sessions/*.key`, other auth files, or process environments. OpenCode `service.json` only through the existing driver helper.
- Never SIGKILL; never signal the OpenCode service, shells, tmux, pid 1, or aether itself; re-verify pid + `lstart` right before signalling.
- Confirm dialogs before any signal to an outside process.
- Never encode meaning in color alone; buttons keyboard-reachable.
- No new dependencies. No commits (user commits by hand). Comments only for non-obvious why.

## Review Focus

- Aether's own agents must not appear as "outside" (own children by ppid chain; own sessions by id).
- A pid reused by another program must never be signalled (start-time check).
- Scan failures (no `lsof`, service down, unreadable files) degrade to fewer/less exact rows, never panic or block.
- Poll loop must not stack scans or keep running work per frame.
- Open in aether must not let two writers drive one session (read-only while the outside pid lives).

---

### Task 1: Discovery (`crates/agent/src/external.rs`)

**Produces:** `ExternalState`, `ExternalAgent` (spec "Model"), `pub fn scan(own_pid, own_sessions) -> Vec<ExternalAgent>`, `Agent::scan_external(own_sessions: HashSet<String>) -> impl Future<Output = Vec<ExternalAgent>> + 'static`; exports from `lib.rs`. May expose `pub(crate)` service/auth helpers from `driver/opencode.rs` (no behaviour change there).

Pure functions (each tested with fixture strings):
- `parse_ps(out: &str) -> Vec<PsRow { pid, ppid, etime, command }>`
- `agent_rows(rows: &[PsRow], own_pid) -> Vec<(Provider, PsRow, origin: String)>` — keeps `claude`/`pi`/`opencode` (and node + `pi-coding-agent`), drops helpers/print/RPC/own descendants, derives origin from the parent row.
- `parse_lsof_cwd(out: &str) -> HashMap<u32, PathBuf>`
- `claude_entry(json: &str) -> Option<(session, cwd, title, ExternalState, tmux: Option<String>)>`
- `pi_state(tail: &str, quiet_secs: u64) -> ExternalState`
- `opencode_states(sessions: &str, active: &str, permissions: &str) -> Vec<(session, dir, title, ExternalState)>`

- [ ] **Step 1: Failing tests** (fixtures):
  - ps: rows for `opencode serve --service` (ppid 1), `opencode` (ppid zsh), `/Users/x/.local/bin/claude` (ppid zsh), `claude --chrome-native-host`, `claude -p --input-format stream-json` with ppid = own pid, a `pi` whose parent's parent is own pid, a `tmux`-parented `claude` → kept: opencode TUI (origin Terminal), claude TUI (Terminal), tmux claude (origin `tmux`); dropped: service, helper, print child, own grandchild.
  - lsof: `p90078\nfcwd\nn/Users/x/a\np10537\nfcwd\nn/Users/x/b\n` → {90078: /Users/x/a, 10537: /Users/x/b}.
  - Claude registry: `{"pid":1,"sessionId":"s1","cwd":"/a","name":"Fix bug","status":"waiting","waitingFor":"input needed","tmux":"aether:@11.%13"}` → Waiting("input needed"), tmux Some; `"status":"busy"` → Working; `"idle"` → Idle; garbage → None.
  - Pi tail: last line `{"type":"message","message":{"role":"assistant","stopReason":"stop"}}` with quiet 10 → Idle; `stopReason":"toolUse"` → Working; last role `toolResult` → Working; quiet 1 with `stop` → Working.
  - OpenCode: sessions `{"data":[{"id":"ses_a","title":"T","location":{"directory":"/a"}},{"id":"ses_b","title":"U","location":{"directory":"/b"}}]}`, active `{"data":{"ses_a":{}}}`, permissions `{"data":[{"id":"per_1","sessionID":"ses_b"}]}` → ses_a Working, ses_b Waiting("approval").
  - dedupe: an entry whose session is in `own_sessions` is dropped (test the merge fn).
- [ ] **Step 2:** run `cargo test -p aether-agent external` — FAIL.
- [ ] **Step 3:** implement parsers + `scan` (spec steps 1-6; each external call failure → skip that source) + `Agent::scan_external`.
- [ ] **Step 4:** `cargo test -p aether-agent && cargo build` PASS. Run `scan` once for real in an `#[ignore]` test printing a redacted summary (provider, pid, folder basename, state, exact, origin) to confirm it finds the running Claude/OpenCode TUIs; record output (no transcript content, no secrets).
- [ ] **Step 5:** Stop. No commit.

### Task 2: Actions (`external.rs`)

**Produces:** `pub fn stop(agent, force) -> Result<(), String>`, `pub fn transcript(agent) -> Vec<(Role, String)>`, `Agent::stop_external(agent: ExternalAgent, force: bool)`, `Agent::transcript(agent: ExternalAgent)` futures.

- [ ] **Step 1: Failing tests:**
  - `signal_plan(agent, force, current_lstart: Option<&str>, current_basename: Option<&str>) -> Result<Option<&'static str>, String>` (pure): Claude → `Some("INT")` / force `Some("TERM")`; Pi → `Some("TERM")`; OpenCode with session → `Ok(None)` (API path); lstart mismatch or basename change or pid None for Claude/Pi → `Err("process changed")`.
  - Claude transcript fixture lines: user text, assistant with text + tool_use, user tool_result, `isMeta` user, `isSidechain` assistant, `ai-title` → `[(User, "hi"), (Assistant, "hello")]`.
  - Pi transcript fixture: session header + user/assistant/toolResult messages → user + assistant texts only.
  - OpenCode messages fixture (shape per research/driver) → text parts only.
- [ ] **Step 2-4:** FAIL → implement (signals via `/bin/kill -<SIG> <pid>`; OpenCode interrupt via the driver's authed request helper; transcript capped at last 200) → `cargo test -p aether-agent && cargo build` PASS. No live stop/signal runs.
- [ ] **Step 5:** Stop. No commit.

### Task 3: App — polling, Agents page, sidebar row

**Consumes:** Task 1 (`ExternalAgent`, `scan_external`).

- [ ] **Step 1: Failing tests** (pure helpers in app): `poll_interval(page_visible) -> Duration` (2 s / 10 s); `external_state_text(&ExternalState, exact) -> String` (`Working`, `Needs input needed`→ use `Needs <waitingFor>`, `Idle`, + ` (estimated)` when not exact); `outside_summary(n) -> Option<String>` (`None` for 0, `1 running outside aether`, `3 running outside aether`).
- [ ] **Step 2-3:** FAIL → implement: `external`, `scan_run`, `scanning` fields; poll loop started in `new` (`cx.spawn_in` loop with `background_executor().timer`); `Page::Agents` + nav item; Agents page per spec (In Aether / Outside Aether sections, Refresh); sidebar summary row → opens the page. Buttons Open/Reveal/Stop render but call stub methods that Task 4 fills (`open_external(i)`, `reveal_external(i)`, `stop_external(i)`), each a no-op for now.
- [ ] **Step 4:** `cargo build && cargo test --workspace` PASS (no new warnings — stub methods must be used by buttons).
- [ ] **Step 5:** Stop. No commit.

### Task 4: App — actions + read-only lock

**Consumes:** Task 2 futures, Task 3 page/stubs.

- [ ] **Step 1: Failing tests:** `transcript_messages(Vec<(Role,String)>) -> Vec<Message>` (assistant messages finished with text, user messages plain); `externally_locked(chat_external_pid, external: &[ExternalAgent]) -> bool`.
- [ ] **Step 2-3:** FAIL → implement per spec: Reveal (`cx.reveal_path`); Stop (OpenCode direct; Claude dialog Interrupt / Quit / Cancel; Pi dialog Quit / Cancel; errors to a status line on the page; rescan after); Open in aether (new chat with provider/cwd/cli_session/title/default access + imported messages, `external_pid`, persisted, selected); composer read-only + banner while locked (Send disabled, Enter ignored, queue ignored).
- [ ] **Step 4:** `cargo build && cargo test --workspace` PASS.
- [ ] **Step 5:** Stop. No commit.
