# Provider compatibility and agent transcript revamp

Date: 2026-10-20. Visual reference: `docs/superpowers/specs/2026-10-20-agent-chat-prototype.html`. Open it in a browser. "Live run" plays one full turn, and the chips switch between every state: new chat, starting, thinking, writing, running tools, subagents, needs approval, question, background, retries and notices, stalled, errors, stopped, steering, done. The user approved the prototype as the target ("looks good, lock it in"). Terms follow `GLOSSARY.md`. New terms used here are listed at the end and get added to the glossary.

## Intent

Aether is a desktop shell around coding CLIs (Claude Code, Codex, OpenCode, Pi, Antigravity) plus the built-in OpenRouter agent. The user's words: "This is the main aspect of the app, without this, the app will be a useless junk." Today every Provider feels broken. The trigger was an OpenCode chat where two subagent rows had spun for 22 minutes with no way to tell whether they were working, stuck or failed. They were in fact blocked on permission prompts that Aether never showed.

An audit of all five Drivers and the transcript pipeline found that each Driver speaks its CLI's protocol well enough to run a turn. The common problem is the core model (`Update`, `Step`, `ToolCall`, `Ask` in `crates/core`). It cannot express subagents, background work that outlives a turn, notices, cost, or approvals tied to a tool row. So every Driver drops or flattens the same things, and several permission paths are unsafe. The revamp fixes the model once, ports every Driver to it, and rebuilds the transcript to match the prototype.

What success looks like to the user:
- **Maximum compatibility.** Everything a Provider does is visible: text, thinking, tools, diffs, subagents and their nested activity, background tasks, plans, approvals, questions, retries, errors, cost.
- **Never guessing.** At any moment the user can tell whether an agent is working, waiting on them, retrying, quiet, stalled or failed, and for how long.

## Goals

1. **No silent hangs.** Every approval, question or form a Provider raises reaches the user, including those from subagents and child sessions. Asks resolved or cancelled elsewhere disappear. A turn that receives no events for 60 s says so, with the process state and the last event seen.
2. **Permissions mean what they say.**
   - Every access mode a Provider offers is enforced by the CLI.
   - "Always allow" is scoped to what the CLI proposes, never a blanket grant.
   - Approval cards show the full command or the real diff.
   - The mode shown is the mode the CLI is actually in.
3. **Full visibility.**
   - Subagents appear as cards with live status, timer, tool count, tokens and current activity, and their nested tool calls.
   - Background shells and agents stay live across turns, and their late completion updates the original row.
   - A turn the agent starts on its own explains why it started.
   - Retries, rate limits, compaction, hooks, model fallback, mode changes, warnings and MCP sign-in appear as one-line notices.
   - Plans and to-dos show live in a tray.
   - Images in tool results are kept.
   - Cost and the model actually used are shown.
4. **Correct turn lifecycle.**
   - Steers land in the right reply.
   - Tool rows are never duplicated or wrongly marked Stopped.
   - A recoverable error does not fail the reply.
   - Live tool output is never wiped when a reply finishes.
   - Quitting mid-turn keeps the reply.
   - The idle reaper never kills sessions that have background work or pending asks.
5. **Transcript at prototype parity**, within the rules of `docs/ux-rubric.md` (see "Prototype vs rubric" below).
6. **Version resilience.** Each Driver checks the CLI version or capabilities at start and says plainly what is unsupported. Saved chats survive new enum variants, and one unreadable message never drops a whole chat.

## Non-goals

- New Providers (Gemini CLI and others).
- TUI-only Provider features with no headless equivalent, such as Pi `/tree` and `/fork`. These are reported as TUI-only rather than sent to the model.
- Changing the built-in OpenRouter agent beyond adapting it to the new core types.
- A light-theme redesign. Both themes must keep working through theme tokens, as today.
- Rewriting the composer. The redesign that just landed stays. Only the trays above it, the queued-steer chips and the dead-agent state are new.

## Anti-goals (failure even if it "works")

- Any state where the UI shows a spinner while the agent is actually blocked, failed or gone.
- Any blanket permission grant the user did not knowingly choose: `--dangerously-skip-permissions`, per-tool "always", or a silent mode switch.
- Silently dropping a CLI event. Unknown events are logged. Unknown server requests are answered with an error or surfaced, never left pending.
- Per-provider special cases in the UI. Views render the core model only. Driver differences end at the Driver boundary.
- Approving something the user could not fully see.

## Constraints

- Rust 2024 workspace, `unsafe_code = "deny"`, GPUI via `gpui-kit 0.7.3`. Unit tests follow the glossary's "Unit test file" rule.
- The CLIs auto-update often. Claude went through 2.1.302 to 2.1.304 in three days, and Codex app-server is marked experimental. Drivers must tolerate unknown fields and variants.
- Installed versions at audit time: Claude Code 2.1.304, Codex 0.163.0, OpenCode 2.0.32, Pi 1.3.0, agy 1.4.2.
- Saved chats from current builds must still load.
- Parallel work in worktrees needs a separate `CARGO_TARGET_DIR` per worktree.
- `docs/ux-rubric.md` is binding for all views.

## Design

### 1. Core model (`crates/core`)

Changes, by type. Field names are indicative. The builder may rename them, but not drop a capability.

**`ToolCall`**
- `parent: Option<String>` is the id of the subagent `ToolCall` that owns this call. `None` means top level. Steps stay flat. The view builds the tree.
- `images: Vec<Attachment>` holds images returned by the tool, saved under the chat's attachments folder per ADR 0001.
- `ToolStatus` gains:
  - `Waiting`: blocked on an approval.
  - `Background`: running detached from the turn.
- `ToolDetail::Task` grows to hold the subagent type, description, prompt, result markdown, tool count, tokens and current activity line ("Reading src/styles.css").

**`Step`**
- `Notice` has an id, a kind, text, a state (running, done, failed) and an optional countdown deadline. Kinds:
  - Retry
  - RateLimit
  - Compaction
  - Hook
  - ModelChange
  - ModeChange
  - Warning
  - McpAuth (carries a URL)
  - Wake (why the agent started on its own)
  - Info
- `Steer { text }` marks where a mid-turn message was delivered.
- `Unknown` is a `#[serde(other)]` fallback, also added to `ToolStatus`, `Ask`, the notice kind and `PlanStatus`. `ToolDetail` already has one.

**`Update`** additions and changes:
- `Notice(Notice)` upserts by id.
- `Steered(String)` is sent when the CLI actually consumes a steer.
- `Wake(String)` now carries the reason.
- `Warning(String)` is recoverable. It becomes a Notice and does not fail the reply.
- `Error { message, detail }` is fatal. It renders a fatal card and is no longer written into `Message.text`.
- `Cost(f64)` is USD as reported by the CLI, summed per reply.
- `Model(String)` is the model actually serving.
- `Mode(AccessMode)` is the permission mode actually in effect.
- `AskResolved(u64)` is sent when an ask was answered, timed out or was cancelled elsewhere.
- `Update::Tool` upserts by id across the whole chat, not only the current reply. A late background completion then updates its original row.

**`Ask` and `Answer`**
- `Approval` carries:
  - `tool_id` (links the card to its row)
  - kind: command, edit, fetch, MCP or other
  - the full input with no first-line clipping
  - an optional unified diff
  - cwd and reason
  - source: the subagent name, if any
  - a list of options the CLI offers. These are scoped rules such as "pnpm add *" or "edits in src/components/**", each with an id and a label.
- `Answer` gains:
  - picking an option by id
  - deny with a note to the agent
  - approve with the full edited input
- `Question` holds several questions, each with header, options and descriptions, multi-select, an "other" free-text flag and a secret flag. The answer carries one value per question.
- New `Form` covers MCP elicitation, OpenCode forms and Pi extension dialogs (select, confirm, input, editor, with an optional timeout). Fields can be text, choice, multi-choice or boolean, with required, default and an external URL step.
- `Plan` carries markdown, not `Vec<String>`.

**`Message` / reply**
- Duration is tracked per Thought step: from its first thinking delta to the next non-thinking update. Today there is one `thought_for` measured from message creation.
- Thinking that arrives after text starts a new Thought step instead of appending above the text.
- `SearchDone` resolves by id, not by "oldest pending".
- `cap_tool_payload` applies only to the serialized copy, never to the live message.

**Persistence (`crates/agent/src/store.rs`)**
- A running reply is saved with a debounce of at most 2 s, and on quit.
- A message that fails to parse is replaced by a placeholder. The rest of the chat still loads.
- On restore:
  - `Running` and `Waiting` tools become Stopped, with the error "Interrupted: Aether closed". OpenCode is the exception (next bullet).
  - Pending asks expire.
  - `Background` tools show as unknown until the Driver re-reports them on resume.
- **OpenCode work outlives Aether (user decision).** Quitting Aether and the idle reaper do not interrupt an OpenCode turn, because its service is shared and keeps running. Instead:
  - At launch, and when a chat is opened, Aether attaches a Driver without sending a prompt to any OpenCode chat whose session is active (`/api/session/active`) or has pending asks. This needs an attach-only start path in the Driver and the agent pump.
  - The chat shows as Working in the sidebar. Its running rows resume live updates, and pending approvals, questions and forms from the session and its child sessions are shown.
  - Restore does not mark OpenCode rows "Interrupted". It leaves them to the attached Driver, which reports their real state.
  - Other Providers' processes die with Aether, so the restore rule above applies to them.

### 2. Turn lifecycle (`crates/agent`, `crates/app/src/app/chat`)

- **Queue and steer.** Today's composer behaviour stays, as the composer redesign kept it.
  - Enter during a turn adds the message to Aether's own queue. It shows as a chip with Send now, Edit and Remove, and runs as a normal prompt once the turn ends.
  - "Send now" (or the send button while running) hands the message to the CLI as a steer. The chip then reads "Delivering", and Edit and Remove disappear, because most CLIs cannot take a message back.
  - When the Driver emits `Steered`, the chip goes away and a `Step::Steer` marker is inserted in the running reply at that point.
  - If the turn ends before the CLI picks up the steer, the turn the CLI then starts with it renders as a normal user message plus reply, not as a Wake.
  - Drivers stop sending `Wake` when a steer is sent.
  - `submit` no longer bumps the run counter on a steer, so the running reply's queued updates are not dropped.
- **Wake.** A turn the CLI starts by itself starts a new reply, preceded by a Wake notice naming the cause ("background task `pnpm test` finished with exit 1"). Updates and asks arriving outside a turn are routed through Wake instead of being dropped or auto-denied (today `agent.rs` drops them).
- **Liveness.**
  - The app records the time of the last update per running chat.
  - The status line under the running reply shows a heartbeat: "live" under 4 s, then "quiet Ns", then "no output Ns" in amber after 30 s.
  - After 60 s with no events and no pending ask, a stall card shows:
    - the process state (alive or exited, with pid)
    - the last event seen
    - whether anything waits on the user
    - actions: keep waiting, ask for a status update (sends a steer), open in the Provider's own TUI where that exists, stop
- **Errors.** Only `Error` sets `failed` and blocks the send queue. A Driver that dies emits `Error` with the stderr tail, not the last line only.
- **Reaper.** The idle reaper skips sessions that have an active turn, Background tools, pending asks or running subagents. Background-task events refresh `last_active`.
- **Stop.**
  - Running and Waiting rows become Stopped.
  - Subagents become Stopped.
  - A stopped marker says what was interrupted.
  - Queued steers return to the composer.
  - Pending asks are denied first, so a CLI blocked in a dialog can abort cleanly.

### 3. Permissions and asks (cross-Driver rules)

- Each Driver maps its CLI's real "always" options into `Approval.options`, never a per-tool boolean. Sources:
  - Claude: `permission_suggestions`. A `setMode` suggestion is shown as its own labeled option ("Switch this chat to Accept edits"), never folded into "Always".
  - OpenCode: the request's `save` patterns, sent as `"always"` to the server.
  - Codex: `availableDecisions`, plus the proposed exec-policy amendment and the network host.
- No new access modes are added. The CLI must enforce each mode a Provider already offers:
  - Codex Read only, through its sandbox.
  - Pi Read only, through the tool allowlist, with a note that extension commands still run.

  Mode arms a Provider does not offer must still map to safe rules, never to allow-all. Today OpenCode's `rules()` maps Read only and Auto to `[]`.
- Asks from subagents and child sessions are surfaced with `source` set. They are listed again on start and on reconnect, so a reopened chat shows asks that were already pending.
- Unknown server requests get an explicit error reply and a Warning notice. They are never left hanging.

### 4. Drivers

Each Driver gets a version or capability check at start, a stderr tail for crash reports, and replay-test fixtures (see Verification). Per-Driver work, from the audit:

**Claude Code** (`crates/driver/src/claude.rs`)
- **Handshake.** Send the `initialize` control request. Enable nested subagent tool calls, agent progress summaries and `--include-hook-events`. Prefer public flags (`--permission-prompts host`, `manual`) where the hidden ones have public replacements.
- **Subagents.** Map `parent_tool_use_id` lines to `ToolCall.parent`. Strip the "agentId … (internal ID …)" trailer from Agent results.
- **Background tasks.** `task_started`, `task_progress`, `task_updated` and `task_notification` drive the original row by `tool_use_id`, through to the final status, output file and usage.
- **Other `system` events:**

  | Event | Maps to |
  |---|---|
  | `api_retry` | Retry notice |
  | `rate_limit_event` | RateLimit notice |
  | `status` compacting, then `compact_boundary` | Compaction notice, with unique ids |
  | `permission_denied` | Denied row with the reason |
  | `informational`, `notification` | Info or Warning notice |
  | `model_fallback` | ModelChange notice |
  | `model_refusal_fallback` | retract the listed messages |
  | `hook_*` | Hook notice |
- **To-dos.** Fold `TaskCreate` and `TaskUpdate` into the plan, keyed by task id. Keep `TodoWrite` support.
- **Results.** For error subtypes, read `errors[]`. `total_cost_usd` becomes Cost, and `modelUsage` adds subagent tokens.
- **Session state.**
  - `init.model` becomes Model.
  - `init.permissionMode` and `status.permissionMode` become Mode.
  - Failed or needs-auth MCP servers become notices.
- **Asks.**
  - `control_cancel_request` becomes AskResolved.
  - Elicitation and `request_user_dialog` become Form.
  - AskUserQuestion supports multi-select and several questions.
  - ExitPlanMode keeps its markdown and its permission suggestions.
- **Robustness.**
  - When no deltas arrived (non-streaming fallback), use the assistant text blocks.
  - The cancel watchdog kills the child and emits Error.
  - A resume that the CLI rejects falls back to a fresh session, with a notice.
  - Keep images from `tool_result`.

**Codex** (`crates/driver/src/codex.rs`)
- **Turn identity.** Bind turn start and completion to turn ids. Handle `activeTurnNotSteerable`, and do not start a second turn while one is live.
- **Approvals.**
  - `mcpServer/elicitation/request` with `codex_approval_kind: mcp_tool_call` becomes an Approval. Other elicitations become Form. Today they are all auto-declined.
  - `item/permissions/requestApproval` becomes an Approval.
  - All questions are asked, not only the first.
  - `serverRequest/resolved` becomes AskResolved.
- **Sandbox.** Stop sending a bare `sandboxPolicy` type per turn that overrides the user's writable roots and network settings.
- **Background terminals.** Do not mark them Stopped at turn end. Late `item/completed` updates the original row. List them on resume with `thread/backgroundTerminals/list`, and terminate them from the tray.
- **Subagents.**
  - Keep child rows across the parent's turn end.
  - Adopt grandchildren.
  - Count child usage.
  - Resolve file paths in child approvals.
- **Items.**
  - review results (`exitedReviewMode`)
  - `imageView`, `imageGeneration`
  - `sleep`
  - plan mode (`plan`, plan deltas, `collaborationMode`)
  - `hookPrompt`, `dynamicToolCall`
  - every `collabAgentToolCall` action (wait, send, follow-up, close, list)
  - `agentMessage.questions`
  - commentary `phase`
- **Notifications.**

  | Notification | Maps to |
  |---|---|
  | `error` with `willRetry` | Retry |
  | `turn/diff/updated` | per-turn changes |
  | `account/rateLimits/updated` | RateLimit and usage |
  | `model/rerouted` | ModelChange |
  | warnings and deprecations | Warning |
  | hook started and completed | Hook |
  | auto-approval reviews | Info, with the rationale |
  | terminal interaction, patch updates, MCP progress | live row updates |
- **Other.**
  - Keep raw reasoning separate from the summary.
  - Keep a stderr tail.
  - Clean command output incrementally, with a bounded buffer.
  - Surface resume failure as a notice.
  - Send Wake for server-started turns.
  - Gate on version via `userAgent`.

**OpenCode** (`crates/driver/src/opencode/`)
- **Child sessions.** Follow child sessions from `subagent`/`task` metadata `sessionID`. Map their tools, text and errors to nested rows, and surface their asks. On start and on resync, list `/permission` and `/form` for the parent and its children, and check `/api/session/active`.
- **Permissions.**
  - Map the unoffered Read only and Auto arms of `rules()` to deny or ask rules instead of `[]`.
  - Send "always" to the server, with the `save` patterns shown on the card.
  - A shell approval lists each command.
  - An edit approval shows `metadata.files[].patch`.
- **Service lifecycle.**
  - Reconnect with a deadline, then a fatal Error.
  - Handle `execution.interrupted` on shutdown.
  - Gate on the `/api/info` version.
  - Send service stderr to a log.
- **Wake.** Send Wake on `session.execution.started` when no turn is active.
- **Diffs.** Use `metadata.files[].patch`, and detect overwrites.
- **Model options.** `set_options` must actually switch back to the default model and apply effort.
- **Mappings:**

  | OpenCode event | Maps to |
  |---|---|
  | `session.retry.scheduled` | Retry |
  | `execute` tool | nested calls |
  | `skill` tool | Skill |
  | step snapshot and files | per-turn changes |
  | `step.ended.cost` | Cost |
  | content-filter stops | Warning |
- **Other.**
  - Forms get hidden, when, required, default, external URL and multi-select.
  - Images are sent as files.
  - `text.ended` repairs missed deltas.
  - Compaction ids are unique.
  - Asks answered elsewhere become AskResolved.

**Pi** (`crates/driver/src/pi.rs`)
- **Steering.**
  - Emit `Steered` at `message_start` role user or `queue_update`. Do not Wake on send.
  - Show queued steers.
  - Return the `clear_queue` text to the composer on cancel.
- **Cancel and dialogs.**
  - On cancel, close pending dialogs first so an open editor dialog cannot kill the session.
  - A dialog timeout becomes AskResolved.
- **Wake.** `agent_start` with no active turn becomes Wake.
- **Stderr.** Stderr lines become Warnings, not reply failures. A failed handshake shows the stderr tail.
- **Notices.** `auto_retry_*` and `summarization_retry_*` become Retry. A clamped thinking level becomes a notice.
- **Extension UI.**
  - `notify` becomes a notice with clickable links, which fixes MCP sign-in.
  - `setStatus` and `setWidget` feed the status line or a notice.
  - `setTitle` becomes Title.
- **Tools.**
  - Stream `toolcall_*` input.
  - Use `details.patch` for edit diffs.
  - Keep bash truncation and `fullOutputPath`.
  - `parentToolCallId` becomes `parent`.
  - Keep images.
- **Results.**
  - `usage.cost.total` becomes Cost.
  - `stopReason: length` becomes Warning.
  - Keep the compaction reason, summary and tokens.
- **Commands.** Intercept TUI commands that have RPC equivalents. Report the rest as TUI-only.
- **Launch.** Resume with `--session`. Gate on version (`agent_settled` needs 0.80.4 or later, `disposition` needs 0.99.0 or later), and remove `PI_SKIP_VERSION_CHECK`. When project-local extensions are skipped because the project is untrusted, show a notice with a "Trust this project" action that relaunches with `--approve`.

**Antigravity** (`crates/driver/src/antigravity.rs`)
- **Approvals.**
  - Remove the silent `--dangerously-skip-permissions` respawn.
  - Look for a per-action approval channel in agy 1.4.2 first.
  - If none exists, the card lists every denied action in full and states that allowing reruns the turn with all permissions. The user chose this over deny-only. The warning names every action that will run with full permissions.
  - Mark denied rows by tool id, not "last tool with empty output".
- **Mode.** Reflect the mode actually in effect (agy ignores `--mode auto` on this plan), with a notice.
- **Tools.**
  - Thinking, if the stream carries it.
  - `invoke_subagent` and `browser_subagent` become Task.
  - `command_status`, `send_command_input` and background `run_command` drive the background lifecycle.
  - Keep `view_file` line ranges.
  - Fix the `search_web` key casing.
  - Report the context Window per model.
- **Turns.**
  - A `user_input` step becomes Steered.
  - Events outside a turn become Wake.
  - Pair each result with its own turn.
- **Responsiveness.** Never block the Driver thread during a respawn, so Stop and answers stay responsive.
- **Launch.**
  - A missing conversation on resume falls back to a fresh session, with a notice.
  - Drop the `antigravity` binary fallback, which is the IDE launcher.
  - Scope `--add-dir` to this chat's attachment folder.

**OpenRouter (built-in)**: adapt to Warning versus Error, and to Notice. No other changes.

### 5. Transcript and chat UI (`crates/ui/src/chat`, `crates/app`)

Match the prototype, screen by screen.

- **Status line** under a running reply:
  - phase: Starting, Thinking, Writing, Running N tools, Running N subagents, Waiting for you, Retrying, Compacting
  - elapsed time and tokens
  - the heartbeat
  - "esc to stop"
- **Live timers** on running tool rows, subagent cards, live thoughts and the sidebar's Working status.
- **Tool rows.**
  - Layout: icon, verb, target, and status right-aligned.
  - Colour only for failed, waiting and background rows.
  - Exit code only when it is non-zero.
  - The first error line stays visible without expanding.
  - Command output streams in place, capped at about 12 lines with "Show all".
  - A finished edit expands to its inline diff in the transcript, not only in the side panel.
  - Runs of reads fold into "Read N files".
- **Thoughts.**
  - Done: "Thought for Ns" with a first-sentence preview.
  - Live: open, rendered as markdown, with a timer.
  - Each Thought has its own duration.
- **Subagent cards.**
  - Collapsed: name, type, description, status, timer, tools, tokens, current activity.
  - Expanded: prompt, nested rows (last N plus "+N earlier") and the result as markdown.
  - Failed cards show the error. A waiting card links to its approval.
- **Approval card.**
  - Shows the title, the full command or diff, cwd, reason and source.
  - Actions: Allow once (Enter), the scoped "always" options, Deny (Esc), deny with a note.
  - Also shows a queue count and a waiting timer.
  - Fully keyboard operable.
- **Question and form card:**
  - a stepper ("Question 1 of 2")
  - multi-select
  - option descriptions
  - "Something else…"
  - "Let the agent decide"
- **Notices** are one quiet line each. The Retry notice has a live countdown.
- **Background.**
  - Tray above the composer: each item shows a live timer, Output or Open, and Stop.
  - The sidebar shows "N in background".
  - A Wake marker sits before a reply the agent started itself.
- **Plan tray** above the composer: progress, current item, collapsible, collapsed by default.
- **Fatal card.**
  - Says what failed and why.
  - Actions: Restart agent, Retry last message, details (stderr tail), Copy.
  - The composer shows a dead-agent bar with "Restart now".
- **Stopped** rows and cards say Stopped, and a stopped marker summarises what was cut off.
- **Steering** shows an inline "You, while it worked" marker in the reply, plus queued chips in the composer.
- **Finished turn.**
  - Folds into one "Worked for X · counts" line, expandable to the full timeline.
  - The final answer, the changes card and any expanded diffs stay visible.
  - Footer: duration, tool count, subagents, model, input, output and cached tokens, and cost. CLI chats get the footer too (remove the `is_agent` suppression).
- **Markdown.**
  - Syntax highlighting in code blocks, diffs and command lines.
  - `~~~` fences and longer backtick fences.
  - Fences inside list items without breaking numbering.
  - Tables and file links.
  - Thinking rendered as markdown.
- **Diffs.**
  - Unified or split toggle.
  - Word-level marks on changed line pairs.
  - Hunk headers as "Line N · context".
  - Expandable unchanged gaps.
  - Line numbers and "Open".
- **New chat** shows a health card per Provider: version, status (ready, signed in, or a degraded mode with the reason) and capability chips.
- **Sidebar statuses:** Working with a timer, Starting, Needs you, Failed, Done (unread), N in background, Quiet, Retrying. Each pairs colour with an icon and text.
- **Performance.**
  - No full markdown re-parse per delta on the UI thread.
  - No `path.exists()` in render.
  - Render caches keyed by stable step id and bounded.
  - A long reply must not lay out every row each frame.
  - Keep the 10 Hz snapshot throttle.

**Also in scope from the prototype:**
- **Startup checklist.** The Starting state lists steps such as "Found opencode 2.0.32", "Service ready" and "Creating session". These are sent as running and done Notices of a `Startup` kind, so no new `Update` is needed. Every Driver emits its real startup steps.
- **Version chip.** The chat header shows the Provider and its version, taken from the same probe as the new-chat health cards.
- **"View summary"** on a compaction notice, wherever the CLI provides the summary text.
- **"Sign in"** on an MCP notice, which opens the sign-in URL.

**Prototype vs rubric.** The prototype shows behaviour and information, but `docs/ux-rubric.md` wins on these points:
- Subagent cards use a 1 px status-coloured border plus a status icon. The rubric bans side stripes wider than 1 px.
- A subagent's approval card renders directly below the subagent card as a sibling, labelled with its source, not nested inside it.
- No unicode glyphs as icons (↕ ↓ ↳). Use the icon set.
- No gradient shimmer on "Thinking". Use a plain label with a spinner.
- Colours come from theme tokens, and both themes are checked.

### 6. Verification

- **Replay fixtures.** Add a record mode (for example `AETHER_RECORD=<dir>`) that saves each Driver's raw CLI stream. Capture these scenarios per Provider:
  - subagents
  - a background shell
  - an approval (main agent and subagent)
  - a question
  - a retry or rate limit
  - compaction
  - a tool error
  - a fatal error
  - cancel
  - steer
  - resume

  Replay tests feed the fixtures to the Driver and assert the `Update` sequence. Tests that currently assert a drop are updated, for example the Codex test asserting that child rows vanish at turn end.
- **Core tests:**
  - the `Message::apply` reducer: parent links, cross-reply upserts, notices, Steered, Warning versus Error, per-thought timing
  - store round trips with unknown variants
  - payload capping only on save
- **Manual check.** Run every prototype scenario against each Provider in the real app, in both themes, then review per `docs/ux-rubric.md`.

## Phasing (builder's plan may refine)

1. **Permission and hang fixes.** Small and shippable alone:
   - OpenCode scoped "always", with its unoffered mode arms made safe
   - OpenCode child-session asks surfaced
   - Codex MCP and permissions approvals surfaced
   - Claude "always" without silent mode switches
   - full command shown on approval cards and kept when edited
   - the Antigravity blanket grant replaced by an explicit, warned rerun with full permissions

   Some of these need the new `Ask` shape. If so, that part of phase 2 moves forward.
2. **Core model, lifecycle and persistence**, with reducer and store tests.
3. **Drivers** on the new model, one track per Provider, in parallel worktrees. Fixtures come first in each track.
4. **Transcript and chat UI** to prototype parity. This can start once phase 2 lands, using fixtures, in parallel with phase 3.

## Left to the builder

- Exact type and field names, and whether multi-file patches are one `ToolCall` per file or one with several files. Renames (Codex `move_path`) must show either way.
- Where liveness timestamps live, how CPU or pid is read (pid and alive are required, CPU is optional), and the exact stall thresholds (60 s is the default).
- How the transcript is split into list items, and how render caches are keyed, as long as the performance bullets hold.
- The syntax highlighter. `gpui-component 0.7.3` ships a tree-sitter highlighter behind features. Use it if it fits, otherwise pick the lightest option.
- The cap on nested rows shown per subagent before "+N earlier".

## Calls made without the user (check these)

- The four-phase order, and that permission and hang fixes ship first.
- Flat steps with `parent` ids rather than nested step trees.
- Tool upserts across the whole chat, for background completions.
- Warning versus Error split. Only Error fails a reply.
- Debounced mid-turn saves of at most 2 s, and restore turning running rows into "Interrupted: Aether closed".
- Plan tray collapsed by default. Its header still shows progress and the current item.
- The Pi "Trust this project" action relaunching with `--approve`.
- The prototype bends to the rubric in the five points listed above.
- The queue and steer rules in section 2 keep today's composer behaviour. A steer that was not delivered before the turn ended, and then starts a turn, shows as a user message, not a Wake.
- No new access modes. Phase 1 makes both the offered modes and the unoffered mode arms safe.
- Every prototype element is in scope, including the startup checklist, the version chip, "View summary" and "Sign in".
- If recording is approved:
  - It runs in a throwaway project.
  - Recordings are scrubbed of paths, file contents, account details and MCP server names before they are committed.
  - Schemas that can be regenerated, such as `codex app-server generate-json-schema` and OpenCode's `/openapi.json`, are not committed.

## Decisions by the user (2026-10-20)

1. **Fixtures.** Recording real fixtures on the user's accounts is approved: about 10 to 12 short turns per Provider, in a throwaway project, scrubbed before commit.
2. **OpenCode outlives Aether.** Work keeps running when Aether quits. Aether reattaches on launch or when the chat opens, and shows pending asks (section 1, Persistence).
3. **Antigravity approvals.** Check agy 1.4.2 for a per-action approval channel first. If there is none, "Allow" reruns the turn with full permissions behind a clear warning that lists every blocked action in full.
4. **The prototype** is the UX target ("looks good, lock it in").

## New terms (to add to GLOSSARY.md)

- **Subagent**: a child agent a Provider spawns inside a turn. It has its own tool calls, shown nested under the call that started it.
- **Background task**: a shell or agent that keeps running after the turn that started it. It is shown in the background tray until it finishes.
- **Notice**: a one-line transcript marker for an event that is not a tool or text: retry, rate limit, compaction, hook, model or mode change, warning, MCP sign-in, wake.
- **Steer**: a message sent while a turn runs. It is delivered into that turn rather than starting a new one.
- **Wake**: a turn the Provider starts by itself, for example when a background task finishes.
- **Stall**: a running turn with no events for 60 s and nothing waiting on the user.
