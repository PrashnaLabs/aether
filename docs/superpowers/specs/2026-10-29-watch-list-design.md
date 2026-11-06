# Watch-list

Status: approved.

## Intent

Quick access to whatever the user is engaged with right now. A few running
agents and a terminal or two are spread across projects, and getting back to
them through the sidebar or tabs takes too many clicks. The watch-list is a
short, user-curated set of those things, one click away from any page, where
the user can check on each one and act on it without leaving the current view.

## What the user decided

- **One global FAB.** A round button in the bottom-right corner of the main
  window, on every page, with a badge showing how many items are watched.
- **What can be watched:** agent chats, direct chats, and an agent chat's
  terminal.
- **List.** Clicking the FAB opens a list above it. Each row has a kind icon
  (provider logo for agent chats, a direct-chat mark, `>_` for terminals), the
  name, a status line (running verb and timer, "Needs approval", "replied"; the
  last command for terminals), and a coloured state dot.
- **Peek.** Clicking a row opens a larger floating card above the FAB. The page
  underneath stays put.
  - Chat peek: recent transcript, live status, and a working composer, so the
    user can steer, reply, approve or answer right there.
  - Terminal peek: the live shell, interactive.
  - Both have **Open ↗** to jump to the real chat or terminal, and **✕**.
- **Adding and removing.** "Watch" / "Unwatch" in the right-click menus of
  sidebar chat rows, chat tabs and terminal tabs, plus an eye toggle in the chat
  header. Each list row also has an ✕.
- **Lifetime.** The list survives restarts. Finished items stay until removed.
  An item drops off by itself when its chat is archived or deleted.
- **Web views.** The Browser tab and the Hacker News reader are native web views
  that paint above everything. There, the web view leaves a thin strip at its
  bottom so the FAB keeps its corner, and opening the list or a peek hides the
  web page for the moment, the way menus already do (`hide_for_menu`).
- The look follows the approved mockup
  (`.superpowers/brainstorm/*/content/watchlist-flow.html`).

## Goals

- Reach any watched item in one click from any page.
- Act on a watched chat or terminal from the peek, without navigating.
- See at a glance which watched items are running, waiting on the user, or done.

## Non-goals

- Watching the global terminal, files, notes, browser tabs or anything else.
- More than one peek open at once.
- A FAB in detached session windows.
- Keyboard shortcuts, reordering, or grouping in the list.
- Notifications driven by the watch-list; existing notifications stay as they are.

## Anti-goals

- The FAB or peek covering controls the user needs, such as the composer's send
  button or the status bar.
- A peek that moves, resizes or refocuses the page underneath it.
- Drawing the same chat or terminal twice at once, which breaks the original
  (shared composer and scroller state; a terminal resizes its PTY to wherever
  it's painted).

## Constraints from the codebase

- A chat is identified by `chat.id: i64`. Direct chats are chats where
  `!chat.is_agent()`. A chat has a terminal only when `is_agent_chat` holds.
- Each agent chat has one terminal drawer (`ChatApp.terminals`, keyed by chat
  id) holding several tabs. Shells and tab ids don't survive a restart.
- A chat's transcript and composer can be drawn outside the active chat via
  `ensure_session_ui` + `in_session`, as tiled session panes already do.
- A terminal view can only be painted in one place. The drawer loops that call
  `set_shown(false)` on every non-active drawer each frame would stop a peeked
  terminal from polling.
- `session_is_watched` already means "the user is looking at this chat" for
  notifications. The new feature needs a name that doesn't collide.

## Calls made by the designer (not asked of the user)

- **Terminal identity.** A watched terminal is a chat's whole terminal drawer,
  stored by chat id, since tabs don't survive restarts. The peek shows the
  drawer as it is, with its active tab.
- **Who can be watched.** Only chats that aren't archived. Terminal watching is
  only offered where the chat has a terminal folder. The menu item doesn't
  appear in the Archive view.
- **No double drawing.** A peeked terminal is shown only in the peek; its usual
  spot is empty while the peek is open. Clicking a watched chat that's already
  on screen (the active chat or a visible pane) focuses it instead of peeking.
- **Peek gives way.** If the peeked chat comes on screen some other way (sidebar,
  palette, notification, tab, pane, detached window), the peek closes. Clicking
  elsewhere on the page leaves the peek open.
- **Seen.** An open chat peek counts as looking at the chat, the same as a pane:
  `session_is_watched` includes it, so no desktop notification or Done mark for a
  reply the user watched arrive in the peek. Opening it clears Done/unread.
- **Esc.** Esc goes to whatever is focused in the peek (stop the agent, deny an
  approval, the shell's own Esc). It never closes the peek.
- **Storage.** A `#[serde(default)]` list in core `Settings`, saved through
  `update_settings`. Archived, deleted or missing chats are pruned on load and at
  the archive and delete call sites; terminal entries are also dropped when the
  chat's terminal is closed for good.
- **Order.** Rows appear in the order they were added.
- **Empty list.** The FAB stays visible, and the empty list says how to add items.
- **Peek replaces the list.** Picking a row swaps the list for the peek. ✕ or
  clicking the FAB closes it. **Open ↗** jumps there and closes the peek.
- **Menus.** "Watch" goes in the sidebar chat row menu (every sidebar mode and
  Direct Chats), the session pane menu, a new context menu on the side panel's
  Agent tab and the monocle tabs, and the terminal tab menu. The terminal crate
  doesn't know about chats, so its menu raises an event the app handles. The eye
  toggle goes in the chat header and the side-panel tab bar, which stands in for
  the header when the side panel fills the window.

## Left to the builder

Sizes, offsets, icons, the badge style, and the exact status wording (reuse
`ChatStatus`, the live verb and `wait_label` where possible). How far the FAB
sits from the bottom-right corner, as long as it avoids the anti-goals above.

## Verification

- Unit tests for the list model: watch, unwatch, toggle, no duplicates, prune on
  archive, delete and load, and serde round-trip with old settings files.
- Compile, then run the existing `aether`, `aether-ui` and `aether-core` tests.
- GUI check by the user: add each kind from each menu and the eye toggle; peek
  and act on a running agent, a direct chat and a terminal; Open ↗; restart and
  confirm the list survives; archive a watched chat and confirm it drops off.

## Additions (approved 2026-10-29, after the first build)

- **FAB icon.** The FAB shows the Aether app icon (the orange tile with 岚,
  `crates/app/assets/app-icons/aether.svg` or a PNG of it) instead of an eye.
  The count badge stays.
- **FAB position setting.** "Watch-list button position" with four corners:
  bottom-right (default), bottom-left, top-right, top-left. It lives in
  Settings → General → Appearance. The list and peek open from whichever corner
  the FAB is in, toward the window's centre.
- **Peek as a floating window.** The peek is a floating window inside Aether:
  drag it by its header, resize it from its edges and corners. Its size and
  position survive restarts and stay clamped inside the main window.
- **Pop out.** A pop-out button in the peek header moves the item into its own
  desktop window and closes the peek:
  - a chat goes to Aether's existing detached session window (the same path as
    "Detach session", `detach_session`), opening a pane for it first if it has
    none;
  - a terminal pops out its chat's detached session window with that chat's
    terminal tab showing, since Aether has no terminal-only desktop window.
- **Per-window side tabs (approved follow-up).** Each detached session window
  keeps its own side panel tab (Agent, Terminal, Explorer, Browser…), so
  switching tabs in one window never changes another. Popping out a watched
  terminal opens its window on the Terminal tab. The main window keeps today's
  behaviour.
