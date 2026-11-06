# Agent manager: every running/open agent, from any program

Date: 2026-10-14. Builds on `2026-10-14-agent-sessions-design.md` and the sidebar Agents section. Research: `docs/wayfinder/research/external-agents.md` (binding for discovery and actions).

## Goal

One place that lists every running or open Claude / Pi / OpenCode agent on the machine, whether aether, a terminal, tmux, or another app started it, with its state, and lets the user open it in aether, reveal its folder, or stop it.

## Decisions

- Agents page (new nav item) + the existing sidebar AGENTS section, which gains one row "N running outside aether" opening the page.
- CLIs: Claude, Pi, OpenCode. Codex and others out of scope.
- Actions on outside agents: view, Stop (confirm-gated for signals), Open in aether, Reveal folder.
- Discovery runs on agent threads only; never while drawing. Poll every 2 s while the Agents page is visible, 10 s otherwise; skip a tick while a scan is in flight.
- Never read `~/.claude/sessions/*.key` or any auth file other than OpenCode's `service.json` (already used for local Basic auth). Never read other processes' environments.
- Never SIGKILL; never signal the OpenCode service, shells, tmux, or pid 1; re-verify pid + process start time right before any signal.

## Model (`crates/agent/src/external.rs`, exported)

```rust
pub enum ExternalState { Working, Waiting(String), Idle }

pub struct ExternalAgent {
    pub provider: Provider,
    pub pid: Option<u32>,      // None: OpenCode session with no TUI process
    pub started: String,       // `ps -o lstart=` text, for pid-reuse checks; "" when pid is None
    pub elapsed: String,       // `ps` etime, e.g. "05:50:07"
    pub cwd: PathBuf,
    pub session: String,       // Claude sessionId | Pi session file path | OpenCode ses_ id; "" if unknown
    pub title: String,         // Claude registry name / OpenCode title / folder name
    pub state: ExternalState,
    pub exact: bool,           // state from registry/API (true) or transcript heuristic (false)
    pub origin: String,        // "Terminal", "tmux <target>", or the parent app's name
}
```

`pub fn scan(own_pid: u32, own_sessions: &HashSet<String>) -> Vec<ExternalAgent>` (blocking):

1. `ps -axo pid,ppid,etime,state,command` once. Keep rows whose command basename is `claude`, `pi`, `opencode` (or node running `pi-coding-agent`); drop helpers (`--chrome-native-host`, `serve`, `app-server`), rows whose ppid chain reaches `own_pid` (aether's own children), and print/RPC rows (`-p`, `--print`, `--mode rpc`).
2. One `lsof -a -p <pids> -d cwd -Fpn` for the kept pids → cwd per pid. Parent command → origin (`-zsh`/`zsh`/`bash`/`fish`/`login` → `Terminal`; `tmux` → `tmux`; else the parent's basename).
3. Claude: for each kept `claude` pid, read `~/.claude/sessions/<pid>.json` (only `*.json`): sessionId, cwd, name, status `busy|waiting|idle` → Working / Waiting(waitingFor or "input needed") / Idle, `tmux` → origin `tmux <target>`; exact. No registry file → Idle, not exact, session "".
4. Pi: newest `*.jsonl` in `~/.pi/agent/sessions/--<cwd with / → ->--/` with mtime ≥ process start; state from the last 64 KB: last `message` is assistant with `stopReason` `stop`/`error` and file quiet > 5 s → Idle, else Working; not exact. Session = that file's path.
5. OpenCode: via the local service (reuse the driver's `service.json` + Basic auth): `GET /api/session?limit=50`, `GET /api/session/active`, `GET /api/permission/request`. Each `opencode` TUI pid → the newest session whose `location.directory` == its cwd. Additionally list sessions that are active or have a pending permission even with no TUI (pid None, origin "OpenCode service"). State: active → Working; pending permission for that session → Waiting("approval"); else Idle; exact. Service unreachable → TUI rows with Idle, not exact.
6. Drop any entry whose `session` is in `own_sessions` (aether chats' `cli_session`).

Actions (blocking, agent thread):

- `pub fn stop(agent: &ExternalAgent, force: bool) -> Result<(), String>`:
  - OpenCode with a session: `POST /api/session/{id}/interrupt?resume=false` (no signal, `force` ignored).
  - Claude: re-check `ps -p <pid> -o lstart=` == `started` and basename still `claude`; then `kill -INT` (or `kill -TERM` when `force`).
  - Pi: same check; `kill -TERM` (Ctrl-C only clears Pi's editor).
  - Signals via `/bin/kill -<SIG> <pid>`; refuse when the check fails ("process changed").
- `pub fn transcript(agent: &ExternalAgent) -> Vec<(Role, String)>`: user/assistant plain text only, oldest first, at most the last 200 messages.
  - Claude: `~/.claude/projects/*/<sessionId>.jsonl` lines with `type` `user`/`assistant`; text blocks only (skip `tool_use`, `tool_result`, `isMeta`, `isSidechain`).
  - Pi: the session file's `message` entries with role `user`/`assistant`, text content only.
  - OpenCode: `GET /api/session/{id}/message?order=desc&limit=200`, reversed to oldest first, text parts only.
- `Agent::scan_external(own_sessions) -> impl Future<Output = Vec<ExternalAgent>> + 'static`, `Agent::stop_external(agent, force) -> impl Future<Output = Result<(), String>>`, `Agent::transcript(agent) -> impl Future<Output = Vec<(Role, String)>>`, all via `spawn_blocking`.

## App (`crates/app`)

- `ChatApp.external: Vec<ExternalAgent>`, `scan_run: u64` generation, `scanning: bool`.
- Poll loop started at launch: wait 2 s if the Agents page is visible else 10 s; skip if `scanning`; collect `own_sessions` from chats; scan; store; `cx.notify()`.
- `Page::Agents` + sidebar nav item "Agents" (icon e.g. `IconName::Bot` or nearest existing). Page (SettingsPage-style layout):
  - Section "In Aether": rows from `agent_state(chat, live)` (existing), click opens the chat, Stop for busy rows.
  - Section "Outside Aether": one row per `ExternalAgent`: CLI label, folder name (tooltip full path), title, state text (`Working`, `Needs <waitingFor>`, `Idle`; append ` (estimated)` when not exact), `Running <elapsed>`, origin. Buttons: Open, Reveal, Stop. Empty → "No agents running outside aether."
  - Refresh button triggers an immediate scan.
- Sidebar AGENTS section: when `external` is non-empty, a final row "N running outside aether" → opens the Agents page.
- Stop (outside): OpenCode → call directly; Claude → confirm dialog "Interrupt Claude in <folder>? This sends Ctrl-C to that terminal session." with buttons Interrupt / Quit (force) / Cancel; Pi → confirm "Quit Pi in <folder>? Pi has no remote interrupt; this ends that session (its history is kept)." Result errors show as a toast/status line; rescan after.
- Reveal: `cx.reveal_path(&cwd)`.
- Open in aether: new chat with provider, `cwd`, `cli_session = session`, title, `access` = provider default, messages from `transcript()` (user → `Message::user`, assistant → `Message::assistant` + `Final(text)` + `finish()`), persisted; `Chat.external_pid: Option<u32>` (in-memory). While that pid appears in the latest scan, the composer is read-only with a banner "Running in <origin>. Stop it or wait for it to finish to continue here." Disabled when `session` is empty.

## Out of scope

Codex/other CLIs, live tool detail for outside agents, notifications, tmux pane focusing, editing outside sessions while their process runs.

## Tests

- external: parsers from fixture strings (ps rows incl. helpers/print/own-child exclusion; lsof `-Fpn` output; Claude registry JSON → state; Pi tail → Working/Idle; OpenCode session/active/permission JSON → states incl. no-TUI sessions; own_sessions dedupe); transcript extraction for all three fixture formats; stop refuses on start-time mismatch (pure check fn).
- app: state text formatting; poll interval choice; open-in-aether message conversion; read-only rule.
- `cargo build` and `cargo test --workspace` pass.
