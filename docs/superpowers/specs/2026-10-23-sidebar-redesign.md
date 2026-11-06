# Aether Sidebar UX & UI Redesign Specification

**Date:** 2026-10-23  
**Status:** Approved by user via interactive prototype testing  
**Target:** `crates/ui/src/chat/sidebar/` and `crates/app/src/app/sidebar.rs`  
**Prototype Reference:** Visual Companion Screen v4 (`sidebar-v4-state-of-the-art.html`)

---

## 1. Intent and Context

The current Aether sidebar (shown in the initial user screenshot) suffers from several UX/UI deficiencies:
- Visual "card soup" with thick gray card backgrounds and lack of refined hierarchy.
- Low-contrast section captions ("Pinned", "Projects") and orphaned count labels ("1", "0").
- Redundant repetition of the project badge `[AE] aether` inside every nested chat under the `aether` folder, wasting ~30% vertical space.
- Ambiguous status indicators (e.g. asterisk/sun icon on the bottom right without textual explanation of state).
- Cluttered bottom navigation with tiny icons bunched into the bottom-right corner.
- Lack of immediate actionable affordances when AI agents are blocked or waiting for user approval.

This redesign elevates the sidebar to a state-of-the-art desktop developer experience (Linear & Zed caliber), prioritizing information density, cognitive clarity, keyboard accessibility (WCAG 2.2 AA), and frictionless multi-agent monitoring.

---

## 2. Goals, Non-Goals, and Anti-Goals

### Goals
- **Information Density & Scannability:** Compact rows (~30px–36px height) enabling 12–16 items in view without scrolling, using borderless idle states and subtle translucent active pills.
- **Zero-Redundancy Hierarchy:** Omit the redundant project mark (`[AE] aether`) for chat items nested under their parent project folder; keep the project badge only for Pinned chats or cross-project search results.
- **Explicit, Accessible Status System:** Replace ambiguous glyphs with clean micro-status pills and pulsating dots (`Working · 42s`, `Needs approval`, `Done`, `Failed`).
- **Semantic Git Statistics:** Display additions/deletions or a clear diff chip instead of flat unstyled text, immediately conveying the scale of changes.
- **Human-in-the-Loop Fast Action:** When an agent is waiting for approval (`Ask::Approval`), provide an inline quick-action trigger (`[Approve ↵]`) directly on the row so developers don't have to navigate away from their current code.
- **Refined Bottom Dock:** Balanced navigation with an explicit `Direct Chats` pill with unread count badge (`2`) on the left, and clean tool buttons (Terminal, Usage, Settings) on the right.
- **Folder Activity Awareness:** When a project folder is collapsed, an activity chip (e.g. `1 Active` with spinner or amber dot) alerts the developer to running/waiting work inside.

### Non-Goals
- Changing the underlying chat storage model, SQLite schema, or IPC messaging protocol.
- Altering the chat view / editor pane outside the sidebar component.
- Rewriting the GPUI rendering engine or replacing `gpui-kit`.

### Anti-Goals
- "Card soup" or heavy outlines around every single item.
- Decorative-only elements that add visual clutter without functional utility.
- Breaking keyboard navigation or screen reader accessibility.

---

## 3. Constraints

- **Framework:** Rust GPUI / `gpui-kit` (`Div`, `h_flex`, `v_flex`, `Button`, `Icon`, `Text`, `ThemeTokens`).
- **Performance:** Sidebar rows must render smoothly at 120 FPS; avoid heavy layout recalculations or unnecessary allocations during list scrolling.
- **Compatibility:** Preserve all existing actions (`ToggleSidebar`, `NewChat`, `rename_editor`, `on_resize`, `context_menu`).
- **Accessibility:** Text contrast ratio must meet WCAG 2.2 AA (4.5:1 minimum against sidebar background in both light and dark themes).

---

## 4. Key Design Decisions

### 4.1 Chat Row Geometry & Progressive Disclosure (`chat_row.rs`, `chrome.rs`)
- In `chrome.rs`:
  - `row_style`: Update fills to use soft translucent hover (`theme.sidebar_accent.opacity(0.4)`) and active fill (`theme.sidebar_accent.opacity(0.8)` or theme tokens). Radius set to `px(6.0)`.
  - Add an active accent indicator: When `chat.active`, render an orange indicator pip (`#f97316` / `theme.accent`) aligning smoothly with the tree guide.
- In `chat_row.rs`:
  - Check `nested: bool`: If the row is nested under a project folder, do NOT render the top `project_mark` / `project_label` line.
  - If `!nested` (e.g. in Pinned chats), render the micro project mark + label so cross-project context is clear.
  - Middle row: chat title (`font_medium()`, `text_color(theme.foreground)`), ellipsis menu revealed on hover/focus.
  - Bottom row: Git branch with `Lucide::GitBranch` icon, followed by semantic diff chips (`18 changed` / `+add -del` styling), and status indicator.
  - When `status == ChatStatus::Waiting`, render an inline `[Approve]` action button on the row.

### 4.2 Project & Worktree Rows (`project_row.rs`)
- Modernized chevron and folder icon.
- Chat count rendered as a subtle rounded chip (`count_chip`) rather than raw unstyled text.
- Collapsed state indicator: If the project is collapsed and contains an active running agent or waiting chat, display a micro-status badge on the project header.

### 4.3 Running Agents Section & Active States (`rows.rs`)
- Active agent rows render:
  - Agent task title
  - Detail line with model / provider icon
  - Live running timer / spinner
  - Red quick-stop button (`Square` icon) when busy
  - If `Needs approval`, amber badge with quick-approval button

### 4.4 Header & Dock Navigation (`render.rs`)
- Header: Brand mark pill with "岚" glyph + "Aether" title + `+` New chat + Collapse sidebar button.
- Body: Integrated search/filter slot or clean list padding.
- Footer: Left-aligned `Direct Chats` pill with unread badge `2`, separated from right-aligned tool icons (Terminal, Usage, Settings).

---

## 5. What's Left to the Builder
- Exact GPUI styling helper methods (`gpui_kit` tailwind-like chainable methods: `.px_2()`, `.py_1()`, `.gap_1p5()`, etc.).
- Unit test updates in `crates/ui/tests/unit/chat/sidebar/` to align with the refined DOM tree and aria attributes.
- Ensuring smooth backward compatibility with any custom sidebar theme settings.
