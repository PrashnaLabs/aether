# Notes

Open **Notes** from the sidebar, command palette, or **Alt+N**. The default
folder is `~/notes`, shared with shellkit's notes script; no import is needed.
Configure the folder and editor under **Settings → Tools → Notes**.

## Editors

- **Aether** opens notes in the native Explorer editor, with Markdown Preview/Edit
  icons, save shortcuts, dirty-buffer prompts, and disk-conflict handling.
- **Neovim / tmux** opens a tab named `notes` in the global terminal and reuses the existing
  Neovim server. Without tmux, Neovim runs directly in that tab; subsequent opens reuse its server.
  Set executable overrides and the session/socket in Notes settings if needed.

A note's context menu can choose either editor regardless of the default.
Opening Notes keeps the agent project's Explorer and buffers separate.

## Finding and writing

Use the tree’s New note icon or the Notes header menu for Daily and Scratchpad. The scope button switches between global
notes and the current project's `projects/<name>` tree. The header menu contains documents,
timestamped notes, credential/password notes, Last note, Templates, Archive,
Trash, cleanup, and manual Git sync. Templates use the existing `.templates`
folder; document creation accepts a named template.

The sidebar always shows the Explorer folder tree. Use Cmd/Ctrl+P for Quick
Open. Press **Cmd/Ctrl+Shift+N** from any page for global Notes search.
Search filenames or select **Grep** (also `/s query`) to search content.
Results show a Markdown preview or matching line context. Use Up/Down to preview,
Enter to open at the matching line, and Escape to return.
The header menu opens Templates, Archive, and Trash as folders in the
same tree.

Right-click a chat reply/message or an editor selection to **Save to note**.
Selected text is retained exactly and appends when the destination already
exists. Folder menus can create notes for a project or inside the notes tree.

## Maintenance

Deletion moves files to `.trash`; restore preserves original folders for notes
trashed by Aether and supports legacy script entries. Empty trash requires
confirmation. Archive preserves paths under `archive`. Unsaved notes must be
saved before maintenance or changing the notes directory.

Git sync is manual: stage, commit, then pull/rebase and push when origin exists.
It uses the notes repository's existing ignore rules and surfaces errors or
conflicts without discarding local changes.
