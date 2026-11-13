# Aether as an organization: fit against the current code

Status: research, no code changed. Branch `feat/misc` at 7d34e17. Paths are
under the repo root. Concept being tested: `docs/idea/org-concept.md`.

Method: every claim below was read from source. Where a doc disagrees with
the code, the code wins and the drift is listed in section 5.

## 0. Verdict

Most of the org already exists if you stop thinking of heads as children and
think of them as peer leads.

- The "child cannot be lead" rule is a UI gate, not a hard rule. A chat with
  no `parent` can be a lead, and every lead has the full tool set. So a head
  can be a top-level lead chat that spawns its own workers today.
- Any lead can message any other non-archived chat, including another lead,
  across projects, and read any chat's history. So lead-to-head,
  head-to-head and head-to-lead messaging works today, with three flaws:
  the reply does not come back on its own, the message carries no sender
  tag, and there is no loop guard.
- Persistence of a chat (messages, provider, model, CLI session id, lead flag)
  already survives restart. Persona, role, pinned model, budget and roster do
  not exist as concepts.
- The hardest missing pieces are not messaging. They are persona injection for
  CLI providers (no channel exists), a role-aware budget, and a global worker
  cap (the cap is per lead, so N heads multiply it).

The smallest credible path starts with zero code (section 3, increment 0).

## 1. Inventory of existing primitives

### 1.1 Lead toggle

Gives the org: the switch that turns a chat into an orchestrator.

- Persisted per chat: `lead INTEGER` column (`crates/agent/src/store.rs:27`),
  `SavedChat.lead` (`store.rs:100`), restored by `restore_chats`
  (`crates/app/src/app/chat/create.rs:557-640`).
- Toggling on an already started chat restarts the driver so tools are
  injected (`set_lead`, `crates/app/src/app/chat/manage.rs:492-519`).
  Toggling off is refused while the lead has running children
  (`manage.rs:497-501`).
- Tool set is decided per chat: lead gets all specs, a non-lead gets board
  tools, plus `read_history` when seeded (`Agent::lead_tools`,
  `crates/agent/src/agent.rs:274-290`; app side `list_tools`,
  `crates/app/src/app/orchestrate.rs:263-271`).
- Any CLI provider can lead, and OpenRouter too in code (`lead_capable`,
  `orchestrate.rs:18-20`).

Limits:

- The only guard against a child leading is `shows_lead_chip`, which requires
  `chat.parent.is_none()` (`orchestrate.rs:22-24`). It gates the composer chip
  (`crates/app/src/app/composer/mod.rs:305`) and the palette entry
  (`crates/app/src/palette.rs:344`). `toggle_lead` and `set_lead` check
  nothing about `parent` (`manage.rs:447-519`). The authority check at call
  time is `chats[ix].lead` (`orchestrate.rs:273-280`).
- Antigravity leads need a one-time global `agy mcp add` with a user confirm
  (`manage.rs:447-455`, `agy_lead_check`).
- OpenCode leads cost one extra `opencode serve` process each
  (`docs/superpowers/specs/2026-11-04-agent-orchestration-design.md:203`,
  `docs/wayfinder/research/orchestration-adapters.md:197-215`). Six OpenCode
  heads mean six extra servers.
- Tools are injected at CLI launch. A chat toggled to lead mid-session needs
  the restart, which the app does itself.

### 1.2 spawn_agent, message_agent, read_agent, stop_agent

Tool schemas: `crates/agent/src/orchestrator/mod.rs:123-175`. Handlers in
`crates/app/src/app/orchestrate.rs`.

`spawn_agent` (`orchestrate.rs:769-881`)
- Gives: a persistent child chat with its own provider and model, optional
  worktree, optional board card, `nonce` idempotency (`778-791`), auto
  delivery of its final reply to the lead (`deliver_to_lead`, `697-725`).
- Provider must be in `CLIS` (claude, codex, pi, opencode, antigravity;
  `crates/app/src/app/mod.rs:82-83`), installed, and the model must be in the
  probe's list (`SpawnArgs::parse`, `orchestrate.rs:56-93`). OpenRouter cannot
  be a child.
- Child access is clamped to the lead's mode (`child_access`, `106-113`).
- A child starts clean: task text plus a fixed "end with a summary" suffix,
  no lead history (`child_prompt`, `115-119`).
- Limit: no persona or system prompt parameter. The only channel is the task
  text.

`message_agent` (`orchestrate.rs:565-645`)
- Gives: send to any chat id, child or not. Busy target queues the text
  (`611-621`). Idle target gets it submitted as a normal user turn (`634`).
- Resolution is by raw chat id across all projects (`target`,
  `crates/app/src/app/control.rs:383-389`).
- Refused when: target is the sender (`573`), archived (`576`), open in a
  terminal (`600`), stopping (`606`), or the target runs with more access than
  the sender and is not the sender's child (`580-585`).
- Limits: a non-child's answer does not come back (`586-591` adds the "read it
  later with read_history" note). The text lands as a plain user message with
  no sender label (`Queued { text, from_children: false, .. }`, `611-617`), so
  the receiver cannot tell which head wrote it. No hop counter, rate limit or
  cycle check exists anywhere in the function, so A to B to A loops are
  bounded only by tokens.

`read_agent` / `stop_agent`
- Child-only (`own_child`, `515-525`). Status vocabulary: `needs_you`,
  `running`, `starting`, `failed`, `idle` (`child_status`, `125-138`).
- `stop_chat` stops any chat except the caller (`orchestrate.rs:296`).

### 1.3 max_children and the spawn queue

- Setting `max_children`, default 8, clamped 1 to 50
  (`crates/core/src/settings.rs:483-491`, field at `819-820`).
- Counted per lead, running or starting children only
  (`running_children`, `orchestrate.rs:955-958`; checks at `796` and `908`).
  Idle finished children do not count, so a head can accumulate many idle
  workers for free.
- Over the cap, a spawn is queued: at most 32 per lead (`MAX_QUEUED_SPAWNS`,
  `197`, enforced `804-808`), position returned, and started later by
  `drain_spawn_queue` (`899-920`), which tells the lead the new child id as
  its next turn.
- Queued spawns live in memory and are dropped on quit
  (`docs/task-board.md:89`).

Limit for the org: the cap is per lead and one value for all leads. With six
heads at the default 8 the app admits up to 48 concurrent workers plus the
heads. No global cap exists (`max_children` is read at only three sites:
`orchestrate.rs:796, 820, 908`). Nothing reads CPU or RAM (see 1.9).

### 1.4 Task board

Spec `docs/superpowers/specs/2026-11-06-tasks-board-design.md`, user doc
`docs/task-board.md`.

Gives the org:
- Fixed columns Backlog, Todo, Doing, Review, Done (`task-board.md:3-4`;
  `Col::ALL`, `crates/app/src/board/model.rs:16`).
- Cards with labels, `blocked_by`, dated notes signed `lead <id>` or
  `agent <id>` (`card_author`, `orchestrate.rs:496-500`; `read_card`,
  `405-461`). Label filter exists (`mod.rs:98`). A spawned child's final reply
  is filed on its card as a note (`board_note_result`,
  `crates/app/src/board/mod.rs:157-178`). Spawning with a `card` fails on open
  blockers (`check_spawn_card`, `orchestrate.rs:883-897`).
- Auto flow: turn start moves Backlog/Todo to Doing; clean finish moves Doing
  to Review (`board_on_turn`, `board/mod.rs:138-154`).
- "Done is human-only" is enforced in two places: `lead_move`
  (`board/model.rs:226-231`) and `lead_update` (`board/model.rs:295`). `add_card`
  also rejects `done` (`orchestrate.rs:344`). This matches concept flow step 5.
- Every chat, not only leads, gets the five board tools (`board_specs`,
  `mod.rs:363-379`; `orchestrate.rs:273-280`).

Limits:
- No assignee field. Roles must ride on labels or the title.
- A lead's board writes are scoped to its own project: `move_card` and
  `update_card` match `c.project == lead_project` (`orchestrate.rs:381-386,
  478-483`), and `lead_project` is the chat's `project_dir`
  (`orchestrate.rs:371-373`, `crates/app/src/app/mod.rs:261-267`). Heads
  whose cwd differ do not share one writable board. A non-lead sees only its
  own project's cards (`task-board.md:84-86`).
- A card reaches Review when its worker finishes, with no reviewer notion.
  Review routing is entirely convention today.

### 1.5 Messaging non-children and read_history

- `read_history` reads any chat for a lead, own chat for others
  (`crates/app/src/app/control.rs:449-462`). Negative offset reads the tail
  (`mod.rs:264`). `list_chats` filters on id, project, state, provider,
  title words (`mod.rs:71-86`, handler `control.rs:~190-215`) and returns
  `id, title, project, provider, state, last_active, pinned, archived, lead`.
  It does not return model, cost, tokens, or queue depth.
- Pinned chats are skipped by the bulk archive/delete tools unless
  `include_pinned` is passed (`guard`, `control.rs:171-180`). Useful to protect
  heads from a confused CEO.
- Limit: the "ask a head, get an answer" round trip is two calls plus a
  history read, and the answer is only visible once the head's turn ends.

### 1.6 Handoff and model switching

Spec `docs/superpowers/specs/2026-11-07-handoff-design.md`, code
`crates/agent/src/handoff.rs`, `prepare_handoff` in `agent.rs`,
`switch_agent` in `crates/app/src/app/providers.rs:389-503`.

Gives the org:
- Moving a head to another provider or model keeps the chat, inserts a divider,
  remembers each provider's CLI session and resumes it later with only the
  delta (`handoff-design.md:72-88`).
- The seed always keeps the first user message when it fits
  (`handoff-design.md:57-59`). A persona written as the first user message
  therefore survives a provider switch, within budget.
- Budget is 16,000 tokens by default, clamped 1,024 to 64,000
  (`handoff-design.md:41-42`).

Limits:
- A head's persona is not special to the handoff code. If the persona sits in
  a later message or the history is long, it can be dropped.
- `switch_agent` rewrites the global default provider as a side effect
  (`providers.rs:498`), and `pick_model` rewrites the provider's default model
  (`providers.rs:~525`). Two heads on one machine would fight over defaults
  if pinning is done through these paths.
- Nothing prevents a human or the CEO's `update_chat` from changing a head's
  model; there is no "pinned model" flag. `update_chat` only renames and
  pins (`control.rs:391-410`).
- `/clear` (`clear_chat`) resets the title to "New chat"
  (`crates/app/src/app/chat/commands.rs:16`) and wipes the session. A
  title-based roster breaks on clear.

### 1.7 Session resume and persistence across restarts

- Chats live in SQLite: messages, provider, model, `cli_session`, access,
  effort, cwd, project, worktree, `parent`, `lead`, pending deliveries,
  pending queue, handoff state (`SavedChat`, `store.rs:78-108`; upsert
  `store.rs:491-518`).
- On restart `restore_chats` rebuilds each chat (`create.rs:557-640`). The next
  send resumes the provider session via `resume: chat.cli_session`
  (`crates/app/src/app/chat/send.rs:476-481`). Idle CLI processes are reaped
  after 600 s (`agent.rs:193`, `reap` at `1112-1119`) and respawned on
  demand, so a parked head costs no process and no tokens.
- Queued child deliveries and queued user messages are persisted
  (`store.rs:104-106`, `SavedDelivery`/`SavedQueued` at `111-126`).
- A running turn can be re-attached after restart (`Agent::attach`,
  `agent.rs:673-684`).

Limits:
- Persistence is of the transcript and the CLI session id. There is no field
  for role, persona, memory file, budget or pinned model (`SavedChat` has
  none of them).
- Context only shrinks by CLI-side compaction. Aether has no head-aware
  summarize-and-reseed.

### 1.8 Worktrees

- `spawn_agent` with `worktree: true` makes a branch and folder off the lead's
  current branch, uncommitted lead changes excluded (`tool_specs`,
  `mod.rs:134`; `worktree_base`, `orchestrate.rs:237-247`).
- Project scripts with `on_new_worktree` run before the first prompt
  (`ProjectScript`, `crates/core/src/settings.rs:758-763`; `setup_queue`,
  `crates/app/src/app/worktree/setup.rs:36-38`).
- A worktree child's setup failure leaves its card in Doing
  (`task-board.md:93-94`).
- Limit: Aether sets no build-environment variables for children. Whatever
  `CARGO_TARGET_DIR` the app inherits, every child inherits
  (`docs/wayfinder/map.md:26`, spawn policy: inherit env). See risk 4.7.
- Merging is not automated (`2026-11-04-agent-orchestration-design.md:30-31`).

### 1.9 Usage and cost tracking

- Per reply: `cost_micros`, tokens, model (`crates/core/src/lib.rs:528-537`,
  `Update::Cost` at `809`). Claude, Pi and OpenCode emit `Update::Cost`
  (`crates/driver/src/claude/events.rs:756`, `pi.rs:1071`,
  `opencode/events.rs:44`). Codex and Antigravity do not, so the footer falls
  back to a token times price estimate (`crates/app/src/app/chat/turn.rs:20-22`).
- Aggregates by day, month, project from CLI logs and saved chats
  (`crates/agent/src/usage_history/aggregate.rs:158-262`, UI in
  `crates/app/src/usage_view/`). Plan limits per provider
  (`PlanUsage`, `crates/agent/src/usage.rs:43-57`).
- Machine load: a CPU and memory sample every 3 s, only while the window is
  active (`crates/app/src/sysinfo.rs`, poll loop
  `crates/app/src/app/new.rs:809-832`, shown in the status bar,
  `status_bar.rs:18`). Stored in `ChatApp.sys_sample`
  (`app/mod.rs:533`).

Limits:
- Nothing aggregates by chat family (head plus its workers) or by role, and
  `list_agents` returns no cost (`orchestrate.rs:305-334`). No budget or
  threshold feeds any decision.
- Machine load is UI-only and never reaches the orchestrator. Core count is
  not read anywhere (a grep for `available_parallelism` and `num_cpus` over
  `crates/` finds nothing).

### 1.10 Watch list

- User-curated list of chats and terminals, persisted in settings, survives
  restarts, drops archived/deleted items (`Watch`, `settings.rs:583-600`,
  field `973`; `docs/superpowers/specs/2026-10-29-watch-list-design.md`).
  Peek card gives a live status line and a working composer.
- For the org it is a free "heads dashboard" for the human: watch all heads.
- Limit: manual, no roles, no ordering, no group (its own non-goals), and not
  visible to agents.

### 1.11 HITL ask routing

- Every permission ask reaches the human; the lead never approves a child's
  asks (`2026-11-04-agent-orchestration-design.md:36-39`).
- A child with a pending ask shows `needs_you` (`child_status`,
  `orchestrate.rs:127-128`). Asks are per-message requests rendered by
  `crates/app/src/hitl.rs`; recent fixes route Claude prompts to the host,
  sync asks across panes and expire orphaned asks (commits 7d4bfe0, 7d34e17).
- Limit: for an org this is a hard ceiling on autonomy. N heads and M workers
  in Ask or AutoEdit mode all queue on one human. Only an `Auto` or `Full`
  mode removes it, and message_agent refuses to message a chat with higher
  access than the sender (`orchestrate.rs:580-585`), so access modes must be
  planned across the whole org.

### 1.12 Token and socket model

- One Unix socket in a 0700 directory, mode 0600, one JSON line per call, 64
  KiB request cap, 32 connections, 30 s call deadline, 16 pending UI calls
  (`orchestrator/server.rs:15-16, 138`; `mod.rs:19`).
- One token per chat key, minted on first use, stable for the app lifetime,
  mapped to the chat in memory (`Agent::lead_token`, `agent.rs:261-272`;
  `Leads` map, `server.rs:18`). Tokens are minted for every chat, not only
  leads (`lead_tools`, `agent.rs:274-290`). Authority is decided later by
  `chat.lead` (`orchestrate.rs:273`).
- The MCP shim lists no tools without a token (`mcp.rs:24-31`).
- Limit: the whole orchestrator runs on the UI thread, one call at a time
  with a 16-deep admission queue (`MAX_PENDING`, `BUSY`, `mod.rs:19-21`). A
  chatty org of 6 heads and 20 workers hitting board tools can see "Aether is
  busy; retry shortly".

## 2. Gaps

Each row says what is missing, whether the code blocks it or merely lacks it,
and the nearest hook.

| Need | State | Evidence and hook |
| --- | --- | --- |
| Persistent named role session | Partly there. A pinned, titled, lead chat persists. No role field. | `SavedChat` has no role (`store.rs:78-108`). Title is the only handle and `/clear` resets it (`commands.rs:16`). Adding a column follows the ALTER pattern at `store.rs:251-253`. |
| Persona injection | Missing for every CLI provider. | The only system-prompt path is native OpenRouter `system_prompt` with a global `settings.instructions` (`crates/agent/src/turn.rs:114-138`, `settings.rs:828`). CLI chats get no system prompt from Aether. Alternatives: first user message (survives handoff selection), or per-CLI memory files discovered from the cwd (`inventory.rs:70-125`: `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`). A shared cwd means shared memory file, so roles need distinct files or first-message injection. |
| Pinned provider and model | Missing. Nothing locks a chat's model. | `pick_model` and `switch_agent` are open to the user and rewrite global defaults (`providers.rs:498, ~525`). `provider_locked` only blocks an empty-chat change (`handoff-aether.md` section 2, now superseded by `switch_agent`). |
| Memory per head | Transcript plus CLI session only. | See 1.7. A durable "memory file the head maintains" has no Aether support, but any CLI head can write a file in the project if its access allows. |
| Lead to head messaging | Works today, no sender tag, no auto return. | `orchestrate.rs:565-645`. |
| Head to head messaging | Works today when both are leads. Children have no `message_agent` (`orchestrate.rs:273-280`). | Contradicts the docs claiming children have no Aether tools (section 5): they have board tools and own `read_history`. |
| Heads as leads of workers | Works today if the head is top-level. Blocked only by the UI gate for children. | `shows_lead_chip` (`orchestrate.rs:22-24`); `set_lead` has no check (`manage.rs:492-519`). A hard rule needs a one-line check there plus in `lead_tools`. |
| Council or vote | Missing. | Tools never block: server deadline is 30 s (`server.rs:138`), Codex tool timeout about 60 s (`2026-11-04-agent-orchestration-design.md:204-205`). A vote must be asynchronous. Existing async pattern: queued spawn plus `tell_lead` (`orchestrate.rs:899-920`, `control.rs:355-366`). |
| Per-role budget | Missing. Data exists per reply (`cost_micros`), partly estimated for Codex and Antigravity. | See 1.9. Nothing enforces at spawn or message time. |
| Worker pool by CPU and RAM | Missing. Samples exist in the UI only. | `sysinfo.rs`, `new.rs:809-832`. Core count absent. Cap logic lives at `orchestrate.rs:796, 908`. |
| Review routing | Missing. Review is a column, not a person. | `board_on_turn` (`board/mod.rs:138-154`) is the only place a card enters Review automatically. No reviewer field. |
| Roster or org chart format | Missing. | Nearest: project scripts and settings maps are keyed by project (`settings.rs:881`). |
| Round-table view | Missing. Related views to reuse: board page (`Page::Board`, spec `tasks-board-design.md:73`), crew strip (`crew`, `orchestrate.rs:922-932`), watch peek, project page cards with `card_status` (`crates/app/src/project_page/model.rs:151-163`). |
| Loop guard on peer messaging | Missing. | See 1.2. |
| Cost or load visibility for the CEO | Missing in tools. | `list_chats` and `list_agents` omit them. |

## 3. Smallest credible path

Each increment ships alone and leaves the previous ones working. File lists
are likely touch points, not a spec.

### Increment 0: zero code, org by convention

Everything here uses features that exist now.

1. The human creates the heads as ordinary top-level chats: pick provider and
   model per chat, name them `[CTO]`, `[PM]`, `[SEC]`, `[TL]`, `[MKT]`, turn on
   the Lead chip, pin them (bulk tools skip pinned, `control.rs:179`), add them
   to the watch list. Only a human can create top-level chats; `spawn_agent`
   always sets `parent` (`create_child`, `orchestrate.rs:960-1023`).
2. The human creates the CEO as a lead chat on the strong model.
3. First message of each head is its persona, written once as the first user
   message (keeps priority in handoff selection, `handoff-design.md:57-59`):
   role, scope, the CEO's chat id, rules ("reply by message_agent to chat
   <CEO id>, sign your messages", "write durable notes to
   `.aether-org/<role>.md` and re-read it first"), and the review rule.
4. The CEO's first message holds the roster (title, chat id, provider, model,
   what the head owns) and the process: cards carry labels `dept:<role>` and
   `review:<role>`; use `list_chats {search:"CTO"}` to find a head
   (`mod.rs:83`); message a head, then end the turn; heads message back; do
   not poll.
5. Board: use one project folder for all heads (the board write scope is the
   project, `orchestrate.rs:371-386`). Do not give heads distinct cwd folders
   for persona files, or they split the board.
6. Hiring: a head spawns workers itself with `spawn_agent` (it is a lead), for
   example `{provider, model, worktree:true, card}`. The "council" is a rule
   in the persona text: before spawning more than N workers, message the CEO
   with reason and budget and wait for a reply.
7. Review: reviewer reads the card (`read_card` includes the worker's final
   reply as a note, `board/mod.rs:157-178`) and writes a note via
   `update_card`. The human drags to Done.
8. Access: give CEO and heads the same access mode, or messaging is refused in
   one direction (`orchestrate.rs:580-585`).

Cost: nothing but prompts. This increment also tells you whether the
convention works before any code is written. Files changed: none.

### Increment 1: make the org legible to the CEO

`list_chats` gains `model`, `queued` (queue length), `tokens`/`cost` summed over
the chat's replies, and `last_reply` age. `list_agents` children gain cost.
Likely files: `crates/app/src/app/control.rs` (`list_chats`, ~190-215),
`crates/app/src/app/orchestrate.rs` (`list_agents`, 305-334), possibly
`crates/agent/src/orchestrator/mod.rs` tool descriptions. A reusable cost sum
already exists per message (`chat/turn.rs:20-22`).

### Increment 2: sender tag and loop guard on message_agent

Prefix delivered text with `[from <title> #<id>]` and carry a hop counter in
the `Queued` struct; refuse after a small bound or when the target's last
message was from the sender with no human message in between. This turns
head-to-head chat into something heads can reason about.
Likely files: `crates/app/src/app/orchestrate.rs` (`message_agent`, 565-645,
`Queued` literal at 611), `crates/app/src/app/mod.rs` (`Queued` definition),
`crates/agent/src/store.rs` (`SavedQueued`, only if the hop must persist).

### Increment 3: role, persona and pinned model on the chat

Add `role` and `persona` (text) and `model_locked` (bool) to the chat.
Persona is prepended to the first prompt of every fresh CLI session (new
session, provider switch, post-clear), so it also survives rewind and
handoff without relying on budget selection. `model_locked` makes
`pick_model`, `switch_agent` and the selector refuse changes and stops them
rewriting global defaults. Sidebar shows the role.
Likely files: `crates/agent/src/store.rs` (columns via the ALTER loop at
`251-253`, `SavedChat`, upsert), `crates/app/src/app/mod.rs` (`Chat`, line
112), every `Chat { .. }` literal (`create.rs` restore at ~600, `create_child`
at `orchestrate.rs:970-1021`, constructors), `crates/app/src/app/chat/mod.rs`
(`persist`), `crates/app/src/app/chat/send.rs` (prompt build near 476, or
`prepare_handoff` in `agent.rs`), `crates/app/src/app/providers.rs`
(`pick_model`, `switch_agent`), `crates/app/src/app/sidebar.rs`,
`crates/app/src/app/chat/commands.rs` (do not reset a role-bearing title on
clear). A mechanical but wide change; the handoff research counted about the
same plumbing for one nullable column (`handoff-aether.md`, option B).

### Increment 4: roster file and `ensure_head`

A roster in `<project>/.aether/org.toml` (role, title, provider, model, access,
persona path, memory path, reviewers). New lead-only tool `ensure_head(role)`
that finds a top-level chat with that role or creates one (provider, model,
lead on, pinned, persona), idempotent like `nonce`. This is what makes the org
survive a fresh checkout or a deleted chat, and it keeps the CEO from hand
rolling heads as children (which would get the child restrictions).
Likely files: new `crates/app/src/org.rs` (parse, match), `crates/agent/src/
orchestrator/mod.rs` (`tool_specs`, and the `history_specs`/`board_specs`
siblings need no change), `crates/app/src/app/orchestrate.rs` (dispatch at
281-298), `crates/app/src/app/chat/create.rs` (a variant of
`new_chat_in_current_project`, line 376, with `parent: None`).

### Increment 5: round-table view (read-only)

A page that lays out CEO, heads, and each head's live workers with status
dots, load and cost, reading the same `Chat` data the sidebar and crew strip
use. Click opens the chat via `open_chat` (`orchestrate.rs:934-953`). Reuse
`card_status`, the crew ordering (`crew_rank`, `162-170`) and watch-peek
patterns. Depends on increment 3 for role labels, otherwise it can fall back
to titles.
Likely files: `crates/app/src/app/mod.rs` (`Page` enum), `render.rs`, a new
`crates/app/src/org_view/` module, `palette.rs`, `actions.rs`.

### Increment 6: global worker pool and per-head quota

Replace the single `running_children >= max_children` test with
`min(per-head quota, global pool)`. Pool size from
`std::thread::available_parallelism()` and the latest `sys_sample`
(memory headroom), with a floor of 1. Reuse the existing queue and
`drain_spawn_queue` so over-cap spawns wait instead of failing. Surface "pool
full, queued at position n" in the existing reply (`orchestrate.rs:813-822`).
Likely files: `crates/app/src/app/orchestrate.rs` (796, 820, 908,
`running_children`), `crates/core/src/settings.rs` (pool and per-role caps,
next to `max_children`), `crates/app/src/sysinfo.rs` (core count), the
settings UI (`settings_view.rs`) and docs (`orchestration.md`).

### Increment 7: review routing

When a card enters Review (`board_on_turn`, `board/mod.rs:138-154`), look up
its `review:<role>` label and message that head with the card id. Reviewer
output goes back as a card note. Done stays human-only.
Likely files: `crates/app/src/board/mod.rs`, `crates/app/src/board/model.rs`
(label parsing), `crates/app/src/app/orchestrate.rs` (reuse the `message_agent`
send path as an internal call).

### Increment 8: per-role budget

Sum `cost_micros` (with the priced fallback for Codex and Antigravity) over a
head and its children per day. Block `spawn_agent` and `message_agent` into an
over-budget head with a clear error, and warn the CEO through `list_chats`.
Likely files: `crates/app/src/app/orchestrate.rs`, `crates/app/src/app/
chat/turn.rs` (extract the cost helper), `crates/agent/src/usage_history/
pricing.rs`, `crates/core/src/settings.rs`.

### Increment 9: council

Start rule-based: the hire request is a tool call that returns `pending`; the
app checks pool, budget and role quota and answers through `tell_lead`
(the pattern at `orchestrate.rs:899-920`). Only if that proves too blunt, add
a real vote: the app messages each head with the request and tallies replies
with a timeout. A vote costs one model turn per head per hire, which is why
it comes last.
Likely files: new `crates/app/src/org.rs`, `orchestrate.rs`, `control.rs`
(`tell_lead`).

## 4. Risks specific to Aether

4.1 Token cost. Persistent heads re-read their growing context on every wake.
Idle heads cost nothing (processes are reaped at 600 s, `agent.rs:193`) but a
head woken by every board change pays for its whole history each time. Peer
review doubles tokens per artifact; a council vote multiplies by head count.
Fan-out is already flagged in the original spec
(`2026-11-04-agent-orchestration-design.md:206-208`). The cost footer is
estimated for Codex and Antigravity (`chat/turn.rs:20-22`), so budgets on a
mixed org are approximate.

4.2 Socket and token model. Tokens are per chat, in-memory, stable for the app
lifetime (`agent.rs:261-272`), so a restart re-mints them and every CLI
process must be relaunched; resumed heads do that on their next send. The
server is a 32-connection, 16-pending, 30-second path serviced on the UI
thread (`server.rs:15-16`, `mod.rs:19`). A noisy org will hit `BUSY`.
Authority is a runtime lookup of `chat.lead`, so the "heads cannot be
children" invariant must be added where `lead` is set, not only in the UI.

4.3 "Child cannot be lead". It is a UI convention (`orchestrate.rs:22-24`),
which is why top-level heads work. The cost of using heads as peers rather
than children: no auto-delivery of head results to the CEO, no crew strip for
heads, no `read_agent`/`stop_agent` on heads (child-only, `515-525`), and
`max_children` does not bound heads. The cost of making heads children of the
CEO: they could not spawn workers without lifting the rule, and depth would
break the original assumption "depth is 1" (`2026-11-04-...:34`) that other
code leans on (`deliver_to_lead` finds the parent lead with
`c.lead && Some(c.id) == child.parent`, `orchestrate.rs:707, 735`). Keep
heads top-level.

4.4 Context bloat in long-lived heads. Handoff only selects within a 16k
token default budget (`handoff-design.md:41`). Claude auto-compacts and other
CLIs have `/compact` (`handoff-aether.md` section 3), but a compaction can
drop persona wording. Mitigation is a durable role file the persona tells the
head to re-read, plus (increment 3) persona re-injection on every fresh
session. `/clear` is the only way to reset a head and it also resets the title
(`commands.rs:16`).

4.5 Cross-provider persona drift. The same persona text behaves differently
on Claude, Codex, Pi, OpenCode and Antigravity, and Antigravity has no typed
limit signal (`handoff-aether.md` section 4) and no cost update. Memory file
conventions differ per CLI (`inventory.rs:70-125`). Pin the model per role
(increment 3) so a limit-driven handoff does not silently change a head's
brain. Marketing on Gemini means Antigravity (in `CLIS`, installable as a
head or worker) or OpenRouter (head only, native tools, not in `CLIS`); whether
a "Gemini 3.8 Flash" id exists is decided by the probe's model list
(`SpawnArgs::parse`, `orchestrate.rs:70-77`), which I did not query.

4.6 Concurrency limits. `max_children` is per lead (see 1.3); six heads at 8
each is 48 workers, plus CLI processes (one per live chat, one extra
`opencode serve` per OpenCode lead). Spawn queue is 32 per lead and lost on
quit. Peer messaging has no loop guard (1.2), and HITL asks all funnel to one
human (1.11). A head that is mid-turn queues incoming peer messages and
processes them in order, so a slow head blocks the CEO's question.

4.7 Shared `CARGO_TARGET_DIR` clobbering. From the owner's memory note
(`shared-target-dir-clobbers`): concurrent worktrees sharing one target dir
overwrite workspace-crate artifacts (cargo hashes path crates by their path
relative to the workspace root), producing phantom compile errors. Aether
inherits env into children and sets nothing (`docs/wayfinder/map.md:26`), so
a global `CARGO_TARGET_DIR` in the user's shell, applied to many worker
worktrees, reproduces it. Older plans already prescribe a per-worktree dir
(`docs/superpowers/plans/2026-11-04-agent-orchestration-p0-p1.md:26`). The
fix is cheap in prose (persona rule: unset it, or set it per worktree) and
cheap in config (an `on_new_worktree` project script, `ProjectScript`,
`settings.rs:758-763`, can seed a per-worktree dir), but disk is the catch:
the main target dir is about 66 GB, so N cold builds is N times that. The
worker pool (increment 6) should count Rust builders against a smaller cap
than non-building workers.

4.8 Autonomy ceiling. Every ask goes to the human (1.11). An org that needs
the human for each edit approval is a very slow org; one that runs in `Auto` or
`Full` needs the access-mode planning from 1.11 and a trust decision the owner
has to make explicitly.

## 5. Doc drift found while reading

These affect how an org built on this would be understood, so they are worth
fixing before anyone designs on top of them.

- `docs/task-board.md:14` and the board spec non-goal (`tasks-board-design.md:38`)
  say children get no Aether tools. In code every chat gets the five board
  tools and `read_history` when seeded (`agent.rs:274-290`,
  `orchestrate.rs:263-280`).
- `docs/superpowers/specs/2026-11-04-agent-orchestration-design.md:34` justifies
  depth 1 by "children don't get the orchestration tools". True for spawn and
  message tools; the depth limit is really the `lead` flag and the UI gate.
- `docs/orchestration.md:20` says OpenRouter chats cannot be leads. Code says
  they can (`lead_capable`, `orchestrate.rs:18-20`), though they cannot be
  spawned as children (`CLIS`).
- `docs/wayfinder/research/handoff-aether.md` line numbers and its claim that
  provider can only change on an empty chat predate the shipped `switch_agent`;
  use `2026-11-07-handoff-design.md`.

## 6. Open items I could not settle from source

- Whether a Claude lead accepts the same `mcp__aether__*` pre-approval across
  `--resume` for peer-lead use at scale. Phase 0 notes say the flags hold
  (`orchestration-adapters.md:9`), but I did not run it.
- Whether Antigravity exposes any Gemini 3.8 model id in this install. It is a
  runtime probe result, not a source fact.
- Real cost of six idle-then-woken heads. Needs a measured run with the usage
  view; the numbers do not exist in the repo.
