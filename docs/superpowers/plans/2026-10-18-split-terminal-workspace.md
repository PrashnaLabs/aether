# Split Terminal Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade Aether's docked project terminal drawer and full-page sidebar terminal view into a unified VS Code-style multi-group, split-pane terminal workspace with top group tabs, a collapsible right-hand session tree, recursive horizontal/vertical splits, in-terminal scrollback search (`Cmd+F`), drawer maximize/restore, and pure icon-only action buttons with tooltips.

**Architecture:** Extract a pure, unit-testable split-tree and group-management state machine (`SplitNode`, `TerminalGroupModel`, `WorkspaceModel`) in `crates/app/src/terminal/workspace.rs`, backed by per-pane `TerminalView` (aliased/used as pane entity) instances in `crates/app/src/terminal/view.rs`. Extend `crates/app/src/terminal/grid.rs` with case-insensitive scrollback + viewport text search and match highlighting, and wire `TerminalWorkspace` into `ChatApp` (`crates/app/src/terminal/mod.rs`) for both per-chat project drawers and the global `Page::Terminal` view.

**Tech Stack:** Rust 2024, `gpui-kit` `0.7.3`, `alacritty_terminal` `0.26`, `aether-core`, `aether-ui`.

**Spec:** `docs/superpowers/specs/2026-10-18-split-terminal-workspace-design.md`

## Global Constraints

- All toolbar buttons, pane subheader buttons, and right-sidebar row action buttons must be icon-only with descriptive hover tooltips—never text labels.
- Do not include a Refresh/Restart (`↻` / `IconName::RotateCw`) icon button in the main toolbar or pane subheaders (exited shells still allow pressing `Enter` in the exit banner to relaunch).
- Split ratios (`ratio: f32`) default to `0.5` and must be clamped to `0.15..=0.85`; double-clicking a split divider resets its ratio to `0.5`.
- When a group has only 1 pane (`pane_count() == 1`), omit the redundant per-pane `24px` subheader so single-pane ergonomics stay clean.
- Both the docked project terminal drawer (`Page::Chat`) and the full-page `Page::Terminal` view must use `TerminalWorkspace`.

## Review Focus

- Closing the currently active middle pane in a 3-way nested split tree must promote its sibling cleanly and move `active_pane` to the first leaf of the promoted subtree without orphaning or duplicating panes.
- Closing the last pane in the last remaining group of a workspace must spawn a fresh dormant default group and return a hide signal (`CloseOutcome::HideWorkspace`) so the workspace never renders an empty/panicking group list.
- Searching scrollback with an empty or whitespace-only query or a query with zero matches must return an empty match list and `0/0` counter without out-of-bounds indexing when cycling next/prev match.
- Out-of-range split drag ratios (`< 0.0`, `> 1.0`, or `NaN`) must clamp safely to `0.15..=0.85`.
- Tree connector prefixes in the right-hand session sidebar must render `▣` (single leaf) when a group has 1 pane, and `┌` (first), `├` (middle), `└` (last) in pre-order leaf traversal when a group has 2+ split panes.

---

### Task 1: Split Tree & Group State Machine (`crates/app/src/terminal/workspace.rs`)

**Files:**
- Create: `crates/app/src/terminal/workspace.rs`
- Modify: `crates/app/src/terminal/mod.rs`

**Interfaces:**
- Consumes: `gpui_kit::{Axis, SharedString}`
- Produces:
  - `GroupId(pub u64)`, `PaneId(pub u64)`, `SplitId(pub u64)`
  - `SplitNode::{Leaf(PaneId), Split { id: SplitId, axis: Axis, ratio: f32, first: Box<SplitNode>, second: Box<SplitNode> }}`
  - `SplitNode::leaves(&self) -> Vec<PaneId>`
  - `SplitNode::split_leaf(&mut self, target: PaneId, new_pane: PaneId, split_id: SplitId, axis: Axis) -> bool`
  - `SplitNode::remove_leaf(&mut self, target: PaneId) -> RemoveResult` (`Removed { promoted_first_leaf: PaneId }`, `RemovedLast`, `NotFound`)
  - `SplitNode::set_ratio(&mut self, id: SplitId, ratio: f32) -> bool` (clamps `ratio` to `0.15..=0.85`, treats non-finite as `0.5`)
  - `TreeConnector::{Single, First, Middle, Last}` and `SplitNode::tree_rows(&self) -> Vec<(PaneId, TreeConnector)>`
  - `WorkspaceModel` managing `groups: Vec<GroupModel>`, `active_group: GroupId`, `sidebar_open: bool`, `maximized: bool`, and methods `new_group`, `split_active`, `close_pane`, `close_group`, `focus_next_pane`, `focus_prev_pane`, `focus_next_group`, `focus_prev_group`.

- [ ] **Step 1: Write the failing unit tests for `SplitNode` and `WorkspaceModel` in `crates/app/src/terminal/workspace.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::Axis;

    #[test]
    fn split_tree_inserts_horizontal_and_vertical_splits_and_computes_connectors() {
        let mut model = WorkspaceModel::new();
        let g1 = model.active_group_id();
        let p1 = model.active_pane_id();
        assert_eq!(
            model.active_group().root.tree_rows(),
            vec![(p1, TreeConnector::Single)]
        );

        let p2 = model.split_pane(p1, Axis::Horizontal).unwrap();
        let p3 = model.split_pane(p2, Axis::Vertical).unwrap();
        assert_eq!(model.active_pane_id(), p3);
        assert_eq!(
            model.group(g1).unwrap().root.tree_rows(),
            vec![
                (p1, TreeConnector::First),
                (p2, TreeConnector::Middle),
                (p3, TreeConnector::Last),
            ]
        );
    }

    #[test]
    fn closing_middle_pane_in_nested_split_promotes_sibling_and_updates_active_pane() {
        let mut model = WorkspaceModel::new();
        let p1 = model.active_pane_id();
        let p2 = model.split_pane(p1, Axis::Horizontal).unwrap();
        let p3 = model.split_pane(p2, Axis::Vertical).unwrap();

        // Focus middle pane p2 and close it
        model.focus_pane(p2);
        let outcome = model.close_pane(p2);
        assert_eq!(outcome, CloseOutcome::Focused(p3));
        assert_eq!(model.active_pane_id(), p3);
        assert_eq!(model.active_group().root.leaves(), vec![p1, p3]);
    }

    #[test]
    fn closing_last_pane_in_last_group_resets_default_group_and_requests_hide() {
        let mut model = WorkspaceModel::new();
        let p1 = model.active_pane_id();
        let outcome = model.close_pane(p1);
        assert!(matches!(outcome, CloseOutcome::HideWorkspace { replacement_pane: _ }));
        assert_eq!(model.groups.len(), 1);
        assert_eq!(model.active_group().root.leaves().len(), 1);
    }

    #[test]
    fn split_ratio_clamps_out_of_range_and_non_finite_values() {
        let mut model = WorkspaceModel::new();
        let p1 = model.active_pane_id();
        let _p2 = model.split_pane(p1, Axis::Horizontal).unwrap();
        let split_id = match &model.active_group().root {
            SplitNode::Split { id, .. } => *id,
            _ => panic!("expected split"),
        };

        model.set_split_ratio(split_id, 0.02);
        assert_eq!(model.active_group().root.ratio_of(split_id), Some(0.15));

        model.set_split_ratio(split_id, 0.99);
        assert_eq!(model.active_group().root.ratio_of(split_id), Some(0.85));

        model.set_split_ratio(split_id, f32::NAN);
        assert_eq!(model.active_group().root.ratio_of(split_id), Some(0.5));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether terminal::workspace::tests`
Expected: FAIL (module/types not yet defined)

- [ ] **Step 3: Implement `SplitNode`, `GroupModel`, `WorkspaceModel`, and `CloseOutcome` in `crates/app/src/terminal/workspace.rs` and declare `pub(crate) mod workspace;` in `crates/app/src/terminal/mod.rs`**

Implement the pure tree and group model operations so every split, close, ratio clamp, pane cycle, and group cycle is deterministic and unit-tested.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether terminal::workspace::tests`
Expected: PASS (4 passed; 0 failed)

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/terminal/workspace.rs crates/app/src/terminal/mod.rs
git commit -m "feat(terminal): add split tree and group workspace state machine"
```

---

### Task 2: Scrollback & Viewport Search + Match Highlighting (`crates/app/src/terminal/grid.rs`)

**Files:**
- Modify: `crates/app/src/terminal/grid.rs`

**Interfaces:**
- Consumes: `alacritty_terminal::Term<T>`, `alacritty_terminal::index::{Column, Line}`
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub(crate) struct SearchMatch { pub line: i32, pub start_col: usize, pub end_col: usize }`
  - `pub(crate) fn search_term<T: EventListener>(term: &Term<T>, query: &str) -> Vec<SearchMatch>`
  - `pub(crate) fn search_lines<'a>(lines: impl IntoIterator<Item = (i32, &'a str)>, query: &str) -> Vec<SearchMatch>`
  - `pub(crate) fn display_offset_for_line(line: i32, screen_lines: usize, history_size: usize, current_offset: usize) -> usize`

- [ ] **Step 1: Write the failing unit tests in `crates/app/src/terminal/grid.rs`**

```rust
#[test]
fn search_lines_finds_case_insensitive_matches_and_ignores_empty_queries() {
    let lines = vec![
        (-2, "Compiling aether v0.1.0"),
        (-1, "Finished `dev` profile in 1.42s"),
        (0, "running 3 tests ... finished ok"),
    ];
    assert!(search_lines(lines.clone(), "").is_empty());
    assert!(search_lines(lines.clone(), "   ").is_empty());
    assert!(search_lines(lines.clone(), "nonexistent").is_empty());

    let hits = search_lines(lines, "FINISHED");
    assert_eq!(
        hits,
        vec![
            SearchMatch { line: -1, start_col: 0, end_col: 8 },
            SearchMatch { line: 0, start_col: 20, end_col: 28 },
        ]
    );
}

#[test]
fn display_offset_for_line_scrolls_history_line_into_view() {
    // 24 screen lines, 100 lines of scrollback history
    assert_eq!(display_offset_for_line(5, 24, 100, 0), 0);
    // Line -30 is 30 lines above viewport top (0); centers or brings into viewport clamped to history_size
    let offset = display_offset_for_line(-30, 24, 100, 0);
    assert!(offset >= 30 && offset <= 100);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether terminal::grid::tests::search_lines`
Expected: FAIL (`search_lines` and `SearchMatch` not found)

- [ ] **Step 3: Implement `SearchMatch`, `search_lines`, `search_term`, and `display_offset_for_line` in `crates/app/src/terminal/grid.rs`, plus highlight rendering for search matches in `snapshot`**

Update `snapshot` in `crates/app/src/terminal/grid.rs` (or an overload/parameter `active_matches: &[(i32, usize, usize, bool)]`) so matched column ranges on visible lines get a distinct search highlight background (`Rgb { r: 94, g: 80, b: 36 }` for matches, `Rgb { r: 166, g: 128, b: 38 }` for the active match).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether terminal::grid::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/terminal/grid.rs
git commit -m "feat(terminal): add scrollback text search and match highlighting"
```

---

### Task 3: Refactor `TerminalView` for Multi-Pane Hosting, Compact Subheader, and Floating Find Bar (`crates/app/src/terminal/view.rs`)

**Files:**
- Modify: `crates/app/src/terminal/view.rs`

**Interfaces:**
- Consumes: `SearchMatch`, `search_term`, `display_offset_for_line` from `grid.rs`; `PaneId` from `workspace.rs`
- Produces:
  - `TerminalPaneEvent::{Focused(PaneId), SplitRight(PaneId), SplitDown(PaneId), ClosePane(PaneId), TitleChanged(PaneId)}`
  - `TerminalView` fields & methods:
    - `pub(crate) pane_id: PaneId`
    - `pub(crate) custom_title: Option<SharedString>`
    - `pub(crate) show_pane_header: bool` (true when group has > 1 pane)
    - `pub(crate) is_active_pane: bool`
    - `pub(crate) find_open: bool`, `find_query: String`, `find_matches: Vec<SearchMatch>`, `find_index: usize`
    - `pub(crate) toggle_find(&mut self, window: &mut Window, cx: &mut Context<Self>)`
    - `pub(crate) find_next(&mut self, cx: &mut Context<Self>)`
    - `pub(crate) find_prev(&mut self, cx: &mut Context<Self>)`
    - `pub(crate) display_label(&self) -> SharedString` (returns `custom_title`, else `title`, else running job or `shell`)
  - Removes the outer drawer header and drawer resize handle from `TerminalView::render` (moved to `TerminalWorkspace` in Task 4) and replaces it with the optional `24px` compact pane subheader (icon-only `Split Right`, `Split Down`, `Close Pane` with tooltips—no Refresh button) and floating `Cmd+F` Find bar.

- [ ] **Step 1: Write the failing unit tests in `crates/app/src/terminal/view.rs`**

```rust
#[test]
fn pane_display_label_prefers_custom_title_then_osc_title_then_shell() {
    assert_eq!(
        resolve_pane_label(Some("api-server"), Some("zsh: cargo"), Some("cargo"), "zsh"),
        "api-server"
    );
    assert_eq!(
        resolve_pane_label(None, None, Some("cargo"), "zsh"),
        "zsh (cargo)"
    );
    assert_eq!(
        resolve_pane_label(None, Some("nvim"), None, "zsh"),
        "nvim"
    );
    assert_eq!(
        resolve_pane_label(None, None, None, "zsh"),
        "zsh"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether terminal::view::tests::pane_display_label`
Expected: FAIL (`resolve_pane_label` not defined)

- [ ] **Step 3: Implement `TerminalPaneEvent`, compact pane subheader (icon-only, no refresh button), floating Find overlay, and `resolve_pane_label` in `crates/app/src/terminal/view.rs`**

Ensure clicking anywhere inside a pane emits `TerminalPaneEvent::Focused(self.pane_id)` so `TerminalWorkspace` tracks the active pane, and when `find_open` is true, typing updates `find_query`, `Enter` advances to the next match, `Shift+Enter` moves to the previous match, and `Escape` closes the Find overlay and refocuses the terminal grid.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether terminal::view::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/terminal/view.rs
git commit -m "feat(terminal): add pane subheader, floating find bar, and pane events"
```

---

### Task 4: Build `TerminalWorkspace` UI & Wire into `ChatApp` (`crates/app/src/terminal/workspace.rs`, `crates/app/src/terminal/mod.rs`)

**Files:**
- Modify: `crates/app/src/terminal/workspace.rs`
- Modify: `crates/app/src/terminal/mod.rs`

**Interfaces:**
- Consumes: `WorkspaceModel`, `SplitNode`, `TreeConnector` from Task 1; `TerminalView`, `TerminalPaneEvent` from Task 3
- Produces:
  - `pub(crate) struct TerminalWorkspace` (`Render` + `EventEmitter<TerminalEvent>`)
  - New GPUI actions in `crates/app/src/terminal/mod.rs`:
    `NewTerminalGroup`, `SplitTerminalRight`, `SplitTerminalDown`, `CloseTerminalPane`, `FocusNextTerminalPane`, `FocusPrevTerminalPane`, `FocusNextTerminalGroup`, `FocusPrevTerminalGroup`, `ToggleTerminalFind`, `ToggleTerminalSidebar`, `ToggleTerminalMaximize`
  - Top Header Bar (`34px`) with:
    - Group tab strip (`[ ▣ label (count) ✕ ] ... [ + ]`)
    - Pure icon-only right toolbar with tooltips (no text labels, no refresh button):
      `Split Right` (`IconName::PanelRight`), `Split Down` (`IconName::PanelBottom`), `Find` (`IconName::Search`), `Clear` (`IconName::Delete`), `Kill Active Pane` (`IconName::Trash`), separator, `Toggle Session Tree Sidebar` (`IconName::PanelRightOpen` / `IconName::PanelRightClose`), `Maximize / Restore` (`IconName::Maximize` / `IconName::Minimize` when `!full_page`), `Hide` (`IconName::Close`).
  - Recursive split canvas rendering `SplitNode` with `4px` draggable dividers (and double-click `0.5` reset).
  - Collapsible right-hand Group & Session Tree Sidebar (`190px` wide) with `┌` / `├` / `└` / `▣` connectors, inline rename input (`IconName::Pencil`), and icon-only row actions (`Rename`, `Split Right`, `Split Down`, `Kill`).
  - `ChatApp` integration in `crates/app/src/terminal/mod.rs`: replaces `Entity<TerminalView>` in `Drawer` with `Entity<TerminalWorkspace>` for both `terminals` (project drawer) and `global_terminal` (`Page::Terminal`), adding drawer maximize/restore support.

- [ ] **Step 1: Write the failing unit tests for drawer maximize height calculation and workspace lifecycle in `crates/app/src/terminal/mod.rs`**

```rust
#[test]
fn effective_drawer_height_uses_three_quarters_viewport_when_maximized() {
    let min = min_height(px(13.));
    assert_eq!(effective_drawer_height(260., 800., min, false), 260.);
    assert_eq!(effective_drawer_height(260., 800., min, true), 600.);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether terminal::tests::effective_drawer_height`
Expected: FAIL (`effective_drawer_height` not defined)

- [ ] **Step 3: Implement `TerminalWorkspace` in `crates/app/src/terminal/workspace.rs` and update `crates/app/src/terminal/mod.rs` to host `TerminalWorkspace` in `ChatApp`**

Wire all actions, shortcuts (`cmd-\`, `cmd-shift-\`, `cmd-shift-\``, `cmd-w`, `cmd-f`, `cmd-alt-left/right`, `cmd-shift-[/]`), top group tabs, recursive split dividers, right-hand group tree sidebar with inline rename, and `effective_drawer_height`.

- [ ] **Step 4: Run full workspace test suite to verify everything compiles and passes**

Run: `cargo test --workspace`
Expected: PASS (0 failures across all crates)

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/terminal/workspace.rs crates/app/src/terminal/mod.rs
git commit -m "feat(terminal): implement VS Code-style split terminal workspace with tabs and group tree"
```
