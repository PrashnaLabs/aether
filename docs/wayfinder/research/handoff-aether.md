# Agent context handoff: what Aether already has

Research only, no code changed. Line numbers from branch `feat/misc` at 828ee84. Paths under `crates/`.

## 1. Duplicate / fork / re-seed

- `ChatApp::duplicate_chat(ix, ..)` at `app/src/app/chat/create.rs:69`. Clones `messages`, `provider`, `provider_model`, `cwd`, `access`, `effort`, project and worktree fields. Allocates a new chat id (`allocate_chat_id`) and session id (`next_session_id`), calls `agent.fork_session(&session, &history(&messages))` (`create.rs:92`), inserts the copy at `ix + 1` titled "<title> copy", persists, selects, then `copy_fork_attachments` (`create.rs:~172`). `cli_session` is reset to `""` (`create.rs:123`), so the copy is always a fresh CLI session. `parent`/`lead` are `None`/`false`.
- `history(messages)` at `app/src/app/chat/mod.rs:41`. Returns `Vec<(Role, String)>` of visible text only. Attachments become a trailing `[attached: a, b]` line. Empty-text messages are dropped. Tool steps, thinking, notices and errors are NOT included.
- `Agent::fork_session(id, history)` at `agent/src/agent.rs:283`. Creates an in-memory ADK session and appends each pair as an event. This only matters for the OpenRouter path. CLI chats get their context from `context_prompt`, not from this session.
- The real CLI seed: `submit_as` at `app/src/app/chat/send.rs:309` computes `hist = history(&chat.messages)` BEFORE pushing the new user turn and passes it to `agent.send(.., &hist, &session)` (`send.rs:362`). `Agent::send_cli` (`agent/src/agent.rs:590`) forwards it in `Command::Prompt`. The live thread (`agent.rs:687-704`) does `if seeded || history.is_empty() { text } else { context_prompt(&history, &text) }`. `seeded = driver.resumed()` after a fresh spawn. So a new CLI process whose `resume` id is empty gets the whole transcript prepended as `"Conversation so far:\nUser: ..\nAssistant: ..\nNew message:\n<text>"` (`agent.rs:1039`). No cap, no summarising.
- Rollback / retry / edit use the same machinery: `rewind` at `app/src/app/chat/edit.rs:211` truncates messages, forks a new session, clears `cli_session` (`edit.rs:225`), closes the live session. `retry` at `edit.rs:46` is `rewind` + `submit`.
- Ticket `docs/wayfinder/tickets/08-rollback-fork-resume.md` confirms the design: fork = fresh CLI session + visible transcript as text; resume after restart = fresh + replay; no native cursors; `fork_session` signature unchanged.
- `/btw` side questions (`app/src/app/chat/side.rs:74`) reuse the same pattern with `HISTORY_CAP = 40` (`side.rs:14`, `side_history` at `side.rs:48`). That is the only existing place that caps transcript seeding.

Takeaway: "seed another agent with this chat's transcript" already works. A duplicate with a different `provider` would send the transcript to that provider on the first `submit`.

## 2. Switching provider / model on an existing chat

- Provider: `pick_provider` at `app/src/app/providers.rs:372`. Guard: `chat.provider_locked()` (`app/src/app/mod.rs:311`) = `!messages.is_empty() || cancel.is_some()`. So the provider can ONLY change on an empty, idle chat. `composer/mod.rs:269` passes `.locked(chat.provider_locked())` to the selector UI. The start page (`start.rs:202`) and `pick_provider_model` (`providers.rs:333`) route through it.
- On a legal switch it sets `provider`, `provider_model = initial_model`, `access = default_access`, `effort.clear()`, saves the global default provider. It does NOT touch `cli_session` because the chat is empty so it is already empty.
- Model: `pick_model` at `providers.rs:341` works mid-chat for the same provider (blocked only while a turn runs). `send_cli` restarts or `set_options` the driver when model/effort change (`agent.rs:690-697`), keeping or reseeding the session per driver.
- `fit_access(provider, current)` at `chat/mod.rs:32` clamps access when a provider lacks that mode. Reuse it for a handoff target.
- `clear_chat` (`chat/commands.rs:8`): clears messages and `cli_session`, closes the session, but keeps provider. Does not unlock a different provider by itself except that messages become empty.
- Consequence: a mid-chat provider swap is deliberately impossible today. Mutating `chat.provider` in place would leave `cli_session` (a Claude session id) pointed at Codex, and the 'resume' value would be fed to the wrong driver (`cli_request` at `send.rs:385-400` sends `resume: chat.cli_session`). Any in-place swap must clear `cli_session` and close the live session (as `rewind` does at `edit.rs:225-226`).

## 3. `/compact`

- `prompt_commands(provider)` at `app/src/suggest/slash.rs:7`: `["compact"]` for every provider except Antigravity (empty).
- Slash entry at `slash.rs:140-146`: label "Summarize and compact conversation context", action `SlashCommand::AgentPrompt("/compact")`. Aether sends the literal string `/compact` to the CLI as a normal prompt. The CLI's own compaction does the work (Claude emits `NoticeKind::Compaction` notices, `driver/src/claude/events.rs`).
- Not reusable as a summary generator: no Aether-side prompt, no reply capture, and the summary stays inside the CLI session (not exposed in `Message.text`). A handoff summary needs a new prompt (see option C).

## 4. Limit signals

- Typed signal exists: `NoticeKind::RateLimit` (`core/src/lib.rs:427`), `NoticeState::{Running,Done,Failed}`, `until_ms` reset time. Stored as `Step::Notice` in `Message.steps` (`core/src/lib.rs:~832-840`, upsert by notice id).
- Claude: `rate_limit()` at `driver/src/claude/events.rs:836`. `rejected` becomes Failed notice "Claude usage limit reached" with `until_ms = resetsAt*1000`; `allowed_warning` becomes Running "usage at N%". Id `ratelimit-<kind>`.
- Codex: `driver/src/codex/events.rs:160-173`. On `error` with `codexErrorInfo` in `usageLimitExceeded|rateLimitExceeded` it emits a Failed `RateLimit` notice id `ratelimit-default` PLUS `Update::Error(text)`. Also `rate_limits()` at `codex/events.rs:274` for `account/rateLimits/updated` snapshots (`rateLimitReachedType`, line 280).
- Pi / OpenCode: only `Retrying (n/m): why` notices (`driver/src/pi.rs:1155`, `driver/src/opencode/events.rs:236`, Claude `events.rs:402`, Codex `events.rs:153`), kind `Retry`. No typed limit signal; limit text arrives only as a generic `Update::Error`.
- Antigravity: "quota" appears only in a list of CLI slash names (`driver/src/antigravity.rs:39`), no limit parsing.
- Context-window-full: no dedicated signal. `Update::Window(n)` / `context_used()` (`app/src/app/mod.rs:292`) feed the composer `ContextRing`, nothing triggers on full. Claude auto-compacts via Compaction notices.
- Fatal error rendering: `Update::Error` sets `Message.failed = true` and joins into `Message.error` (`core/src/lib.rs:732`). `fatal_row` at `app/src/app/chat/manage.rs:641` builds `FatalCard` (`ui/src/chat/notice.rs:535`) with buttons "Restart agent" (agent chats, `can_restart`), "Retry last message" (`on_retry` -> `ChatApp::retry`), "Show details", "Copy". Buttons render only on the last message (`.actions(msg + 1 == chat.messages.len())`, `manage.rs:658`).
- Other attach points: `NoticeLine` (rate-limit notices render via `ui/src/chat/notice.rs`, reset countdown at line ~107); `DeadAgentBar` (`ui/src/chat/composer.rs:742`, wired at `app/src/app/composer/mod.rs:286`) for a dead CLI; `TurnFooter::on_retry` (`ui/src/chat/turn.rs:335`, built in `app/src/app/chat/turn.rs:96`).
- Best UI hook: `FatalCard` (add an "Continue in another agent" ghost button next to Retry) and the failed RateLimit `NoticeLine`. Detecting "limited" for the card: last assistant `Message.failed` AND a `Step::Notice` with `kind == RateLimit && state == Failed`. Pi/OpenCode have no such notice, so the button must also be offered on any failed reply (cheap) or via a `/handoff` slash command.

## 5. Orchestration reuse

- `spawn_agent` (`app/src/app/orchestrate.rs:570`) is the lead's tool entry: parses `SpawnArgs`, enforces `max_children`, `child_access` (`orchestrate.rs:104`, child may not exceed lead access), optional worktree, board card, then `create_child` (`orchestrate.rs:~690`) + `submit(ix, child_prompt(task))`. Built for model-driven fan-out and requires `lead` chat plus `cwd`. Not a fit for a user-driven handoff: lead-only, board card side effect, delivery of the child result back into the lead's queue (`deliver_to_lead`, `orchestrate.rs:498`).
- `create_child` (`orchestrate.rs:~692`) is the closest reusable piece: allocates id + session, sets `parent`, copies `cwd`/`project`. But it builds an empty chat with a task prompt, no transcript.
- `parent: Option<i64>` on `Chat` and `SavedChat` (`agent/src/store.rs:63`) is semantically "lead that spawned this". The sidebar groups chats whose `parent` is a lead right after it (`app/src/app/sidebar.rs:145-153`, `live = !chat.lead && parent.is_some_and(|p| leads.contains(&p))`). Many call sites test `c.parent == Some(lead)` (`orchestrate.rs:286,343,508,536,585`), and `shows_lead_chip` requires `parent.is_none()` (`orchestrate.rs:21`). Overloading `parent` for handoff would make the new chat nest under, and be treated as a child of, a non-lead chat. Only safe if the source is a lead; otherwise it is a silent no-op in the sidebar but still wrong for `stop_agent`/`read_agent`. Do not reuse.

## 6. Persistence

- `SavedChat` at `agent/src/store.rs:46`: `id, title, messages, pinned, archived, provider, provider_model, cli_session, access, effort, cwd, project, worktree_path, worktree_branch, parent, lead, lead_seen, pending_deliveries`.
- Schema evolves by additive `ALTER TABLE chats ADD COLUMN` loop at `store.rs:170-190` (guarded by existing-column check). Load query at `store.rs:231`, upsert at `store.rs:359-383` (column list + `ON CONFLICT` set at 366).
- `Chat` (app) mirrors it at `app/src/app/mod.rs` (struct ~l.135, default at ~l.846; `create.rs` has 3 constructors at 123, 267, 341 + restore at 394/436/540; `chat/mod.rs:424` builds `SavedChat` in `persist`). A new field means touching: `SavedChat`, schema loop, SELECT, row decode (`store.rs:254`), INSERT cols/binds, `Chat` struct, each `Chat { .. }` literal (grep `lead_seen: None`), `persist` mapping, restore mapping.
- "Handed off from chat X": one nullable `handoff_from INTEGER` suffices (source chat id). A title/message marker (below) works with zero schema change.
- Existing message-level hook with zero schema change: a first `Message::user` carrying a text preamble, or a `Step::Notice(Info)` on the new chat's first message. `Step::Notice` is already persisted inside `messages` JSON.

## Design options (smallest first)

### A. "Continue in <agent>" = duplicate with a provider override. No schema change, no new driver code.

What: new `ChatApp::handoff_chat(ix, provider, model, window, cx)` beside `duplicate_chat`. Clone the transcript into a new chat, set `provider`, `provider_model = initial_model(&self.settings, provider)`, `access = fit_access(provider, old_access)`, `effort` cleared, `cli_session` empty. Title `"<title> -> <Provider>"`. Leave the draft box prefilled with "Continue where we left off." (or auto-`submit` the last unanswered user text). First `submit` seeds the new CLI with `context_prompt` automatically (`send.rs:309`, `agent.rs:699`).

Touch points:
1. `app/src/app/chat/create.rs:69`: refactor `duplicate_chat` to share a helper (`fork_chat(ix, provider_override)`), minimal: add a `fn handoff_chat` that calls the same body with overridden provider fields. Reuse `copy_fork_attachments` (`create.rs:~172`).
2. `app/src/app/chat/mod.rs:32`: reuse `fit_access`. Reuse `initial_model` (`providers.rs`).
3. UI entry 1: `ui/src/chat/notice.rs:535` `FatalCard` add `on_handoff: Option<OnOpen>` + ghost button "Continue in another agent" next to "Retry last message" (`notice.rs:~632`). Wire in `app/src/app/chat/manage.rs:641-670` (`fatal_row`).
4. UI entry 2: slash command `handoff` in `app/src/suggest/slash.rs` near line 140 (and `SlashCommand` enum), plus chat context menu / sidebar row menu beside Duplicate (grep `duplicate_chat` callers in `sidebar.rs`).
5. Provider choice: reuse the existing provider list (`start.rs:202` buttons / `probes` in `providers.rs`) as a small menu; skip providers whose probe says unavailable.

Gaps accepted: transcript is text only (no tool steps), uncapped (a huge chat can overflow a small-context target; cap with `side_history`-style truncation to the last N pairs, `side.rs:48`), source chat stays untouched (good for rate-limited source). No visible link back (sidebar shows "<title> -> Codex").

Cost: about 60 lines of Rust + one UI button. Smallest viable.

### B. A + a persisted back-link (`handoff_from`)

Same as A plus `handoff_from: Option<i64>`.

Touch points (on top of A):
1. `agent/src/store.rs`: field on `SavedChat` (line 46-69), `ALTER TABLE chats ADD COLUMN handoff_from INTEGER` in the loop at `store.rs:181`, SELECT (`231`), decode (`254`), INSERT/ON CONFLICT (`359-383`).
2. `app/src/app/mod.rs` `Chat` struct + default literal (~846), all `Chat {..}` literals in `create.rs` (123, 267, 341, ~436, ~540), `persist` mapping (`chat/mod.rs:424`).
3. Chat header / sidebar: small "from <Claude chat title>" chip that calls `open_chat(id)` (`orchestrate.rs:~645` `open_chat` already selects or splits a chat by id). If the source is deleted, hide the chip.
4. Do NOT reuse `parent` (see section 5).

Benefit: navigation back to the limited chat, and enables a later "return to source" or dedupe ("already handed off"). Cost: about 25 lines of plumbing across 4 files, mostly mechanical. Do it only if the back-link is wanted day one; A can add it later because the column is nullable and additive.

### C. A with an Aether-generated handoff brief (summary instead of raw transcript)

Same as A, but the seed is a model-written brief rather than the full transcript. Lets a small-context target take a long chat, and carries tool results/decisions that `history()` drops.

Touch points (on top of A):
1. Ask the SOURCE agent (if not rate-limited) or the TARGET agent itself to summarize: send a one-shot prompt through the existing side-question path `ask_side` (`chat/side.rs:74`), which already runs a throwaway forked CLI session over `history()`. Reuse it with a new prompt `handoff_prompt()` next to `side_prompt` (`side.rs:42`): "Write a handoff brief: goal, decisions, files touched, open work, next step. Under 400 words."
2. On the side card's `finish_side` (`side.rs:165`) hand the reply text to `handoff_chat` as the new chat's first user message preamble.
3. Needs a fallback for the main use case: when the source is rate-limited the summary cannot come from it. Then use the target agent: new chat with raw capped history (option A), first message = "Summarize the transcript above, then continue." That makes C unnecessary in the limit case.
4. Do NOT build on `/compact` (section 3): it is a CLI-internal command with no capturable output.

Cost: about 80-120 lines, extra model spend, new failure mode. Defer until A shows real overflow problems.

## Recommendation

Ship A first, wire it to the `FatalCard` button and a `/handoff` slash command. Add B's `handoff_from` only if the user wants the link in the UI. Skip C until transcript size bites; cap history with `side_history`-style truncation in A to avoid that.

## Open questions

- Should handoff always create a new chat (recommended: source keeps its provider-locked state and its own `cli_session`, and the rate limit may clear later), or offer an in-place swap? In-place needs `provider_locked` relaxed for "last reply failed with a RateLimit notice", plus clear `cli_session` and `close_session`. Larger blast radius (`pick_provider` runs `load_bases`/`refresh_git` and rewrites the global default provider).
- Which providers appear as targets? Probes (`app.probes`) know CLI availability; OpenRouter needs an API key (`send.rs:342`).
- Should the new chat's cwd/worktree be shared with the source? `duplicate_chat` copies `worktree_path`, so two agents would edit the same worktree. For handoff that is intended (continue the same work), but only one should run at a time. The source is idle or limited, so it is fine.
