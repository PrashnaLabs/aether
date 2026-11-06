# Research: Claude Code sessions, steer, modes, effort, tools

Sources: `reference/crates/waku-core/src/driver/claude.rs` (W:), `reference/crates/waku-core/src/claude_session.rs`, `reference/crates/waku-protocol/src/model.rs`, `reference/crates/waku-core/src/model_catalog.rs`; local `claude --help`; aether `crates/agent/src/driver/claude.rs` (A:), `crates/agent/src/agent.rs`.
Nothing was run against a model. "Verified" below means a Waku comment claims it was checked against CLI 2.1.223/2.1.228/2.1.258; not re-verified here.

## CLI flags (verbatim from `claude --help`)

```
--add-dir <directories...>   Additional directories to allow tool access to
--effort <level>             Effort level for the current session (low, medium, high, xhigh, max)
--fork-session               When resuming, create a new session ID instead of reusing the original (use with --resume or --continue)
--include-partial-messages   Include partial message chunks as they arrive (only works with --print and --output-format=stream-json)
--input-format <format>      Input format (only works with --print): "text" (default), or "stream-json" (realtime streaming input)
--model <model>              Model for the current session. Provide an alias for the latest model (e.g. 'fable', 'opus', or 'sonnet') or a ...
--no-session-persistence     Disable session persistence - sessions will not be saved to disk and cannot be resumed (only works with --print)
--permission-mode <mode>     (choices: "acceptEdits", "auto", "bypassPermissions", "manual", "dontAsk", "plan")
--permission-prompts <target> Who answers permission prompts with --print: "host" (the SDK host or --permission-prompt-tool) or "none" ...
-r, --resume [value]         Resume a conversation by session ID, or open interactive picker with optional search term
-c, --continue               Continue the most recent conversation in the current directory
--session-id <uuid>          Use a specific session ID for the conversation (must be a valid UUID)
--replay-user-messages       Re-emit user messages from stdin back on stdout for acknowledgment (only with stream-json in/out)
--dangerously-skip-permissions  Bypass all permission checks.
--allow-dangerously-skip-permissions  Enable bypassing all permission checks as an option, without it being enabled by default.
```

Notes: `--permission-prompt-tool stdio` is NOT in `--help` (W:1-12 doc comment, W:156-160), but `--permission-prompts host` is, and `--help` here lists `manual` as a mode while Waku's Ask uses `default`. `--thinking-display summarized` (W:151) is also used; confirm it still exists on the installed version before copying (grep it in `claude --help`, it was not in my filtered grep).

## 1. Session continuity

- Waku keeps ONE live process per chat across turns: spawn once with `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages ...` (W:142-168), keep stdin open, write one `user` JSONL per turn (payload W:67-76), and wait for the `result` event to settle each turn. Process dies only on Shutdown/Drop (W:588-592).
- Session id is chosen by the host, not read from `system/init`: `Uuid::new_v4()` up front, passed as `--session-id <uuid>` on a fresh session (W:198-216). On restart, `--resume <id>` with the stored id (W:212-213). Waku emits `DriverEvent::Connected{provider_cursor}` immediately (W:236-247) so the id is persisted before turn 1.
- Persisted cursor = `ProviderResumeCursor::Claude{session_id, resume_at}` (waku-protocol `model.rs:176`). `resume_at` is the latest transcript message uuid, re-read after each `result` from the CLI's own transcript (W:1581-1596) for rewind/fork. Aether does not need `resume_at`.
- Transcript lives at `$CLAUDE_CONFIG_DIR or ~/.claude` + `/projects/<encoded-cwd>/<session-id>.jsonl` (`claude_session.rs:68-98`). Waku locates by scanning every project dir for `<id>.jsonl` rather than computing the encoding, and records `cwd` from entries (`claude_session.rs:276`).
- Resume cwd: the CLI keys session storage by cwd (the `projects/<encoded-cwd>` dir). Waku always sets `current_dir(&cwd)` on the child (W:196) and never changes cwd for a resumed session, so assume `--resume` must run in the same cwd as the original (not tested here; `--help` for `-c` says "in the current directory"). If a chat's workspace changes, start a new session (or `--fork-session`, which gives a new id).
- `system/init` does carry `session_id` (Waku tests use `{"type":"system","subtype":"init","session_id":"s","tools":[]}`, W:2475) but Waku does not need it.
- Aether today: `send_cli` (agent.rs:190-260) spawns a driver per message, sends `context_prompt(&history,&text)` (whole transcript as text), then kills it. No session id.

## 2. Steer and queue

- Mid-turn write of another `user` JSONL = STEER, not queue: "A mid-turn user message is held by the CLI and folded into the running turn at the next model call — one `result` settles everything, and the isReplay echo marks the moment it is absorbed. Verified against the real CLI (2.1.223)" (W:309-316; module doc W:1-12).
- Waku steer path (W:308-352): reject if no active turn (`writer_turn` flag false) with `SteerRejected`; else write the same user payload, no `TurnStarted`, emit `SteerAccepted`. Entry points `supports_steer()=true`, `steer()` (W:536-542).
- `--replay-user-messages` (W:153-155) echoes each stdin user message on stdout with `"isReplay": true`; reader ignores it as transcript (W:1517-1523). Echo timing is the signal "agent absorbed it", which distinguishes absorbed from queued.
- QUEUE is a Waku host feature, not a CLI one: `QueuedMessage{id,content,...}` (waku-protocol `model.rs:709-713`), "becomes its own turn once the current turn settles successfully". Rule in `SessionStatus::Background` doc (`model.rs:688-696`): when busy and provider idle (parked on background work), a new message steers straight in; otherwise it waits in the follow-up queue. UI chooses steer vs queue; the driver only exposes `prompt` and `steer`.
- Aether needs: a `steer(text)` on the live driver that writes the same payload, plus a host-side queue that calls `prompt()` after `Final`.

## 3. Access modes

Waku `permission_mode` (W:93-101) and flags (W:158-166):

| Waku mode | `--permission-mode` | extra |
|---|---|---|
| Ask | `default` | host answers `can_use_tool` via UI |
| AutoAcceptEdits | `acceptEdits` | host auto-allows every `can_use_tool` (`auto_approve = mode != Ask`, W:~268) |
| Auto | `auto` | same auto-allow |
| FullAccess | `bypassPermissions` | plus `--dangerously-skip-permissions` |

- All modes also pass `--permission-prompt-tool stdio`; the CLI then sends `control_request{subtype:"can_use_tool",request_id,request:{tool_name,display_name,input,description,blocked_path}}` (W:1327-1334, 1697-1750).
- Reply (W:360-383): `{"type":"control_response","response":{"subtype":"success","request_id":<id>,"response":{"behavior":"allow"}}}` or `{"behavior":"deny","message":"..."}`. AskUserQuestion replies with `behavior:"allow", updatedInput:{questions,answers}` (W:384-430).
- Live change: NO. Waku treats mode as launch-only: `apply_options` returns false if `options.mode != self.mode`, which makes the daemon tear down and recreate the driver ("changing what a running agent may touch deserves a fresh session", W:572-579; contract `driver/mod.rs:172-176`). The recreate uses `--resume <same id>`, so the conversation survives. Waku never sends `set_permission_mode`; I did not find evidence the CLI accepts it in stream-json, so do not rely on it without testing.
- Cheaper alternative inside aether: `Ask`/`AutoAcceptEdits`/`Auto` all use the same host-side `can_use_tool` gate, so only a switch to or from FullAccess needs a respawn if you keep `--permission-mode default` and auto-allow in the host. Not what Waku does, but valid since the host answers every prompt.

## 4. Effort

- Launch flag `--effort <level>` (W:209-211). Values in Waku's catalog (`model_catalog.rs:1452-1468`): `low, medium, high, xhigh, max`, default `high`; xhigh-capable models additionally accept `ultracode` (resolves to xhigh plus dynamic-workflow orchestration). `--help` lists only `low, medium, high, xhigh, max`.
- No live setter: `CommandMessage::Options` (W:421-455) only handles model (`control_request` subtype `set_model`, `{"request":{"subtype":"set_model","model":m}}`, accepted by CLI 2.1.228 per W:430-432). Changing effort therefore needs a respawn with `--resume` (apply_options only forwards model/window; effort change is not applied live in W:572).
- Model + 1M window: `wire_model` appends `[1m]` to the id (W:107-115).
- Aether: `Reasoning::effort()` already exists (`crates/core/src/settings.rs:29`), `DriverStartOptions` has only `model` (driver/mod.rs:16-20), so add `effort: Option<String>`.

## 5. Tool activity wire shapes

Waku builds rows from COMPLETE messages, and ignores partial `content_block_start/input_json_delta`: the `stream_event` handler only reads `message_start` (resets flags) and `delta.type` `text_delta`/`thinking_delta` (W:1367-1405). Tool rows come from:

1. `{"type":"assistant","parent_tool_use_id":null,"message":{"model","usage","content":[{"type":"text"},{"type":"thinking","thinking"},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}` (W:1409-1490, test W:2483). Text/thinking blocks are used only if no streamed delta was seen (`saw_text_delta`, W:1432-1447).
2. `{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"hi","is_error":false}]},"tool_use_result":{...}}` (W:1506-1565). `tool_use_result` carries structured edit hunks for file changes. Skip if `isReplay==true` or `parent_tool_use_id` set (subagent output).
3. `content` of a tool_result may be a string OR an array of blocks (text/image); Waku passes it to `activity::format_output` (reference `activity.rs`).

Row lifecycle: `tool_use` -> `RichActivity{source_id=id, complete=false}` (running; no separate "start" event); `tool_result` -> same id, `complete=true`, `failed=is_error` (W:1448-1490, 1526-1565). Waku keeps `state.tools: id -> (kind,title,wire_name,command)` to join them (W:1466-1475). Tool failure is a failed ROW, never a turn error.

Kind (`ActivityKind::from_tool_name`, waku-protocol `model.rs:1745-1836`), lowercase-normalised, `mcp__` prefix stripped:
- Command: `bash`, shell, terminal, execute...
- FileChange: `edit`, `write`, `multiedit`, `notebookedit`, `patch`, `create`...
- FileRead: `read`; FileSearch: `grep`, `glob`, `find`, `ripgrep`; FileList: `ls`, `list`
- Search: `webfetch`, `websearch`; Plan: `todowrite`; everything else (incl. `Task`/`Agent`) is `Tool`.
Title: `input.title` if present, else the wire name; for `Task`/`Agent` use `input.description` (W:1458-1475). A one-line subject fallback keys `command, file_path, path, pattern, query, url, description` (W:1218-1230), which is the right summary source per tool (Bash=command, Read/Edit/Write=file_path, Grep/Glob=pattern, WebFetch=url, Task=description).
Subagent (Task) inner messages arrive on the main channel with `parent_tool_use_id` set; Waku routes them to a task pane (W:1190-1252). Aether can drop them.
Partial tool input (`content_block_start{content_block:{type:tool_use,id,name}}` + `input_json_delta{partial_json}`) exists on the wire but Waku skips it. Use only if you want "tool started" before the input is complete.

Aether today (A:208-212, 275-277): matches a top-level `"type":"tool_use"` line, which the real CLI never emits (tool_use is nested in `assistant.message.content`), and writes a code fence into the text. `tool_result` is dropped (test A:331). So tools currently show nothing for real runs.

## 6. Per-chat cwd

- Waku: `command.current_dir(&cwd)` (W:196); `--add-dir` is not used anywhere in `claude.rs`. Session files are grouped by cwd in `~/.claude/projects/<encoded-cwd>/`; Waku finds them by scanning all dirs, so it does not depend on the encoding.
- Aether: `send_cli` derives cwd from `settings.workspace_root` else home (agent.rs:215-219). Need per-chat cwd stored with the chat and fixed at session creation. `--add-dir` only if extra folders are wanted.

## Aether changes needed

1. Change the Claude driver to a persistent per-chat process: the Agent keeps `HashMap<ChatId, ClaudeDriver>`; `send_cli` calls `driver.prompt(text)` on the live one and sends only the new text (drop `context_prompt(history)` for Claude).
2. Session id: generate a UUID per chat at creation, persist it in chat metadata, spawn `--session-id <uuid>` first time and `--resume <uuid>` when restoring (same cwd). If resume fails (file missing), fall back to fresh session + context_prompt once.
3. Fix wire shapes in A:111-160: user payload `content` must be `[{"type":"text","text":..}]` plus `"parent_tool_use_id":null`; interrupt is `{"type":"control_request","request_id":"..","request":{"subtype":"interrupt"}}`, not `{"type":"interrupt"}`; permission replies are `{"type":"control_response","response":{"subtype":"success","request_id":<string from request>,"response":{"behavior":"allow"|"deny"}}}`.
4. Fix parsing (A:168-245): handle `control_request/can_use_tool` (not `permission_prompt`), `assistant` content blocks, `user` tool_result, `stream_event` text/thinking deltas, `result` as the turn terminator (`is_error`, `result`, `usage`).
5. Add flags: `--permission-mode <map>` (+`--dangerously-skip-permissions` for FullAccess), `--effort <level>`, `--replay-user-messages`, optional `--thinking-display summarized`.
6. Add `mode` and `effort` to `DriverStartOptions` (driver/mod.rs:16). Mode/effort change = drop driver and respawn with `--resume`; model change = `control_request set_model` live.
7. Add `steer(text)` (reject if no active turn) and a host queue (send after `Final`). Steer folds into the running turn; the `isReplay` echo signals absorption.
8. Tool rows: add an `Update` variant for tool activity `{id, kind, title, detail, status Running/Done/Failed}`; map `tool_use` -> Running, `tool_result` -> Done/Failed by id; kind via lowercase name table above; stop emitting code fences (A:275).
9. Turn settle: guard exactly-once `Final` (Waku "parked" background-task turns can be skipped initially).
10. Per-chat cwd stored with the chat; reuse it for every resume. Drop the process when the chat is closed or the cwd changes.
