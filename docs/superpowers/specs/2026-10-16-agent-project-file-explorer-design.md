# Agent-Project Specific File Explorer & In-App Editor — Design

Date: 2026-10-16
Status: approved design

## Intent and why

Previously, the File Explorer was introduced as a global navigation page (`Page::Explorer`) rooted at `workspace_root`. However:
1. Developers in Aether work in **agent projects** (e.g., individual crates, repositories, or isolated git worktrees).
2. A global explorer clutters the top-level navigation and forces users to browse from the workspace root rather than the focused context of their active agent.
3. Users want the File Explorer to be **agent project specific**: a dedicated full-stage view rooted right at the active agent's project directory / worktree (`cwd`), accessible directly from within the agent chat and from the sidebar project groups.

This design transitions the File Explorer from a global sidebar page to an **Agent-Project Specific Dedicated View**.

---

## Goals

1. **Remove Global Sidebar Navigation Tab**:
   - Remove `Page::Explorer` from the top-level app sidebar navigation (`.nav(IconName::Folder, "Explorer", ...)`).
   - Sidebar navigation retains only global application contexts (Chats, Global Terminal, Agents, Settings).

2. **Dedicated Project View in Chat Header**:
   - In the active Agent chat header, provide a dedicated view switcher:
     - `[💬 Chat]` (default agent conversation canvas).
     - `[📂 Files]` / `Project Explorer (⌘⇧E)` (dedicated project explorer canvas).
   - Hotkey **`⌘⇧E`** toggles between Chat and Project Explorer for the current agent.

3. **Sidebar Project Group Trigger**:
   - Each project group row in the sidebar (e.g., `📁 aether (2 chats)`) has a quick `[📂 Files]` affordance.
   - Clicking it switches to that project's most active chat (`max_by_key(|c| c.activity())`) and directly opens its dedicated Project Explorer view.

4. **Project-Scoped Directory Tree & Search**:
   - The file tree root is strictly set to the agent's active folder (`chat_cwd(chat, root)`), resolving the isolated git worktree branch if the agent runs in a worktree, or the base repo folder.
   - Quick Open (`⌘P`) and Grep (`⌘⇧F`) search within the active agent's project directory, ignoring `.git/`, `target/`, and `node_modules/`.

5. **Multi-Project Independent State**:
   - Explorer state (open tabs, dirty edits, active tab, tree expansion) is maintained per project/worktree folder (`HashMap<PathBuf, Entity<ExplorerState>>`). Switching between an agent in `crates/app` and an agent in `crates/agent` preserves each project's open editor buffers and dirty state.
   - Each chat stores its view state (`chat.view: ChatViewMode`), preserving whether you were viewing chat or files on that specific session.

6. **Fallback for Non-Project / OpenRouter Chats**:
   - Chats without an active directory or OpenRouter chats fall back to `workspace_root` (or home dir) so file browsing, Quick Open, and Grep remain accessible anywhere.

7. **Seamless Agent Chat Handoff**:
   - The editor action bar contains an **`[Ask Agent 💬]`** button that switches back to the agent chat, pre-populating composer with the active file path for immediate assistance.

---

## Non-goals

- Not embedding the full editor inside a cramped 200px drawer; it retains the full dedicated stage canvas for comfortable code editing.
- Not altering global terminal behavior; Global Terminal remains window-wide.
- Not building an LSP language server (completion/diagnostics) in this phase.

---

## Architectural & Data Model Design

### 1. View Mode on `Chat` & Cache in `ChatApp`

In `Chat`:
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum ChatViewMode {
    #[default]
    Chat,
    Explorer,
}

pub(crate) struct Chat {
    ...
    pub(crate) view: ChatViewMode,
}
```

In `ChatApp`:
```rust
pub(crate) struct ChatApp {
    ...
    /// Explorer states keyed by active directory PathBuf (worktree or repo root).
    pub(crate) project_explorers: HashMap<PathBuf, Entity<crate::explorer::ExplorerState>>,
    ...
}
```

### 2. Resolving the Active Project Folder and Explorer

```rust
impl ChatApp {
    pub(crate) fn active_project_folder(&self) -> PathBuf {
        let root = &self.settings.workspace_root;
        if let Some(chat) = self.chats.get(self.active) {
            crate::app::chat_cwd(chat, root)
        } else {
            crate::terminal::resolve_global_folder(root)
        }
    }

    pub(crate) fn active_project_explorer(&mut self, cx: &mut Context<Self>) -> Entity<crate::explorer::ExplorerState> {
        let folder = self.active_project_folder();
        if let Some(exp) = self.project_explorers.get(&folder) {
            return exp.clone();
        }
        let exp = cx.new(|_| crate::explorer::ExplorerState::new(folder.clone()));
        self.project_explorers.insert(folder, exp.clone());
        exp
    }
}
```

### 3. Navigation & Actions

- `ToggleExplorer` (`⌘⇧E`):
  - If `self.page != Page::Chat`: switches to `Page::Chat`.
  - Toggles `self.chats[self.active].view` between `ChatViewMode::Chat` and `ChatViewMode::Explorer`.
  - If entering Explorer, syncs active buffer to textarea and refreshes tree.

- `ChatApp::render`:
  - When `self.page == Page::Chat`:
    - Renders header with `[💬 Chat] [📂 Files]` toggle.
    - If `self.chats[self.active].view == ChatViewMode::Chat`: renders hero/chat transcript + composer.
    - If `self.chats[self.active].view == ChatViewMode::Explorer`: renders the dedicated `ProjectExplorer` canvas.

---

## Builder Decisions Log

1. **Worktree Resolution**: `active_project_folder()` strictly resolves `chat_cwd(chat, root)` so agents running in git worktrees view and edit their specific branch worktree files, not the parent main branch.
2. **View Mode Lifecycle**: `ChatViewMode` is stored directly on `Chat` so switching between chats remembers each chat's view mode (Chat vs Explorer) naturally.
3. **OpenRouter / Draft Fallback**: Chats without explicit directories fall back to `workspace_root`, ensuring zero dead ends.
4. **Project Row Files Button**: Clicking `[📂 Files]` on a sidebar project group picks the newest chat in that project (`chats.max_by_key(|c| c.activity())`) and switches its view to `ChatViewMode::Explorer`.

---

## Verification Plan

1. **Unit Tests**:
   - `test_chat_view_mode_toggle`: verify `⌘⇧E` toggles between `Chat` and `Explorer` on the active chat.
   - `test_project_explorer_scoping`: verify `active_project_folder()` resolves the agent chat's worktree path when in a worktree, and the repo path otherwise.
   - `test_multi_project_state_isolation`: verify opening files in Project A does not leak into Project B.
2. **Compilation & Suite Verification**:
   - `cargo check --workspace` and `cargo test --workspace` pass with 0 errors.
