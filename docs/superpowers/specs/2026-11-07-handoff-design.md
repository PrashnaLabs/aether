# Agent context handoff

Date: 2026-11-07. Research: `docs/wayfinder/research/handoff-aether.md`,
`handoff-other-tools.md`. Decisions made by the lead while the user was away.

## Intent

When an agent hits its usage limit, or the user wants a different agent or
model to continue, move the conversation to another agent in one step and keep
working in the same folder.

## Design

Follows T3 Code's portable handoff (budget-selected transcript, not an AI
summary) on top of Aether's existing fresh-session re-seed, because it works
for every provider and needs nothing from a rate-limited source agent.

- `ChatApp::handoff_chat(ix, provider, model)` creates a new chat beside the
  source, like `duplicate_chat` (`app/chat/create.rs`): same messages, cwd,
  project and worktree fields; `provider`/`provider_model` set to the target;
  access fitted to the target; effort cleared; `cli_session` empty; title
  `"<title> → <Provider label>"`. The new chat is selected and its composer is
  prefilled (not sent) with:
  "Continue where the previous agent left off. You're in the same folder, so
  check the working tree for changes it already made."
  The source chat is left untouched.
- Seeding: unchanged mechanism. The first send on a fresh CLI session already
  prepends the transcript via `context_prompt` (`agent/src/agent.rs`). Add a
  budget there for every fresh re-seed (handoff, duplicate, rewind): keep the
  first user message and the newest messages, newest first, until 64,000
  characters; when anything is dropped, say "(N earlier messages omitted)"
  in the preamble. The user's new message is never shortened. OpenRouter
  targets are seeded through `fork_session` with the same budgeted history.
- Entry points:
  1. Failed reply card (`FatalCard`, `ui/src/chat/notice.rs`): a "Continue in
     another agent" button next to Retry, on the last message only. It opens a
     menu of installed providers other than the current one; each item uses
     that provider's default model (`initial_model`).
  2. Slash command `/handoff`: one suggestion per installed provider
     ("Continue in Codex", …) excluding the current one; picking one hands off
     immediately (the typed command is cleared, not sent).
  3. Sidebar chat row menu: "Hand off to ▸" with the same provider list,
     next to Duplicate.
  Direct (OpenRouter) chats can hand off to CLI agents and vice versa only when
  the target has a project folder; when the source has no folder and the
  target is a CLI, use the workspace root, else show the existing "pick a
  folder" error.
- Busy source: allowed. The source keeps running; the new chat gets the
  transcript as it is now (a running reply is copied as-is).

## Non-goals (v1)

- AI-written handoff brief, delta handoffs, returning to the source's native
  session, "handed off from" database link, auto-sending.

## Testing

- Pure budget helper: keeps first user + newest within the budget, reports the
  omitted count, never splits a message, empty input → empty.
- `handoff_chat`: new chat has target provider/model, empty `cli_session`,
  same messages and cwd/worktree fields, title arrow, is selected, composer
  prefilled; source unchanged.
- Slash suggestions list installed providers except the current one.
