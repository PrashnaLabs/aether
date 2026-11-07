# Task board

A kanban board for agent work. Columns are fixed: Backlog, Todo, Doing,
Review, Done. Cards can link to an agent chat; a linked card shows the chat's
live status and moves on its own as the chat works.

Open it from the **+** tab menu (Board) in any chat layout, the board icon in
the status bar, the command palette (**Tasks board**), or **Alt+B**. The tab
shows the current project's cards with a **This project / All projects**
switch; the status bar and Alt+B open every project's cards.

## Using it from a lead

Only lead chats get board tools. Children a lead spawns have no Aether tools,
but their cards still update automatically.

1. Toggle the chat to **Lead** (composer chip or command palette) before its
   first message. Toggling an existing chat restarts its driver so the tools
   are injected. Any CLI provider can lead (Claude Code, Codex, Pi, OpenCode,
   Antigravity); see [orchestration.md](orchestration.md).
2. Ask the lead to plan, track or dispatch work on the board.

### Tools

| Tool | Arguments | Reply |
| --- | --- | --- |
| `list_cards` | none | One line per card not in Done: `#<id> [<column>] <title> (agent <chat id>)` |
| `add_card` | `title`, `body?`, `col?` (`backlog`, `todo`, `doing`, `review`; default `todo`) | `Created #<id> in todo: <title>` |
| `move_card` | `id`, `col` | `Moved #<id> todo -> review: <title>` |
| `spawn_agent` | usual arguments plus optional `card` | Reply includes `card` |

Replies are plain text to keep tokens low. With Pi the tools appear as
`aether_list_cards` and so on.

### What moves on its own

- `spawn_agent` without `card` adds a card for the child in Doing, titled after
  the child, with the task as its description.
- `spawn_agent` with `card: <id>` links that card instead and moves it to Doing.
  An unknown id fails the spawn with `card #<id> not found`.
- When a linked chat starts a turn, its card moves from Backlog or Todo to
  Doing.
- When the turn finishes cleanly, the card moves from Doing to Review.
- A failed or stopped turn leaves the card in Doing; failures show a red pill.
- Done is human-only. Leads get `only a human can move a card to done`; drag
  the card yourself after review.

### Example prompts

> Plan the auth refactor as board cards: one card per step in Todo.

> Take every Todo card and spawn a claude agent for each, passing the card id,
> in its own worktree.

> List the board and move anything stale back to Backlog.

> Spawn a codex agent to fix the flaky test and track it on the board.

## From the board itself

- **New card** or a column's **Add card** creates a card in the current
  project; type its title and description in the side panel.
- **Run with agent** starts a chat in the card's project with the title and
  description as the prompt, links it and moves the card to Doing.
- **Open chat** jumps to a linked chat. Drag cards between and within columns.

## Limits

- Board tools are not scoped to the lead's project: `list_cards` shows every
  project's cards and `move_card` can move any of them.
- Card ids are millisecond timestamps, so ids in replies are long.
- A lead running in a worktree records the worktree folder on the cards it
  creates, so they may be missing from **This project**; **All projects** shows
  them.
- A worktree child whose setup fails leaves its card in Doing; delete or move
  it by hand.
