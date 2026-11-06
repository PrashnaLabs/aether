# VS Code-Style Split Terminal Workspace Design

## 1. Intent & Why

Currently, Aether's terminal (`crates/app/src/terminal/`) supports only a single shell per project chat (and a single global shell on `Page::Terminal`). Developers frequently run multiple processes simultaneously—such as a dev server (`cargo run`), a test watcher (`cargo test --watch`), and an interactive shell for `git` or ad-hoc commands—and need to view them side by side or stacked vertically, organize them into terminal groups, and search or manage panes just like in VS Code's integrated terminal.

This project upgrades Aether's terminal into a **VS Code-style multi-group, split-pane terminal workspace** shared by both the docked project terminal drawer (`Page::Chat`) and the full-page sidebar terminal view (`Page::Terminal`).

---

## 2. Goals, Non-Goals, and Anti-Goals

### Goals
- **Unified Terminal Workspace (`TerminalWorkspace`)**: Both the bottom project drawer (per chat/worktree folder) and the full-page `Page::Terminal` (global workspace folder) use the same multi-group, split-pane workspace controller.
- **Terminal Groups (Top Tab Bar + Right Session Tree)**:
  - A workspace contains one or more **Terminal Groups** (`TerminalGroup`), each holding a binary split tree of **Terminal Panes** (`TerminalPane`).
  - **Top Header Group Tabs**: Displays horizontal tabs for each group (`[ ▣ zsh (3) ✕ ] [ ▣ server ✕ ] [ + ]`) on the left side of the terminal header bar for instant group switching, creation, and closing.
  - **Collapsible Right-Hand Session Tree Sidebar**: Shows all terminal groups and their nested split panes using tree connectors (`┌`, `├`, `└` for multi-pane groups, `▣` for single-pane groups), active pane indicator (`●` vs `○`), live foreground process / custom name, and inline icon-only hover actions.
- **Recursive Horizontal & Vertical Splits**:
  - Any pane can be split horizontally (**Split Right**, `Axis::Horizontal`, `Cmd+\` on macOS / `Ctrl+Shift+5` on Linux/Windows) or vertically (**Split Down**, `Axis::Vertical`, `Cmd+Shift+\`).
  - Splits form a recursive binary tree (`SplitNode::Leaf(PaneId)` / `SplitNode::Split { id, axis, ratio, first, second }`) where `ratio: f32` defaults to `0.5` (clamped to `0.15..=0.85`).
  - Draggable resize divider between split children; double-clicking a split divider resets its `ratio` to `0.5`.
- **Icon-Only Buttons with Hover Tooltips (No Text Labels)**:
  - All toolbar buttons, pane subheader buttons, and right-sidebar row action buttons are rendered as compact icon-only chips (`icon_chip` / `Icon`) with descriptive hover tooltips and keyboard shortcut hints—never text labels.
  - **No Refresh/Restart button** in the main toolbar or pane subheaders (redundant when users can freely create, split, and kill panes; exited shells still allow pressing `Enter` in the exit banner to relaunch).
- **In-Terminal Scrollback Find (`Cmd+F`)**:
  - Pressing `Cmd+F` (or clicking the Find icon in the toolbar) opens a compact floating find overlay in the top-right corner of the active pane.
  - Searches case-insensitively across the active pane's scrollback + visible grid lines, scrolls the matching line into view, displays `current/total` match count, and navigates matches with `Enter` / `Shift+Enter` or Up/Down icon buttons (`Esc` closes find and returns focus to the terminal pane).
- **Drawer Maximize / Restore**:
  - In the docked project terminal drawer, an icon-only Maximize/Restore toggle (`IconName::Maximize` / `IconName::Minimize`) expands the drawer to maximum height (`75%` of viewport) and restores it back to the user's saved `terminal_height`.

### Non-Goals
- Persisting live PTY processes or scrollback buffers across application restarts (shells are spawned lazily when opened, and cleaned up on app exit).
- Drag-and-drop reordering of panes between groups (panes are created/split/closed via toolbar, shortcuts, or tree buttons).
- Multiplexing over remote SSH sessions (local login shells in the project/global folder only).

### Anti-Goals
- Text labels on terminal toolbar, pane header, or sidebar action buttons—every action button must be icon-only with a tooltip.
- Keeping a Refresh/Restart (`↻`) icon button in the toolbar or pane headers.
- Breaking single-pane ergonomics: when a group has only 1 pane, the UI remains clean and uncluttered.

---

## 3. Architecture & Data Model

### 3.1 Module Layout (`crates/app/src/terminal/`)
- `mod.rs`: Public actions, keybindings, `ChatApp` integration (`terminals: HashMap<i64, WorkspaceDrawer>`, `global_terminal: Option<WorkspaceDrawer>`), drawer height & maximize state.
- `workspace.rs` (new): `TerminalWorkspace` GPUI entity managing `Vec<TerminalGroup>`, `active_group: GroupId`, `SplitNode` recursive layout tree, split drag resizing, right-sidebar toggle & inline rename input, top group tabs, and icon-only header toolbar.
- `view.rs`: `TerminalPane` (refactored from `TerminalView`), representing a single PTY session + Alacritty grid canvas + floating Find bar + compact pane header.
- `session.rs`, `grid.rs`, `keys.rs`, `palette.rs`, `boxes.rs`: Preserved and extended with scrollback text search helper in `grid.rs`.

### 3.2 Core Types (`workspace.rs`)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GroupId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PaneId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SplitId(pub u64);

#[derive(Clone, Debug)]
pub(crate) enum SplitNode {
    Leaf(PaneId),
    Split {
        id: SplitId,
        axis: Axis, // Axis::Horizontal = left/right (Split Right), Axis::Vertical = top/bottom (Split Down)
        ratio: f32, // 0.15..=0.85, default 0.5
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

pub(crate) struct TerminalGroup {
    pub id: GroupId,
    pub custom_title: Option<SharedString>,
    pub root: SplitNode,
    pub active_pane: PaneId,
}
```

### 3.3 Split Tree Operations
- **Create Group (`new_group`)**: Allocates a new `TerminalPane` entity in `self.folder`, wraps it in `SplitNode::Leaf(pane_id)`, appends a new `TerminalGroup`, sets `active_group` to it, and focuses the new pane.
- **Split Active Pane (`split_pane(target: PaneId, axis: Axis)`)**:
  - Spawns a new `TerminalPane` entity in `self.folder`.
  - Replaces `SplitNode::Leaf(target)` in the group's tree with:
    ```rust
    SplitNode::Split {
        id: next_split_id,
        axis,
        ratio: 0.5,
        first: Box::new(SplitNode::Leaf(target)),
        second: Box::new(SplitNode::Leaf(new_pane_id)),
    }
    ```
  - Updates `group.active_pane = new_pane_id` and focuses the new pane.
- **Close / Kill Pane (`close_pane(pane_id: PaneId)`)**:
  - Calls `shutdown()` on the removed `TerminalPane` entity.
  - Removes `SplitNode::Leaf(pane_id)` from its group's `SplitNode` tree, promoting its sibling node into the parent slot.
  - If the closed pane was `group.active_pane`, focuses the nearest remaining leaf in that group (first leaf of the promoted sibling).
  - If the group has no panes left:
    - Removes the `TerminalGroup`.
    - If other groups remain in the workspace, activates an adjacent group and focuses its `active_pane`.
    - If no groups remain in the workspace, spawns a fresh default group (left dormant/closed) and emits `TerminalEvent::Hide` so closing the last terminal hides the drawer/page just like VS Code.
- **Close Group (`close_group(group_id: GroupId)`)**:
  - Shuts down all panes in `group.root.leaves()` and removes the group; follows the same fallback rule when the last group is closed.
- **Split Ratio Dragging**:
  - Each `SplitNode::Split` renders a `4px` interactive divider between `first` and `second` (`cursor_col_resize` for `Axis::Horizontal`, `cursor_row_resize` for `Axis::Vertical`).
  - Dragging updates `ratio = ((mouse_pos - container_origin) / container_size).clamp(0.15, 0.85)`.
  - Double-clicking the divider resets `ratio = 0.5`.

---

## 4. UI & Interaction Specification

### 4.1 Top Header Bar (`HEADER_HEIGHT = 34px`)
- **Left Section — Group Tabs Strip**:
  - Renders each `TerminalGroup` as a compact tab:
    - Terminal icon (`IconName::SquareTerminal`, `12px`).
    - Group label: `group.custom_title`, else active pane's custom title / OSC title / foreground job / shell name (`zsh`).
    - When `group.pane_count() > 1`, shows a pill badge with the pane count (e.g., `3`) and tooltip `"3 split panes"`.
    - Icon-only Close Group button (`IconName::Close`, `11px`, tooltip `"Close group"`).
  - Immediately after the tabs: an icon-only `+` button (`IconName::Plus`, tooltip `"New terminal"` with `NewTerminal` action).
- **Right Section — Icon-Only Action Toolbar (No Text Labels)**:
  1. **Split Right**: `IconName::PanelRight`, tooltip `"Split terminal right (⌘\\)"`
  2. **Split Down**: `IconName::PanelBottom`, tooltip `"Split terminal down (⇧⌘\\)"`
  3. **Find**: `IconName::Search`, tooltip `"Find in terminal (⌘F)"` (toggled active state when find bar is open)
  4. **Clear**: `IconName::Delete`, tooltip `"Clear terminal (⌘K)"`
  5. **Kill Active Pane**: `IconName::Trash`, tooltip `"Kill active terminal pane"`
  6. *Vertical separator (`1px` border)*
  7. **Toggle Session Tree Sidebar**: `IconName::PanelRightOpen` / `IconName::PanelRightClose`, tooltip `"Toggle terminal groups sidebar"` (on by default, width `190px`)
  8. **Maximize / Restore Drawer** *(shown only when `!full_page`)*: `IconName::Maximize` / `IconName::Minimize`, tooltip `"Maximize terminal size"` / `"Restore terminal size"`
  9. **Hide Terminal**: `IconName::Close`, tooltip `"Hide terminal (⌃`)"`

### 4.2 Split Canvas & Pane Subheaders
- When a group has **multiple panes (`pane_count() > 1`)**, each pane renders a compact `24px` subheader above its terminal grid:
  - Left: active focus dot (`●` in accent color when focused, `○` muted when unfocused), pane label (`custom_name` or `shell · job_or_title`), and folder basename.
  - Right (icon-only buttons with tooltips):
    - Split Right (`IconName::PanelRight`, tooltip `"Split right"`)
    - Split Down (`IconName::PanelBottom`, tooltip `"Split down"`)
    - Close Pane (`IconName::Close`, tooltip `"Close pane"`)
  - Focused pane has a `1px` accent border highlight (`theme.ring` / `#cba6f7`) so the user always sees which split receives keyboard input.
- When a group has **only 1 pane (`pane_count() == 1`)**, the redundant pane subheader is omitted to maximize vertical terminal rows (since the top group tab and main toolbar already display the title and actions).

### 4.3 Right-Hand Group & Session Tree Sidebar (`width = 190px`)
- Collapsible via the toolbar toggle button (`sidebar_open: bool`, default `true`).
- Lists every `TerminalGroup` in order:
  - **Single-pane group**: Single row with `IconName::SquareTerminal`, pane label (plus foreground job if running), and hover/right icon-only buttons:
    - Rename (`IconName::Pencil`, tooltip `"Rename terminal"`)
    - Split Right (`IconName::PanelRight`, tooltip `"Split right"`)
    - Split Down (`IconName::PanelBottom`, tooltip `"Split down"`)
    - Kill (`IconName::Trash`, tooltip `"Kill terminal"`)
  - **Multi-pane split group**: Renders each leaf pane in pre-order traversal with box-drawing tree prefixes:
    - First pane: `┌`
    - Middle panes: `├`
    - Last pane: `└`
    - Clicking any row selects that group and focuses that specific pane.
    - Hovering/viewing a row shows the same icon-only actions (Rename, Split Right, Split Down, Kill Pane).
- **Inline Rename**: Clicking the Rename (`IconName::Pencil`) icon or double-clicking a row/tab turns the label into a compact inline text input (`Enter` commits custom title, `Esc` cancels).

### 4.4 Floating Scrollback Find Overlay (`Cmd+F`)
- Scoped to the active `TerminalPane` (`find_open: bool`, `find_query: String`, `find_matches: Vec<(i32, usize, usize)>`, `find_index: usize`).
- Rendered as an absolute-positioned floating bar at `top: 8px, right: 12px` inside the active pane:
  - Search input field (`gpui_kit::component::input::Input` or keyboard-captured query box when focused) + match counter (`1/4` or `0/0`).
  - Icon-only Previous Match (`IconName::ArrowUp`, tooltip `"Previous match (⇧↵)"`), Next Match (`IconName::ArrowDown`, tooltip `"Next match (↵)"`), and Close Find (`IconName::Close`, tooltip `"Close find (Esc)"`).
- Searches across `alacritty_terminal::Term` grid lines from `-history_size` to `screen_lines - 1`, scrolling the grid (`Scroll::Delta`) so the active match row is visible and highlighting matched columns on visible rows.

### 4.5 Keyboard Shortcuts
| Action | macOS | Linux / Windows | Context |
|---|---|---|---|
| Toggle Terminal Drawer | `ctrl-\`` | `ctrl-\`` | Global |
| Focus Terminal / Composer | `cmd-j` | `ctrl-j` | Global |
| New Terminal Group | `cmd-shift-\`` | `ctrl-shift-\`` | `Terminal` |
| Split Terminal Right (Horizontal) | `cmd-\` | `ctrl-shift-5` | `Terminal` |
| Split Terminal Down (Vertical) | `cmd-shift-\` | `ctrl-alt-\` | `Terminal` |
| Close Active Terminal Pane | `cmd-w` | `ctrl-shift-w` | `Terminal` |
| Focus Previous / Next Pane | `cmd-alt-left` / `cmd-alt-right` | `alt-left` / `alt-right` | `Terminal` |
| Focus Previous / Next Group | `cmd-shift-[` / `cmd-shift-]` | `ctrl-pageup` / `ctrl-pagedown` | `Terminal` |
| Find in Terminal | `cmd-f` | `ctrl-shift-f` | `Terminal` |
| Clear Active Pane | `cmd-k` | `ctrl-l` | `Terminal` |

---

## 5. What's Left to the Builder
- Exact pixel spacing and subtle background shade steps using `resolve_terminal_palette` so tabs, pane headers, split dividers, and the right session tree blend naturally with every `TerminalTheme`.
- Internal ID generation (`AtomicU64` or sequential counter on `TerminalWorkspace`) for `GroupId`, `PaneId`, and `SplitId`.
- Unit tests covering split tree insertion, sibling promotion on pane close, group fallback on last pane close, scrollback search matching, and maximize/restore height toggling.
