# Explorer upgrade: speed, edit UX, agent file links

Goal: the chat Explorer (Browse Files) stays smooth on big repos, edits like a real editor, and
file paths in agent replies open in it at the right line instead of in Finder.

## Decisions (settled with the user)

1. A file link in a reply switches that chat to the Explorer and opens the file at its line. A
   folder link reveals the folder in the tree. Non-file links (http, mailto) keep opening in the OS.
2. Agent edits a file that is open in a tab: a clean tab reloads silently and keeps its cursor; a
   dirty tab shows a "Changed on disk: Reload / Keep mine" banner, and saving while that banner is
   up asks before overwriting. Checks run when an agent tool step finishes and when the Explorer
   comes into view. No file watcher dependency.
3. Edit UX: one editor state per tab (undo, cursor, scroll survive tab switches); go-to-line from
   grep hits and links; Ln/Col in the status bar; save failures are shown, never swallowed; binary
   (NUL in first 8 KB) and large (> 10 MB) files show a message instead of failing silently;
   tree file ops: New File, New Folder, Rename, Delete (to Trash via `/usr/bin/trash` or
   `gio trash`, confirmed; if neither tool exists, refuse with a message, never hard-delete).
   No auto-save.
4. Tree shows everything except `.git`, loads a folder's children only when it is expanded, and
   dims git-ignored entries. Quick Open and grep use `git ls-files -co --exclude-standard -z`
   (fallback for non-git folders: a walk that skips `target`, `node_modules`, `.git`, `dist`,
   `build`, `.idea`, `.vscode`, `.superpowers`).
5. Inline code in replies that names an existing file (`` `src/foo.rs:42` ``) becomes a file link.
6. Agent integration extras: "Open in Explorer" on Read/Edit tool rows and on rows of a reply's
   changes card; "Ask Agent" with a selection inserts `@rel/path:L10-20` plus the selected lines
   in a fenced block; grep runs as you type, debounced ~200 ms, on the background executor,
   stale runs dropped.

## Shared contract (already in the tree, keep these signatures)

- `ChatApp::open_in_explorer_at(path: PathBuf, line: Option<usize>, window, cx)`
  (`crates/app/src/explorer/mod.rs`): switch to Explorer, folder -> `ExplorerState::reveal`,
  file -> `open_explorer_file_at`. `open_in_explorer(path, ..)` = `open_in_explorer_at(path, None, ..)`.
- `ChatApp::open_explorer_file_at(path, line: Option<usize>, window, cx)`
  (`crates/app/src/explorer/buffers.rs`): open as tab, cursor on 1-indexed `line`.
  `open_explorer_file(path, ..)` = `open_explorer_file_at(path, None, ..)`.
- `ExplorerState::reveal(&mut self, path: &Path)` (`crates/explorer/src/view.rs`): expand
  ancestors up to the root and select `path`.

## Work split (file ownership)

### A. Sidebar: tree, Quick Open, grep, file ops (worktree)

Owns `crates/explorer/src/{tree.rs,search.rs,lib.rs}`, the tree / grep / quick-open parts of
`crates/explorer/src/view.rs`, any new files in `crates/explorer/src/`,
`crates/app/src/explorer/{tree.rs,grep.rs,quick_open.rs,mod.rs}`, the quick-open and grep
subscriptions in `crates/app/src/app/new.rs`, and their tests.

- Lazy tree: children loaded per folder on expand, flattened visible rows cached and rebuilt only
  when expand state or listing changes; refresh re-lists loaded folders only. No deep clone per
  frame. Render with `uniform_list` (virtualized). Keep keyboard nav and the a11y roles.
- Opening/revealing a file selects it in the tree and scrolls it into view.
- File index from git (decision 4) loaded on the background executor; ignored set for dimming.
- Quick Open results computed on query change and cached, not in render.
- Grep: compile the regex once per run, not per line; run on the background executor over the
  index with a generation counter; live as you type (decision 6); clicking a hit calls
  `open_explorer_file_at(path, Some(line), ..)`.
- File ops (decision 3) in the tree context menu; new file opens in a tab; rename retargets an
  open tab; delete closes its tab. Each failure is shown to the user.

### B. Editor: tabs, buffers, reload, status (main tree)

Owns `crates/explorer/src/buffer.rs`, the tab parts of `crates/explorer/src/view.rs`,
`crates/app/src/explorer/{editor.rs,buffers.rs}`, editor fields in `crates/app/src/app/{mod.rs,new.rs}`,
`crates/app/src/app/chat/mod.rs` (explorer sync on chat switch), the save action in
`crates/app/src/app/render.rs`, the agent-edit hook in `crates/app/src/app/chat/send.rs`,
`ask_agent_about_file` in `crates/app/src/app/sidebar.rs`.

- One `Entity<EditorState>` per open tab, created on open, dropped on close; no `set_value` on
  tab switch. Keep `open_explorer_file_at` working with it (cursor lands on the line and is
  scrolled into view, also for a file opened for the first time).
- Stop cloning every tab's text in render. Git diff for the active file computed once per
  (file, git status change) off the UI thread and cached; gutter markers and the diff view read
  the cache.
- Decisions 2, 3 (editor parts), 6 (Ask Agent with selection; path relative to the chat's
  folder, not the workspace root).
- Agent-edit hook: when a chat's `Update::Tool` step finishes, re-check that chat folder's open
  tabs against disk (and refresh git status).

### C. Agent file links (worktree)

Owns `crates/ui/src/chat/*` (new `links.rs`), `crates/ui/src/lib.rs` exports, and the app wiring
where the link handler is installed (app startup) plus `crates/app/src/side_panel.rs` if needed.

- `parse_file_link(href, cwd) -> Option<FileLink { path, line, col }>`: `file://` (percent
  decoded), absolute, or cwd-relative paths; suffixes `:12`, `:12:3`, `:12-20`, `#L12`,
  `#L12-L20`, `#L12C3`; must exist on disk; anything with another scheme is `None`.
- Markdown in replies: `TextView::on_link_click` resolves file links and opens them through a
  `Global` handler the app installs (calls `open_in_explorer_at`); everything else, and any link
  in a non-agent chat, falls back to `cx.open_url`.
- Decision 5: rewrite inline code naming an existing file into a link; never inside fenced code
  or existing links; no filesystem calls per frame for unchanged text.
- Decision 6: "Open in Explorer" on Read/Edit tool rows and changes-card rows.

## Verify

- `cargo test -p aether-explorer -p aether-ui -p aether` and `cargo clippy --workspace` clean.
- Run the app on this repo: expand `target/` without a stall; type in Quick Open and grep without
  lag; switch tabs and undo still works; click a file link in a reply and land on its line.
