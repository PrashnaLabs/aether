# Chat layouts

Three modes for agent chat. Pick one in **Settings > Appearance > Chat layout**,
the command palette (`Chat layout: Tabbed/Sidebar/Split`), or cycle with
`CycleChatLayout` (no default key).

- **Tabbed** — current mode. Chat lives in top tabs.
- **Sidebar** — chat center, right panel (Agent, Explorer, Terminal, Browser)
  docked beside it. Panel can collapse.
- **Split** — like Sidebar, but the right panel is always docked and cannot
  collapse.

## Tiling interaction

Session panes (`TileLayout`: Manual, Stack, Columns, Grid, Monocle) tile in all
three modes. In Sidebar and Split, tiled panes sit in the chat area beside the
right panel; the panel and panes coexist without special handling.

Monocle shows one pane at a time with the rest behind tabs. Hoisted Monocle
tabs still appear in Tabbed and Sidebar. In Split, Monocle is disabled
(`chat_layout_allows_monocle` returns false): panes stay tiled and tabs render
inside the workspace instead of being hoisted. Panel state is preserved when
Monocle hides it.
