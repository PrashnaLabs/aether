# Agent context handoff

Date: 2026-11-07, revised 2026-11-12. Code: `crates/agent/src/handoff.rs`,
`prepare_handoff` in `crates/agent/src/agent.rs`, `handoff_history` in
`crates/app/src/app/chat/mod.rs`, `switch_agent` in
`crates/app/src/app/providers.rs`.

## Intent

When an agent hits its usage limit, or the user wants a different agent or
model to continue, move the conversation to another agent in one step and keep
working in the same folder. The receiving agent must get an honest, bounded
picture of what happened, and must never lose the user's new message.

## What travels

A handoff is a budgeted selection of the saved transcript, not an
agent-written summary.

- Each chat message becomes one or more items (`handoff_history`): the user
  message (with an `[attached: …]` note), and for a reply its commands
  (`Command`, `Exit code`, output), edited file paths, plans, the reply text
  and its error. Reasoning, other tool calls, tool state and attachments are
  not replayed.
- Every item carries its status: `completed`, `failed`, `interrupted` or
  `running`, so partial work from a failed or stopped turn is visible as such.
- Each item is rendered with an attribution line:
  `[Historical {role}; {kind}; chat=…; message=…; item=…; model=…; status=…]`.
- A coverage note precedes the items: which messages it covers, the item
  range, how to recover what was left out with `read_history`, and that the
  history is context, not a new request or higher-priority instructions.
- The user's new message follows under `User message:` and is never shortened.

Codex receives the items natively through `thread/inject_items`. Other agents
get the same text inline in the first prompt.

## Budget

`handoff::budget` decides how many bytes of history fit:

- Cap: `AETHER_CONTEXT_HANDOFF_TOKEN_CAP` (default 16,000, clamped to
  1,024–64,000), and never more than 64,000 bytes.
- Window: the window reported by the last reply from the same model, else
  128,000.
- Subtract what the session already holds (reported context tokens, or an
  estimate from the saved transcript and attachments), the new message
  (JSON-escaped bytes), 8,192 per image and 4,096 per other attachment, and a
  reserve of max(16,000, window / 4) for instructions, tools and further work.

One UTF-8 byte counts as one token, which is pessimistic. Selection
(`select_history`) keeps the last user message, the last reply and the first
user message when they fit, then fills newest-first. Items are kept or dropped
whole; the result never exceeds the budget. When even the coverage note does
not fit, the send fails with "This message leaves no room for the handed-off
conversation. Shorten it or pick a larger-context model."

## Recovering omitted history

`read_history` (offset, limit, max_chars, text_offset) returns any message in
full with its tool calls. A long message is read in pages by following
`next_text_offset`.

## Switching agents in place

`switch_agent` keeps the chat and inserts a "Context handoff" divider.

- Each chat remembers the native CLI session of every agent it used
  (`agent_sessions`, with how many messages that session already holds).
- Switching to a new agent starts a fresh session seeded with the budgeted
  full history (`full_thread_summary`).
- Switching back to an earlier agent resumes its session and sends only what
  happened since it last took part (`delta_since_target_last_seen`). If the
  resume fails, or nothing fits in the resumed session, a fresh session gets
  the full history instead.

## Delivery

The pending delta (`handoff_seen`) is marked `pending` at send and cleared on
the first agent output.

- A turn that errors before any output leaves it unsent, so the next send
  retries it. For Codex, which took the delta into its thread natively, the
  next send starts a fresh session instead, so the history is not injected
  twice.
- A delta still pending at the next send means delivery is uncertain (the app
  quit mid-turn): the next send starts a fresh session with the full history.
- A failed turn with no agent output was never received by the native
  session; it is re-delivered as part of the next delta.

## Copies and forks

- "Continue in new chat" copies the chat onto another agent; the copy gets the
  budgeted full history on its first send.
- Duplicate on the same agent forks the native session when the agent
  supports it (Claude `--fork-session`, Codex `thread/fork`) and the source is
  idle. If the fork fails, the copy starts fresh with the portable history and
  shows "Could not fork the source conversation natively (…). Started a fresh
  session with its history."

## Side questions

`/btw` gets a compact digest of the main chat instead of the transcript: user
and assistant lines, commands and file changes, each compacted to 240
characters, newest kept within 8,000 characters for the whole block, with a
note when earlier items were dropped. Earlier turns of the same side chat are
kept verbatim.

## Entry points

The failed reply card ("Continue in another agent"), the `/handoff` slash
command, the agent picker in the composer, and the sidebar chat menu
("Hand off to").

## Limits

- Missed-turn detection is a heuristic (failed, no output), not exact
  per-turn acceptance tracking.
- Model windows are known only from earlier replies; a fresh target without
  one assumes 128,000.
- Native fork covers Claude and Codex; Pi, OpenCode and Antigravity always get
  the portable history.
- Rewinding or editing a message drops all remembered agent sessions.
