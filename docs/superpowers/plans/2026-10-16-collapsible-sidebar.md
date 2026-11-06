# Collapsible Left Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow the user to collapse the left sidebar completely to maximize screen space for the chat and terminal, and bring it back using header buttons or the `Cmd+B` / `Ctrl+B` keyboard shortcut.

**Architecture:** Add `sidebar_collapsed: bool` to `Settings` so the state persists across restarts. Add `ToggleSidebar` action bound to `secondary-b`. Update `ChatSidebar` to include a collapse button (`PanelLeftClose`). When collapsed, hide the sidebar and provide an expand button (`PanelLeftOpen`) in the main section header across all pages.

**Tech Stack:** Rust, GPUI, `gpui-kit` components and Lucide icons.

---

### Task 1: Add `sidebar_collapsed` to Core Settings

**Files:**
- Modify: `crates/core/src/settings.rs`
- Test: `crates/core/src/settings.rs`

- [ ] **Step 1: Write the failing test**
  In `crates/core/src/settings.rs` tests module, add `sidebar_collapsed_defaults_and_round_trips`:
  ```rust
  #[test]
  fn sidebar_collapsed_defaults_and_round_trips() {
      let s = Settings::default();
      assert!(!s.sidebar_collapsed);

      let parsed: Settings = serde_json::from_str("{}").unwrap();
      assert!(!parsed.sidebar_collapsed);

      let serialized = serde_json::to_string(&Settings {
          sidebar_collapsed: true,
          ..Settings::default()
      }).unwrap();
      let restored: Settings = serde_json::from_str(&serialized).unwrap();
      assert!(restored.sidebar_collapsed);
  }
  ```

- [ ] **Step 2: Run test to confirm failure**
  Run: `cargo test -p aether-core sidebar_collapsed_defaults_and_round_trips`

- [ ] **Step 3: Implement setting field**
  In `crates/core/src/settings.rs`:
  - Add field to `Settings`:
    ```rust
    /// Whether the left navigation sidebar is collapsed.
    #[serde(default)]
    pub sidebar_collapsed: bool,
    ```
  - In `Settings::default()`: initialize `sidebar_collapsed: false`.

- [ ] **Step 4: Run test to confirm it passes**
  Run: `cargo test -p aether-core`

---

### Task 2: Action & Keybinding `secondary-b` for `ToggleSidebar`

**Files:**
- Modify: `crates/app/src/actions.rs`
- Test: `crates/app/src/actions.rs`

- [ ] **Step 1: Declare `ToggleSidebar` action**
  In `crates/app/src/actions.rs`:
  - Add `ToggleSidebar` to `actions!(aether, [ ... ])`.
  - In `bind_keys(cx: &mut App)`: add `KeyBinding::new("secondary-b", ToggleSidebar, None)`.

- [ ] **Step 2: Verify it compiles**
  Run: `cargo check -p aether`

---

### Task 3: Add Collapse Button to `ChatSidebar`

**Files:**
- Modify: `crates/ui/src/chat/sidebar.rs`
- Test: `crates/ui/src/chat/sidebar.rs`

- [ ] **Step 1: Update `ChatSidebar` struct and methods**
  In `crates/ui/src/chat/sidebar.rs`:
  - Add `on_collapse: Option<ClickHandler>` to `ChatSidebar`.
  - Add builder method:
    ```rust
    pub fn on_collapse(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut Context<Self>) + 'static) -> Self {
        self.on_collapse = Some(Rc::new(handler));
        self
    }
    ```
  - In the top header of `ChatSidebar` (where `BrandMark` and `new-chat` button live), add a button with `IconName::PanelLeftClose`, tooltip "Collapse sidebar (Cmd+B)", and `when_some(self.on_collapse, |b, handler| b.on_click(handler))`.

- [ ] **Step 2: Add unit test in `crates/ui/src/chat/sidebar.rs`**
  Verify `ChatSidebar::new("Aether").on_collapse(|_, _, _| {})` compiles and initializes.

- [ ] **Step 3: Run UI tests**
  Run: `cargo test -p aether-ui`

---

### Task 4: Connect Toggle in `ChatApp` and Render Expand Button When Collapsed

**Files:**
- Modify: `crates/app/src/app.rs`
- Test: `crates/app/src/app.rs`

- [ ] **Step 1: Add `toggle_sidebar` method on `ChatApp`**
  In `crates/app/src/app.rs`:
  ```rust
  pub(crate) fn toggle_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
      self.settings.sidebar_collapsed = !self.settings.sidebar_collapsed;
      crate::settings::save(&self.settings);
      cx.notify();
  }
  ```
  And bind action in `ChatApp::render`:
  `.on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)))`

- [ ] **Step 2: Hook up `on_collapse` on sidebar**
  In `render_sidebar`:
  Add `.on_collapse(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)))`.

- [ ] **Step 3: Render Expand Button when collapsed**
  - When `!self.settings.sidebar_collapsed`: render `.child(self.render_sidebar(cx))`.
  - In `render_header(cx)`: when `self.settings.sidebar_collapsed`, render a button at the far left:
    ```rust
    Button::new("expand-sidebar")
        .ghost()
        .small()
        .icon(IconName::PanelLeftOpen)
        .tooltip("Expand sidebar (Cmd+B)")
        .accessibility_label("Expand sidebar")
        .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)))
    ```
  - For non-chat pages (`Settings`, `Skills`, `Agents`, `Terminal`): when `self.settings.sidebar_collapsed`, add a compact floating or docked expand button in the top left so the user can easily re-expand the sidebar.

- [ ] **Step 4: Verify test suite**
  Run: `cargo test --workspace`

---

### Task 5: Manual & Integration Verification

- [ ] **Step 1: Build and run workspace tests**
  Run: `cargo test --workspace`
- [ ] **Step 2: Review git diff**
  Ensure clean formatting, no unused variables or warnings.
- [ ] **Step 3: Commit changes**
  Run `git commit` with descriptive commit message.
