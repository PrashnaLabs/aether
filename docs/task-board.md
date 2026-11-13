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
| `list_cards` | `filter?` (same fields as `delete_cards`: `ids`, `col`, `project`, `added_by`, `search`, `label`), `bodies?` (show descriptions, indented) | One line per card: `#<id> [<column>] <title> (agent <chat id>) {labels} blocked by #<id> · <n> note(s)`, plus `[project <folder>]` for other projects. Without `col` or `ids`, Done cards are left out |
| `read_card` | `id`, `brief?` (leave out the description) | JSON: description, labels, blockers, linked chats with their state, every note including linked agents' results, and `added_by` (`user`, `lead <id>` or `agent <id>`) |
| `add_card` | `title`, `body?`, `col?` (`backlog`, `todo`, `doing`, `review`; default `todo`), `labels?`, `blocked_by?` | `Created #<id> in todo: <title>` |
| `move_card` | `id`, `col` | `Moved #<id> todo -> review: <title>` |
| `update_card` | `id`, any of `title?`, `body?` (replaces; empty clears), `append?` (adds a paragraph), `note?` (dated note signed by the caller), `col?`, `before?` (card id to sit above), `chat?` (also link this chat), `labels?` (replaces), `blocked_by?` (replaces; `[]` clears) | `Updated #<id> todo (title, description): <title>` |
| `spawn_agent` | usual arguments plus optional `card` | Reply includes `card`; `{"status": "queued", "position": n}` when every agent slot is busy |

Unknown arguments fail with `unknown argument "<name>"; expected one of: …`
instead of being ignored.

Replies are plain text to keep tokens low. With Pi the tools appear as
`aether_list_cards` and so on.

### What moves on its own

- `spawn_agent` without `card` adds a card for the child in Doing, titled after
  the child, with the task as its description.
- `spawn_agent` with `card: <id>` links that card too and moves it to Doing.
  A card can track several chats. An unknown id fails the spawn with
  `card #<id> not found`, and a card with open blockers fails with
  `card #<id> is blocked by #<ids>`.
- When every agent slot is busy, `spawn_agent` queues the spawn and starts it
  when a child finishes; the lead hears the new child's id as its next turn.
- When a linked agent finishes, its final reply is filed on the card as a note
  signed `agent <chat id>` (up to 20,000 characters).
- When a card is done or deleted, it leaves every other card's `blocked_by`.
- When a linked chat starts a turn, its card moves from Backlog or Todo to
  Doing.
- When the turn finishes cleanly, the card moves from Doing to Review. With
  several linked chats, it waits until the last one finishes.
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

- `move_card` and `update_card` only reach cards in the lead's own project;
  `list_cards` and `delete_cards` take `project: "all"` for the rest.
- Chats that are not leads only see cards in their own project. For them,
  `list_cards` ignores `project` and `read_card` won't open another project's
  card.
- Card ids are short counters (`#66`) and are never reused, even after a
  delete. Cards made before short ids were renumbered on first launch.
- Queued spawns live in memory; quitting Aether drops them.
- A lead running in a worktree records the worktree folder on the cards it
  creates, so they may be missing from **This project**; **All projects** shows
  them.
- A worktree child whose setup fails leaves its card in Doing; delete or move
  it by hand.
