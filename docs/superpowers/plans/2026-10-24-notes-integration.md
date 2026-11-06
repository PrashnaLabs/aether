# Notes Integration Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans to implement task-by-task.

**Goal:** Port shellkit notes into Aether with native and Neovim/tmux editing.
**Architecture:** Shared Rust file service, two editor routes, dedicated Notes page using the existing Explorer canvas and terminal. Settings remain in core.
**Tech Stack:** Rust, GPUI, chrono, serde, std::fs/process; existing terminal and Markdown components.
**Spec:** docs/superpowers/specs/2026-10-24-notes-integration-design.md

## Global Constraints
- Existing ~/notes layout and ~/.last_note remain compatible.
- Native operations never invoke notes.sh/fzf/bat/rg.
- No live user-note mutations or live Git sync during verification.
- Do not replace the agent project's Explorer when opening Notes.
- Tests follow GLOSSARY.md paths.

## Review Focus
- Reject traversal and symlink escapes on creation, restore, rename, and templates.
- Preserve dirty native buffers when switching roots or editing externally.
- Shell metacharacters in filenames never execute.
- Late searches cannot overwrite new root/scope results.
- Missing Neovim/tmux and Git conflicts surface actionable errors.

### Task 1: File service
Files: crates/app/src/notes/store.rs, tests/unit/notes/store.rs.
Interface: Store::new(root: PathBuf), scope(project: Option<&str>), create(kind, name, template), list(query), rename, trash, restore, archive, clean_empty, last, remember, sync.
- [x] Write fixture tests for layouts, nonoverwrite/templates, traversal, symlinks, trash collision and search.
- [x] Implement native operations and run `cargo test -p aether notes::store`.

### Task 2: Settings
Files: crates/core/src/settings.rs, core/lib.rs, app/settings_view.rs, settings_search.rs, core settings tests.
Interface: Settings.notes: NotesSettings { directory, editor, project_scope, nvim, tmux, session, socket }. NotesEditor::{Aether,Neovim}.
- [x] Add persistence/default tests, implement serde defaults and settings UI under Tools with searchable entries.
- [x] Test core settings and settings search.

### Task 3: External editor
Files: crates/app/src/notes/external.rs, tests/unit/notes/external.rs.
Interface: prepare(settings, root, path) -> Result<String,String>, returning safely quoted terminal command after process handoff; tmux/socket reuse and nvim fallback.
- [x] Test quoting and argument construction using fixtures/fake executables.
- [x] Implement subprocess operations off UI thread, with no destructive respawn.

### Task 4: Notes destination and menus
Files: notes/mod.rs, notes/view.rs; app/{mod,new,render,sidebar}.rs, actions.rs, palette.rs; explorer/{buffers,tree,editor}.rs; composer message menus.
Interfaces: ChatApp::open_notes, open_note, save_to_note; NotesModel owns search/name inputs, isolated ExplorerState and TerminalWorkspace.
- [x] Add GUI tests for navigation, create/open, context actions, stale search, preview/edit and project Explorer preservation.
- [x] Implement Notes toolbar/list/search, creation dialogs, maintenance actions and dedicated terminal route.
- [x] Wire sidebar/palette/shortcut and editor/chat context menus.
- [x] Run notes, settings, Explorer tests and `cargo build -p aether`.

### Task 5: Review
- [x] Review spec coverage and complete uncovered paths.
- [x] Run focused tests, formatting and diff checks; record outcomes below.

## Execution ledger
- Approved design; implementation in shared workspace. No automatic commits: user has requested commits explicitly after each prior feature.

- Implemented shared file service, Notes UI, menus, settings, and external terminal route.
- Ruling: keep the app module boundary rather than introduce a new workspace crate; all file/process operations stay behind the notes service.
- Ruling: reuse a live Neovim server's actual tmux window/session, rather than create an empty window when the configured session differs. The explicit server/socket owns live-editor routing.
- Ruling: capture appends exact text to existing notes, and native maintenance requires clean buffers.
- Verification: notes-related app suite passes (38 tests), core Notes persistence tests pass (2), Explorer suite passes (12), settings UI suite passes (11). GUI fixtures never write user notes or ~/.last_note; external fixtures never touch real tmux sessions.
- Normal app build passes. Only the pre-existing block v0.1.6 future-compatibility warning remains.
- Final review fixed Notes Quick Open isolation and scoped Archive discovery; additional regression tests cover delayed native opens, capture, disk conflicts, and folders.

- User refinement: Neovim/tmux opens a single command tab named `notes` inside the global terminal; removed the separate Notes terminal workspace. Direct server reuse attaches a remote UI when needed.
