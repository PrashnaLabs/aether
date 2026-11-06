# Agent sessions: live CLI sessions, steer + queue, access, effort, folder

Date: 2026-10-14. Sub-project 1 of 3 (2 = rich tool activity UI, 3 = running overview). Builds on `2026-10-14-provider-picker-design.md`.

Wire references (binding for driver work): `docs/wayfinder/research/claude-sessions.md`, `pi-sessions.md`, `opencode-sessions.md`. Waku reference: `reference/crates/waku-core/src/driver/{claude,pi,opencode}.rs`.

## Goal

A CLI chat runs one live agent session that keeps its own memory across turns and app restarts, can be steered or queued while working, runs in a chosen project folder with a chosen access mode and effort, and shows its tool calls as steps. Today every message spawns a fresh CLI and re-sends the transcript, and all three drivers speak invented wire formats (no real turn completes for Pi or OpenCode; Claude approvals, interrupt, and tool events are broken).

## Decisions

- One live driver per chat, reused across turns; only the new message is sent.
- Any change to provider, binary, folder, model, access, or effort respawns the driver and resumes the same conversation by its resume id.
- Idle sessions (no running turn) close after 10 minutes and resume on the next message.
- Enter while busy queues the message; each queued message offers Steer now (Send when idle) and Remove. Queue sends the next item when a turn ends, unless the user pressed Stop.
- New chats start on Ask (Claude, OpenCode) or Read only (Pi).
- Pi access: Read only (`--no-approve --tools read,grep,find,ls --exclude-tools mcp__*`, so project-local `.pi` extensions and MCP tools stay off) and Full access (`--approve`); Ask and Auto-edit are unavailable for Pi.
- Folder locks after the first message (resume needs the same folder).
- Minimal tool steps ship here because the rewritten drivers must handle tool events; rich tool UI is sub-project 2.
- Implementers may run one tiny live turn per CLI (throwaway temp folder, Ask/Read only, prompt "Reply with exactly: ok"), plus one resume and one cancel check.

## Core (`crates/core`)

- `AccessMode { Ask, AutoEdit, ReadOnly, Full }`, serde lowercase (`ask`, `autoedit`, `readonly`, `full`), `Default = Ask`, `label()`: `Ask`, `Auto-edit`, `Read only`, `Full access`.
- `ToolStatus { Running, Done, Failed }`.
- `Update::Session(String)`: the driver's resume id once known (also after a fresh start replaces a dead one).
- `Update::Tool { id: String, title: String, detail: String, status: ToolStatus }`: upserts a tool step by `id`.
- `Step::Tool { id, title, detail, status }`. `Message::apply(Update::Tool)` replaces the step with the same `id` in place, else pushes. `Update::Session` is a no-op in `Message::apply`.
- Tool failure never sets `Message.failed`.

## Store (`crates/agent/src/store.rs`)

- New `chats` columns, `ALTER TABLE ADD COLUMN` like before: `cli_session TEXT NOT NULL DEFAULT ''`, `access TEXT NOT NULL DEFAULT 'ask'`, `effort TEXT NOT NULL DEFAULT ''`, `cwd TEXT NOT NULL DEFAULT ''`.
- `SavedChat` gains `cli_session: String`, `access: AccessMode`, `effort: String`, `cwd: String`. Unknown access strings load as `Ask`.

## Driver contract (`crates/agent/src/driver/mod.rs`)

```rust
pub struct DriverStartOptions {
    pub binary: String,
    pub cwd: PathBuf,
    pub model: String,
    pub effort: String,      // "" = CLI default
    pub access: AccessMode,
    pub resume: String,      // "" = fresh session
}

pub trait Driver: Send {
    /// Starts a turn with `text`.
    fn prompt(&mut self, text: String);
    /// Adds `text` to the running turn; with no turn running, same as `prompt`.
    fn steer(&mut self, text: String);
    /// Stops the running turn; the session stays alive.
    fn cancel(&mut self);
    /// Answers the driver's own Ask `id`.
    fn answer(&mut self, _id: u64, _allow: bool) {}
    /// True when `resume` was non-empty and the session was reattached.
    fn resumed(&self) -> bool;
}
```

- Construction may block (handshake, service adopt); it always runs off the UI thread.
- Every turn ends with exactly one `Update::Final` (empty when the turn produced no final text, e.g. after cancel). A steered message folds into the running turn and the single `Final` settles both.
- A driver emits `Update::Session(id)` once its resume id is known.
- Ask ids are driver-local; the agent renumbers them.
- Dropping the driver closes the session gracefully (close stdin / unsubscribe), then kills after 3 s if still alive. It never deletes the CLI's stored session.
- Tool events map to `Update::Tool`; no code fences in text. Titles: Claude/Pi tool names map to `Run command` (bash), `Read file` (read), `Edit file` (edit, multiedit), `Write file` (write), `Search files` (grep, glob), `Find files` (find), `List files` (ls), `Fetch` (webfetch), `Web search` (websearch), `Task` (task/agent: uses `input.description`), else the raw name. `detail` = first of `command`, `file_path`, `path`, `pattern`, `query`, `url`, `description` in the tool input, one line, max 120 chars.

### Claude (`driver/claude.rs`, rewrite per `claude-sessions.md`)

- Spawn: `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --replay-user-messages --permission-prompt-tool stdio`, `--session-id <uuid>` (fresh, uuid generated by the driver) or `--resume <id>` (only if `~/.claude/projects/*/<id>.jsonl` exists, else fresh), `--model`, `--effort`, `--permission-mode`: Ask → `default`, AutoEdit → `acceptEdits`, Full → `bypassPermissions` + `--dangerously-skip-permissions`. Live smoke decides `default` vs `manual` if `default` is rejected.
- Wire: user payload `{"type":"user","message":{"role":"user","content":[{"type":"text","text":…}]},"parent_tool_use_id":null}`; interrupt = `control_request` subtype `interrupt`; permission = `control_request` subtype `can_use_tool` → `Ask::Approval`, reply `control_response` success with `behavior` allow/deny; text from `stream_event` deltas; tools from `assistant` `tool_use` blocks and `user` `tool_result` blocks (skip `isReplay` and `parent_tool_use_id` set); `result` ends the turn.

### Pi (`driver/pi.rs`, rewrite per `pi-sessions.md`)

- Spawn `pi --mode rpc`, env `PI_SKIP_VERSION_CHECK=1`, cwd = chat folder, `--thinking <effort>` when set; Full adds `--approve`, ReadOnly adds `--no-approve --tools read,grep,find,ls --exclude-tools mcp__*`.
- Handshake: `get_state` → `switch_session{sessionPath}` when resuming and the file exists → `set_model{provider,modelId}` (split on first `/`) when set → `get_state` (emit `Session(sessionFile)`); 30 s handshake timeout.
- Wire: `prompt{id,message}`; `steer{message}` while running; `abort` on cancel; turn ends on `agent_settled`, on `success:false`, or `data.disposition:"handled"`; text from `message_update` `text_delta`, reasoning from `thinking_delta`; tools from `tool_execution_start/update/end` by `toolCallId`; `extension_ui_request` `confirm|select` → `Ask`, reply `extension_ui_response`; other UI requests ignored.

### OpenCode (`driver/opencode.rs`, rewrite per `opencode-sessions.md`)

- Service: read `${XDG_STATE_HOME:-~/.local/state}/opencode/service.json` (url, password); Basic auth `opencode:<password>` on every request and the SSE stream. When absent or unreachable, run `opencode serve --service` and wait up to 20 s for the file. Never kill the service.
- Session: resume id = client-minted `ses_…` (format per research). Resume: `GET /api/session/{id}` 200 → reuse (`resumed() = true`); 404 → create under the same id. Create body `{id, location:{directory: canonical cwd}, model:{providerID,id,variant?}}`; subscribe to `/api/event` before create.
- Wire: `POST /api/session/{id}/prompt {text}`; steer `{text, delivery:"steer"}` + `PATCH inbox/{id} {delivery:"steer"}` if not honored; cancel `POST …/interrupt`; turn ends on `session.execution.succeeded|failed|interrupted` (not `shutdown`); text from `session.text.delta`; tools from `session.tool.*` by call `id`; `permission.asked` → `Ask::Approval` unless access auto-answers (AutoEdit: `edit|write|patch`; Full: all), reply `POST …/permission/{requestID}/reply {decision:"once"|"reject"}`; effort = `model.variant`.
- `catalog::probe` and `missing_binary_error` treat OpenCode as installed when the binary exists or `service.json` points at a reachable service (replaces the port-4096 check).

## Agent session manager (`crates/agent/src/agent.rs`)

- `Agent` holds `sessions: Arc<Mutex<HashMap<String, Live>>>` keyed by chat key (the chat's store id as a string). `Live` = driver command channel, current turn sender, `SessionKey { provider, binary, cwd, model, effort, access }`, last activity, and an Ask id map (agent id → driver id).
- Each `Live` runs one thread that owns the `Box<dyn Driver>` (constructed there) and executes commands `Prompt(text, history) | Steer(text) | Cancel | Answer(id, allow)`, plus one pump thread forwarding driver updates to the current turn sender. The pump renumbers `Ask` ids from the shared counter and registers them for `Agent::answer`; on `Final` it forwards, then clears the turn sender.
- First prompt of a session that did not resume (`!driver.resumed()`) with non-empty history sends `context_prompt(history, text)`; otherwise just `text`.
- `send(..., chat: &ChatSession)` for CLI providers, where `ChatSession { key, cwd, access, effort, resume }`: build the `SessionKey`; drop the live session if its key differs; create one if missing (driver factory is a field so tests can inject a fake); open a new turn channel; queue `Prompt`. Returns `Reply` whose cancel queues `Cancel`.
- `steer(key, text, ...) -> Reply`: opens a new turn channel replacing the current one (the old reply stream ends), queues `Steer`. With no live session it behaves exactly like `send`.
- `answer(id, answer)`: OpenRouter gate as today; CLI ids forward `Answer(driver_id, allows)` to the owning session.
- `close_session(key)`: drops the live session (rewind, delete, duplicate source unaffected).
- Reaper thread every 30 s drops sessions with no turn sender and no activity for 10 minutes.
- Missing binary keeps today's preflight `Update::Error`.

## App (`crates/app`)

### Chat state

- `Chat` gains `cwd: String`, `access: AccessMode`, `effort: String`, `cli_session: String` (all persisted), and in-memory `queue: Vec<String>`, `stopped: bool`.
- New chat: `access` = `ReadOnly` for Pi, else `Ask`; `effort` empty; `cwd` empty until the first send, when it is fixed to the Settings workspace or home. Provider pick on an empty chat resets `access` to that provider's default if the current one is not offered.
- `Update::Session(id)` sets `chat.cli_session` and persists.
- Rewind (edit/retry) and delete call `agent.close_session(key)`; rewind also clears `cli_session`. Duplicate copies messages but not `cli_session`.

### Composer (CLI chats only)

- Folder chip: folder name (`Home` for the home dir), tooltip full path; click opens the folder picker; disabled after the first message with tooltip "Folder is fixed for this chat".
- Access chip: dropdown of the provider's modes (Claude, OpenCode: Ask, Auto-edit, Full access; Pi: Read only, Full access), checked current; disabled while busy.
- Effort chip: dropdown `Default` + levels (Claude `low medium high xhigh max`; Pi `off minimal low medium high xhigh max`); hidden for OpenCode; disabled while busy.

### Queue and steer

- Enter with the chat busy pushes the text onto `chat.queue` (composer clears) instead of cancelling.
- Queue rows render above the composer: text (one line, truncated), a `Steer now` button (`Send` when idle), and `Remove`.
- Steer now: removes the item, finishes the current assistant bubble, appends the user message and a new assistant bubble, and routes the stream from `agent.steer`. For OpenRouter it is cancel + send.
- When a run finishes and `!chat.stopped`, the next queued item is submitted. `stop()` sets `stopped`; any submit clears it.

### Tool steps

- `ThinkingPanel` renders `Step::Tool` as one row: title, muted detail, and a status word (`running`, `done`, `failed`) next to an icon. The summary line counts tools ("Ran 3 tools").

## Out of scope

- Rich tool output, diffs, per-tool icons (sub-project 2). Running overview and notifications (sub-project 3). Native fork/rewind cursors, OpenCode effort picker, Claude `ultracode`, `always` approvals, Claude subagent (Task) transcripts.

## Tests

- core: tool upsert by id; `Session` no-op; access serde names.
- store: new columns round-trip; legacy DB loads defaults.
- agent: session manager with a fake driver: reuse across sends, respawn on key change, steer swaps the turn channel, Final closes the reply, reaper drops idle sessions, Ask renumbering + answer routing, seed only when not resumed.
- drivers: mapping tests from the canned wire lines in the research docs (text, final, tool start/end, permission ask/reply, session id); live smoke per CLI (fresh turn, resume turn, cancel).
- app: queue pop rules (`stopped`), access defaults per provider, folder lock.
- `cargo test --workspace` and `cargo build` pass.
