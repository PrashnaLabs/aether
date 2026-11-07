# Outside-agent resume path

Bug: user adopts an outside agent into Aether, then cannot send to resume it.

## Adoption

- `open_external` (`crates/app/src/agents_view.rs:86`) rejects empty session or `pid=None`, then `add_external_chat`
  (`crates/app/src/app/chat/create.rs:496`).
- Sets `cli_session=a.session`, `external_pid=a.pid`, provider, cwd, and forks an in-process session from the
  transcript snapshot (`create.rs:511-543`). `fork_session` error returns silently; no chat, no error shown.
- Transcript freezes at adoption (`crates/agent/src/agent.rs:283`); no live sync while locked.
- `external_pid` is in-memory only (no store column), so after restart only the session-match lock below holds.

## Send path (primary failure point)

- Composer computes lock at `crates/app/src/app/composer/mod.rs:214-215` via
  `externally_locked` (`crates/app/src/agents_view.rs:66-76`): any scanned external agent whose `pid==chat.external_pid`
  OR `session==chat.cli_session`. Banner `composer/mod.rs:216-221`: "Running in {origin}. Stop it or wait…".
- Send button disabled with reason "Running in another app (read-only)" (`composer/mod.rs:539-546`).
- `send()` returns early on `locked()` (`crates/app/src/app/chat/send.rs:105`); `steer_queued` same (`send.rs:211`).
  `locked()` at `crates/app/src/app/chat/mod.rs:447-450`. No queue, no error toast; send silently does nothing.
- `submit_as` (`send.rs:283`) has **no** locked guard; only the composer/`send()` gate it. Internal callers
  (edit `crates/app/src/app/chat/edit.rs:56`, queue drain `send.rs:609-611`) can bypass.
- Unlock depends on rescan clearing `external_pid` (`crates/app/src/app/providers.rs:177-185`); a failed scan keeps
  the old list (`providers.rs:176`). Polls every 2s/10s (`agents_view.rs:17-20`).
- `SidebarView::after_select` (`crates/app/src/app/mod.rs:102-108`) keeps an adopted chat in the Outside view while locked.

## Must the outside process stop first?

In Aether's design, yes for a same-session resume: while `self.external` still contains a match, sending never
reaches a driver. Stopping is offered in two places, not in the composer/chat:

- Outside sidebar row stop button (`crates/app/src/app/sidebar.rs:855-861`).
- Settings > Outside Aether stop button (`agents_view.rs:286`), plus banner text telling the user to stop it.
- `stop_external` (`agents_view.rs:108-162`): Claude gets Interrupt/Quit alert, Pi gets Quit alert, others stop
  directly. Actual signal via `external::stop` (`crates/agent/src/external.rs:486-509`): SIGINT/SIGTERM for
  processes, OpenCode interrupt API for service sessions.

## Driver resume behavior (once unlocked / if it ever ran)

- **Claude** — `--resume <id>` if `~/.claude/projects/**/<id>.jsonl` exists, else silently `--session-id <new>`
  (`crates/driver/src/claude/spawn.rs:340-342`). Resume while alive: stderr match on `--fork-session` /
  "running in the background" (`spawn.rs:255-265`) triggers one respawn with `--fork-session` (`spawn.rs:281-286`),
  notice "open elsewhere… continues in a fork". Works while alive but forks; original session untouched.
  Unmatched failure ends "Claude didn't start" (`spawn.rs:103-107`).
- **Pi** — `--session <file>` only if `Path::is_file()` (`crates/driver/src/pi.rs:661-663`); `resumed=is_file`
  (`pi.rs:233`). No lock detection: two pi processes sharing one jsonl append risk interleave/corruption.
  External session itself is guessed: newest `*.jsonl` in `~/.pi/agent/sessions/--<cwd>--` modified after the
  process started (`crates/agent/src/external.rs:133-150`), so a wrong file is possible. Works after stop.
- **OpenCode** — resume iff id starts `ses_` (`crates/driver/src/opencode/session.rs:36`); `GET /api/session/<id>`,
  404 creates a fresh session reusing the id (`session.rs:55-78`), silently losing history. Adoption is usually
  impossible: service-merged sessions have `pid=None` (`external.rs:411`), and `open_blocker`
  (`agents_view.rs:40-48`) refuses with "Busy in another OpenCode client"; `open_external` returns early.
  Only TUI-joined sessions (`external.rs:396`) can be adopted.
- **Codex** — `thread/resume {threadId, excludeTurns:true}` (`crates/driver/src/codex/spawn.rs:163-175,341-347`).
  On "thread writer lock" the error notice says "open in another Codex window. Started a new one."
  (`spawn.rs:300-306`) and it falls back to `thread/start`; `Update::Session` then overwrites `cli_session`
  (`send.rs:685`). Resume-while-alive fails over to a new thread. Needs outside process gone (lock released).
- **Antigravity** — `--conversation <id>` (`crates/driver/src/antigravity.rs:744-746`); `resumed` only if `init`
  returns the same id (`antigravity.rs:145`). Mismatch notice "Couldn't find the earlier agy conversation, so
  this is a new one." (`antigravity.rs:674-681`). No lock handling in code; two `agy` on one conversation is
  unguarded. Works after stop.

## Ranked root causes

1. Lock-by-design: composer disables Send and `send()` returns while the outside agent is still scanned.
   Stop UI lives outside the chat (Outside sidebar/Settings), so the user reads a dead chat.
2. Codex/Pi/Antigravity/OpenCode cannot truly resume a live session: they fork, fall back to a new
   session/thread, or share a file unguarded. Only Claude has a live-resume path, and it forks.
3. Silent fallbacks (`--session-id`, new OpenCode session, `thread/start`) rename `cli_session`, breaking the
   link to the outside transcript with only a small notice.
4. Adoption gaps: OpenCode service sessions unopenable (`pid=None`), silent `fork_session` failure,
   snapshot transcript goes stale while locked.

## Verdict per provider

| Provider | Resume while outside alive | After outside stops |
|---|---|---|
| Claude | forks (`--fork-session`) | yes, `--resume` |
| Pi | unguarded shared file | yes, `--session <file>` |
| OpenCode | usually unadoptable | yes if TUI-joined, else new session |
| Codex | falls back to new thread | yes, `thread/resume` |
| Antigravity | unguarded / new conversation | yes, `--conversation` |

Skipped: live CLI runs of each provider; all claims from code paths only.
Risk: provider-side lock messages matched by substring (Claude/Codex) can drift with CLI updates.
