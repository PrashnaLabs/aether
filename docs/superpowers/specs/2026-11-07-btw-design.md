# `/btw` side questions

Date: 2026-11-07. Research: `docs/wayfinder/research/btw-aether-design.md`,
`btw-claude-codex.md`, `btw-opencode-agy.md`. Decisions made by the lead while
the user was away; the user asked for wise defaults.

## Intent

Ask a quick question about the current chat ("by the way, which file holds the
retry logic?") without interrupting, steering or polluting the main agent's
work. Claude Code, Codex, OpenCode and Antigravity all ship `/btw`; none of
them expose it on the headless transport Aether uses in a uniform way, so v1
emulates it the same way for every provider.

## Behaviour

- Typing a whole message that starts with `/btw ` (or `/side `) and pressing
  Send asks a side question. An empty question does nothing.
- Works while the main turn is running, idle, queued or read-only locked by an
  outside agent. It never touches the main run (`cancel`, queue, steer, turn
  state) and never enters `messages`, `history()` or the store.
- The answer streams into a card above the composer: header "By the way",
  the question (one line, truncated), the answer as Markdown, a spinner while
  running, an error line on failure, and a close button. Several cards can
  stack; newest last; max 3 kept (oldest dropped). Cards live only in memory
  on the chat and disappear on restart or when closed. Closing a running card
  cancels its side run.
- `/btw` appears in the slash menu as an Aether command ("Ask a side question
  without interrupting the agent"), inserting `/btw ` like other prefill
  commands. It must win over any CLI-probed `btw`; Antigravity's
  `TERMINAL_ONLY` `btw` guard must never see it.

## How it runs (v1, every provider)

- One side run per question under a fresh agent session key
  `btw:<chat id>:<n>`, started through the existing `Agent::send` with:
  - the chat's provider, model and cwd,
  - access forced to read-only (the strictest mode each driver supports),
  - `history` = the chat's existing `history(&messages)` snapshot, capped to
    the last 40 entries,
  - text = a short preamble + the question: "Side question from the user.
    Answer briefly from the conversation so far. Do not use tools or change
    files unless reading is essential. Question: <q>".
- Stream `Delta`/`Final` text into the card; ignore tool, ask and notice
  updates except to mark failure. Approval asks are auto-denied (read-only).
- On finish, failure or close: `Agent::close_session(key)`.
- No CLI session is resumed or persisted for the side key; the side run must
  not write the main chat's `cli_session`.

## Non-goals (v1)

- Native per-provider paths (backlog cards): Claude `side_question`
  control_request on the live process, Codex `thread/fork {ephemeral:true}`,
  OpenCode `POST /api/session/:id/generate`.
- "Bring back" (insert the answer into the next prompt).
- Persisting side Q&A.

## Testing

- Pure helper `btw_question(text) -> Option<&str>`: `/btw x` → `x`,
  `/side x` → `x`, `/btw` / `/btw   ` → None, `/btwx` → None, leading
  whitespace allowed, case-sensitive.
- Pure helper for the side prompt and history cap.
- gpui test: sending `/btw hi` on a chat leaves `messages` unchanged and adds
  one side card; closing removes it.
