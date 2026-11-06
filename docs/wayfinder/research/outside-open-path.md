# Outside-open path: why clicking an outside agent can do nothing

Providers `scan` can return (`crates/agent/src/external.rs:56-105`):
Claude, Pi, Codex, Antigravity (via `ps`, `agent_rows` L268-296) plus
OpenCode (TUI merge + service-only rows, `merge` L451-490).

## Path

1. Sidebar: `render_outside_sidebar` (`crates/app/src/app/sidebar.rs:786-796`)
   -> `outside_rows` (L851-865) builds a row per `self.external` entry.
   Row click always fires `open_external`; unlike the Agents page, the
   sidebar row is never disabled (`crates/ui/src/chat/sidebar/rows.rs:245-284`).
   The only hint is the hover tip: `open_blocker` text (`sidebar.rs:102-104`).
2. Agents page: `render_agents` (`crates/app/src/agents_view.rs:217-298`)
   disables Open when `open_blocker` is `Some`, tooltip shows why (L247-267).
3. `open_external` (`agents_view.rs:81-101`):
   - L86-88: `session.is_empty() || pid.is_none()` -> silent `return`.
   - L90-92: `external_chat()` hit -> `select()` only, transcript never refetched.
   - Else spawn `agent.transcript(a)` (`agent.rs:524-530` -> `external::transcript`
     `external.rs:512-540`), then `add_external_chat`.
4. `add_external_chat` (`crates/app/src/app/chat/create.rs:496-577`):
   re-checks `external_chat` (L503), then `fork_session` (L512-514):
   any error -> silent `return`, no chat, no notice, nothing stored.
5. `external_chat` (L489-494) matches any chat with same `external_pid`
   **or** same `cli_session`, with no archived/staleness check.
   `select()` (`chat/mod.rs:179-233`) does un-archive (L216-219), so an
   archived match still opens; an empty/stale match opens the stale content.

## Silent no-ops, by provider

| Case | Fires where | User feedback |
|---|---|---|
| Session unresolved (empty `session`) | `open_external` L86 | Sidebar: none (hover tip only). Agents page: Open disabled. |
| OpenCode service-only row (`pid: None`) | `open_external` L86 | Same. Row exists for busy no-TUI sessions (`external.rs:474-486`) yet can never open; blocker text: "Busy in another OpenCode client" (`agents_view.rs:43`). |
| `fork_session` error | `create.rs:512-514` | None, anywhere. `sessions_made` resets to 0 each launch (`app/new.rs:590`), so `chat-N` can collide with a persisted session id. |
| Transcript unavailable/empty | `external.rs:512-540` returns `[]` | Chat opens empty; user sees a blank "resume". |
| Already opened once | `external_chat` hit | Opens existing chat; new outside activity never imported. |

Session resolution failures (empty `session` while process is live):

- Claude: registry file `~/.claude/sessions/<pid>.json` missing, pid mismatch,
  or no `sessionId` -> `claude_registry` returns early (`external.rs:126-143`).
- Pi: no jsonl in the cwd-derived session dir newer than process start
  (`pi_session` L145-164); wrong/empty cwd from `lsof` also misses.
- OpenCode TUI: no session in same cwd that is fresh (`merge` L458-470);
  service unreachable -> `opencode_sessions` `None` (L204-213), no match at all.
- Codex: no `session_index.jsonl`, or every entry own (`codex_session` L166-183).
  Note: match ignores cwd/thread, so a non-empty session may belong to
  a different Codex thread.
- Antigravity: no `brain/` dirs or all own (`antigravity_session` L185-204);
  same no-cwd caveat as Codex.

Transcript gaps:

- Claude: file not found under `config_dir/projects/*/<session>.jsonl`
  (`claude_file` L542-551) -> `[]`.
- Pi: `session` not ending `.jsonl`, or file unreadable (L515-517) -> `[]`.
- OpenCode: service unreachable or non-200 (L518-527) -> `[]`.
- Codex / Antigravity: no branch at all (`_ => Vec::new()`, L529) ->
  **always** an empty chat. These two providers can never resume content.
- Send path for such a chat: `reattach_wanted`/`cli_request` resume by
  `cli_session`, but with an empty forked history the context is gone.

## Second click does nothing new

- `external_chat` matches on pid alone: pid reuse by a different outside
  process selects the old chat, never the new agent's transcript.
- `cli_session` match is permanent (survives restart via `persist`,
  `chat/mod.rs:410-446`): a first open with an empty/failed transcript
  freezes that content; later clicks only `select()` it. No refetch, no
  "refresh" affordance anywhere on the path.
- `scan_agents` clearing `external_pid` when unlocked (`app/providers.rs:186-193`)
  does not clear `cli_session`, so the session match still wins.

## Most likely root causes, ranked

1. Empty `session` at click time (Claude registry / Pi mtime / OpenCode merge
   miss) -> sidebar click silently returns; most common "can't resume".
2. OpenCode service-only rows (`pid: None`) are listed but structurally
   unopenable by `open_external` L86.
3. Codex/Antigravity have zero transcript support -> empty "resume" chat,
   looks like a failed resume.
4. `fork_session` failure swallowed at `create.rs:512-514` (session-id
   collision after restart is the plausible trigger).
5. One-shot `external_chat` match: stale/empty chat served forever,
   pid-reuse selects the wrong chat.
