# Global File Explorer, In-App Editor, Quick Open (⌘P) & Grep (⌘⇧F) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a first-class Global File Explorer page in Aether with a tabbed in-app editor, live dirty state tracking, `⌘S` saving, Quick Open (`⌘P`) file finder, Workspace Grep (`⌘⇧F`), and folder-scoped agent chat dispatch.

**Architecture:** Create an `explorer` module under `crates/app/src/explorer/` containing: `buffer.rs` for tab buffers and file saving; `tree.rs` for directory tree traversal and filtering; `search.rs` for fuzzy file matching and async workspace text grep; and `view.rs` for the dual-pane UI. Wire `Page::Explorer` into `crates/app/src/app.rs` with sidebar navigation (`IconName::Folder`), global hotkeys (`⌘⇧E`, `⌘P`), and direct integration with `new_chat_in(folder)` for folder agent launches.

**Tech Stack:** Rust 2024 edition, GPUI (`gpui`, `gpui-kit`), `aether-ui`, `aether-core`, `std::fs`, `tokio`/background executor.

**Spec:** [docs/superpowers/specs/2026-10-16-global-file-explorer-design.md](../specs/2026-10-16-global-file-explorer-design.md)

## Global Constraints

- Must never block the GPUI UI thread during large directory walks, file reads, or global grep searches.
- Preserve all existing keybindings (`ctrl-``` terminal toggle, `cmd-j`, `cmd-k`, etc.).
- Never overwrite unsaved file edits without confirmation.
- Keep agent launching consistent with `new_chat_in(folder)` in `app.rs`.
- Follow Catppuccin and Aether design system tokens (`theme.background`, `theme.border`, `theme.sidebar`, `theme.accent`).

## Review Focus

1. File tree walk on workspace with deeply nested or massive directories (`target/`, `node_modules/`, `.git/`): must respect ignore rules and not exhaust memory or freeze.
2. Saving a file when the file on disk was modified or deleted externally: must detect baseline mismatch and avoid silent data loss.
3. Closing a dirty tab or switching pages while edits are pending: in-memory edits must stay alive in state, and closing a dirty tab must prompt for confirmation.
4. Quick Open (`⌘P`) with empty query or non-existent paths: must display recent files or empty state without crashing or panicking on index bounds.
5. Workspace Grep (`⌘⇧F`) with invalid regex or empty string: must handle regex syntax errors gracefully with clear user feedback rather than crashing background task.

---

### Task 1: Editor Buffer Model & Dirty State (`buffer.rs`)

**Files:**
- Create: `crates/app/src/explorer/buffer.rs`
- Create: `crates/app/src/explorer/mod.rs`
- Modify: `crates/app/src/main.rs:7-15`
- Test: `crates/app/src/explorer/buffer.rs:tests`

**Interfaces:**
- Produces: `struct EditorBuffer` holding `path: PathBuf`, `title: String`, `original: String`, `content: String`, `cursor: (usize, usize)`, `scroll: f32`, `is_dirty: bool`.
- Produces: `EditorBuffer::open(path: PathBuf) -> std::io::Result<Self>`.
- Produces: `EditorBuffer::set_content(&mut self, new_content: String)`.
- Produces: `EditorBuffer::save(&mut self) -> std::io::Result<()>`.
- Produces: `EditorBuffer::revert(&mut self)`.
- Produces: `EditorBuffer::detect_language(&self) -> &'static str`.
- Produces: `EditorBuffer::line_col_at(&self, char_offset: usize) -> (usize, usize)`.

- [ ] **Step 1: Write failing tests for `EditorBuffer` lifecycle, dirty state, save, and revert**

In `crates/app/src/explorer/buffer.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn buffer_tracks_dirty_state_and_saves() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        {
            let mut f = std::fs::File::create(&file_path).unwrap();
            f.write_all(b"fn main() {}\n").unwrap();
        }

        let mut buffer = EditorBuffer::open(file_path.clone()).unwrap();
        assert_eq!(buffer.is_dirty, false);
        assert_eq!(buffer.detect_language(), "Rust");

        buffer.set_content("fn main() { println!(\"hello\"); }\n".to_string());
        assert_eq!(buffer.is_dirty, true);

        buffer.revert();
        assert_eq!(buffer.is_dirty, false);
        assert_eq!(buffer.content, "fn main() {}\n");

        buffer.set_content("fn main() { println!(\"saved\"); }\n".to_string());
        assert_eq!(buffer.is_dirty, true);
        buffer.save().unwrap();
        assert_eq!(buffer.is_dirty, false);

        let on_disk = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(on_disk, "fn main() { println!(\"saved\"); }\n");
    }

    #[test]
    fn buffer_computes_line_and_column() {
        let buffer = EditorBuffer::virtual_buffer("test.txt", "abc\ndefgh\nij\n");
        assert_eq!(buffer.line_col_at(0), (1, 1));
        assert_eq!(buffer.line_col_at(4), (2, 1));
        assert_eq!(buffer.line_col_at(6), (2, 3));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether buffer_tracks_dirty_state_and_saves`
Expected: FAIL with "module `explorer` not found"

- [ ] **Step 3: Implement `EditorBuffer` in `crates/app/src/explorer/buffer.rs` and register module in `crates/app/src/explorer/mod.rs` & `main.rs`**

Implement `EditorBuffer` methods: `open`, `save`, `revert`, `set_content`, `detect_language`, and `line_col_at`. Register `pub(crate) mod explorer;` in `crates/app/src/main.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether buffer_tracks_dirty_state_and_saves`
Expected: PASS

- [ ] **Step 5: Verify all buffer tests pass**

Run: `cargo test -p aether explorer::buffer`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/explorer/ crates/app/src/main.rs
git commit -m "feat(explorer): implement EditorBuffer model with dirty tracking and save"
```

---

### Task 2: Workspace Directory Tree & File Status (`tree.rs`)

**Files:**
- Create: `crates/app/src/explorer/tree.rs`
- Modify: `crates/app/src/explorer/mod.rs`
- Test: `crates/app/src/explorer/tree.rs:tests`

**Interfaces:**
- Produces: `struct FileTreeNode` with `path: PathBuf`, `name: String`, `is_dir: bool`, `depth: usize`, `is_expanded: bool`, `git_status: Option<char>`, `children: Vec<FileTreeNode>`.
- Produces: `struct FileTreeState` with `root: PathBuf`, `filter_query: String`, `expanded: HashSet<PathBuf>`, `selected: Option<PathBuf>`.
- Produces: `FileTreeState::new(root: PathBuf) -> Self`.
- Produces: `FileTreeState::visible_nodes(&self, entries: &[FileTreeNode]) -> Vec<&FileTreeNode>`.
- Produces: `FileTreeState::toggle_expand(&mut self, path: &Path)`.
- Produces: `FileTreeState::set_filter(&mut self, query: String)`.
- Produces: `scan_directory(root: &Path, max_depth: usize) -> std::io::Result<Vec<FileTreeNode>>`.

- [ ] **Step 1: Write failing tests for `FileTreeState` expansion, filtering, and directory traversal**

In `crates/app/src/explorer/tree.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_traversal_ignores_dotgit_and_target() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let git_dir = dir.path().join(".git");
        let target_dir = dir.path().join("target");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(git_dir.join("config"), "git config").unwrap();
        std::fs::write(target_dir.join("build.log"), "log").unwrap();

        let tree = scan_directory(dir.path(), 5).unwrap();
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"src"));
        assert!(!names.contains(&".git"));
        assert!(!names.contains(&"target"));
    }

    #[test]
    fn tree_state_filters_nodes() {
        let mut state = FileTreeState::new(PathBuf::from("/workspace"));
        state.set_filter("main".to_string());
        assert!(state.matches_filter("src/main.rs"));
        assert!(!state.matches_filter("src/lib.rs"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether tree_traversal_ignores_dotgit_and_target`
Expected: FAIL with "cannot find function `scan_directory`"

- [ ] **Step 3: Implement `FileTreeNode`, `FileTreeState`, and `scan_directory` in `crates/app/src/explorer/tree.rs`**

Implement directory traversal ignoring hidden/build folders (`.git`, `target`, `node_modules`, `.superpowers`), tree row hierarchy calculation, and filter matching.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether tree_traversal_ignores_dotgit_and_target`
Expected: PASS

- [ ] **Step 5: Verify all tree tests pass**

Run: `cargo test -p aether explorer::tree`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/explorer/tree.rs crates/app/src/explorer/mod.rs
git commit -m "feat(explorer): implement FileTreeState and directory scanner"
```

---

### Task 3: Quick Open (⌘P) & Workspace Grep Engine (`search.rs`)

**Files:**
- Create: `crates/app/src/explorer/search.rs`
- Modify: `crates/app/src/explorer/mod.rs`
- Test: `crates/app/src/explorer/search.rs:tests`

**Interfaces:**
- Produces: `struct QuickOpenItem` with `path: PathBuf`, `rel_path: String`, `score: i64`.
- Produces: `fuzzy_match_path(query: &str, candidate: &str) -> Option<i64>`.
- Produces: `struct GrepMatch` with `path: PathBuf`, `line_number: usize`, `line_text: String`, `match_indices: (usize, usize)`.
- Produces: `struct GrepOptions` with `query: String`, `case_sensitive: bool`, `whole_word: bool`, `is_regex: bool`.
- Produces: `search_file_content(path: &Path, content: &str, options: &GrepOptions) -> Vec<GrepMatch>`.

- [ ] **Step 1: Write failing tests for fuzzy path matching and grep content search**

In `crates/app/src/explorer/search.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matching_prefers_exact_basename_and_contiguous_matches() {
        assert!(fuzzy_match_path("app", "crates/app/src/app.rs").is_some());
        assert!(fuzzy_match_path("gpui", "crates/ui/src/lib.rs").is_none());

        let score_direct = fuzzy_match_path("app", "crates/app/src/app.rs").unwrap();
        let score_sparse = fuzzy_match_path("app", "crates/agent/src/protocol.rs").unwrap();
        assert!(score_direct > score_sparse);
    }

    #[test]
    fn grep_finds_lines_with_correct_numbers_and_spans() {
        let content = "use crate::app;\npub fn new_chat_in() {}\nlet x = 1;\n";
        let opts = GrepOptions {
            query: "new_chat_in".to_string(),
            case_sensitive: true,
            whole_word: true,
            is_regex: false,
        };
        let matches = search_file_content(Path::new("app.rs"), content, &opts);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_number, 2);
        assert_eq!(matches[0].line_text, "pub fn new_chat_in() {}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether fuzzy_matching_prefers_exact_basename`
Expected: FAIL with "cannot find function `fuzzy_match_path`"

- [ ] **Step 3: Implement fuzzy matcher and grep scanner in `crates/app/src/explorer/search.rs`**

Implement `fuzzy_match_path` with character bonus scoring for boundaries and extensions, and `search_file_content` with regex and literal matching.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether fuzzy_matching_prefers_exact_basename`
Expected: PASS

- [ ] **Step 5: Verify all search tests pass**

Run: `cargo test -p aether explorer::search`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/explorer/search.rs crates/app/src/explorer/mod.rs
git commit -m "feat(explorer): implement fuzzy quick-open matcher and workspace grep searcher"
```

---

### Task 4: Explorer State & Tabbed View Component (`view.rs`)

**Files:**
- Create: `crates/app/src/explorer/view.rs`
- Modify: `crates/app/src/explorer/mod.rs`
- Test: `crates/app/src/explorer/view.rs:tests`

**Interfaces:**
- Produces: `enum ExplorerSidebarTab { Files, Grep }`.
- Produces: `struct ExplorerState` managing:
  - `tabs: Vec<EditorBuffer>`
  - `active_tab: Option<usize>`
  - `tree_state: FileTreeState`
  - `sidebar_tab: ExplorerSidebarTab`
  - `quick_open_open: bool`
  - `quick_open_query: String`
  - `grep_query: String`
  - `grep_results: Vec<GrepMatch>`
- Produces: `ExplorerState::open_file(&mut self, path: PathBuf) -> std::io::Result<()>`.
- Produces: `ExplorerState::close_tab(&mut self, index: usize) -> bool`.
- Produces: `ExplorerState::save_active(&mut self) -> std::io::Result<()>`.
- Produces: `ExplorerState::active_folder(&self) -> Option<PathBuf>`.

- [ ] **Step 1: Write failing tests for `ExplorerState` tab management and active folder resolution**

In `crates/app/src/explorer/view.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_state_manages_tabs_and_resolves_active_folder() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("crates").join("app");
        std::fs::create_dir_all(&sub).unwrap();
        let file = sub.join("app.rs");
        std::fs::write(&file, "code").unwrap();

        let mut state = ExplorerState::new(dir.path().to_path_buf());
        state.open_file(file.clone()).unwrap();
        assert_eq!(state.tabs.len(), 1);
        assert_eq!(state.active_tab, Some(0));
        assert_eq!(state.active_folder(), Some(sub));

        // Opening same file selects existing tab without duplicating
        state.open_file(file.clone()).unwrap();
        assert_eq!(state.tabs.len(), 1);

        assert!(state.close_tab(0));
        assert_eq!(state.tabs.len(), 0);
        assert_eq!(state.active_tab, None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether explorer_state_manages_tabs_and_resolves_active_folder`
Expected: FAIL with "cannot find type `ExplorerState`"

- [ ] **Step 3: Implement `ExplorerState` and GPUI rendering in `crates/app/src/explorer/view.rs`**

Implement tab bar rendering (with `●` dirty dot, close button `✕`), editor canvas with line numbers gutter and status bar, Quick Open (`⌘P`) modal overlay, and Sidebar dual-mode tab switcher (`Files` vs `Grep`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether explorer_state_manages_tabs_and_resolves_active_folder`
Expected: PASS

- [ ] **Step 5: Verify all view unit tests pass**

Run: `cargo test -p aether explorer::view`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/explorer/view.rs crates/app/src/explorer/mod.rs
git commit -m "feat(explorer): implement ExplorerState and UI components"
```

---

### Task 5: App Integration, Sidebar Navigation (`Page::Explorer`), and Agent Dispatch

**Files:**
- Modify: `crates/app/src/app.rs:54-62, 470-520, 2570-2615, 3740-3770`
- Modify: `crates/app/src/actions.rs`
- Test: `crates/app/src/app.rs:tests`

**Interfaces:**
- Consumes: `ExplorerState` from Task 4.
- Produces: `Page::Explorer` in `crates/app/src/app.rs`.
- Produces: `ToggleExplorer` action in `crates/app/src/actions.rs`.
- Produces: `ChatApp::toggle_explorer_page(&mut self, window: &mut Window, cx: &mut Context<Self>)`.
- Produces: Folder-scoped agent launch from Explorer triggering `this.new_chat_in(folder, window, cx)`.

- [ ] **Step 1: Write failing test for `Page::Explorer` navigation and toggle**

In `crates/app/src/app.rs` under tests:
```rust
#[test]
fn explorer_page_toggles_and_preserves_previous_page() {
    let mut page = Page::Chat;
    let mut prev = Page::Chat;

    // Toggle to Explorer
    if page == Page::Explorer {
        page = prev;
    } else {
        prev = page;
        page = Page::Explorer;
    }
    assert_eq!(page, Page::Explorer);
    assert_eq!(prev, Page::Chat);

    // Toggle back
    if page == Page::Explorer {
        page = prev;
    } else {
        prev = page;
        page = Page::Explorer;
    }
    assert_eq!(page, Page::Chat);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether explorer_page_toggles_and_preserves_previous_page`
Expected: FAIL with "no variant `Explorer` on enum `Page`"

- [ ] **Step 3: Update `crates/app/src/app.rs` and `crates/app/src/actions.rs`**

1. Add `Explorer` variant to `Page` enum.
2. Add `ToggleExplorer` and `QuickOpen` actions.
3. In `crates/app/src/app.rs`:
   - Initialize `explorer: Entity<ExplorerState>` in `ChatApp::new`.
   - Add `.nav(IconName::Folder, "Explorer", self.page == Page::Explorer, cx.listener(|this, _, window, cx| this.toggle_explorer_page(window, cx)))` to sidebar.
   - Render `ExplorerView` when `self.page == Page::Explorer`.
   - Wire `Open Agent in this folder` buttons to invoke `this.new_chat_in(folder, window, cx)` and switch to `Page::Chat`.
   - Wire `⌘P` action to trigger Quick Open modal in Explorer or globally.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether explorer_page_toggles_and_preserves_previous_page`
Expected: PASS

- [ ] **Step 5: Run full project compilation and test suite**

Run: `cargo check -p aether && cargo test -p aether`
Expected: PASS with 0 errors.

- [ ] **Step 6: Update `features.md`**

Mark `- [x] Global File Explorer` in `features.md`.

- [ ] **Step 7: Commit**

```bash
git add crates/app/src/app.rs crates/app/src/actions.rs features.md
git commit -m "feat: integrate Global File Explorer into Aether navigation and folder agent dispatch"
```
