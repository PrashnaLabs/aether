# Aether Sidebar Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform the Aether desktop sidebar into a high-density, accessible, state-of-the-art developer experience with 1-click agent unblocking, zero redundant project tags under folders, semantic git diff metrics, and refined macOS/Linear/Zed caliber aesthetics.

**Architecture:** Refactor `crates/ui/src/chat/sidebar/` and `crates/app/src/app/sidebar.rs` to replace heavy card outlines with borderless quiet rows, compact 2-line layouts for nested chats, precise status pills/dots with inline quick-actions for blocked agents, and a balanced bottom dock.

**Tech Stack:** Rust 2024, GPUI (`gpui-kit`), Lucide icons, WCAG 2.2 AA accessible contrast tokens.

**Spec:** [docs/superpowers/specs/2026-10-23-sidebar-redesign.md](../specs/2026-10-23-sidebar-redesign.md)

## Global Constraints
- Target crates: `crates/ui` and `crates/app`.
- All text and status tokens must satisfy WCAG 2.2 AA minimum 4.5:1 contrast against light and dark sidebar backgrounds.
- No changes to SQLite schema or chat persistence models.
- Maintain smooth 120 FPS list scrolling; avoid allocations in the item render path.
- Existing keyboard navigation (`navigate`, `F2` rename, `Enter` selection) must remain intact.

## Review Focus
1. **Narrow sidebar widths (200px–240px):** Long branch names, changed chips, and status text must gracefully truncate without layout overflow.
2. **Keyboard focus ring visibility:** Focus ring must be clearly visible on selected rows without causing pixel shifts.
3. **Collapsed folder activity awareness:** Folded project headers must visually indicate when an agent inside is actively working or blocked.
4. **Pinned vs. Nested row layout consistency:** Pinned chats must inline their project badge to maintain uniform ~32px row heights.
5. **Human-in-the-loop quick action safety:** Quick approve affordances must accurately target only `Ask::Approval` states.

---

### Task 1: Row Chrome, Active Accent Indicator & Tree Guideline Refinement

**Files:**
- Modify: `crates/ui/src/chat/sidebar/chrome.rs`
- Test: `crates/ui/tests/unit/chat/sidebar/chrome.rs`

**Interfaces:**
- Consumes: `gpui_kit` styling tokens (`theme.sidebar_accent`, `theme.sidebar_border`, `theme.accent`)
- Produces: `pub fn row_style<E: StatefulInteractiveElement + Styled>(el: E, selected: bool, cx: &App) -> E`, `pub fn nest(row: impl IntoElement, nested: bool, cx: &App) -> AnyElement`

- [ ] **Step 1: Write unit tests for refined `row_style` and `nest`**
  Add assertions in `crates/ui/tests/unit/chat/sidebar/chrome.rs` checking border radius, translucent selected fills, and guide border styling.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-ui -- chat::sidebar::chrome`
  Expected: FAIL on new assertions.

- [ ] **Step 3: Implement refined row styling and guide line in `chrome.rs`**
  - Set `rounded(px(6.0))` and soft translucent fills (`sidebar_accent` opacity).
  - Add active indicator styling support and subtle 1px tree border.

- [ ] **Step 4: Run test to verify it passes**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-ui -- chat::sidebar::chrome`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add crates/ui/src/chat/sidebar/chrome.rs crates/ui/tests/unit/chat/sidebar/chrome.rs
  git commit -m "feat(ui): refine sidebar row chrome, active accent indicator and tree guides"
  ```

---

### Task 2: Compact & Zero-Redundancy Chat Row

**Files:**
- Modify: `crates/ui/src/chat/sidebar/chat_row.rs`
- Test: `crates/ui/tests/unit/chat/sidebar/chat_row.rs`

**Interfaces:**
- Consumes: `ChatItem`, `ChatLine`, `RowPlace`, `row_style`
- Produces: `pub(super) fn render_chat(...) -> AnyElement`

- [ ] **Step 1: Write failing unit test for nested row rendering**
  In `crates/ui/tests/unit/chat/sidebar/chat_row.rs`, test that when `nested == true`, the redundant project badge line is omitted, while when `nested == false` (pinned), project badge is retained inline.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-ui -- chat::sidebar::chat_row`
  Expected: FAIL

- [ ] **Step 3: Update `render_chat` in `chat_row.rs`**
  - Check `nested`: omit top project badge row if `nested == true`.
  - Tighten spacing to achieve ~32px row height.
  - Render title line with ellipsis menu reveal on hover/focus.
  - Render bottom line with branch icon, branch text, and git diff chip.

- [ ] **Step 4: Run test to verify it passes**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-ui -- chat::sidebar::chat_row`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add crates/ui/src/chat/sidebar/chat_row.rs crates/ui/tests/unit/chat/sidebar/chat_row.rs
  git commit -m "feat(ui): implement compact zero-redundancy chat rows in sidebar"
  ```

---

### Task 3: State-of-the-Art Status Badges & Quick Action Trigger

**Files:**
- Modify: `crates/ui/src/chat/sidebar/mod.rs`
- Modify: `crates/ui/src/chat/sidebar/chat_row.rs`
- Test: `crates/ui/tests/unit/chat/sidebar/chat_row.rs`

**Interfaces:**
- Consumes: `ChatStatus` variants (`Working`, `Waiting`, `Done`, `Failed`, etc.)
- Produces: `ChatStatus::mark`, `ChatStatus::badge`, inline action handler when `Waiting`

- [ ] **Step 1: Write failing unit test for status badges and waiting action pill**
  Test that `ChatStatus::Working` renders spinning micro-ring + elapsed text, and `ChatStatus::Waiting` renders amber attention indicator and optional quick-action affordance.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-ui -- chat::sidebar::chat_row`
  Expected: FAIL

- [ ] **Step 3: Implement micro-status indicators and waiting action pill**
  - Update `ChatStatus::mark` and `ChatStatus::badge` in `mod.rs`.
  - In `chat_row.rs`, render an inline `[Approve]` action button when `status == ChatStatus::Waiting` and callback is present.

- [ ] **Step 4: Run test to verify it passes**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-ui -- chat::sidebar::chat_row`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add crates/ui/src/chat/sidebar/mod.rs crates/ui/src/chat/sidebar/chat_row.rs crates/ui/tests/unit/chat/sidebar/chat_row.rs
  git commit -m "feat(ui): add state-of-the-art status badges and inline approval affordances"
  ```

---

### Task 4: Project Header & Collapsed Rollup Status

**Files:**
- Modify: `crates/ui/src/chat/sidebar/project_row.rs`
- Test: `crates/ui/tests/unit/chat/sidebar/project_row.rs`

**Interfaces:**
- Consumes: `ProjectHeader { name, path, count, collapsed, status, missing }`
- Produces: `ProjectRow::render`

- [ ] **Step 1: Write failing unit test for project header styling and collapsed status**
  Test that `project_row` renders a subtle `count_chip`, and when `collapsed == true` with an active status, displays the status dot/spinner chip.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-ui -- chat::sidebar::project_row`
  Expected: FAIL

- [ ] **Step 3: Implement refined project header in `project_row.rs`**
  - Use `count_chip` styling for project chat count.
  - When `collapsed` and `status != ChatStatus::Idle`, render status indicator.
  - Refine hover actions (`browse files`, `new chat`).

- [ ] **Step 4: Run test to verify it passes**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-ui -- chat::sidebar::project_row`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add crates/ui/src/chat/sidebar/project_row.rs crates/ui/tests/unit/chat/sidebar/project_row.rs
  git commit -m "feat(ui): refine project row headers and collapsed activity indicators"
  ```

---

### Task 5: Running Agents Section Enhancement

**Files:**
- Modify: `crates/ui/src/chat/sidebar/rows.rs`
- Test: `crates/ui/tests/unit/chat/sidebar/rows.rs`

**Interfaces:**
- Consumes: `AgentState`, `OutsideState`, `ChatRow::Agent`, `ChatRow::Outside`
- Produces: `fn agent_lines(...) -> Div`

- [ ] **Step 1: Write failing unit test for agent lines layout**
  Verify `agent_lines` renders task title, provider icon, detail string, live spinner/progress indicator, and stop button.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-ui -- chat::sidebar::rows`
  Expected: FAIL

- [ ] **Step 3: Implement enhanced agent layout in `rows.rs`**
  - Update `agent_lines` with high-contrast text and micro-status pill.
  - Position stop button with clean hover background.

- [ ] **Step 4: Run test to verify it passes**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-ui -- chat::sidebar::rows`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add crates/ui/src/chat/sidebar/rows.rs crates/ui/tests/unit/chat/sidebar/rows.rs
  git commit -m "feat(ui): enhance running agents and outside agents row layout"
  ```

---

### Task 6: Refined Header & Balanced Bottom Dock Navigation

**Files:**
- Modify: `crates/ui/src/chat/sidebar/render.rs`
- Modify: `crates/app/src/app/sidebar.rs`
- Test: `crates/app/tests/unit/app/sidebar.rs`

**Interfaces:**
- Consumes: `ChatSidebar`, `footer_rows`, `nav`
- Produces: `ChatSidebar::render`

- [ ] **Step 1: Write failing unit test for footer dock structure**
  Verify footer separates `Direct Chats` pill with count badge on the left from tool action buttons on the right.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-app -- app::sidebar`
  Expected: FAIL

- [ ] **Step 3: Implement refined header and balanced dock in `render.rs` and `sidebar.rs`**
  - Align `Direct Chats` (and `Outside aether`) on the footer left with high-contrast badge pill.
  - Align Terminal, Usage, and Settings icons on the footer right.
  - Refine header padding and border dividers.

- [ ] **Step 4: Run test to verify it passes**
  Run: `cargo test --manifest-path /Users/iris/Downloads/xzx/RUST/aether/Cargo.toml -p aether-app -- app::sidebar`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add crates/ui/src/chat/sidebar/render.rs crates/app/src/app/sidebar.rs crates/app/tests/unit/app/sidebar.rs
  git commit -m "feat(ui): refine sidebar header and balanced bottom dock navigation"
  ```
