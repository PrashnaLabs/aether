# Non-Agentic Chats Custom Sidebar and Back Button Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Provide a dedicated custom sidebar and back button navigation for non-agentic (OpenRouter / adk-rust) chats, organizing them into chronological and pinned sections while cleanly segregating them from agent project chats.

**Architecture:** 
1. Extend `projects.rs` with `ChatSection` time categorizer (`Pinned`, `Today`, `Yesterday`, `Previous 7 Days`, `Previous 30 Days`, `Older`).
2. Enhance `ChatSidebar` in `crates/ui` to support an optional back button in the header and flexible custom titled sections.
3. Add `SidebarView` state in `crates/app` (`Projects` vs `DirectChats`) with seamless switching on chat selection, header back buttons, and dedicated new non-agent chat creation.

**Tech Stack:** Rust, GPUI (`gpui_kit`, `gpui`), `chrono`.

**Spec:** Requirement from user request and `features.md`:
- "sidebar: distinguish between the adk-rust chat and agent chat. proper UI"
- "Non-agentic chats in their own sections in the sidebar with custom sidebar and back button"

## Global Constraints
- Preserve existing git worktree, terminal, and agent chat behaviors.
- Maintain existing keyboard shortcuts and accessibility labels.
- Follow GPUI styling conventions with semantic theme colors (`cx.theme()`).

## Review Focus
1. Switching between an agent chat and a non-agent chat updates `sidebar_view` correctly so the relevant sidebar is immediately displayed.
2. Clicking the back button in the custom sidebar header returns to the `Projects` sidebar and switches active selection to an agent chat if available.
3. Clicking the back button in the chat header (when viewing a non-agent chat) returns to `Projects` and selects an agent chat.
4. Pinned non-agent chats appear in the `Pinned` section of the `DirectChats` custom sidebar and do not leak into the `Projects` pinned agent chats section.
5. In the `Projects` sidebar, non-agent chats are displayed in their own distinct "Direct Chats" section rather than being mixed with agent project chats.

---

### Task 1: Add Chat Section Categorization by Time

**Files:**
- Modify: `crates/app/src/projects.rs:1-120`
- Test: `crates/app/src/projects.rs` (unit tests at bottom of file)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub(crate) enum ChatSection {
      Pinned,
      Today,
      Yesterday,
      Previous7Days,
      Previous30Days,
      Older,
  }

  impl ChatSection {
      pub(crate) fn label(&self) -> &'static str;
  }

  pub(crate) fn categorize_time<Tz: chrono::TimeZone>(at_ms: i64, now_ms: i64, tz: &Tz) -> ChatSection;
  ```

- [ ] **Step 1: Write the failing tests in `projects.rs`**
Add tests asserting `categorize_time` maps timestamps correctly to `Today`, `Yesterday`, `Previous7Days`, `Previous30Days`, and `Older`.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test --package aether --lib projects::tests`
Expected: FAIL due to missing `ChatSection` / `categorize_time`.

- [ ] **Step 3: Implement `ChatSection` and `categorize_time` in `crates/app/src/projects.rs`**
Implement the enum and the categorization logic using `chrono`.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test --package aether --lib projects::tests`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add crates/app/src/projects.rs
git commit -m "feat(projects): add ChatSection time categorization"
```

---

### Task 2: Extend `ChatSidebar` with Back Button and Custom Sections

**Files:**
- Modify: `crates/ui/src/chat/sidebar.rs`
- Test: `crates/ui/src/chat/sidebar.rs` (unit tests at bottom of file)

**Interfaces:**
- Produces:
  ```rust
  impl ChatSidebar {
      pub fn on_back(
          mut self,
          tooltip: impl Into<SharedString>,
          handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
      ) -> Self;

      pub fn chat_in_section(
          mut self,
          section: impl Into<SharedString>,
          ix: usize,
          id: i64,
          title: impl Into<SharedString>,
          active: bool,
          status: ChatStatus,
          pinned: bool,
          line: Option<ChatLine>,
          on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
          on_rename: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
          on_duplicate: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
          on_pin: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
          on_archive: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
          on_delete: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
          on_remove_worktree: Option<ClickHandler>,
          on_status: Option<ClickHandler>,
      ) -> Self;
  }
  ```

- [ ] **Step 1: Write the failing unit tests for `ChatSidebar` back button and custom sections**
Add unit tests verifying `on_back` can be attached and that `chat_in_section` populates rows grouped by section title.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test --package aether_ui --lib chat::sidebar::tests`
Expected: FAIL due to missing methods.

- [ ] **Step 3: Implement `on_back` and `chat_in_section` in `crates/ui/src/chat/sidebar.rs`**
Add `on_back: Option<(SharedString, ClickHandler)>` and `custom_sections: Vec<(SharedString, Vec<ChatItem>)>` to `ChatSidebar`. In the header, render `sidebar-back` button when `on_back` is set. In `rows()`, render each custom section with `header(&section_title)`.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test --package aether_ui --lib chat::sidebar::tests`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add crates/ui/src/chat/sidebar.rs
git commit -m "feat(ui): add back button and custom sections to ChatSidebar"
```

---

### Task 3: Implement `SidebarView` and Custom Non-Agent Sidebar in `ChatApp`

**Files:**
- Modify: `crates/app/src/app.rs`
- Test: `crates/app/src/app.rs` (unit tests at bottom of file)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
  pub(crate) enum SidebarView {
      #[default]
      Projects,
      DirectChats,
  }

  impl ChatApp {
      pub(crate) fn new_direct_chat(&mut self, window: &mut Window, cx: &mut Context<Self>);
      pub(crate) fn switch_sidebar_to_projects(&mut self, window: &mut Window, cx: &mut Context<Self>);
      pub(crate) fn switch_sidebar_to_direct_chats(&mut self, window: &mut Window, cx: &mut Context<Self>);
      fn render_direct_chats_sidebar(&self, cx: &mut Context<Self>) -> ChatSidebar;
  }
  ```

- [ ] **Step 1: Write failing tests for sidebar switching and segregation**
Write unit tests in `app.rs` asserting:
- `select` on a non-agent chat sets `sidebar_view` to `DirectChats`.
- `select` on an agent chat sets `sidebar_view` to `Projects`.
- `switch_sidebar_to_projects` returns to `Projects` and selects an agent chat when on a non-agent chat.
- `new_direct_chat` creates a non-agent chat with `Provider::OpenRouter`.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test --package aether --bin aether app::tests`
Expected: FAIL.

- [ ] **Step 3: Implement `SidebarView`, methods, and `render_direct_chats_sidebar` in `crates/app/src/app.rs`**
1. Add `pub(crate) sidebar_view: SidebarView` to `ChatApp`.
2. Implement `new_direct_chat`, `switch_sidebar_to_projects`, and `switch_sidebar_to_direct_chats`.
3. In `select`, update `self.sidebar_view` based on `chat.is_agent()`.
4. In `render_sidebar`, branch on `self.sidebar_view`:
   - If `DirectChats`, render `render_direct_chats_sidebar`.
   - If `Projects`, filter out pinned non-agent chats from "Pinned", and render non-agent chats under their own "Direct Chats" section with an action to enter the custom sidebar.
5. In `render_direct_chats_sidebar`, organize direct chats into `Pinned`, `Today`, `Yesterday`, `Previous 7 Days`, `Previous 30 Days`, and `Older`.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test --package aether --bin aether app::tests`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add crates/app/src/app.rs
git commit -m "feat(app): add custom DirectChats sidebar and view switching"
```

---

### Task 4: Add Back Button to Chat Header for Non-Agentic Chats and Update Features

**Files:**
- Modify: `crates/app/src/app.rs:1257-1309` (render_header)
- Modify: `features.md:23`
- Test: `crates/app/src/app.rs`

- [ ] **Step 1: Write failing test in `app.rs` verifying chat header back button for non-agent chats**
Verify the chat header contains back-to-projects affordance when `!is_agent`.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test --package aether --bin aether app::tests`
Expected: FAIL.

- [ ] **Step 3: Implement back button in `render_header` and update `features.md`**
In `render_header`, when `!is_agent`, render a back button:
`Button::new("chat-header-back").icon(IconName::ArrowLeft).tooltip("Back to Agent Projects").on_click(...)`
Update `features.md` line 23 to `- [x] Non-agentic chats in their own sections in the sidebar with custom sidebar and back button`.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test --package aether --bin aether app::tests`
Expected: PASS.

- [ ] **Step 5: Run full workspace test suite**
Run: `cargo test --workspace`
Expected: PASS with 0 failures.

- [ ] **Step 6: Commit**
```bash
git add crates/app/src/app.rs features.md
git commit -m "feat(app): add back button to chat header and mark feature complete"
```
