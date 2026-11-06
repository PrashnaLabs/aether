# Agent-Project Specific File Explorer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform the File Explorer from a global sidebar navigation page into a dedicated view scoped strictly to the active agent's project directory or git worktree, accessible via in-chat header switchers, sidebar project groups, and hotkeys.

**Architecture:** Each `Chat` tracks its own `view: ChatViewMode` (`Chat` or `Explorer`). `ChatApp` maintains a per-project cache of `ExplorerState`s keyed by `PathBuf` (resolving the agent's worktree or repo directory via `chat_cwd`). Global navigation sidebar removes the orphaned global Explorer button, while the chat header gains a `[💬 Chat] [📂 Files]` switcher and each sidebar project group gains a `[📂 Files]` affordance.

**Tech Stack:** Rust, GPUI (`gpui_kit`, `aether_ui`, `crates/app`), Git worktrees, Catppuccin theme.

**Spec:** `docs/superpowers/specs/2026-10-16-agent-project-file-explorer-design.md`

## Global Constraints

- Never lose unsaved buffer edits when toggling between Chat and Explorer or switching between chats.
- Worktree directories (`chat.worktree_path` / `chat_cwd`) must be resolved as the explorer root when an agent operates in a worktree, keeping worktree branch edits visible.
- Non-project / OpenRouter chats fall back to `workspace_root` gracefully without crashes.
- All workspace tests must pass (`cargo test --workspace`) with 0 failures.

## Review Focus

1. **Worktree Isolation**: Explorer opened in a worktree agent chat must browse and edit files in that worktree, not the parent repo root on main.
2. **Buffer Persistence across View Toggles**: Switching from `ChatViewMode::Explorer` to `ChatViewMode::Chat` and back must preserve open tabs, cursor position, and dirty buffer contents.
3. **Multi-Project Tab Separation**: Buffers open in Project A (e.g. `crates/app`) must not leak into Project B (e.g. `crates/agent`).
4. **Project Group `[📂 Files]` Trigger**: Clicking Files on a folded/unfolded sidebar project group must select that group's most active chat and switch directly to its explorer view.
5. **Clean Global Navigation**: The global left sidebar nav must not display a global Explorer button.

---

### Task 1: Chat Data Model & Project Scoping Resolution

**Files:**
- Modify: `crates/app/src/app.rs`
- Test: `crates/app/src/app.rs:tests`

**Interfaces:**
- `pub(crate) enum ChatViewMode { Chat, Explorer }`
- `Chat.view: ChatViewMode`
- `ChatApp::active_project_folder(&self) -> PathBuf`

- [ ] **Step 1: Write unit tests for `ChatViewMode` toggle and `active_project_folder` scoping**

In `crates/app/src/app.rs` under `mod tests`:
```rust
#[test]
fn chat_view_mode_defaults_to_chat_and_toggles() {
    let mut mode = ChatViewMode::default();
    assert_eq!(mode, ChatViewMode::Chat);
    mode = match mode {
        ChatViewMode::Chat => ChatViewMode::Explorer,
        ChatViewMode::Explorer => ChatViewMode::Chat,
    };
    assert_eq!(mode, ChatViewMode::Explorer);
}

#[test]
fn active_project_folder_resolves_chat_cwd_or_workspace_fallback() {
    let mut chat = Chat::new(1, Provider::Claude, "");
    chat.cwd = "/tmp/my-project".to_string();
    let root = PathBuf::from("/workspace");
    let resolved = chat_cwd(&chat, &root);
    assert_eq!(resolved, PathBuf::from("/tmp/my-project"));
}
```

- [ ] **Step 2: Run test to verify compilation failure or success**

Run: `cargo test -p aether chat_view_mode_defaults_to_chat_and_toggles`
Expected: FAIL until `ChatViewMode` is defined.

- [ ] **Step 3: Define `ChatViewMode`, add `view: ChatViewMode` to `Chat`, and implement `active_project_folder`**

In `crates/app/src/app.rs`:
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum ChatViewMode {
    #[default]
    Chat,
    Explorer,
}

// In struct Chat:
pub(crate) view: ChatViewMode,

// In Chat::new:
view: ChatViewMode::Chat,

// In impl ChatApp:
pub(crate) fn active_project_folder(&self) -> PathBuf {
    let root = &self.settings.workspace_root;
    if let Some(chat) = self.chats.get(self.active) {
        chat_cwd(chat, root)
    } else {
        crate::terminal::resolve_global_folder(root)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p aether chat_view_mode_defaults_to_chat_and_toggles`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/app.rs
git commit -m "feat(chat): add ChatViewMode and active_project_folder resolution"
```

---

### Task 2: Multi-Project Explorer State Cache in `ChatApp`

**Files:**
- Modify: `crates/app/src/app.rs`
- Modify: `crates/app/src/explorer/page.rs`
- Test: `crates/app/src/explorer/view.rs:tests`

**Interfaces:**
- `ChatApp.project_explorers: HashMap<PathBuf, Entity<ExplorerState>>`
- `ChatApp::active_project_explorer(&mut self, cx: &mut Context<Self>) -> Entity<ExplorerState>`
- `ChatApp::explorer_for_folder(&mut self, folder: PathBuf, cx: &mut Context<Self>) -> Entity<ExplorerState>`

- [ ] **Step 1: Write test for multi-project explorer isolation**

In `crates/app/src/explorer/view.rs:tests`:
```rust
#[test]
fn explorer_instances_maintain_separate_roots_and_tabs() {
    let dir_a = std::env::temp_dir().join("aether_test_scope_a");
    let dir_b = std::env::temp_dir().join("aether_test_scope_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    let file_a = dir_a.join("file_a.txt");
    let file_b = dir_b.join("file_b.txt");
    std::fs::write(&file_a, "A").unwrap();
    std::fs::write(&file_b, "B").unwrap();

    let mut exp_a = ExplorerState::new(dir_a.clone());
    let mut exp_b = ExplorerState::new(dir_b.clone());

    exp_a.open_file(file_a).unwrap();
    exp_b.open_file(file_b).unwrap();

    assert_eq!(exp_a.tabs.len(), 1);
    assert_eq!(exp_a.tabs[0].title, "file_a.txt");
    assert_eq!(exp_b.tabs.len(), 1);
    assert_eq!(exp_b.tabs[0].title, "file_b.txt");

    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test -p aether explorer_instances_maintain_separate_roots_and_tabs`
Expected: PASS

- [ ] **Step 3: Update `ChatApp` and `crates/app/src/explorer/page.rs` for per-project explorer routing**

1. In `ChatApp`:
   Replace single `explorer: Entity<ExplorerState>` with `pub(crate) project_explorers: HashMap<PathBuf, Entity<ExplorerState>>`.
2. Add helper:
   ```rust
   pub(crate) fn active_project_explorer(&mut self, cx: &mut Context<Self>) -> Entity<ExplorerState> {
       let folder = self.active_project_folder();
       if let Some(exp) = self.project_explorers.get(&folder) {
           return exp.clone();
       }
       let exp = cx.new(|_| crate::explorer::ExplorerState::new(folder.clone()));
       self.project_explorers.insert(folder, exp.clone());
       exp
   }
   ```
3. In `crates/app/src/explorer/page.rs`, update all methods (`open_explorer_file`, `save_active_explorer_file`, `render_explorer_page`, etc.) to obtain `let explorer = self.active_project_explorer(cx);`.

- [ ] **Step 4: Run compilation and test suite**

Run: `cargo check -p aether && cargo test -p aether`
Expected: PASS with 0 errors.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/app.rs crates/app/src/explorer/page.rs crates/app/src/explorer/view.rs
git commit -m "feat(explorer): implement per-project explorer caching and scoping"
```

---

### Task 3: In-Chat Header Switcher & Global Navigation Cleanup

**Files:**
- Modify: `crates/app/src/app.rs`
- Modify: `crates/app/src/actions.rs`
- Test: `crates/app/src/app.rs:tests`

**Interfaces:**
- Remove `Page::Explorer` from sidebar `.nav(IconName::Folder, ...)`
- Chat header view toggle: `[💬 Chat] [📂 Files]`
- `ToggleExplorer` (`⌘⇧E`): toggles `self.chats[self.active].view`
- Main stage rendering: renders `ProjectExplorer` when `chat.view == ChatViewMode::Explorer`

- [ ] **Step 1: Write test for `ToggleExplorer` action toggling active chat's view mode**

In `crates/app/src/app.rs:tests`:
```rust
#[test]
fn toggle_explorer_switches_active_chat_view_mode() {
    let mut view = ChatViewMode::Chat;
    view = match view {
        ChatViewMode::Chat => ChatViewMode::Explorer,
        ChatViewMode::Explorer => ChatViewMode::Chat,
    };
    assert_eq!(view, ChatViewMode::Explorer);
    view = match view {
        ChatViewMode::Chat => ChatViewMode::Explorer,
        ChatViewMode::Explorer => ChatViewMode::Chat,
    };
    assert_eq!(view, ChatViewMode::Chat);
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test -p aether toggle_explorer_switches_active_chat_view_mode`
Expected: PASS

- [ ] **Step 3: Update `ChatApp::render_header`, sidebar, and main page rendering**

1. Remove `.nav(IconName::Folder, "Explorer", ...)` from sidebar in `crates/app/src/app.rs`.
2. In `render_header`:
   Add view mode pills:
   - `[💬 Chat]` (active when `chat.view == ChatViewMode::Chat`)
   - `[📂 Files]` (active when `chat.view == ChatViewMode::Explorer`)
   Clicking toggles `chat.view`.
3. In main page rendering around line 3750:
   When `self.page == Page::Chat`:
   If `self.chats[self.active].view == ChatViewMode::Explorer`: render `self.render_explorer_page(window, cx)`.
   If `ChatViewMode::Chat`: render the chat hero or message scroller + composer.
4. Update `toggle_explorer_page`:
   ```rust
   pub(crate) fn toggle_explorer_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
       if self.page != Page::Chat {
           self.open_page(Page::Chat, cx);
       }
       if let Some(chat) = self.chats.get_mut(self.active) {
           chat.view = match chat.view {
               ChatViewMode::Chat => ChatViewMode::Explorer,
               ChatViewMode::Explorer => ChatViewMode::Chat,
           };
           if chat.view == ChatViewMode::Explorer {
               let exp = self.active_project_explorer(cx);
               exp.update(cx, |e, _| e.refresh_tree());
               if let Some(content) = exp.read(cx).active_buffer().map(|b| b.content.clone()) {
                   self.explorer_editor.update(cx, |ed, cx| ed.set_value(content, window, cx));
               }
           }
       }
       cx.notify();
   }
   ```

- [ ] **Step 4: Run test suite and check compilation**

Run: `cargo check -p aether && cargo test -p aether`
Expected: PASS with 0 errors.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/app.rs
git commit -m "feat: embed dedicated project explorer in chat header and remove global nav item"
```

---

### Task 4: Sidebar Project Group `[📂 Files]` Trigger & Ask Agent Handoff

**Files:**
- Modify: `crates/app/src/app.rs`
- Modify: `crates/app/src/explorer/page.rs`
- Test: `crates/app/src/app.rs:tests`

**Interfaces:**
- Sidebar project group row has `[📂 Files]` button.
- Clicking switches to the project's most active chat and sets `chat.view = ChatViewMode::Explorer`.
- Editor toolbar `[Ask Agent 💬]` button switches `chat.view` back to `ChatViewMode::Chat` and focuses composer with file path.

- [ ] **Step 1: Write unit test for project group chat selection and file view switch**

In `crates/app/src/app.rs:tests`:
```rust
#[test]
fn opening_project_files_selects_most_active_chat_and_sets_explorer_mode() {
    let mut chat1 = Chat::new(1, Provider::Claude, "");
    chat1.last_active = 100;
    chat1.cwd = "/workspace/pkg".to_string();

    let mut chat2 = Chat::new(2, Provider::Claude, "");
    chat2.last_active = 200;
    chat2.cwd = "/workspace/pkg".to_string();

    let chats = vec![chat1, chat2];
    let newest_ix = chats.iter().enumerate().max_by_key(|(_, c)| c.activity()).map(|(i, _)| i).unwrap();
    assert_eq!(newest_ix, 1);
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test -p aether opening_project_files_selects_most_active_chat_and_sets_explorer_mode`
Expected: PASS

- [ ] **Step 3: Implement sidebar project files trigger and Ask Agent handoff**

1. In `ChatApp`: add method `pub(crate) fn open_project_explorer(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>)`:
   - Finds the most active chat with matching project directory:
     `self.chats.iter().enumerate().filter(|(_, c)| project_dir(c, &self.settings.workspace_root) == folder).max_by_key(|(_, c)| c.activity()).map(|(i, _)| i)`.
   - If found, `self.select(ix, window, cx)`. If not found, `self.new_chat_in(folder, window, cx)`.
   - Sets `self.chats[self.active].view = ChatViewMode::Explorer`.
   - Syncs editor buffer and refreshes tree.
2. In sidebar project title bar:
   Add `icon_chip` or `Button` with `IconName::Folder` labeled `Files`, invoking `open_project_explorer(folder)`.
3. In `crates/app/src/explorer/page.rs`:
   Update `Ask Agent 💬` button in editor toolbar:
   - Sets `self.chats[self.active].view = ChatViewMode::Chat`.
   - Pre-populates composer with `@file <path> ` if empty.
   - Focuses composer.

- [ ] **Step 4: Run full project compilation and workspace test suite**

Run: `cargo test --workspace`
Expected: All 145+ tests pass with 0 failures.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/app.rs crates/app/src/explorer/page.rs
git commit -m "feat: add sidebar project files trigger and in-editor Ask Agent handoff"
```

---

### Task 5: Final Verification & Documentation

**Files:**
- Modify: `docs/superpowers/plans/2026-10-16-agent-project-file-explorer.md`
- Modify: `features.md`

- [ ] **Step 1: Run comprehensive verification**

Run:
```bash
cargo check --workspace
cargo test --workspace
```
Confirm 0 compiler warnings/errors and 100% test pass rate.

- [ ] **Step 2: Update feature tracking & ledger**

Confirm `features.md` accurately describes the Agent-Project File Explorer and update SDD progress ledger.

- [ ] **Step 3: Commit**

```bash
git add features.md docs/superpowers/plans/2026-10-16-agent-project-file-explorer.md
git commit -m "docs: complete agent-project specific file explorer implementation plan"
```
