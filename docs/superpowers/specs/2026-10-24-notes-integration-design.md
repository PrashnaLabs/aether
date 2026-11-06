# Notes integration

## Intent

Integrate shellkit's personal notes workflow into Aether in Rust. Both native
Aether editing and Neovim/tmux operate on the existing plain-text notes tree.
No Bash script, fzf, bat, or ripgrep executable is required for native notes.

Reference: `/Users/iris/Downloads/xzx/shellkit/notes/notes.sh` and its README.

## Approach

Use one Rust notes service with two opening methods. The service owns paths,
creation, templates, discovery, search, and maintenance; opening a note selects
Aether or Neovim/tmux. This preserves compatibility with the existing script.

Alternatives considered: invoking notes.sh would retain shell dependencies and
would not satisfy the native Rust requirement. A separate Aether notes database
would split the two editing methods and require import/export. Neither fits the
requested integration, so the shared file tree is the proposed approach.

## Files and workflows

Default root: `~/notes`, configurable without moving existing files.
Preserve `misc`, `docs`, `daily`, `scratchpad`, `creds`, `projects`, `archive`,
`.trash`, and `.templates`. Support md, txt, json, and text files.

Port the script's workflows: named and timestamped notes, today's daily note,
scratchpad entries with timestamp headers, named/default templates, recent and
last note, file/content/tag search, wiki-link backlinks, project scopes,
rename/move, trash/restore, archive by age, empty-note cleanup, and manual Git
sync. Keep existing credential/password filename conventions as plain files.
Keep `~/.last_note` compatible; opening a note updates it.

Global bare names resolve to misc; project bare names resolve to that project's
root. Daily notes use daily/YYYY-MM-DD.md; scratchpads use
scratchpad/YYYY-MM-DD.md. Creation never replaces nonempty existing content.
Templates follow the script's creation rules. Trash accepts legacy entries and
records original paths for newly trashed files so restoration preserves folders.
Do not overwrite conflicting restore destinations.

## Native Aether method

Add a Notes destination with a notes-root Explorer, searchable note list,
project/global scope selector, and actions for New, Daily, Scratchpad, Recent,
Last, Templates, Archive, Trash, and Sync. Reuse the Explorer editor and native
Markdown preview, save shortcuts, dirty-tab confirmation, and disk-conflict
handling. Notes navigation must not replace the active agent project's root.

Notes context menus offer open using either editor, rename/move, archive,
trash, copy path, and reveal. Explorer folder menus offer New note here.
Chat reply and editor-selection context menus offer Save to note, with a name
and global/project destination prompt; retain selected text exactly.

## Neovim/tmux method

Use Aether's terminal infrastructure to attach a dedicated notes terminal to
tmux session/window `notes:notes`. Launch Neovim with `--listen` at the script's
socket default (`$XDG_RUNTIME_DIR/notes-nvim.sock`, else `/tmp/notes-nvim.sock`).
Reuse a responding Neovim server through argument-based remote commands.
Reuse the notes terminal instead of opening a terminal for every file.
Without tmux, run Neovim directly in the notes terminal, matching the script's
fallback. Missing Neovim is an actionable UI error with native editing available.
Never kill an unrelated tmux window to open a note.

Rust implements file operations; external processes are limited to Neovim,
tmux, and explicitly requested Git sync. Validate user paths and quote only at
terminal/shell boundaries; do not build executable commands from note text.

## Settings

Add a searchable Notes settings section:

- Notes directory (default ~/notes).
- Default editor: Aether or Neovim/tmux (proposed default Aether).
- Default scope: global or current project (proposed default global).
- Neovim and tmux executable overrides, with PATH defaults.
- tmux session name and Neovim socket override, with script-compatible defaults.

Changing the root requires resolving dirty native buffers before switching.
Existing settings files deserialize with defaults. Editor overrides remain
available in note context menus regardless of the selected default.

## Reliability and verification

Run discovery/search and process operations away from the UI thread. Guard
results when root, scope, or query changes. Scope file operations to the notes
root, including traversal and symlink checks. Preserve dirty editor text when
external edits arrive. Git sync runs only from an explicit action and surfaces
conflicts/errors without resetting the notes tree.

Test path/layout compatibility, templates, scratchpad/daily idempotence, search,
trash/restore collisions, stale results, settings persistence, command arguments,
and both editor routing paths. GUI checks exercise Notes navigation, context
menus, Markdown preview/edit, and preservation of the agent project's Explorer.
Build the app and run focused affected suites. Do not invoke live sync, delete
user notes, or alter the user's tmux sessions during verification.

## Scope boundary

No note-format migration, database, cloud sync service, or AI indexing. Existing
Markdown chat/plan rendering remains shared with note previews. The default
editor and scope above are implementation proposals awaiting review.
