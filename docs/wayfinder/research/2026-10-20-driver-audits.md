# Driver and transcript audits (2026-10-20)

Five read-only audits ran before the revamp spec (`docs/superpowers/specs/2026-10-20-agent-compat-chat-revamp-design.md`). No paid prompts were used. File:line references point at commit `8bdca2f`, so they drift as phases 1 and 2 land. Installed versions at the time: Claude Code 2.1.304, Codex 0.163.0, OpenCode 2.0.32, Pi 1.3.0, agy 1.4.2.

## Claude Code driver (`crates/driver/src/claude.rs`)

Ground truth came from three sources:
- the embedded JS of the 2.1.304 binary
- the SDK 0.3.220 type definitions (`sdk.d.ts`, `sdk-tools.d.ts`), under "~/Library/Application Support/Code - Insiders/agent-host/sdk-cache/claude/0.3.220/…"
- `~/.claude/projects/**/*.jsonl`: 624 files with `isSidechain`, 82 with `run_in_background`, 205 with task-notification

### Broken or fragile

1. **Subagent activity is dropped.** The `!sub` guards at :504/:538/:549/:560/:567 skip every line that has a `parent_tool_use_id`. The 2.1.304 CLI emits first-level subagent tool_use and tool_result lines carrying `parent_tool_use_id`, `agent_id` and `subagent_type`. The user sees only the Agent row's final text, including the "agentId: … (internal ID - do not mention to user…)" trailer. Subagent `can_use_tool` asks still arrive, but there is no row for them.
2. **Background shells and agents never settle.** The tool_result arrives immediately ("Command running in background with ID: …" or "Async agent launched…"), and the row is marked Done. Then:
   - The later `system/task_notification {task_id, tool_use_id, status, summary, output_file, usage}` is dropped at :536. So are `task_started`, `task_progress`, `task_updated` and `background_tasks_changed`.
   - The 600 s reaper kills sessions whose background jobs run longer than 10 minutes, because the dropped task events never refresh `last_active`.
3. **TodoWrite is disabled by default in 2.1.304.** The CLI uses `TaskCreate{subject,description,activeForm}` and `TaskUpdate{taskId,status}` instead, which show as raw JSON rows. A persistent list keyed by task id is needed.
4. **Error results lose their message.** The error subtypes (`error_during_execution`, `error_max_turns`, `error_max_budget_usd`) carry `errors: string[]` and no `result`. :626-633 reads only `result`.
5. **"Always allow" forwards every `permission_suggestions` entry** with destination "session" (:435-441). For Write/Edit asks, that entry is `{type:"setMode",mode:"acceptEdits"}`. Fixed in phase 1.
6. **Editing a Bash approval truncates it.** The edit sends only the first line, cut at 200 chars (:430-433). Fixed in phase 1.
7. **`control_cancel_request` leaves stale asks.** At :592 the driver removes the ask without telling the UI.
8. **The cancel watchdog** (:278-287) marks the session dead without emitting an Error and without killing the child. The `--resume` respawn then overlaps the old process.
9. **Assistant text blocks are ignored** (:549-559). On stream idle timeout the CLI falls back to a non-streaming request, so text before tool calls is lost and partial text is duplicated.

### Dropped or flattened

- **Dropped `system` subtypes:**
  - `api_retry {attempt, max_retries, retry_delay_ms, error_status, error}`
  - `status {status:"compacting", permissionMode, compact_result, compact_error}`
  - `permission_denied {tool_name, tool_use_id, decision_reason, decision_reason_type}`
  - `informational {content, level, prevent_continuation}`
  - `notification`, `model_fallback`, `local_command`, `memory_recall`, `commands_changed`
  - `model_refusal_fallback`, whose `retracted_message_uuids` should retract the refused output
- **Dropped other events:** `rate_limit_event {status: allowed_warning|rejected, resetsAt, utilization, rateLimitType}`, and `hook_*` events (`--include-hook-events` is not passed).
- **Collisions:** the compact row id `compact-{turn}` (:530) collides when two compactions happen in one turn.
- **Elicitation and dialogs:** control_request `elicitation` (MCP auth and forms) and `request_user_dialog` get an "unsupported" error reply (:649-655).
- **Unused result and init fields:**
  - `result.total_cost_usd`
  - `modelUsage` per model (subagent spend)
  - `permission_denials`, `terminal_reason`
  - `init.mcp_servers[].status` (failed, needs-auth), `init.permissionMode`, `init.capabilities`
  - `init.model` is never emitted as the served model
  - `Count` uses main-loop usage only (:620-625)
- **Tool payloads:**
  - Images in tool_result are dropped by `result_text` (tool.rs:305-313).
  - The Skill tool becomes a generic row instead of `Update::Skill`.
  - NotebookEdit shows as raw JSON.
- **Asks:** AskUserQuestion ignores `multiSelect` (:658-666). ExitPlanMode discards its permission suggestions.
- **Unused handshake:** the `initialize` control request is never sent. So `sdkForwardNestedSubagentToolCalls`, `agentProgressSummaries` and `supportedDialogKinds` stay off, and `--forward-subagent-text` is unused.

### Launch and compat

- The driver relies on hidden flags: `--thinking-display` and `--permission-prompt-tool stdio`, which the help text now points to `--permission-prompts host` for. `default` is no longer listed in `--permission-mode` help ("manual" is the public alias). There is no version check.
- The CLI can refuse `--permission-mode auto` ("auto mode is unavailable for your plan/while fast mode is on/for this model"). The driver never reads `init.permissionMode`.
- `session_exists` (:371) only checks that the file exists. A resume can still fail ("No conversation found…", "…is running in the background… Add --fork-session"). There is no fresh-session fallback.
- `new_uuid` (:379) falls back to a fixed UUID if `/dev/urandom` is unreadable.
- The fixture `claude_tools.jsonl` has 15 main-thread lines and nothing else.

## Codex driver (`crates/driver/src/codex.rs`)

The protocol is `codex app-server` over stdio, JSON-RPC v2, with `experimentalApi:true`. Ground truth:
- the schemas from `codex app-server generate-json-schema` (regenerate as needed)
- 109 rollouts in `~/.codex/sessions`
- `~/.codex/logs_2.sqlite`, which records events sent to the `aether` client

### Broken or fragile

1. **MCP tool approvals are auto-declined.** Every `mcpServer/elicitation/request` is declined (:793-797). MCP approvals use `_meta.codex_approval_kind:"mcp_tool_call"`. Fixed in phase 1.
2. **Turn boundaries aren't tied to turn ids.**
   - `turn/completed` checks only `s.active` (:545), and `turn/started` checks only `turn_id.is_none()` (:525).
   - Cancel followed by an immediate prompt lets the old turn's completion end the new turn. The `turn/start` reply then hits `Call::Turn if !s.active`.
   - A failed `turn/steer` always starts a new turn (:611-615), even on `activeTurnNotSteerable` during /review or /compact.
3. **Background terminals.** Running rows are drained and marked Stopped at turn end (:546-552). Dev servers can finish up to 13202 s later. Their late `item/completed` then vanishes or shows up as a new row in a later reply (:829). `thread/backgroundTerminals/list|terminate` are unused.
4. **Subagents.**
   - Rows are drained at the parent's turn end (:707). The test asserts this drop (tests/unit/codex.rs:673).
   - Child output deltas, errors and usage are dropped.
   - Grandchildren are never adopted (adopt is only called at :700). They are discarded via `side_event` (:483-489).
   - A child's fileChange approvals show empty paths (:776-781).
5. **`item/permissions/requestApproval` gets -32601** (:798-801). Fixed in phase 1.
6. **Per-turn `sandboxPolicy` sends only `{"type":…}`** (:400-406). The schema defaults (`writableRoots:[]`, `networkAccess:false`) then override the user's config.
7. **Questions and decisions.**
   - Only the first of several questions is asked (:785-791). `isSecret` and `isOther` are ignored.
   - "Always" sends `acceptForSession` without checking `availableDecisions`. A real payload offers `accept`, `{acceptWithExecpolicyAmendment:{execpolicy_amendment:[…]}}` and `cancel`.
   - `proposedExecpolicyAmendment` and `networkApprovalContext.host` are never shown.
8. **The crash reason keeps only the last stderr line** (:124-128). For a Rust panic that line is "note: run with RUST_BACKTRACE=1…".
9. **Quadratic output cleaning.** Each output delta re-cleans the whole unbounded `s.output` under the lock (:504-510).
10. **Resume failure is silent** (`.ok()` at :190). A thread locked by another client gives a fresh thread with no notice.

### Dropped or flattened

- Server-started turns never send Wake (:525/:545). This hides queue turns (`thread/queue/changed`) and goal turns (`thread/goal/updated`).
- **Items that produce no row:**
  - `exitedReviewMode{review}`: findings. Likely why /review shows nothing.
  - `imageView` (79 in rollouts), `imageGeneration{savedPath,revisedPrompt}`
  - `sleep` (38)
  - `plan` and `item/plan/delta`: plan mode. `collaborationMode` is never sent.
  - `hookPrompt`, `dynamicToolCall`
  - `collabAgentToolCall` for wait, sendMessage, followupTask, closeAgent and listAgents (wait 123, send_message 112)
  - `subAgentActivity` interacted and completed
  - `agentMessage.questions` (18). The `phase` field is ignored, so commentary merges into the final answer.
- **Notifications dropped at the catch-all (:586):**
  - `error` with `willRetry:true`
  - `turn/diff/updated`
  - `account/rateLimits/updated`
  - `model/rerouted`, `model/verification`
  - `warning`, `configWarning`, `deprecationNotice`, `guardianWarning`
  - `hook/started` and `hook/completed` (78 each in the aether logs; the user has hooks)
  - `item/autoApprovalReview/*` and `autoApprovalReview/strictReviewRequired`
  - `serverRequest/resolved`
  - `item/commandExecution/terminalInteraction`
  - `item/fileChange/patchUpdated` and `outputDelta`
  - `item/mcpToolCall/progress`
  - `thread/status/changed`
- **Partly mapped:**
  - Raw reasoning (`textDelta`) is mixed with the summary.
  - `fileChange` `move_path` (renames) is ignored.
  - MCP results keep only text (no image, no `structuredContent`).
  - The plan's `explanation` is dropped.

### Launch and compat

- **Experimental surface:** app-server is experimental and nothing checks `InitializeResponse.userAgent`. `currentTime/read` and `item/tool/call` get -32601.
- **Auth:** there is no `account/read`, so logged-out users only learn of it from a failed turn.
- **Timeouts:** the handshake deadline is 30 s (:181), and the 10 s cancel watchdog marks the driver dead (:283-290).
- **Private interfaces:**
  - The catalog parses the private `models_cache.json` (catalog.rs:384) instead of calling `model/list`.
  - Titles come from any model whose name contains "luna" (:619), at an extra paid call per new chat.
  - usage.rs:505 calls a private ChatGPT endpoint with a hard-coded `codex/0.163.0` User-Agent.
- **Platform:** `unwrap_shell` assumes `sh -lc` (:950-958).

## OpenCode driver (`crates/driver/src/opencode/`)

Ground truth:
- the live `/openapi.json` from a temp `serve`
- event schemas extracted from the 2.0.32 binary
- the `opencode.db` tables `session_v2` and `session_message`

2.0.32's `/api/event` emits v2 `session.*` events. The v1 names exist only in the legacy layer.

### Broken or fragile

1. **Subagent permissions and questions are dropped,** so the parent hangs forever. `session.rs:101-106` drops frames from other sessions, and child sessions inherit the parent's ask rules. Live proof: session `ses_ea8dcccf3fff69ivQR29E4nlk1` was stuck on pending requests on its children. Fixed in phase 1.
2. **Resume never lists pending asks or a running execution** (`session.rs:34-61`).
3. **`rules()` returns `[]` for ReadOnly and Auto** (`session.rs:276-277`). Only offered modes are reachable, but the arms are unsafe. Fixed in phase 1.
4. **"Always" is stored per action** (`mod.rs:192-194`, `events.rs:129`), so one "always" on a shell command approves every later shell command. OpenCode's own `always` saves only the request's `save` patterns. Fixed in phase 1.
5. **Shell approvals merge commands** into one line (`events.rs:147`). `message`, `metadata.files[].patch` and `save` are ignored. Fixed in phase 1.
6. **Service crash or stop hangs the turn.** The reconnect loop retries every second forever with no deadline (`session.rs:112-123`). `execution.interrupted{reason:"shutdown"}` is ignored (`events.rs:65`).
7. **No Wake on `session.execution.started`** when no turn is active. Background `shell` and `subagent` calls say "you will be resumed automatically", and that resumed work is then lost.
8. **Edit diffs.** Edit and patch metadata in 2.0.32 is `files:[{file,patch,status,additions,deletions}]`, but `events.rs:258` reads only `meta["diff"]`. `write` metadata no longer has `exists`.
9. **`set_options` returns true** when `model_ref` is `None` (`mod.rs:149-158`), so switching back to the default model does nothing, and an effort-only change is dropped.

### Dropped or flattened

- **Nested subagent activity.** `subagent` (input `{agent,description,prompt,model,background,sessionID}`) isn't classified as Task, because `tool.rs:81` matches only `task` and `agent`. Its first `session.tool.progress` carries `metadata:{sessionID:<child>,status:"running"}`, which is how to follow the child.
- **Unmapped events and tools:**
  - `session.retry.scheduled {attempt,at,error}`
  - `execute` (code mode): input `{code}`, nested `metadata.toolCalls`
  - the `skill` tool and `session.skill.activated`
  - step `snapshot` and `files`, and `GET /api/session/{id}/diff`
  - cost from `step.ended.cost`, compaction, `usage.recorded`
  - content-filter `step.failed`, which is suppressed
- **Forms:** `ask_form` ignores `hidden`, `when`, `required`, `default` and the external `url`. Multiselect accepts one value.
- **Attachments:** images are ignored (`mod.rs:84,114`). `/prompt` accepts `files:[{uri}]`.
- **Also dropped:**
  - `text.ended`
  - `model/agent.selected`, `session.synthetic`, `inbox.enqueued`, `shell.*`
  - asks answered elsewhere (`events.rs:152-160`)
- **Collisions:** the compaction id is always `"compaction"`.

### Launch and compat

- **Version gate:** only `/api/info` returning 200 is checked. A 1.x install fails with a generic timeout, and stdout and stderr go to null.
- **Service environment:** Aether may start the shared service with a GUI environment, forwarding only `OPENROUTER_API_KEY`.
- **Event stream:** each chat opens its own `/api/event` stream, which carries every session. Missed deltas are not replayed.
- **APIs to use:** `GET /api/session?parentID=…` lists children. `GET /api/permission/request` and `GET /api/form` list pending items globally. `GET /api/session/active` lists running sessions.

## Pi (`crates/driver/src/pi.rs`) and Antigravity (`crates/driver/src/antigravity.rs`)

Pi ground truth: the 1.3.0 docs and types under `~/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/pi-coding-agent`, in particular `docs/rpc*.md` and `dist/modes/rpc/rpc-types.d.ts`. agy is installed at `~/.local/bin/agy`, version 1.4.2. Its `--help` shows no per-action approval channel; `--sandbox` exists.

### Broken or fragile

1. **Steers land in the wrong bubble** (both drivers). `Wake` fires when the steer is sent (pi.rs:228-235, antigravity.rs:248), but the CLI consumes it later:
   - Pi consumes it at `message_start` with role user, or on `queue_update`.
   - agy runs it as its own queued turn, marked by a `user_input` step.
2. **Pi cancel loses queued steers.** The `clear_queue` reply is discarded (pi.rs:274).
3. **Pi cancel with an open editor dialog kills the session.** The editor has no abort, so the 10 s watchdog marks it dead (pi.rs:276-285).
4. **Pi dialog `timeout` is ignored** (pi.rs:500-529), so stale cards remain.
5. **Pi runs the user didn't start are dropped.**
   - `agent_start` without an active turn sends no Wake (pi.rs:564-568).
   - A run started by an extension command more than 1.5 s later is dropped (pi.rs:333, 427-428).
6. **agy approval is a blanket grant.** It respawns with `--dangerously-skip-permissions` and injects a message (antigravity.rs:299-306). Only the last empty-output tool is marked Denied (:543, :593-597). Phase 1 makes this warned and complete.
7. **Stop blocks during restarts.** agy cancel sends SIGINT, and the respawn blocks the driver thread up to 45 s (init) plus 3 s (:167-169, :330-339). Pi's handshake blocks up to 30 s (pi.rs:140).
8. **agy drops events outside an active turn** (antigravity.rs:491-492). A background-answer turn's `result` can end the user's turn early (:591, :603).
9. **Pi stderr.** Only lines containing error:, fatal or no such file pass through (spawn.rs:21-24), and each one fails the reply. Startup failures show a generic handshake error (pi.rs:133-135, 177-179).
10. **agy resume of a missing conversation** may loop (:102, :232-238).

### Dropped or flattened

Pi (catch-all at pi.rs:574):
- **Dropped events:** `auto_retry_start/end`, `summarization_retry_*`, `queue_update`, `thinking_level_changed` (`set_thinking_level` clamps silently) and `entry_appended`.
- **Extension UI:**
  - `notify` info and warning messages are dropped (pi.rs:521-525). This includes the MCP sign-in URL at `dist/extensions/mcp/index.js:955` and the `/mcp` status at :1151.
  - `setStatus`, `setWidget`, `setTitle` and `set_editor_text` are dropped too.
- **Tool calls:**
  - `toolcall_start/delta/end` argument streaming is ignored (pi.rs:436-444).
  - Tool `details` are dropped (pi.rs:486, 494). That includes edit `details.patch`, a real unified diff (edit.d.ts:17-24); bash truncation; and `fullOutputPath`.
  - `parentToolCallId` and ids like `<parent>/<n>` show as top-level rows.
- **Results:**
  - `usage.cost.total` is dropped (pi.rs:462-469).
  - `stopReason: length` is silent (pi.rs:460).
  - The compaction reason, summary and `tokensBefore` are dropped (pi.rs:542-563).
  - `custom` and `bashExecution` messages are dropped.
- **Commands:** built-in TUI commands and `!cmd` are sent to the LLM as plain text. Only /compact is intercepted (pi.rs:361-365).
- **Sessions:** a session switch made by an extension is not tracked.

agy:
- **No thinking text,** although every usage block has `thinking_tokens`. Whether stream-json carries the text is unverified.
- **Only `agent_response` and tool steps are mapped** (:497-520).
- **Tools that fall through to raw Other rows** (:550-583): `invoke_subagent`, `browser_subagent`, `command_status`, `send_command_input`, background `run_command` (WaitMsBeforeAsync), `manage_task` and `generate_image`.
- **`view_file`** drops StartLine and EndLine (:553).
- **`search_web`** reads lowercase `query` while every other key is PascalCase (:580).
- **No `Update::Window`,** so the UI falls back to 1,048,576 tokens.
- **Images** are not sent inline. They go through `--add-dir`.

### Launch and compat

- **Mode:** agy's log shows "`--mode auto` is unavailable (rollout flag off), continuing in the default mode", while the UI still shows Auto.
- **Wrong binary:** the fallback to an `antigravity` binary is probably the IDE launcher.
- **Brittle internals:**
  - The TERMINAL_ONLY list of slash commands is hard-coded (:20-65).
  - The title is scraped from `annotations/<id>.pbtxt` (:411-439).
- **`--add-dir`:** the shared attachments dir exposes every chat's attachments.
- **No version checks.**
  - Pi needs `agent_settled` (0.80.4 or later) and `disposition` (0.99.0 or later), and sets `PI_SKIP_VERSION_CHECK=1` (pi.rs:69).
  - agy needs 1.1.15 or later for stream-json input and 1.1.27 or later for `denied_actions`.
- **Pi launch:**
  - There is no `--approve`, so project-local extensions are silently skipped (docs/security.md:75-80).
  - Read only is only a tool allowlist (pi.rs:337-340).
  - Resume spawns a fresh session and then calls `switch_session` (pi.rs:142-146), instead of passing `--session`.
  - The catalog probe passes `--no-skills --no-prompt-templates` (catalog.rs:151-158).
- **Binary paths:** an explicit path is checked with `is_file()` only (paths.rs:45-47).

## Transcript pipeline (`Update` → `Message` → views)

The reducer is `Message::apply` (core lib.rs:375-459), driven by `ChatApp::apply` (app chat/send.rs:424-465). The agent pump swallows `Wake` (agent.rs:628-647).

### Bugs

1. **Tool output is wiped when a reply finishes.** `finish()` runs `cap_tool_payload()` on the live message (lib.rs:479-525). Fixed in phase 2.
2. **Steering marks finished tools Stopped and duplicates them.** The run counter bumps (send.rs:196), the pump swaps the sender on Wake (agent.rs:633-636), and the old reply's `finish()` stops its rows. Fixed in phase 2.
3. **Any error fails the reply,** blocks the queue (send.rs:363) and is written into `text` (lib.rs:451-455). Fixed in phase 2 (Warning versus Error).
4. **Mid-run persistence and restore are broken.** Fixed in phase 2.
5. **A later thought can land above text it came after** (lib.rs:395-396). Fixed in phase 2.
6. **`thought_for` includes tool time,** and only the first thought gets a label (lib.rs:527-531, message.rs:654-660).
7. **Approval cards show a one-line summary** (claude.rs:675, 689; lib.rs:177-181). Fixed in phase 1.
8. **Plan cards number every markdown line** (claude.rs:668-670; ui/chat/request.rs:225-248).
9. **Streaming cost.**
   - `markdown()` re-runs `link_inline_code`, a full mdast parse plus `path.exists()` per code span, on the UI thread (message.rs:836-845; links.rs:61-80, 111-119).
   - `parse_markdown_blocks` runs every frame for every visible message (message.rs:800).
   - `AssistantMessage::new` clones the steps twice (message.rs:282-291).
   - Every update triggers a remeasure (send.rs:451-452).
   - A whole reply is one list item.
   - `LIVE_ROWS` (30) lives in `ThinkingPanel` (thinking.rs:19, 202), which is unused. `on_toggle_steps` is a no-op (message.rs:379).
10. **Render caches** (`MarkdownStates` message.rs:819, `ToolRows` tool_row.rs:36-43, `CopiedCodeBlocks`) are never cleared. Their keys use `body.len()`, which shifts.
11. **Scrolling.**
    - Every send calls `reset()` (send.rs:203, 275), so chats of more than 100 messages lose their measured heights (scroller.rs:108-112).
    - Running tool rows auto-open and then collapse (message.rs:727), which shifts content.
12. **Code blocks.**
    - The font is hard-coded to "JetBrainsMonoNL Nerd Font" (code_block.rs:140, 184).
    - Only ``` fences are recognised (code_block.rs:30-85).
    - Fences are pulled out of list items.
13. **Minor:**
    - Approvals are matched to rows by summary string (message.rs:269-281, 726).
    - `reply_edits` counts Stopped edits (changes.rs:71).
    - `SearchDone` resolves the oldest pending search. Fixed in phase 2.

### What the model can't represent (addressed in phases 2 and 3)

- **Subagent nesting:** fixed in phase 2 with `parent`.
- **Background work across turns, and Wake cause:** phase 2.
- **Streaming output:** every delta resends the full snapshot, which is quadratic. stdout and stderr are merged, and ANSI is stripped (tool.rs:565-585).
- **Todos and plans:** each one is a per-call snapshot with no chat-level plan.
- **Rich tool results:** text only (tool.rs:305-313).
- **Approvals:** not linked to their tool row, one-line input, a boolean "always". `Ask::Plan` is `Vec<String>`.
- **Notices:** none existed (phase 2).
- **Errors:** a bare String inside `text` (phase 2).
- **Usage:** no cost, no per-call breakdown, no served model (phase 2).
- **Tool input streaming:** Claude's `content_block_start` and `input_json_delta` are ignored (claude.rs:538-547).

### Rendering and UX gaps

- **No syntax highlighting.** Code blocks render as plain `SelectableText` (code_block.rs:187-190). gpui-base's `code_block_highlighter` hook (gpui-base state.rs:116) is unused, and gpui-component 0.7.3 ships a tree-sitter highlighter behind features.
- **Diffs:** unified only, raw `@@` headers, no word-level marks, no syntax colour, no expand-context (diff_view.rs:217-246).
- **Finished edits** open the side panel instead of an inline diff (message.rs:729-736).
- **Plans:** there is no sticky to-do or plan panel.
- **Thinking** is collapsed while streaming and shown as plain text (message.rs:650-651, 714-717).
- **Tool rows.**
  - There is no live timer, and times under 1 s are hidden (tool_row.rs:546).
  - A running Task shows a bare spinner (tool_row.rs:116-118), and its result renders as mono instead of markdown (:586-595).
  - MCP rows show raw JSON.
  - Read rows show no content.
- **Errors** render as an inline paragraph. Cancelled replies get no marker.
- **No per-turn usage footer in CLI chats** (the `is_agent` suppression at app/composer/message.rs:68).
- **Selection** breaks at code fences and step boundaries. Copy takes raw `text` (manage.rs:156).
- **Long replies** have no grouping or summary.
- **Math** renders literally (gpui-base markdown.rs:25-26). Code blocks have no max height, line numbers, wrap toggle or open-in-editor.
- **Keep:**
  - incremental markdown append (gpui-base `set_text` prefix path)
  - the per-message virtual list with follow-tail and a jump button
  - the virtualized side-panel diff (`DiffLines`)
  - the 10 Hz snapshot throttle
  - request cards placed inline at their text offset
