# Global File Explorer, In-App Editor, Quick Open (⌘P) & Grep (⌘⇧F) — Design

Date: 2026-10-16
Status: approved design

## Intent and why

Aether currently provides a persistent Global Terminal (`Page::Terminal`), agent management (`Page::Agents`), skills (`Page::Skills`), and chat sessions. However:
1. **No Workspace File Explorer**: Users have to switch outside Aether (to Finder or an external editor/terminal) to explore directory trees, locate files, or inspect project structures.
2. **No In-App File Editing & Quick Saving**: Users cannot quickly view or tweak files, configs, or source code inside Aether without opening an external IDE.
3. **Friction Launching Agents in Subfolders**: While Aether supports scoped agent chats (`new_chat_in(folder)`), there is no visual way to browse to a specific directory or crate (e.g. `crates/app` or a microservice folder) and launch an agent right there.
4. **Missing VS Code Navigation Conveniences**: Essential developer navigation workflows like Quick Open fuzzy file search (`⌘P`), workspace-wide text grep (`⌘⇧F`), and in-file search (`⌘F`) are absent from the global navigation.

This design introduces a first-class **Global File Explorer** with an integrated **in-app Editor**, **Quick Open (⌘P)**, **Workspace Grep (⌘⇧F)**, and seamless **Folder-Scoped Agent Integration**.

## Goals

### 1. Global Navigation & Dedicated Page (`Page::Explorer`)
- Add `Page::Explorer` to `crates/app/src/app.rs`.
- Add a new "Files" / Explorer navigation item in the left sidebar nav (using `IconName::Folder` or `IconName::FolderTree`), situated alongside `Terminal`, `Agents`, `Skills`, and `Settings`.
- Global hotkey / action toggle: `⌘⇧E` or clicking the nav item toggles between the previous page and the Explorer page.

### 2. Workspace File Tree
- Collapsible recursive directory tree rooted at `workspace_root` (falling back to current project or `$HOME`).
- Filtering / file search input at the top of the tree.
- Git status indicators next to files and directories (`M` modified, `A` added, `D` deleted, `U` untracked).
- Quick folder actions on hover & right-click:
  - **`🤖 Open Agent in this folder`**: invokes `new_chat_in(folder_path)` to launch or switch to an agent scoped to that folder.
  - **`🖥 Open Terminal here`**: opens/switches to Global Terminal with cwd set to this folder.
  - **`Reveal in Finder` / `Copy Path`**.

### 3. Full-Fledged In-App Editor
- **Multi-File Tab Bar**:
  - Open multiple files in tabs at the top of the editor pane.
  - Active tab highlighting, close button (`✕`), and dirty state tracking dot (`●`).
- **Editor Canvas**:
  - Gutter with line numbers and git change indicator strip.
  - Monospaced text editing surface supporting syntax-aware presentation, selection, and keyboard editing.
  - In-file find bar (`⌘F`) with match count and next/prev navigation.
- **Saving & Dirty State**:
  - Keyboard shortcut: **`⌘S`** saves changes to disk.
  - Top action toolbar button: **`Save (⌘S)`** lights up when dirty; **`Revert`** button to discard changes back to disk version.
  - Dirty dot in the tab bar and next to the filename in the file tree while unsaved changes exist.
  - Unsaved changes confirmation dialog if closing a dirty tab or navigating away.
- **Status Bar**:
  - Line number & column cursor indicator (`Ln X, Col Y`).
  - Indentation (`Spaces: 4` / `Tabs`).
  - File encoding (`UTF-8`), language mode (Rust, Markdown, TOML, JSON, etc.), and Git branch name.

### 4. Quick Open File Finder (`⌘P`)
- Global modal palette accessible anywhere in the app via **`⌘P`** (or top search pill).
- Fuzzy search across all workspace file paths (ignoring `.git/`, `target/`, `node_modules/`).
- Keyboard navigation: `Up` / `Down` arrows to navigate list, `Enter` to open file in editor.
- **Agent Integration in Quick Open**:
  - Option to open agent in that file's folder via `Option+Enter` or clicking the `🤖 Open Agent` action badge on any search row.

### 5. Workspace Grep & Text Search (`⌘⇧F`)
- Search tab in the Explorer sidebar (toggled via `⌘⇧F` or tab pill).
- Workspace-wide text search powered by asynchronous background search (or `grep`/`ripgrep`).
- Search options: Match Case (`Aa`), Match Whole Word (`\b`), Regular Expressions (`.*`), and include/exclude file glob patterns.
- Results grouped by file path with match counts and clickable line preview snippets that jump directly to that line and highlight it in the editor.

### 6. Deep Agent Integration Across Explorer
- Prominent **`[🤖 Open Agent in folder]`** button in the editor header, reflecting the parent directory of the open file.
- **`[💬 Ask Agent about this file]`** button: switches to an agent chat with the file path (and optional selected line range) pre-populated in the composer.
- External editor handoff: **`[Open in Editor]`** utilizing Aether's existing `open_in` discovery (Zed, VS Code, Cursor, etc.).

## Non-goals

- Not building an LSP language server client (diagnostics, completion popups, go-to-definition) in this phase. The editor focuses on fast browsing, viewing, quick editing, and saving.
- Not replacing terminal-based Neovim/tmux workflows; the persistent Global Terminal remains available alongside the Explorer.
- Not supporting binary file editing (images and binary files show metadata / preview when possible, or an "Open in External App" prompt).

## Anti-goals

- Do not block the GPUI main thread during large workspace indexing, file tree walks, or global grep. All disk I/O and text searches must run on background executor threads (`cx.background_executor().spawn`).
- Do not lose unsaved edits when switching between pages (e.g. Chat -> Explorer -> Terminal); tabs and in-memory buffer edits must persist in application state.
- Do not spawn external agent processes without setting their working directory (`cwd`) strictly to the chosen folder.

## Constraints

- Built in Rust using GPUI Kit (`gpui`, `gpui_kit`, `aether_ui`, `crates/app`).
- Follow Catppuccin theme styling and Aether design system tokens.
- Comply with sandbox file access permissions within the workspace root.
- Reuse existing `new_chat_in` in `crates/app/src/app.rs` for launching folder-scoped agents.

## Decided how (from conversation & prototype feedback)

1. **Top-Level Navigation**: Explorer is a dedicated nav page (`Page::Explorer`) in the sidebar, rather than a narrow slide-out panel, providing ample width for code viewing, editing, and grep results.
2. **Multi-Tab Editor with Dirty Dot**: Visual dirty indicators (`●`) on the active tab and tree row, cleared upon `⌘S` save or `Revert`.
3. **Quick Open Modal (⌘P)**: Floating centered modal for instant fuzzy file jumping with secondary shortcut to open an agent in that file's folder.
4. **Dedicated Grep View (⌘⇧F)**: Tabbed sidebar switch (`Files` vs `Grep`) with collapsible grouped file results and line context.
5. **Folder Agent Triggers Everywhere**: Top toolbar, tree row hover buttons, right-click context menu, and Quick Open rows all allow launching an agent in that specific folder.

## Technical Architecture & Builder Decisions

1. **Editor Implementation**:
   - Backed by GPUI `EditorState` / `TextareaState` per tab with an `EditorBuffer` struct storing path, baseline text, current buffer, scroll offset, and cursor position.
   - Gutter renders line numbers dynamically based on line count and scroll position.
2. **Workspace Indexing & Quick Open (⌘P)**:
   - Traversed on background thread using async directory walks, caching the relative paths in memory.
   - Excludes standard ignore directories (`.git`, `target`, `node_modules`, `.superpowers`).
   - Fuzzy scoring ranks matches based on contiguous substring matching and basename priority.
3. **Global Grep (⌘⇧F)**:
   - Dispatches streaming background search via async background executor tasks, yielding results in chunks of 50 matches so UI stays responsive.
4. **External File Modification**:
   - If a file changes on disk while open: if buffer is clean, reloads silently; if dirty, retains local edits and presents an inline "File modified on disk" banner with `Compare` / `Overwrite` / `Reload` options.
5. **Folder Agent Dispatch**:
   - Calls `this.new_chat_in(folder_path, window, cx)` and automatically switches `self.page` to `Page::Chat`.
