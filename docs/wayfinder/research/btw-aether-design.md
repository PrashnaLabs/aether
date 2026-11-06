# `/btw` — Aether side-question design research

Read-only research. No code changed. File refs as found this session.

## 1. Aether composer pipeline

- Suggestion menus: `crates/app/src/suggest/mod.rs` `find_trigger` (:52) only
  opens `/` at input start; `update_suggestions` (:82) builds the menu;
  `SlashCommand` enum (:15) + `run_command` (:189) execute a pick.
- Command list: `crates/app/src/suggest/slash.rs` `slash_commands` (:13).
  Aether-native acts directly (new, clear, doctor, diff, terminal, theme,
  model, mode…). The rest come from live CLI probes
  (`crates/driver/src/catalog.rs` `ProviderProbe`, `probe`) and are inserted
  as `SlashCommand::Prefill("/name ")` — the composer keeps the text and the
  CLI parses it on send. `prompt_commands` (slash.rs:7) adds the one
  Aether-forged prompt command (`/compact`, except AgY).
  TUI-only / reserved names (`help`, `model`, `new`, `stop`, …) are filtered
  out of the probe list (slash.rs:125). Plan doc:
  `docs/superpowers/plans/2026-10-16-cli-agent-commands-support.md`.
- Intercept point: `ChatApp::send` in `crates/app/src/app/chat/send.rs` (:104).
  Order: hard stops (locked, worktree gate, attach, voice, `!` shell mode) →
  attachments + `cli_command` check (:130, :194) → busy queue (:174) →
  `begin_setup_or_submit` → `submit` (:271). `submit` pushes a user
  `Message` + an empty assistant `Message`, calls
  `Agent::send(session, text, …, history, &ChatSession)` and streams
  `Update`s into that reply row (`stream` :498, `apply` :723, `finish_run`).
- Reply rendering: `Message` (`crates/core/src/lib.rs`, `Step` enum there)
  carries text + steps (tools, notices, steers, search). `aether_ui::notice`
  renders `Notice` steps (lead + body + collapsible “View summary”).
  Progress chrome (phase / stall / retry countdown) lives in
  `crates/app/src/app/chat/live.rs`.

## 2. Ephemeral / side concepts that already exist

- **Notice** (`crates/core` `Notice`, `NoticeKind::Info`, `Step::Notice`,
  `aether_ui/src/chat/notice.rs`): the only in-transcript “extra card” that
  can show a short answer, upserts by id, collapses with “View summary”.
  Persisted inside the turn’s `Message` though — not ephemeral by default.
- **Watch peek** (`crates/app/src/app/watch/`, `view.rs` `render_peek`:
  400px floating panel): shows *another chat / terminal* live. Aether-wide
  peek for watching other work, not for answering a new question in place.
- **Steer** (`send.rs` `steer_queued`, core `Update::Steered`,
  `docs/wayfinder/tickets/07-steer-policy.md`, now closed): mid-turn follow-up
  continues the *same* run as `Delta`s and becomes part of that turn’s
  context. `/btw` must NOT steer — it must not enter main context.
- **Second driver in parallel:** `crates/agent/src/agent.rs` keeps one live
  session per `chat.key` (`spawn_live` :654, `live` map). No lock against a
  second key — a fake key (`btw:<chat>:<n>`) spawns a fully independent
  `Driver` + thread, own `TurnTx`, no clash with the main run. There is no
  “one-shot” API yet; `send`/`steer` always bind the reply to a chat row.
  `Agent::fork_session(id, history)` (:283) seeds a fresh model session from
  the visible transcript — the pattern a side turn needs.

## 3. What T3 Code does with `/btw`

- `pingdotgg/t3code`, `packages/shared/src/composerTrigger.ts`:
  `parseSideChatCommand` matches whole-message `/side [prompt]` / `/btw
  [prompt]` (tests in `composerTrigger.test.ts`).
- Menu: `apps/web/src/components/chat/useComposerMenuItems.ts` — `/side`
  (“Ask a side question in a new side chat”) and `/btw` (“Same as /side”),
  gated by `sideChatAvailable`; `composerSlashCommandSearch.ts` allows them
  only at prompt start. Picking one just inserts `/btw ` into the composer
  (`ChatComposer.tsx`); submission forks.
- Runtime: `apps/web/src/components/chat/useSideChat.ts` `open()` =
  **fork the thread at its latest stable run** (`forkSideChat`), send the
  prompt there, open the result as a right-panel tab (`SideChatPane`).
  `docs/user/thread-sidebar.md` § Side chats (lines ~297-319): inherits
  thread context + permissions + cwd, main thread keeps running, hidden from
  sidebar, **“Bring back”** attaches it to the next main message (only while
  idle). Protocol: full session fork at a checkpoint, not a side channel,
  not a one-shot. `ThreadForkService.ts`: side chats can’t nest; fork needs
  a finished/waiting source run.

## 4. Orca

- `which orca` is empty, but the app exists:
  `/Applications/Orca.app/Contents/Resources/bin/orca` (v1.4.249).
  Its `--help` has no `/btw` or side-question command; commands cover
  worktrees, terminals, agents (`agent launch`, `agent-context`,
  `worktree create`), hosts, plugins. The `skills get orca-cli` guide
  (`~/.agents/skills/orca-cli/SKILL.md`) is entirely about worktree/agent
  handoff and terminal control. `app.asar` has no `sideChat`/`/btw`/`parseSideChat`
  strings. **Conclusion: Orca has no side-question feature** — it parallelizes
  via worktrees, not via forked Q&A.

## 5. Pi feasibility

- Aether drives Pi via `--mode rpc` (`crates/driver/src/pi.rs`
  `launch_args` :647): `--session <chat.cli_session>` re-opens one file,
  `--tools read,grep,find,ls` when read-only. Pi RPC runs one agent per
  process; two processes on the same session file would fight (driver even
  `shut()`s the old child before re-opening the file).
- `pi --help` (checked read-only): `--no-session` = ephemeral run,
  `--no-tools/-nt` disables tools, `--fork <path|id>` copies a session.
  So a side turn can run `pi -p --no-session --no-tools "Q"` (fresh, no
  writes) — but that loses conversation context. Context can be supplied by
  (a) transcript snapshot in the prompt (`history()` in chat/mod.rs does
  exactly this for fresh sessions), or (b) a temp copy of the session file via
  `--fork`. (a) is simpler; (b) keeps tool-call history.
- Other CLIs (`claude/codex/opencode --help`) show no `/btw` or side-command
  flags here. Only AgY lists one: `"btw"` is in
  `crates/driver/src/antigravity.rs:43` `TERMINAL_ONLY` — on AgY’s stream
  transport it **ends the process**, and Aether’s driver intercepts it
  (`terminal_only` :829) to warn/error. So even AgY’s native `/btw` is not
  usable over Aether’s stream; it must be emulated or intercepted.

## 6. Recommendation: one design for all providers

**UI:** inline card in the transcript, not a popover, not a new chat.
Add `Step::Btw { question, answer, state }` (or reuse a `NoticeKind::Side`
to skip a variant) rendered by the existing notice card with a “by the way”
lead, streaming deltas into the card, collapsed by default when done.
It sits on the *current* reply as a step, or as its own slot between the
last two messages if no run is live. Excluded from `history()` (it only
reads `Message.text`), never queued as a steer, never counted as a turn.

**Behaviour:**

- **Idle main:** identical path. `/btw` never touches `chat.cancel`.
- **Busy main:** `/btw` bypasses the queue check in `send()` entirely —
  main run undisturbed. The side run is tracked on a separate
  `side: Vec<SideQ>` (or on-the-fly channel), and `finish_run` for the main
  turn cannot kill it (different session key).
- **“Bring back”:** skip in v1. If added later, mirror T3: opt-in paste of
  the answer into the next prompt, only when idle.

**Plumbing:** one new `Agent::side_prompt(key, history, question) -> Reply`
that spawns a fresh `Live` under `btw:<chat_id>:<n>` with
`history = history(&chat.messages)` (snapshot, not live) and a lead line
“side question — answer only, no tools, do not change the workspace”. The
app streams its `Update::Delta`s into the card, then `close_session(key)`.
No `Driver` trait change needed; every provider reuses its existing spawn.

| Provider | Strategy |
|---|---|
| Claude Code | Emulated: ephemeral driver key, seeded with history snapshot, read-only mode (`plan`), new session id (fresh `--session-id`) |
| Codex | Emulated: ephemeral app-server thread, history snapshot, read-only sandbox |
| Pi | Emulated: ephemeral driver key, or `pi -p --no-session --no-tools` with the snapshot (copy of session file if tool history is wanted) |
| OpenCode | Emulated: ephemeral key, fresh session via the configured service (shared-service TUI is not reused) |
| Antigravity (AgY) | Emulated: `/btw` is terminal-only on AgY’s stream and ends the process, so intercept and run the ephemeral path like the others |
| OpenRouter (native) | Emulated in-process: second `Turn` with the same snapshot — no CLI spawn, no tools |

**Why not the alternatives:** T3’s full fork + tab duplicates a session and
needs a “Bring back” story; Orca has nothing to copy; steer pollutes main
context; watch-peek is for other chats, needs the watchlist and shows live
state, not a fresh answer. Smallest surface: one slash suggestion
(`/btw` → literal prefix insertion, exactly like `/side` in T3 and
`Prefill` in Aether), one intercept in `send()`, one `Step`/notice card,
one ephemeral spawn.

## Risks / unknowns

- Extra model cost per question; nothing cached.
- “No tools” is only as strict as each driver’s read-only mode — tools may
  still attempt reads; side runs can’t edit if `ReadOnly`/`plan` is forced.
- Long transcripts make snapshots large; cap to last N turns for v1.
