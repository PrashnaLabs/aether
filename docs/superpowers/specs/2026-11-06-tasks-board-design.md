# Tasks board design

Date: 2026-11-06. Inspired by [kanzo](https://github.com/emanuele-em/kanzo).
Research: `docs/wayfinder/research/kanzo-product.md`, `kanzo-agent-protocol.md`,
`board-integration.md`.

## Intent

Aether runs many agent chats at once (leads, children, plain project chats). There
is no single place that says what work exists, who is on it, and what needs a human.
The Tasks board is a native kanban page where each card is a unit of work. A card can
be linked to an agent chat; the chat's live status shows on the card and moves it
between columns. Humans add and drag cards; lead agents add and move cards through
the orchestration MCP.

## Goals

- One global board page, columns fixed: Backlog, Todo, Doing, Review, Done.
- Human cards: create, edit title/body, drag between columns and reorder, delete.
- Linked cards: show the chat's live status pill (Working, Waiting, Failed, Done,
  Idle) using the existing `project_page::model::card_status`.
- Run a card: start a new agent chat in the card's project with the card body as the
  prompt; card links to it and moves to Doing.
- Every child spawned through `spawn_agent` gets a linked card automatically, or links
  to an existing card when `card` is passed.
- Auto-move on linked chats: turn starts while card is in Backlog/Todo, card moves to
  Doing; turn finishes without failure while card is in Doing, card moves to Review.
  Failure leaves the card in Doing with a Failed pill. Done is human-only (kanzo rule).
- Lead tools: `list_cards`, `add_card`, `move_card`, terse plain-text replies.
- Persist in the existing `aether.db`.

## Non-goals

- Custom columns, multiple boards, tags, priority, assignee, comments, attachments.
- Filter/query language, search palette integration.
- Sync or sharing (kanzo git sync, LAN share).
- Leases or auto-revert for dead agents (Failed pill is the signal).
- Board tools for children (children have no Aether MCP tools today).

## Anti-goals

- A second source of truth for chat status. Status is always read from the `Chat`.
- Blocking the UI thread on SQLite. Writes go through the Store's async queue.

## Data

New table in `crates/agent/src/store.rs`, created next to `chats`:

```sql
CREATE TABLE IF NOT EXISTS cards (
    id INTEGER PRIMARY KEY,          -- creation ms, like chats
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    col TEXT NOT NULL DEFAULT 'todo',  -- backlog|todo|doing|review|done
    position REAL NOT NULL DEFAULT 0,  -- order inside col, ascending
    chat INTEGER,                      -- linked chat id, nullable
    project TEXT NOT NULL DEFAULT '',  -- folder path, '' = none
    updated_at INTEGER NOT NULL DEFAULT 0
)
```

`SavedCard` struct mirrors it. Store gains `load_cards()`, `save_card(SavedCard)`
(upsert, queued on the existing `Write` channel), `delete_card(id)` (queued).
Cards live in memory on `ChatApp` as `Vec<BoardCard>`, loaded at startup.
Ordering: drop between cards uses the midpoint of neighbour positions; drop on a
column appends `last + 1`. No renumbering (ponytail: f64 midpoints run out after
~50 inserts in one gap; renumber the column then).

Deleting a linked chat leaves the card; its pill disappears (chat lookup misses).

## UI

- `Page::Board` in `crates/app/src/app/mod.rs`, rendered from `render.rs`, Esc goes
  back to chat. Opened from command palette ("Tasks board"), View menu, and an
  `OpenBoard` action bound to `alt-b` (check free first; pick another if taken).
- Layout: header with title and "New card" button; five equal columns side by side,
  each with a header (name + count) and a vertical list of cards; columns scroll
  independently.
- Card: title (truncate), 2-line body preview, project folder name, status pill when
  linked (reuse `pill` and colors from `project_page/view.rs`). Click selects.
- Drag: gpui `on_drag(CardDrag{id})` on cards, `drag_over` + `on_drop` on columns and
  cards (pattern: `crates/ui/src/chat/composer.rs`).
- Selected card opens a right-side detail panel: title input, multi-line body input,
  column shown, buttons: Run with agent (unlinked only), Open chat (linked only),
  Delete. Edits save on blur/Enter.
- New card: creates a card in Todo with the current project, selects it, focuses
  title. Empty title on blur deletes it.

## Orchestration

In `crates/agent/src/orchestrator/mod.rs::tool_specs` add:

- `list_cards {}` returns one line per non-done card:
  `#<id> [<col>] <title> (agent <chat>)`.
- `add_card {title, body?, col?}` returns `Created #<id> in <col>: <title>`.
- `move_card {id, col}` returns `Moved #<id> <old> -> <new>: <title>`. Leads cannot
  move to `done`.
- `spawn_agent` gains optional `card: integer`. Given, links that card; absent, adds a
  card titled with the child title, body = task, project = lead folder. Either way
  the card moves to Doing and `card` id is included in the reply.

Dispatch in `crates/app/src/app/orchestrate.rs::orchestrate`. Replies are plain
strings (`Value::String`), matching kanzo's terse style.

## Hooks into chat lifecycle

- Turn start: where a run begins (`submit_as` path in `chat/send.rs`), call
  `board_on_turn(chat_id, started)`.
- Turn end: `finish_run` in `chat/send.rs`, call `board_on_turn(chat_id, finished,
  failed)`.
One `ChatApp` method applies the auto-move rules and persists changed cards.

## Testing

Unit tests next to the code, no new frameworks:
- Store: open temp db, save, load, delete, migration on a db without `cards`.
- Pure board logic in `crates/app/src/board/model.rs`: position midpoint, auto-move
  rules, list_cards formatting, move_card done refusal.
Manual: run app, create/drag/run a card, spawn child from a lead and watch its card.

## Calls made without the user

User said "you decide" to all questions; every choice above is the builder's call.
Notable ones: global board (not per project), fixed columns, Done human-only, child
spawns auto-create cards, no comments/tags, `alt-b` binding.
