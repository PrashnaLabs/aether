# Research: Pi sessions, steer/queue, modes, effort, tools

Sources: Waku `reference/crates/waku-core/src/driver/pi.rs` (cited `pi.rs:N`), `model_catalog.rs`, `daemon.rs`; installed Pi 1.2.0 docs at
`~/.pi/agent/install/releases/1.2.0/node_modules/@earendil-works/pi-coding-agent/docs/` (`rpc-commands.md`, `rpc.md`, `json.md`, `rpc-extension-ui.md`, `sessions.md`, `session-format.md`, `security.md`); `pi --help` (run with `PI_SKIP_VERSION_CHECK=1`). Verified live (no prompt sent): `get_state`, `get_available_thinking_levels` over `pi --mode rpc --no-session`.

Related: `pi-wire.md` (handshake/event overview).

## 0. Aether's current Pi driver does not speak real Pi RPC

`crates/agent/src/driver/pi.rs` uses invented shapes:
- writes `{"type":"negotiate","version":2}` (not a Pi command; `negotiate_protocol` is Oh My Pi only, `pi.rs:366`),
- `{"type":"set_model","model":...}` (real: `provider` + `modelId`),
- `{"type":"prompt","id":N,"text":...}` (real field is `message`, id is a string),
- parses `message_delta`, `reasoning`, `usage`, `tool_use`, `tool_approval`, `question`, `session_metadata` (none exist in Pi; real: `message_update{assistantMessageEvent}`, `message_end`, `tool_execution_*`, `agent_settled`).
Unit tests in that file pin the fake shapes. Treat the driver as a rewrite, not a patch.

## 1. Session continuity

Pi CLI (verbatim from `pi --help`):
```
--continue, -c                 Continue previous session
--resume, -r                   Select a session to resume
--session <path|id>            Use specific session file or partial UUID
--session-id <id>              Use exact project session ID, creating it if missing
--fork <path|id>               Fork specific session file or partial UUID into a new session
--session-dir <dir>            Directory for session storage and lookup
--no-session                   Don't save session (ephemeral)
--name, -n <name>              Set session display name
```
Storage (`session-format.md`): `~/.pi/agent/sessions/--<path>--/<timestamp>_<session-id>.jsonl`; `<path>` = cwd with leading `/` removed and `/ \ :` replaced by `-` (e.g. `/Users/iris` -> `--Users-iris--`). Override: `--session-dir`, env `PI_CODING_AGENT_SESSION_DIR`, setting `sessionDir`. JSONL header line: `{"type":"session","version":3,"id":"uuid","timestamp":...,"cwd":"/path"}`; entries form an id/parentId tree. Sessions auto-save unless `--no-session`.

cwd dependence: `--continue` = "most recent session for the current working directory" (`sessions.md`), so it IS cwd-scoped. `--session <path>` and rpc `switch_session` take a full path and load it regardless of cwd (Waku relies on this; the process cwd stays the chat's cwd, `pi.rs:268`). Beware: resuming a file whose header `cwd` differs from the process cwd still runs tools in the process cwd.

RPC (`rpc-commands.md`):
- `{"type":"get_state"}` -> `data.sessionFile` (abs path), `data.sessionId`, `data.sessionName`, `model`, `thinkingLevel`, `isStreaming`, `isCompacting`, `steeringMode`, `followUpMode`, `messageCount`, `pendingMessageCount`.
- `{"type":"switch_session","sessionPath":"/path/x.jsonl"}` -> `data.cancelled`.
- `{"type":"new_session"[,"parentSession":path]}`; `set_session_name {name}`; `get_messages`; `get_entries {since?}` (durable cursor); `get_session_stats`.

Multiple turns on one process: yes. RPC is "a long-lived subprocess" (`rpc.md`). `prompt` -> response `data.disposition` `"started"|"queued"|"handled"`; `agent_end` ends one low-level run; `agent_settled{aborted}` means nothing more will run automatically (retries, compaction, queued messages done). Wait for `agent_settled`, not `agent_end`. Close stdin = orderly shutdown (`rpc.md` Shutdown).

Waku:
- One process per live session, kept across turns: `PiDriver::start` spawns once (`pi.rs:257-266`), writer thread loops `command_rx.recv()` handling `Prompt` repeatedly (`pi.rs:~521`); daemon calls `driver.prompt()` per turn (`daemon.rs:1718` area). Dropping the driver sends `Shutdown` (`pi.rs:687, 834`).
- Persisted cursor `ProviderResumeCursor::Pi{session_id, session_file}` (`waku-protocol/src/model.rs:215`), built from `get_state` `data.sessionId`/`data.sessionFile` (`pi.rs:936-945`), emitted in `DriverEvent::Connected` (`pi.rs:~470`). Missing sessionId = init failure (`pi.rs:456`).
- Resume = spawn fresh process (cwd = chat cwd), `get_state`, then `switch_session {sessionPath}`; `cancelled:true` is an error (`pi.rs:384-408`); missing `session_file` in cursor = refuse (`pi.rs:226-231`). Handshake order: `get_state` -> `switch_session?` -> `set_model?` -> `set_thinking_level?` -> `get_state` -> `get_session_stats` -> `get_commands` (`pi.rs:366-505`). Timeouts: 10 s control RPC (`pi.rs:28`), 30 s handshake (`pi.rs:35`); `prompt` is sent without waiting so `abort` can interleave (`pi.rs:881-905`).
- Prompt-response bookkeeping: `PendingResponse::Prompt` id retained until `agent_settled`; a failed `success:false` response ends the turn with error (`pi.rs:1284-1315`); stale prompt ids evicted on the next prompt (`pi.rs:893`). Current Pi uses `data.disposition:"handled"` (old: `agentInvoked:false`, `pi.rs:1287`); Waku only checks the old field, so a handled slash command may hang until process end. Handle both.

Aether implication: with `--session <path>`/`switch_session` aether can drop `context_prompt(history, text)` re-sending entirely.

## 2. Steer + queue

Docs (`rpc-commands.md`):
- `{"type":"steer","message":"..."}` (optional `images: [{"type":"image","data":"<base64>","mimeType":"image/png"}]`, the same field on `prompt` and `follow_up`, per Pi 1.3.0 `docs/rpc-commands.md`): delivered "after the current assistant turn finishes executing its tool calls, before the next LLM call". Response `data.disposition` `"queued"|"handled"`. Extension commands not allowed.
- `{"type":"follow_up","message":"..."}`: delivered "only when agent has no more tool calls or steering messages" (after the run finishes).
- `{"type":"prompt","message":...,"streamingBehavior":"steer"|"followUp"}`: prompt during streaming without `streamingBehavior` returns an error (`success:false`).
- `clear_queue` -> `data:{steering:[...],followUp:[...]}` (send before `abort` to restore text); `abort` (waits for idle; queued messages continue if still in session); `set_steering_mode`/`set_follow_up_mode {mode:"all"|"one-at-a-time"}` (default one-at-a-time).
- Event `queue_update{steering:[..],followUp:[..]}` carries the complete current queues (`json.md`). `get_state.pendingMessageCount` too.

Waku sends only `steer`; never `follow_up`:
- `CommandMessage::Steer` -> `send_request {"type":"steer","message":prompt}`; Ok -> `DriverEvent::SteerAccepted{message}`, Err -> `SteerRejected{message,reason}` (`pi.rs:541-560`). `supports_steer()` = true (`pi.rs:750`); daemon routes `Command::Steer` to it (`daemon.rs:1718`, events `daemon.rs:1915`).
- Waku ignores `queue_update` (falls into `_ => {}`, `pi.rs:~1556`). Queued follow-ups are app-level: Waku holds them and sends a normal `prompt` after `TurnFinished`.
- The steered user message shows up as `message_start/message_end` with `role:"user"` mid-run; Waku does not render it from events (UI shows it on `SteerAccepted`).

## 3. Access modes

Pi has no permission system and no approval prompts by default (`security.md`: "it does not ask for approval before every tool call"; "lacks a built-in sandbox").
- `--approve, -a` = "Trust project-local files for this run" (project `.pi/` config/resources/mcp), NOT tool approval. `--no-approve, -na` ignores them. Waku passes `--approve` always (`pi.rs:62-65, 257`); trust persists in `~/.pi/agent/trust.json` (`{"/path": true}`). Pi has no `--yolo` (that is Oh My Pi).
- Tool gating flags (verbatim):
```
--no-tools, -nt                Disable all tools by default (built-in and extension)
--no-builtin-tools, -nbt       Disable built-in tools by default but keep extension/custom tools enabled
--tools, -t <tools>            Comma-separated allowlist of tool names or patterns (*) to enable
                               Keeps MCP tools unless an entry starts with mcp__
--exclude-tools, -xt <tools>   Comma-separated denylist of tool names or patterns (*) to disable
                               Applies to all tools, MCP tools included
```
  Built-ins: read, bash, edit, write, grep, find, ls. Help example: `pi --tools read,grep,find,ls -p "..."` = "Read-only mode".
- Approval over RPC only via extensions: `extension_ui_request` with `method` `select|confirm|input|editor` (blocking, optional `timeout` ms; auto-resolves to undefined/false on timeout), answered with `{"type":"extension_ui_response","id":<req id>,"value":"..."|"confirmed":true|"cancelled":true}` (`rpc-extension-ui.md`). Doc example: `select` "Allow dangerous command?" `["Allow","Block"]`. Only appears if the user has a permission-gate extension loaded; fire-and-forget methods (`notify`, `setStatus`, `setWidget`, `setTitle`, `set_editor_text`) also arrive (confirmed live: ponytail extension emitted `setStatus` + `notify` at startup).
- Waku mapping: Pi accepts Full access ONLY. `mode != FullAccess` -> start error "currently supports Full access only" (`pi.rs:220-225`); `apply_options` returns false for other modes, forcing restart (`pi.rs:770-779`). Dialog requests are auto-cancelled (`CommandMessage::CancelExtensionRequest` -> `extension_ui_response{id,cancelled:true}`, `pi.rs:1515-1525, 643`), so gated extension actions are denied. `respond()` is a no-op (`pi.rs:768`).
- Feasible mapping for aether: Ask/AutoAcceptEdits/Auto cannot be enforced natively. Options: (a) Full only, hide others for Pi (Waku); (b) Ask-ish = read-only via `--tools read,grep,find,ls` (spawn-time, restart on change; or `--exclude-tools bash,edit,write`); (c) surface `extension_ui_request select/confirm` as Ask::Approval/Question and answer with `extension_ui_response` (only works when an extension asks).

## 4. Effort (thinking)

- CLI: `--thinking <level>`  "Set thinking level: off, minimal, low, medium, high, xhigh, max". Also `--model <pattern>` "supports "provider/id" and optional ":<thinking>"" e.g. `--model sonnet:high`.
- RPC: `{"type":"set_thinking_level","level":"high"}`; levels `off|minimal|low|medium|high|xhigh|max`; `xhigh`/`max` only if the model supports them. `get_available_thinking_levels` -> `data.levels` (live result for kimi-k2.6: `["off","minimal","low","medium","high"]`; `["off"]` if no reasoning). `cycle_thinking_level`; event `thinking_level_changed{level}`; `get_state.thinkingLevel`.
- Per-model support from `get_available_models` -> `data.models[]` Model objects: `id`, `name`, `provider`, `reasoning` (bool), `input`, `contextWindow`, `maxTokens`, `cost`, `thinkingLevelMap` (level -> null means unsupported; absent = supported; `xhigh`/`max` require a non-null entry). Waku `pi_reasoning_options` (`model_catalog.rs:1160-1190`): if `reasoning != true` no efforts; else filter the 7 levels by `thinkingLevelMap` (xhigh/max only when mapped non-null). Default effort = current `thinkingLevel` when model is the `get_state` default, else `medium`, else first (`model_catalog.rs:1118-1140`).
- Waku applies at start (`set_thinking_level` after `set_model`, `pi.rs:~420`) and live on option change (`pi.rs:~574-640`); model switch live via `set_model {provider, modelId}` (response `data.contextWindow` -> usage event). Model slug is `provider/model_id`, split on FIRST `/` (`parse_model_slug`, `pi.rs:~905`; note OpenRouter ids contain more `/`, e.g. provider `openrouter`, id `moonshotai/kimi-k2.6`).
- Model discovery spawns a throwaway `pi --mode rpc --no-session --no-skills --no-prompt-templates --no-context-files ...` then `get_available_models` + `get_state`, kills it (`model_catalog.rs:1016-1085`).

## 5. Tool activity events (verified in `json.md`)

```
{"type":"tool_execution_start","toolCallId":"call_abc123","toolName":"bash","args":{"command":"ls -la"}}
{"type":"tool_execution_update","toolCallId":"call_abc123","toolName":"bash","args":{...},"partialResult":{"content":[{"type":"text","text":"partial output"}],"details":{}}}
{"type":"tool_execution_end","toolCallId":"call_abc123","toolName":"bash","result":{"content":[{"type":"text","text":"complete output"}],"details":{}},"isError":false,"durationMs":12}
```
Whether `partialResult` replaces or extends depends on the tool (bash: treat as latest snapshot). Nested tool calls carry `parentToolCallId` and ids `<parent>/<n>`. Model-side: `message_update.assistantMessageEvent` types `toolcall_start{id,toolName}`, `toolcall_delta`, `toolcall_end{toolCall}` (earlier than execution start). Results in transcript: `role:"toolResult"` messages.

Known arg names (from Pi built-ins; verify against a real run): bash `{command, timeout?}`, read `{path, offset?, limit?}`, edit `{path, oldText, newText}`, write `{path, content}`, grep `{pattern, path?, glob?}`, find `{pattern, path?}`, ls `{path?}`.

Waku (`pi.rs:1449-1511`): on any of the 3 events -> one `DriverEvent::RichActivity(activity::tool_activity(id, kind, title, args, output, output, failed, complete))`. `kind = ActivityKind::from_tool_name(name)` (`waku-protocol/src/model.rs:1745`: normalizes name, last `__`/`:`/`.`/`/` segment; bash->Command, edit/write/create/patch->file change, read/grep/find/ls->file read/search/list, todo/plan->Plan, else Tool). `title = tool_title(name)` (`pi.rs:1602`): bash "Run command", edit "Edit file", write "Write file", read "Read file", grep "Search files", find "Find files", ls "List files", else raw name; overridden by `args.title` (also `arguments.title`, `input.title`) if present (`driver/activity.rs:54`). Title/kind cached per toolCallId from start (`state.tools`) and removed on end. `output`: `partialResult` on update, `result` on end; formatted by `activity.rs` (`format_output`: string, `structuredContent`, else joined text content), truncated at 16000 chars (`activity.rs:8`); `failed = isError == true`; failed `detail` = first non-empty output line (`activity.rs:34-41`); image urls collected. A tool error alone never fails the turn (turn failure only from `assistantMessageEvent.type=="error"` or `auto_retry_end{success:false,finalError}`, `pi.rs:~1420, 1500`). There is no per-tool arg summary (no "path" in title); the UI shows `arguments` JSON.

Text/reasoning streaming (same handler): `message_update.assistantMessageEvent` `text_delta{delta}` -> TextDelta, `thinking_delta{delta}` -> ReasoningDelta, `error{reason|error}` -> Error; `message_end` role assistant -> usage + gap-fill `content[]` text/thinking blocks when no deltas were seen (`pi.rs:1425-1445, 1600-1650`). Context usage: `message_end.message.usage` via `pi_message_context_tokens`. `session_info_changed{name}` -> title. `agent_start`/`turn_start` -> TurnStarted. `agent_settled` -> TurnFinished (`pi.rs:1368-1380`).

## 6. Per-chat cwd

- No flag: Pi takes the project dir from the process cwd. Waku: `command.current_dir(&cwd)` (`pi.rs:268`). Also determines session dir bucket (`--<cwd>--`), `--continue` lookup, AGENTS.md/CLAUDE.md context discovery, and trust (`trust.json`, project `.pi/`).
- cwd cannot change on a live process. A chat whose cwd changes needs a new process (+ new session; `switch_session` to an old-cwd file keeps tools in new cwd). Aether `send_cli` already derives cwd from `settings.workspace_root` (fallback `home_dir()`); must become per-chat and be part of the process key.
- Ephemeral probes (model list) should use `--no-session` and a neutral cwd so they do not create session files.

## Aether changes needed

1. Rewrite `crates/agent/src/driver/pi.rs` to the real wire (see section 0): drop `negotiate`; `prompt{id:"aether-N",message}`; `set_model{provider,modelId}`; parse `message_update`/`message_end`/`tool_execution_*`/`agent_start`/`agent_settled`/`auto_retry_end`/`session_info_changed`; drop fake-shape tests. Spawn `pi --mode rpc --approve` + `PI_SKIP_VERSION_CHECK=1`, cwd = chat cwd, stderr kept.
2. Handshake like Waku (`pi.rs:366-505`): `get_state` -> `switch_session?` -> `set_model?` -> `set_thinking_level?` -> `get_state` (read `sessionId`/`sessionFile`) with 30 s timeouts; request-id -> response map; do not block the writer on `prompt` so `abort` works mid-turn. Complete the turn on `agent_settled`, not `agent_end`; also finish when prompt response is `success:false` or `data.disposition:"handled"`.
3. Change the `Driver` trait from "one driver per message" to long-lived: add `steer(text)`, `apply_options(model, effort)`, keep `prompt`/`cancel`/`answer`; `cancel` = rpc `abort` (kill only as fallback after timeout; today it kills at 3 s, which would end the live session).
4. `agent.rs send_cli`: hold a per-chat driver map (key: chat id + provider + cwd + binary), spawn on first send, reuse for later sends; stop building `context_prompt(history, text)` for Pi (send only the new user text; session file holds history); only rebuild/seed transcript if no session cursor exists (e.g. first run after upgrade or switch from another provider). Terminate on chat delete, provider/cwd change, app exit (close stdin, then kill).
5. Persist per chat `{pi_session_file, pi_session_id}` (new field on chat model in core) captured from `get_state`; on restart spawn + `switch_session{sessionPath}`; if file missing or `cancelled`, fall back to fresh session + transcript reseed. Optionally `--name <chat title>` / `set_session_name`.
6. Steer/queue: while a turn runs, user message -> `steer` (maps to Waku). Optional queued "after finish" -> `follow_up` (Waku does it app-side; `follow_up` is native and survives, and `queue_update{steering,followUp}` gives the queue state to display; `clear_queue` on cancel to restore the text). Do not send plain `prompt` mid-run (error without `streamingBehavior`).
7. Access modes: Pi has no native approvals. Either restrict the picker to Full access for Pi (Waku), or map read-only/Ask-like to `--tools read,grep,find,ls` / `--exclude-tools bash,edit,write` at spawn (restart on mode change). Replace the fake `tool_approval` -> `Ask::Approval` path with real `extension_ui_request` `select|confirm|input` -> `Ask`, answered by `extension_ui_response{id,value|confirmed|cancelled}`; auto-cancel unknown dialogs; ignore `notify/setStatus/setWidget/setTitle/set_editor_text`.
8. Effort: new setting per provider; list via `get_available_models` (`reasoning`, `thinkingLevelMap`) or `get_available_thinking_levels` for the current model; apply with `--thinking <lvl>` at spawn or `set_thinking_level` live. Model slug `provider/id` split on first `/` only.
9. Tool rows: replace `tool_use` fence hack with `tool_execution_start/update/end` keyed by `toolCallId`; titles per name (bash Run command, read Read file, edit Edit file, write Write file, grep Search files, find Find files, ls List files); show args (`command`, `path`, `pattern`) as summary and `partialResult`/`result` content text as output; `isError` marks failure; do not fail the turn on tool error. Needs a new `Update` variant for tool activity (`aether_core::Update` has none).
10. cwd per chat: pass chat cwd to spawn; include in driver key; model-discovery probe uses `--no-session --no-context-files` in a temp/home cwd.
