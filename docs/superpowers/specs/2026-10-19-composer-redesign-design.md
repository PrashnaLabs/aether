# Composer redesign (variant B), projects, attachments, Ctrl+G editor

Date: 2026-10-19. Visual reference: `crates/ui/prototype/composer-redesign.html`. Open it in a browser and use the top bar to switch between Start, Messages and Scrolling, and between light and dark. Only variant B is in scope. Variants A and C are hidden in the same file and were rejected. Terms follow `GLOSSARY.md` (Project, Agent chat, Direct chat, Draft, Attachment). The attachment storage decision is recorded in `docs/adr/0001-attachments-stored-as-copied-files.md`.

## Intent

The composer feels bloated. An agent chat's toolbar carries up to seven dropdowns and toggles, and none of them look current. The user benchmarked ChatGPT and T3 Code. This slice rebuilds the composer as one calm card:
- The agent, model and reasoning collapse into one picker.
- The workspace and context move into a tray under the card.
- When you scroll back, the composer gets out of the way.

It also adds what the new composer needs to be complete:
- a real Project concept for the start screen;
- file and image Attachments, including pasting from the clipboard;
- a Ctrl+G editor for long Drafts, like Claude Code's CLI.

## Goals

1. **Composer card (variant B):** an input card with a single row of quiet controls, a tray under it, a resting style while you read, and a compact bar while you scroll back.
2. **Start screen:** "What should we build in <project>?" with a project switcher and the project's recent chats.
3. **Projects:**
   - a saved list, so a folder with no chats can still be a Project;
   - "Add project…" in the switcher and in the sidebar;
   - Cmd+N starts in the current Project.
4. **Attachments:**
   - add them with the paperclip, drag and drop, pasting an image, or pasting files copied in Finder;
   - shown in the composer and in the transcript;
   - delivered to each agent in the best form it accepts.
5. **Ctrl+G** opens the Draft in a full-height editor overlay that uses the app's own code editor.

## Non-goals

- Banners above the card stay as they are today: usage limits, "Running in another terminal", the worktree note, the setup note, queued messages and "Editing your message". They keep their current position and content. Restyling them as a T3-style drawer is a later slice.
- The running state is unchanged apart from where things now sit: the stop button and the steer/queue behaviour stay as they are.
- No nvim or `$EDITOR` option for Ctrl+G yet. It may come later as a setting.
- Images are not resized. Image support is not checked per model.
- We won't write our own clipboard reader. When the clipboard holds both text and an image, gpui pastes the text, and that is accepted for now.

## Anti-goals

- No modal dialogs where a popover or inline view works (`docs/ux-rubric.md`). The Ctrl+G overlay is a full-height sheet over the chat column, not a centered dialog.
- No I/O reachable from `render`. Copying files, decoding images, reading catalogs and scanning projects all run on the background executor, with results stored on the entity.
- No raw hex colours. Use theme tokens and the existing `theme::*` helpers. The prototype's colours are guidance, not values.
- The composer must not jump in height when the resting style, the compact bar or the attachment row toggles. Every change animates or stays in place.

## Constraints

- GPUI via gpui-kit 0.7.3, linking gpui-pre 0.3.9.
  - The textarea already has `on_paste(|&ClipboardItem, ..| -> bool)` (gpui-component `input/textarea.rs:163`).
  - `ClipboardEntry::Image` and `ExternalPaths` exist.
  - `div().on_drop::<ExternalPaths>` and `drag_over` exist.
  - `img()` takes `Arc<Image>` or a path.
  - gpui-component ships `Attachment` / `AttachmentGroup` chips.
- Icons must be bundled through `icon_assets!` (`crates/app/src/assets.rs`) and listed in the asset test.
- Tests go in `crates/<crate>/tests/unit/<path>.rs` (GLOSSARY.md, "Unit test file"). Pure functions get tested. Spoken labels are pulled into pure functions and tested.
- New `Settings` fields need a default and a round-trip test.
- New `Message` fields must be `#[serde(default)]`. The store drops any chat whose JSON fails to parse (`crates/agent/src/store.rs:148`).
- Key bindings go in `bind_keys` (`crates/app/src/actions.rs`) under a key context that is always present on the composer (new: `Composer`), registered after the input's own keys. `ctrl-g` is unbound today.
- Another agent works on `main`. Build in the `composer-redesign` worktree with its own `CARGO_TARGET_DIR`.

## 1. Composer card (variant B)

Replaces the `Composer` layout in `crates/ui/src/chat/composer.rs` and its assembly in `crates/app/src/app/composer/mod.rs` and `chips.rs`.

**Card.** One rounded card (radius about 24) with the textarea on top, then one control row.

The placeholder is "Ask <agent> anything, @tag files, / for commands", where <agent> is the provider's short label (for example "Claude", "Codex"). Direct chats say "Ask anything, @tag files, / for commands".

If there are Attachments, they appear in a row above the textarea (section 4).

The control row, left to right:

1. **Model picker trigger:**
   - provider icon, model name, and the reasoning level in muted text (no level shown when the provider has none or it is Default), then a caret;
   - it opens the model picker below.
2. A thin divider, then **Access:**
   - the icon and label of the current access mode, then a caret;
   - it opens the existing access menu, with each mode's description;
   - shown only when the provider has access modes.
3. **Direct chats only:** Search and Plan toggles in place of Access. The pressed state uses the accent colour.
4. Right side: **Attach** (paperclip icon button) and **Send/Stop**, keeping today's `SendButton` behaviour.

These leave the control row:
- the separate effort chip;
- the folder chip and the worktree chip (they move to the tray);
- the context ring (it moves to the tray);
- the footnote "Aether can make mistakes. Check important info.", which is removed.

**Model picker.** This extends the existing `AgentSelector` panel and keeps its rail, search, favourites and keyboard behaviour. Changes:
- Each model row shows a second line. Inside a provider tab, that line is the catalog description when there is one, otherwise the upstream name. Under Favorites or search, it shows the provider label. Rows keep the star and the current-model check.
- A **Reasoning** row of pills sits under the list. It shows Default plus the levels from `effort_options(chat)`, so it follows each model where the catalog says, otherwise each provider. If the provider has none, it shows "<provider> sets its own reasoning".
- Once the chat has messages, a lock note sits at the bottom: "This chat stays on <provider>. Start a new chat to switch agents." Other providers' rail tabs look disabled and explain the lock in their tooltip. This is today's lock, made visible.
- Picking a model closes the panel. Changing reasoning, switching tabs and starring keep it open.

**Tray.** A strip hangs under the card, inset 20px on each side, so its sides start where the card's bottom corners finish curving. It has rounded bottom corners, a slightly different surface tint from the card, and the same outline.

Agent chats:
- Left:
  - the **folder** control (Project name), which opens the project switcher (section 2), with the same disabled rules and tooltips as today's folder chip;
  - the **branch/worktree** control, which opens today's `WorkspaceSelect` menu with the same rules and tooltips.
- Right: **context**, a small ring plus "41% context", with today's `ContextRing` hover breakdown. It is hidden until the chat has usage.

Direct chats: "Direct chat, no workspace" on the left, with context on the right.

**Resting style.** It applies whenever the chat has messages and the Draft is empty (no text and no Attachments), focused or not:
- controls drop to about 22px with 70% opacity;
- the send button drops to 30px.

Typing or adding an Attachment restores full size. The change animates.

**Compact bar while scrolling back.** It applies when the user scrolls the transcript up with the wheel or trackpad (`ChatScrollerState::is_scrolled_up`). This holds even if the composer had focus or the Draft has text:
- The card folds into one 46px bar, the tray tucks away, and the composer loses focus.
- The bar shows the provider icon, then the first line of the Draft (in normal text colour) or "Reply to <agent>…" (muted), then the model and reasoning in muted text, then a "Scroll to end" button with a down arrow.

What it does:
- Clicking the bar scrolls to the end, unfolds the card and focuses the textarea.
- Clicking "Scroll to end" only scrolls. Reaching the end unfolds the card.
- Folding and unfolding animate.
- The scroller's built-in jump button is turned off, because the bar replaces it. The bottom fade stays.
- Unfolding puts focus back in the composer. The Draft is never lost by folding.

**Drafts are per chat.** Today one shared textarea carries typed text from chat to chat. With this change, each chat keeps its own Draft (text and pending Attachments):
- Switching chats saves the Draft and restores it when you come back.
- When `new_chat` reuses an empty draft chat, that chat keeps its own Draft.
- Everything that sets the textarea today must read from and write to the active chat's Draft: send, edit and cancel edit, worktree-send rejoin, and history navigation.

## 2. Start screen and Projects

**Start screen, agent chat** (replaces the `Hero` branch in `crates/app/src/app/render.rs` when the chat is an Agent chat with no messages):
- The headline is "What should we build in <project>?" at about 30px, normal weight. The project name has a dotted underline and opens the project switcher.
- If the folder is the home folder (no Project), the headline is "What should we build?" and the switcher is reached through the tray's folder control.
- Under it are the card and tray, vertically centered as a group.
- Below that is **Recent in <project>**, up to 4 of the Project's chats, most recently active first, leaving out the current Draft. Each row shows the provider icon, the title, the branch (monospace, muted) and a relative time (`projects::relative_time`). Clicking a row opens that chat. "View all" on the right of the heading opens the project page. The section is hidden when there are no other chats.

**Start screen, direct chat:** "What can I help with?" with no switcher and no recents.

**Project switcher** (one popover, used from the headline and from the tray's folder control):
- Lists every Project, most recently active first. Saved Projects with no chats come last, in alphabetical order.
- Each row shows the project mark, the name, a short path (`~/…`) and a check on the current Project.
- Last row: "Add project…" opens the native folder picker, saves the folder as a Project and selects it.
- Picking a Project repoints the current Draft's folder, with the same rules as today's `choose_folder`: only while `workspace_can_change`. The text, Attachments and agent stay. There is no remove option here.

**Saved Projects:**
- A new settings list holds the folders the user added.
- The set of Projects is the saved list plus the folders of Agent chats (today's grouping), minus hidden folders.
- A saved Project with no chats shows in the sidebar as a group header with no rows.
- The sidebar's Projects heading gets a "+" button. It opens the folder picker, saves the folder and starts a Draft in it (`new_chat_in`).
- "Remove from sidebar" keeps its current hide behaviour. Adding a hidden folder again un-hides it.

**Cmd+N** keeps today's agent choice (the last agent picked, `settings.provider`):
- If that agent is OpenRouter, Cmd+N makes a Direct chat, as it does today.
- If it's a coding agent, the new chat starts in:
  1. the current chat's Project;
  2. otherwise, from a Direct chat or with no Project, the most recently active Project;
  3. otherwise today's `workspace_root` or home fallback.

If `new_chat` reuses an existing empty Draft, that Draft is repointed to the chosen Project, under the same `workspace_can_change` rule.

## 3. Ctrl+G editor overlay

- **Opening:** Ctrl+G with the composer focused opens a full-height sheet over the chat column. The sidebar stays visible. It works in Agent and Direct chats.
- **Contents:**
  - The header reads "Edit draft".
  - The hint line reads "Esc or Ctrl+G to return · ⌘↩ to send" and is followed by a "Discard changes" button.
  - The body is the app's code editor (the same editor as the Explorer's tabs) in markdown mode with soft wrap. It holds the Draft text and is focused with the cursor at the end.
- **Closing:**
  - Esc, Ctrl+G or Cmd+S writes the editor text into the Draft, closes the sheet and focuses the composer with the cursor at the end.
  - Cmd+Enter writes the text back and sends.
  - "Discard changes" closes the sheet and leaves the Draft as it was.
- **What it doesn't touch:** Attachments. Ctrl+G does nothing while the composer is disabled.

## 4. Attachments

**Model:**
- A message gains a list of Attachments (`#[serde(default)]`). Each holds a display name, the stored copy's path, the mime type and the size.
- The current Draft holds pending Attachments in memory, per chat.

**Storage** (ADR 0001):
- Files are copied into `<config dir>/attachments/<chat id>/` when added. Pasted images are named "Pasted image N.<ext>".
- Removing a pending Attachment deletes its copy.
- Deleting a chat deletes the folder.
- On startup, folders with no matching chat are removed.
- A fork copies its source messages' Attachments into the new chat's folder.

**Ways to add, all into the same pending list:**
- The paperclip opens the native picker, files only, multiple selection.
- Dropping files on the card. The card shows a drop highlight while files hover over it.
- Pasting an image: the textarea's `on_paste` sees `ClipboardEntry::Image` and returns true.
- Pasting files copied in Finder: `ExternalPaths` on paste.
- Pasting text works as before.

Images whose format a provider may not accept (TIFF, BMP, …) are converted to PNG when added. The `image` crate is already in the dependency tree.

**Limits:**
- At most 10 per message and 20 MB per file.
- Direct chats only accept images, PDFs and text files. Any other file is refused when added.
- A refusal or going over shows an inline error line above the card:
  - "Up to 10 attachments"
  - "<name> is over 20 MB"
  - "<name>: this chat can only take images, PDFs and text files"

  It clears on the next change.

**In the composer:**
- A row inside the card, above the textarea: 56px image thumbnails and file chips (icon, name, size), each with a remove button.
- The row's appearance animates the card height.
- A message with Attachments and no text can be sent.

**In the transcript:**
- A row of 64px thumbnails and file chips sits above the user bubble.
- Clicking one opens it in its default macOS app.
- A missing file shows as a muted chip reading "File missing".

**Queue, steer, edit and retry:**
- Queued and steer messages carry their Attachments.
- Editing a sent message puts its Attachments back into the composer as pending, where they can be removed.
- Retry resends them.

**Delivery per provider:**

| Provider | Images | Other files |
|---|---|---|
| Claude Code | `image` content block, base64, beside the text block; over 5 MB goes as a path in the prompt instead | path in the prompt |
| Codex | `localImage { path }` input item | path in the prompt |
| Pi | `images` field on prompt/steer (verify against Pi's RPC; if it isn't there, use the path) | path in the prompt |
| OpenCode | path in the prompt | path in the prompt |
| Antigravity | path in the prompt | path in the prompt |
| OpenRouter | inline data part (adk `with_inline_data`) | PDFs as a file part; UTF-8 text files under 100 KB inlined as text with a filename header, larger ones as a file part; other types are refused when added |

"Path in the prompt" means appending a block to the text sent to the agent: "Attached files:", then one line per file with its absolute stored path and mime type. The transcript shows the user's own text, not this block.

Every CLI driver must be able to read the attachments folder (for example Claude Code's `--add-dir`). The builder confirms each driver's read access.

OpenRouter history rebuilt after an edit or retry stays text-only. Earlier messages' Attachments appear in that history as "[attached: name]".

## Phases

1. Composer card (section 1). Everything else plugs into it.
2. Projects and start screen (section 2).
3. Attachments and image paste (section 4).
4. Ctrl+G editor (section 3).

Phases 2 and 4 depend only on 1 and can run in parallel with 3. Each phase is committed only after the user approves it.

## Calls made in writing this (not yet confirmed by the user)

- The tray's folder control opens the project switcher, not the native folder dialog. "Add project…" inside it is the way to reach a new folder.
- The home folder (no Project) shows "What should we build?" without a clickable name.
- Saved Projects with no chats sort last, alphabetically, in the switcher.
- The sidebar "+" adds a Project and immediately starts a Draft in it.
- A message with Attachments and no text can be sent.
- Images in formats a provider may not accept are converted to PNG when added.
- Unsent pending Attachments survive only while the app runs. Their files are cleaned up on the next launch.
- OpenRouter history after an edit or retry refers to earlier Attachments by name only.
- The "Attached files" block appended for CLI agents is hidden from the transcript.

## Left to the builder

- Exact theme tokens, sizes and animation durations, staying close to the prototype.
- Icon choices within the bundled set (bundle new ones as needed).
- Module layout, entity structure and names.
- The exact Ctrl+G sheet chrome.
- Error and edge-case copy beyond what is written here.
- Test selection, following the repo's unit-test conventions.
