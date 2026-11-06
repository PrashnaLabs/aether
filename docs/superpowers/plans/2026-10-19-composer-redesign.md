# Composer redesign (B), projects, attachments, Ctrl+G Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:**
- Rebuild the chat composer as the locked variant B card: one control row, a tray, a resting style and a fold-on-scroll bar.
- Add per-chat Drafts and saved Projects, with a project switcher, recents and a smarter Cmd+N.
- Add file and image Attachments, including clipboard paste.
- Add a Ctrl+G editor overlay for the Draft.

**Architecture:**
- **ui crate (`aether-ui`):** gains pure label and mode functions plus new render pieces: composer card modes, the tray, the fold bar, the attachment strip, model-picker reasoning pills and the lock note.
- **app crate (`aether`):** owns state.
  - The per-chat `draft`, `pending` and `queue` live on `Chat`.
  - The project list helpers live in `projects.rs`.
  - Attachment file storage lives in `attachments.rs` (copied files, ADR 0001).
  - The Ctrl+G overlay lives in `app/draft_editor.rs`.
- **core:** owns the `Attachment` type, its limits and the delivery rules.
- **agent and driver:** carry attachments to each CLI. Images go natively where supported, everything else as an "Attached files" path block.
- **OpenRouter:** gets adk inline-data parts.

**Tech Stack:**
- Rust 2024, GPUI through gpui-kit 0.7.3 (links gpui-pre 0.3.9).
- sqlx sqlite, adk-rust 2.3.0.
- New workspace dependencies:
  - `image = { version = "0.25", default-features = false, features = ["png", "jpeg", "gif", "webp", "tiff", "bmp"] }`
  - `base64 = "0.22"`

  Both versions are already in Cargo.lock as transitive dependencies.

**Spec:** `docs/superpowers/specs/2026-10-19-composer-redesign-design.md`.

Other references:
- ADR: `docs/adr/0001-attachments-stored-as-copied-files.md`.
- Visual reference: `crates/ui/prototype/composer-redesign.html`. Open it in a browser. Only variant B applies.

## Global Constraints

**Build and test commands**
- Work only in `/Users/iris/Downloads/xzx/RUST/aether-wt-composer`, branch `composer-redesign`. Another agent works on `main`.
- Every cargo command runs with `CARGO_TARGET_DIR=/Users/iris/Downloads/xzx/RUST/aether-wt-composer-target`.
- Seed that target dir once with `cp -c -R /Users/iris/Downloads/xzx/RUST/aether/target/debug /Users/iris/Downloads/xzx/RUST/aether-wt-composer-target/`.
- Crate names for `cargo test -p`: `aether-ui`, `aether` (the app), `aether-core`, `aether-agent`, `aether-driver`.
- Lint gate at the end of every phase: `cargo clippy --workspace` with no new warnings.

**Commits**
- Commit only after the user approves a phase. Never commit, stage or push without that approval.
- Commit messages follow Conventional Commits in caveman-commit style.
- NEVER add a `Co-Authored-By` line or any AI attribution.

**Tests**
- Unit tests for `crates/<crate>/src/<path>.rs` live in `crates/<crate>/tests/unit/<path>.rs`, wired with `#[cfg(test)] #[path = "..."] mod tests;` at the end of the source file. The test file starts with `use super::*;`.
- There are no gpui test contexts in this repo. Tests are pure functions, or `include_str!` source checks.

**Code rules**
- No I/O reachable from `render`. File copies, image decoding, existence checks and `open` all run on `cx.background_executor()`, with results stored on the entity.
- No raw hex colours. Use theme tokens (`theme.secondary`, `theme.background`, `theme.border`, `theme.muted_foreground`, `theme.muted`, `theme.accent`) and the existing `theme::*` / `overlay` / `overlay_strong` / `field_fill` helpers.
- Animations use `gpui_kit::base::motion::{transition, Transition}` with `Transition::new(Duration::from_millis(260))`. When `cx.reduce_motion()` is true, jump straight to the end state.
- New `Settings` fields: `#[serde(default)]`, a hand-written default, and a round-trip test.
- New `Message` fields: `#[serde(default)]`. The store drops chats whose JSON fails to parse.
- Icons outside gpui-kit's default set must be listed in `icon_assets!` (`crates/app/src/assets.rs`) and in `crates/app/tests/unit/assets.rs`.

**Copy, verbatim**

| Where | Text |
|---|---|
| Composer placeholder, agent chat | "Ask <agent> anything, @tag files, / for commands" |
| Composer placeholder, direct chat | "Ask anything, @tag files, / for commands" |
| Fold bar | "Reply to <agent>…" |
| Fold bar, direct chat | "Reply…" |
| Fold bar button | "Scroll to end" |
| Tray, direct chat | "Direct chat, no workspace" |
| Tray context | "<N>% context" |
| Model picker lock note | "This chat stays on <provider>. Start a new chat to switch agents." |
| Model picker, no reasoning | "<provider> sets its own reasoning" |
| Start screen, agent chat | "What should we build in <project>?" |
| Start screen, home folder | "What should we build?" |
| Start screen, direct chat | "What can I help with?" |
| Recents | "Recent in <project>" and "View all" |
| Project switcher | "Add project…" |
| Attachment errors | "Up to 10 attachments", "<name> is over 20 MB", "<name>: this chat can only take images, PDFs and text files", "<name> is over 10 MB" |
| Pasted image name | "Pasted image <N>.<ext>" |
| Transcript, missing file | "File missing" |
| Prompt block | "Attached files:" |
| History marker | "[attached: <names>]" |
| Ctrl+G sheet | "Edit draft", "Esc or Ctrl+G to return · ⌘↩ to send", "Discard changes" |

**Limits**

| Limit | Value |
|---|---|
| Attachments per message | 10 |
| Size per file | 20 MB |
| Claude native image | 5 MB |
| Direct chat file | 10 MB (adk `with_inline_data` panics above 10 MB) |
| Inlined text file | 100 KB |

## Review Focus

1. **Deleting, duplicating or creating a chat while another chat's Draft is in the composer.** The Draft must land in the chat it was typed in, never in the chat that slid into its index. Pinned by Task 1 `drafts_are_stashed_by_chat_id_not_index`.
2. **Pasting a 15 MB Retina screenshot into a Direct chat.** It is refused on add with "<name> is over 10 MB" and never reaches adk, which panics above 10 MB. Pinned by Task 11 `limits_and_direct_chat_types_are_checked_on_add`.
3. **Pasting plain text.** It must still go into the textarea. The paste hook only handles images and files. Pinned by Task 13 `text_paste_is_left_to_the_textarea`.
4. **Chats saved before this change** (no `attachments` field in their messages JSON). They must load unchanged. Pinned by Task 11 `chats_saved_before_attachments_load_without_them`.
5. **Scrolling up while a multi-line Draft is typed.** The card folds, the bar shows the first non-empty line, and unfolding brings back the full text and focus. Pinned by Task 2 `fold_bar_shows_first_draft_line_or_reply_hint` and `mode_folds_before_resting`.

---

## Phase 1: Composer card

### Task 1: Per-chat Draft text

**Files:**
- Modify: `crates/app/src/app/mod.rs`
  - `struct Chat` (:91): add `pub(crate) draft: String`.
  - `ChatApp` (:300): add `pub(crate) draft_owner: Option<i64>`.
  - `chat_with` literal (:497): set `draft: String::new()`.
- Modify: `crates/app/src/app/chat/create.rs`: the `Chat` literals at :46, :103, :153, :227 and :294 all get `draft: String::new()`.
- Modify: `crates/app/src/app/chat/mod.rs`: the `select` method (:37), plus a new free function.
- Modify: `crates/app/src/app/worktree/setup.rs`: `setup_cancel` (:175-180).
- Modify: `crates/app/src/app/new.rs`: initialise `draft_owner` to the first active chat's id.
- Test: `crates/app/tests/unit/app/chat/mod.rs`.

**Interfaces:**
- Produces:
  - `pub(super) fn stash_draft(chats: &mut [Chat], owner: Option<i64>, text: String) -> bool`, in `chat/mod.rs`. It writes `text` into the chat whose `id == owner` and returns whether that chat was found.
  - `ChatApp::swap_draft(&mut self, window: &mut Window, cx: &mut Context<Self>)`.
  - `Chat.draft` and `ChatApp.draft_owner`. Later tasks read `chats[ix].draft` for non-active chats.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn drafts_are_stashed_by_chat_id_not_index() {
    let mut chats = vec![chat_with("a", &[]), chat_with("b", &[])];
    chats[0].id = 1;
    chats[1].id = 2;
    assert!(stash_draft(&mut chats, Some(2), "half typed".into()));
    chats.remove(0);
    assert_eq!(chats[0].draft, "half typed");
    assert!(!stash_draft(&mut chats, Some(1), "gone".into()));
    assert!(!stash_draft(&mut chats, None, "x".into()));
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test -p aether drafts_are_stashed_by_chat_id_not_index`
Expected: compile error, because `stash_draft` and `draft` don't exist yet.

- [ ] **Step 3: Implement**
  - Add the fields and `stash_draft`.
  - `select` calls `stash_draft(&mut self.chats, self.draft_owner, composer value)` before it writes `self.active` (:38). After the active chat changes, it sets the composer to `chats[ix].draft` with `set_value`, sets `draft_owner = Some(chats[ix].id)`, and keeps today's focus.
  - In `setup_cancel`:
    - when `ix == self.active`, keep today's composer path;
    - otherwise set `self.chats[ix].draft = rejoin(job.text, &self.chats[ix].draft)` instead of dropping the text.

- [ ] **Step 4: Run it and confirm it passes**

Run: `cargo test -p aether drafts_are_stashed_by_chat_id_not_index canceled_prompt_goes_before_the_current_draft`
Expected: PASS.

### Task 2: Composer card shell (ui)

**Files:**
- Modify: `crates/ui/src/chat/composer.rs` (whole `Composer` render :92-208, `SendButton` :213-327).
- Modify: `crates/ui/src/chat/mod.rs`: re-export the new items.
- Test: `crates/ui/tests/unit/chat/composer.rs`.

**Interfaces:**
- Produces:
  - `pub enum ComposerMode { Full, Resting, Folded }`, deriving `Clone, Copy, Debug, PartialEq, Eq`.
  - `pub fn composer_mode(has_messages: bool, draft_empty: bool, user_scrolled_up: bool) -> ComposerMode`.
    - Folded when `has_messages && user_scrolled_up`.
    - Otherwise Resting when `has_messages && draft_empty`.
    - Otherwise Full.
  - `pub fn composer_placeholder(agent: Option<&str>) -> String`.
  - `pub fn fold_bar_text(draft: &str, agent: Option<&str>) -> (String, bool)`. It returns the first non-empty trimmed line and `true`, or the reply hint and `false`.
  - `pub fn context_label(used: u64, total: u64) -> Option<String>`. Returns `None` when `used == 0 || total == 0`. The percent is `(used * 100 + total / 2) / total`, clamped to 100.
  - New `Composer` builders:
    - `mode(ComposerMode)`
    - `tray(impl IntoElement)`
    - `bar(impl IntoElement)` (rendered instead of the card body when Folded)
    - `attachments(impl IntoElement)` (rendered above the textarea; filled in Task 13)
  - `pub fn fold_bar(icon: Icon, text: SharedString, is_draft: bool, model: SharedString, on_expand: ClickHandler, on_jump: ClickHandler, cx: &App) -> AnyElement`.
  - `SendButton::compact(bool)`: 30px when true, 36px when false.
- Removes: `Composer::footnote` and its field.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn mode_folds_before_resting() {
    assert_eq!(composer_mode(true, true, true), ComposerMode::Folded);
    assert_eq!(composer_mode(true, false, true), ComposerMode::Folded);
    assert_eq!(composer_mode(true, true, false), ComposerMode::Resting);
    assert_eq!(composer_mode(true, false, false), ComposerMode::Full);
    assert_eq!(composer_mode(false, true, true), ComposerMode::Full);
}

#[test]
fn placeholder_names_the_agent() {
    assert_eq!(composer_placeholder(Some("Claude")), "Ask Claude anything, @tag files, / for commands");
    assert_eq!(composer_placeholder(None), "Ask anything, @tag files, / for commands");
}

#[test]
fn fold_bar_shows_first_draft_line_or_reply_hint() {
    assert_eq!(fold_bar_text("\n  fix the queue\nsecond", Some("Codex")), ("fix the queue".to_string(), true));
    assert_eq!(fold_bar_text("   ", Some("Codex")), ("Reply to Codex…".to_string(), false));
    assert_eq!(fold_bar_text("", None), ("Reply…".to_string(), false));
}

#[test]
fn context_label_rounds_and_hides_when_unused() {
    assert_eq!(context_label(82_000, 200_000), Some("41% context".to_string()));
    assert_eq!(context_label(0, 200_000), None);
    assert_eq!(context_label(5, 0), None);
    assert_eq!(context_label(250_000, 200_000), Some("100% context".to_string()));
}
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p aether-ui chat::composer`
Expected: compile errors for the missing functions.

- [ ] **Step 3: Implement the functions and the card layout**

Card:
- Radius 24, same surface and focus border as today (:96-97), textarea first, then one control row.
- Leading, trailing and action slots stay as they are.
- Resting: controls are 22px tall at 0.7 opacity, and the action gets `compact(true)`.

Tray:
- Rendered under the card, inset 20px left and right, `mt(-1px)`, bottom radius 16.
- Same outline as the card, and a tint that differs from the card surface.
- Hidden when Folded.

Folded:
- Render `bar` in place of the textarea and controls. The textarea entity keeps its state.
- Animate card height and tray opacity with `transition`.

`fold_bar`:
- A 46px row: icon, text (muted when `!is_draft`), model (muted), and a "Scroll to end" pill with an `ArrowDown` icon.
- The pill's click handler calls `cx.stop_propagation()` so it never also fires `on_expand`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether-ui chat::composer`
Expected: PASS, including the existing `send_button_default_state`.

### Task 3: Control row, tray and placeholder (app)

**Files:**
- Modify: `crates/core/src/settings.rs`: add `Provider::short_label` after `label` (:111).
- Modify: `crates/app/src/app/composer/mod.rs`: `composer_card` (:58-248).
- Modify: `crates/app/src/app/composer/chips.rs`: split `cli_chips` (:94) into `access_control` and `tray`.
- Modify: `crates/ui/src/components/composer_chip.rs`: add `control_chip`.
- Modify: `crates/app/src/app/chat/mod.rs` and `crates/app/src/app/providers.rs`: set the placeholder in `select` and `sync_selector` (:197).
- Test:
  - `crates/core/tests/unit/settings.rs`
  - `crates/app/tests/unit/app/composer/mod.rs`

**Interfaces:**
- Consumes: from Task 2, `Composer::{tray, mode}`, `context_label`, `composer_placeholder` and `SendButton::compact`.
- Produces:
  - `pub fn short_label(self) -> Option<&'static str>` on `Provider`: Claude→"Claude", Codex→"Codex", OpenCode→"OpenCode", Pi→"Pi", Antigravity→"AGY", OpenRouter→`None`.
  - `pub fn control_chip(id: impl Into<ElementId>, active: bool, cx: &App) -> Button`: height 28, `px(9)`, radius 8, medium weight, 12.5px text, muted, hover `overlay_strong`, active colour `mango_text`.
  - `ChatApp::access_control(&self, cx) -> Option<AnyElement>`.
  - `ChatApp::tray(&self, cx) -> AnyElement`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/core/tests/unit/settings.rs
#[test]
fn short_labels_name_cli_agents_only() {
    assert_eq!(Provider::Claude.short_label(), Some("Claude"));
    assert_eq!(Provider::Antigravity.short_label(), Some("AGY"));
    assert_eq!(Provider::OpenRouter.short_label(), None);
}

// crates/app/tests/unit/app/composer/mod.rs
#[test]
fn composer_drops_the_footnote_and_the_effort_chip() {
    let card = include_str!("../../../../src/app/composer/mod.rs");
    let chips = include_str!("../../../../src/app/composer/chips.rs");
    assert!(!card.contains("Aether can make mistakes"));
    assert!(!chips.contains("composer_chip(\"effort\""));
}
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p aether-core short_labels_name_cli_agents_only && cargo test -p aether composer_drops_the_footnote`
Expected: FAIL.

- [ ] **Step 3: Implement the control row**

Leading slot, in order:
1. `AgentSelector`, unchanged for now. Task 4 restyles its trigger and moves effort into its panel.
2. A 1×14px divider.
3. Then one of:
   - `access_control`, the existing access `ChoiceMenu`, triggered by a `control_chip`, when `FooterChips::access` is true;
   - for OpenRouter, the existing Search and Plan toggles built as `control_chip`s, with `active` meaning pressed.

The effort `ChoiceMenu` and the folder and worktree chips leave the row.

- [ ] **Step 4: Implement the tray**

Agent chat:
- Left:
  - folder control: a `control_chip` with the Folder icon and `folder_label`, which calls `choose_folder` for now (Task 8 swaps it for the switcher), with the same disabled rules and tooltips;
  - the existing `WorkspaceSelect`, rebuilt from `cli_chips` :163-243.
- Right: `ContextRing` at 14px plus `context_label` text. The whole right group is hidden when `context_label` is `None`.

Direct chat: the Globe icon plus "Direct chat, no workspace" on the left, with context on the right.

Wiring:
- Remove `.footnote(..)`.
- Pass `mode` with `composer_mode(!messages.is_empty(), draft_empty, false)`. Task 5 supplies the scroll flag.
- Set the placeholder with `composer.update(cx, |s, cx| s.set_placeholder(composer_placeholder(provider.short_label()), window, cx))` in `select` and after `pick_agent`.

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p aether-core && cargo test -p aether composer`
Expected: PASS.

### Task 4: Model picker: reasoning pills, descriptions, visible lock

**Files:**
- Modify: `crates/driver/src/catalog.rs`
  - `CliModel` (:34): add `pub description: String`.
  - `parse_claude_models` (:523): read `m["description"]`.
  - `parse_codex_models_json` (:310): read `description`.
- Modify: `crates/ui/src/chat/model_picker.rs`: `ModelItem` (:11) gets `pub description: SharedString`, and `custom` sets it empty.
- Modify: `crates/ui/src/chat/agent_selector.rs`: `sync` (:203), the events (:137), `render_row` (:430), the panel render (:529-635), and the non-form trigger (:688-697).
- Modify: `crates/app/src/app/providers.rs`: `cli_items` (:476) copies `description`, and `sync_selector` (:197) passes efforts.
- Modify: `crates/app/src/app/new.rs`:
  - remove the effort-menu creation (:89) and its subscription (:232-238);
  - handle `AgentSelectorEvent::Effort` in the existing subscription (:240) by calling `set_effort`.
- Modify: `crates/app/src/app/mod.rs`: remove `effort_menu` (:309) and its dismiss calls at chat/mod.rs:68 and send.rs:198.
- Test:
  - `crates/ui/tests/unit/chat/agent_selector.rs`
  - `crates/driver/tests/unit/catalog.rs`

**Interfaces:**
- Consumes: `control_chip` (Task 3).
- Produces:
  - `pub fn sync(&mut self, providers: Vec<ProviderModels>, favorites: Vec<String>, provider: Provider, model: SharedString, efforts: Vec<String>, effort: String, cx: &mut Context<Self>)`.
  - The non-form trigger becomes `control_chip("agent-selector", false, cx)`: the provider icon, then the model name, then `effort_label(effort)` in muted text, but only when `efforts` is non-empty and `effort` is not empty.
  - `AgentSelectorEvent::Effort(String)`.
  - `pub struct RailEntry { pub tab: SelectorTab, pub enabled: bool }`.
  - `pub fn rail_entries(providers: &[ProviderModels], lock: Option<Provider>) -> Vec<RailEntry>`.
    - Favorites first.
    - Hidden providers are dropped.
    - Locked-out providers stay, with `enabled: false`.
    - `rail_tabs` stays as it is for keyboard cycling.
  - `pub fn row_subtitle(item: &ModelItem, provider_label: &str, cross: bool) -> SharedString`.
  - `pub fn lock_note(provider_label: &str) -> String`.
  - `pub fn no_reasoning_note(provider_label: &str) -> String`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/ui/tests/unit/chat/agent_selector.rs (reuse or add a ProviderModels helper)
#[test]
fn locked_rail_keeps_other_providers_visible_but_disabled() {
    let providers = vec![models(Provider::Claude, false), models(Provider::Codex, false), models(Provider::Pi, true)];
    let rail: Vec<_> = rail_entries(&providers, Some(Provider::Claude)).iter().map(|e| (e.tab, e.enabled)).collect();
    assert_eq!(rail, vec![
        (SelectorTab::Favorites, true),
        (SelectorTab::Provider(Provider::Claude), true),
        (SelectorTab::Provider(Provider::Codex), false),
    ]);
}

#[test]
fn row_subtitle_prefers_the_description_inside_a_provider_tab() {
    let mut item = ModelItem { id: "gpt-6.1-sol".into(), name: "GPT-6.1-Sol".into(), detail: "OpenAI · Codex".into(), default: false, description: "Latest workhorse model".into() };
    assert_eq!(row_subtitle(&item, "Codex", false), "Latest workhorse model");
    assert_eq!(row_subtitle(&item, "Codex", true), "Codex");
    item.description = "".into();
    assert_eq!(row_subtitle(&item, "Codex", false), "OpenAI · Codex");
}

#[test]
fn picker_notes_name_the_provider() {
    assert_eq!(lock_note("Claude Code"), "This chat stays on Claude Code. Start a new chat to switch agents.");
    assert_eq!(no_reasoning_note("OpenCode"), "OpenCode sets its own reasoning");
}

// crates/driver/tests/unit/catalog.rs: extend the existing codex models fixture with
// "description": "Latest workhorse model for coding and everyday work." and assert:
//   assert_eq!(models[0].description, "Latest workhorse model for coding and everyday work.");
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p aether-ui chat::agent_selector && cargo test -p aether-driver catalog`
Expected: FAIL.

- [ ] **Step 3: Implement**

Model rows: the second line is `row_subtitle`, where `cross` means the Favorites tab or a non-empty query.

Rail:
- Built from `rail_entries`.
- A disabled tab renders at 0.3 opacity and doesn't react to clicks.
- Its tooltip reads `"<label> · this chat stays on <current provider label>"`.

Under the list:
- A "Reasoning" caption, then a wrapping row of pills for `[""] + efforts`, labelled with `effort_label`.
- The current pill is filled with foreground on background.
- A click emits `Effort(level)` and keeps the panel open.
- When `efforts` is empty, show `no_reasoning_note(label)` instead.

When locked, add a top-bordered row with the Lock icon and `lock_note`.

Raise the panel height cap from 390 to 470 to fit the new rows.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether-ui chat::agent_selector chat::model_picker && cargo test -p aether-driver && cargo test -p aether`
Expected: PASS.

### Task 5: Resting style and fold on scroll

**Files:**
- Modify: `crates/ui/src/chat/scroller.rs`: `ChatScrollerState` (:41-180).
- Modify: `crates/app/src/app/render.rs` (:116-121): add `.jump_button(false)`.
- Modify: `crates/app/src/app/new.rs`: observe the scroller.
- Modify: `crates/app/src/app/composer/mod.rs`: compute the mode and build the bar.
- Test: `crates/app/tests/unit/app/composer/mod.rs`.

**Interfaces:**
- Consumes: from Task 2, `composer_mode`, `fold_bar` and `fold_bar_text`.
- Produces:
  - `pub fn user_scrolled_up(&self) -> bool` on `ChatScrollerState`.
    - The list's scroll handler (:58) only fires on wheel events (gpui list.rs:1600). It sets a `Cell<bool>`.
    - The getter returns `flag && self.is_scrolled_up()`, and clears the flag when the list is no longer scrolled up.
    - `scroll_to_end` and `reset` clear the flag.
  - `ChatApp::sync_fold(&mut self, window: &mut Window, cx: &mut Context<Self>)`. On the change into Folded, if the composer is focused, it calls `window.focus(&self.focus)`.
  - `ChatApp::unfold(&mut self, window, cx)`: `scroll_to_end` plus focusing the composer.
  - `ChatApp.folded: bool`, which remembers the last mode so blur happens only on the transition.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn transcript_hands_jumping_to_the_fold_bar() {
    let render = include_str!("../../../../src/app/render.rs");
    let card = include_str!("../../../../src/app/composer/mod.rs");
    assert!(render.contains(".jump_button(false)"));
    assert!(card.contains("user_scrolled_up()"));
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test -p aether transcript_hands_jumping_to_the_fold_bar`
Expected: FAIL.

- [ ] **Step 3: Implement**
  - In new.rs: `cx.observe_in(&scroller, window, |this, _, window, cx| this.sync_fold(window, cx)).detach()`.
  - In `composer_card`: `mode = composer_mode(has_messages, draft_empty, self.scroller.read(cx).user_scrolled_up())`.
  - The bar shows the provider icon, `fold_bar_text(draft, provider.short_label())`, and the model and effort label. `on_expand` calls `unfold`; `on_jump` calls `scroll_to_end` only.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether-ui && cargo test -p aether`
Expected: PASS.

- [ ] **Step 5: Phase 1 gate**
  1. Run `cargo clippy --workspace`.
  2. Run the app (`cargo run -p aether`) and check, in light and dark:
     - the card and tray;
     - the resting style after a reply;
     - the fold on wheel-up while typing;
     - that the bar click restores the text and focus;
     - that the access menu and the reasoning pills work;
     - the OpenRouter tray.
  3. Ask the user to approve Phase 1. On approval, commit, for example `feat(composer): variant B card, tray, fold bar, per-chat drafts`.

## Phase 2: Projects and start screen (after Phase 1; can run beside Phase 3)

### Task 6: Saved Projects list

**Files:**
- Modify: `crates/core/src/settings.rs`:
  - add `pub projects: Vec<String>` after `hidden_projects` (:284);
  - set it to `Vec::new()` in `Default` (:382-388).
- Modify: `crates/app/src/projects.rs`: new items.
- Modify: `crates/app/src/app/sidebar.rs`: `project_groups` (:140) appends saved-only Projects, and an empty group renders as a header only.
- Test:
  - `crates/core/tests/unit/settings.rs`
  - `crates/app/tests/unit/projects.rs`

**Interfaces:**
- Produces:
  - `pub(crate) struct ProjectEntry { pub folder: PathBuf, pub last_activity: Option<i64> }`.
  - `pub(crate) fn project_list(active: Vec<(PathBuf, i64)>, saved: &[String], hidden: &[String]) -> Vec<ProjectEntry>`.
    - Active Projects first, newest first.
    - Then saved Projects without chats, sorted by lowercase folder name.
    - Hidden folders are dropped, using `listed`.
  - `pub(crate) fn add_project(settings: &mut Settings, folder: &Path)`: `set_listed(projects, true)` plus `set_listed(hidden_projects, false)`.
  - `pub(crate) fn short_path(folder: &Path, home: &Path) -> String`.
  - `pub(crate) fn with_saved(groups: Vec<Project>, saved: &[String]) -> Vec<Project>`. It appends `Project { folder, chats: vec![] }` for saved folders that aren't already a group, in lowercase name order.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/core/tests/unit/settings.rs
#[test]
fn saved_projects_default_empty_and_round_trip() {
    let s: Settings = serde_json::from_str(r#"{"appearance":"dark"}"#).unwrap();
    assert!(s.projects.is_empty());
    let s = Settings { projects: vec!["/w/a".into()], ..Settings::default() };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.projects, vec!["/w/a".to_string()]);
}

// crates/app/tests/unit/projects.rs
#[test]
fn saved_projects_without_chats_sort_last_alphabetically() {
    let active = vec![(PathBuf::from("/w/old"), 10), (PathBuf::from("/w/new"), 20)];
    let saved = vec!["/w/zeta".to_string(), "/w/new".to_string(), "/w/alpha".to_string()];
    let list = project_list(active, &saved, &[]);
    let folders: Vec<_> = list.iter().map(|p| p.folder.to_str().unwrap()).collect();
    assert_eq!(folders, ["/w/new", "/w/old", "/w/alpha", "/w/zeta"]);
    assert_eq!(list[2].last_activity, None);
}

#[test]
fn hidden_projects_stay_out_and_adding_one_unhides_it() {
    let mut s = Settings::default();
    s.hidden_projects = vec!["/w/a".into()];
    assert!(project_list(vec![(PathBuf::from("/w/a"), 5)], &[], &s.hidden_projects).is_empty());
    add_project(&mut s, Path::new("/w/a"));
    assert_eq!(s.projects, vec!["/w/a".to_string()]);
    assert!(s.hidden_projects.is_empty());
}

#[test]
fn short_path_uses_a_tilde_inside_home() {
    assert_eq!(short_path(Path::new("/Users/i/code/aether"), Path::new("/Users/i")), "~/code/aether");
    assert_eq!(short_path(Path::new("/opt/x"), Path::new("/Users/i")), "/opt/x");
}

#[test]
fn saved_folders_join_the_groups_as_empty_projects() {
    let groups = vec![Project { folder: "/w/a".into(), chats: vec![0] }];
    let merged = with_saved(groups, &["/w/b".to_string(), "/w/a".to_string()]);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[1].folder, PathBuf::from("/w/b"));
    assert!(merged[1].chats.is_empty());
}
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p aether-core saved_projects && cargo test -p aether projects`
Expected: FAIL.

- [ ] **Step 3: Implement the functions**

In `project_groups`: after `group_projects`, apply `with_saved(groups, &self.settings.projects)`, then `split_hidden` as today. A group with no chats renders its header and no rows.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether-core && cargo test -p aether projects sidebar`
Expected: PASS.

### Task 7: Sidebar "+" adds a Project

**Files:**
- Modify: `crates/ui/src/chat/sidebar/rows.rs`: `ChatRow::Header` (:32, render :127-134) gets an optional trailing "+" icon button.
- Modify: `crates/ui/src/chat/sidebar/mod.rs`: add a builder near `on_new_chat` (:603).
- Modify: `crates/app/src/app/sidebar.rs`: wire it up in `render_sidebar` (:234), plus a new method.
- Test: `crates/app/tests/unit/app/sidebar.rs`.

**Interfaces:**
- Consumes: `add_project` (Task 6) and `new_chat_in` (sidebar.rs:719).
- Produces:
  - `pub fn on_add_project(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self` on `ChatSidebar`. It is shown only on the "Projects" header, with the tooltip "Add project" and the Plus icon.
  - `ChatApp::add_project_from_picker(&mut self, window: &mut Window, cx: &mut Context<Self>)`. It opens `prompt_for_paths` (directories only, single selection, prompt "Add"). On a path:
    1. `update_settings(cx, |s| add_project(s, &path))`
    2. `new_chat_in(path, window, cx)`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn projects_heading_offers_add_project() {
    let src = include_str!("../../../src/app/sidebar.rs");
    assert!(src.contains(".on_add_project("));
    assert!(src.contains("add_project_from_picker"));
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test -p aether projects_heading_offers_add_project`
Expected: FAIL.

- [ ] **Step 3: Implement**

The header row stays non-focusable, and the "+" button itself is focusable (`docs/ux-rubric.md`: a hit area of at least 24px and a `focus_visible` ring).

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether-ui chat::sidebar && cargo test -p aether sidebar`
Expected: PASS.

### Task 8: Project switcher

**Files:**
- Modify: `crates/ui/src/components/choice_menu.rs`:
  - `ChoiceOption` (:33): add `pub mark: Option<SharedString>` and `#[derive(Default)]`;
  - render a `project_mark_sized(name, px(18.), cx)` in place of the icon when `mark` is set.
- Modify: every `ChoiceOption { .. }` literal (chips.rs :115, :129 and others found by `grep -rn "ChoiceOption {" crates`): add `mark: None` or use `..Default::default()`.
- Modify: `crates/app/src/app/composer/chips.rs`:
  - move the mutation in `choose_folder` (:25-61) into `set_draft_folder`, called from both places;
  - the tray's folder control becomes `ChoiceMenu::new(&self.project_menu, control_chip(..))`.
- Modify: `crates/app/src/app/mod.rs`: add a `project_menu: Entity<ChoiceMenuState>` field.
- Modify: `crates/app/src/app/new.rs`: create it and subscribe to `ChoiceMenuEvent::Pick`.
- Modify: `crates/app/src/projects.rs`: add `project_switcher_options`.
- Test: `crates/app/tests/unit/projects.rs`.

**Interfaces:**
- Consumes: `project_list`, `short_path` and `add_project` (Task 6), and `project_names` (sidebar.rs:153).
- Produces:
  - `pub(crate) fn set_draft_folder(&mut self, ix: usize, folder: PathBuf, cx: &mut Context<Self>)`, a `ChatApp` method.
    - It is a no-op unless `workspace_can_change(ix)`.
    - It sets `cwd` and clears `new_worktree`, `new_worktree_existing` and `worktree_error`.
    - It then calls `persist(ix)`, `load_bases`, `refresh_git(false)` and notifies.
  - `pub(crate) fn project_switcher_options(list: &[ProjectEntry], names: &HashMap<PathBuf, String>, current: &Path, home: &Path) -> Vec<ChoiceOption>`.
    - Each option has `mark` and `title` = the name (falling back to `folder_label`), `description` = `short_path`, and `checked` = `folder == current`.
    - It is followed by `ChoiceOption { title: "Add project…", icon: Some(IconName::Plus), ..Default::default() }`.
  - `ChatApp::open_project_switcher_sync(&mut self, cx)` syncs the options before the menu opens.
  - A pick at the last index runs `add_project_and_move(window, cx)`: the picker, then `add_project`, then `set_draft_folder(active, path)`. Any other index runs `set_draft_folder(active, list[i].folder.clone())`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn switcher_lists_projects_then_add_project() {
    let list = vec![
        ProjectEntry { folder: "/Users/i/aether".into(), last_activity: Some(2) },
        ProjectEntry { folder: "/Users/i/dotfiles".into(), last_activity: None },
    ];
    let names = HashMap::from([(PathBuf::from("/Users/i/aether"), "aether".to_string())]);
    let opts = project_switcher_options(&list, &names, Path::new("/Users/i/aether"), Path::new("/Users/i"));
    let titles: Vec<_> = opts.iter().map(|o| o.title.to_string()).collect();
    assert_eq!(titles, ["aether", "dotfiles", "Add project…"]);
    assert!(opts[0].checked && !opts[1].checked);
    assert_eq!(opts[1].description.as_ref().map(|d| d.to_string()), Some("~/dotfiles".to_string()));
    assert_eq!(opts[0].mark.as_ref().map(|m| m.to_string()), Some("aether".to_string()));
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test -p aether switcher_lists_projects_then_add_project`
Expected: FAIL.

- [ ] **Step 3: Implement**

This uses the menu header "Projects", width 320, and `below()` when it opens from the start-screen headline.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether-ui && cargo test -p aether`
Expected: PASS.

### Task 9: Start screen with headline and recents

**Files:**
- Create: `crates/app/src/app/start.rs` (add `mod start;` in app/mod.rs), test file `crates/app/tests/unit/app/start.rs`.
- Modify: `crates/ui/src/chat/hero.rs`: add `heading(impl IntoElement)`. When it is set, it replaces the title text, and no subtitle is shown.
- Create: `crates/ui/src/chat/recent.rs`, exported from chat/mod.rs.
- Modify: `crates/app/src/app/render.rs` (:104-108): the empty-chat branch calls `self.render_start(composer, cx)`.

**Interfaces:**
- Consumes:
  - `project_cards` (sidebar.rs:581) → `Card` (project_page/model.rs:111);
  - `relative_time` (projects.rs:61);
  - `open_project_page` (app/mod.rs:439);
  - `project_menu` (Task 8);
  - `select`.
- Produces:
  - `pub(crate) fn start_headline(agent: bool, project: Option<&str>) -> (String, Option<String>, String)`.
  - `pub(crate) fn recent_chats(cards: Vec<Card>, exclude: i64, limit: usize) -> Vec<Card>`: newest `activity_ms` first, without the card whose `id == exclude`, truncated to `limit`.
  - `pub struct RecentChats` in ui, built with `RecentChats::new(heading: SharedString).view_all(handler).row(icon: Icon, title: SharedString, branch: SharedString, when: SharedString, on_click: handler)`. Rows are 38px with the branch in monospace, plus a "View all" link.
  - `ChatApp::render_start(&self, composer: impl IntoElement, cx) -> AnyElement`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn headline_names_the_project_for_agent_chats() {
    assert_eq!(start_headline(true, Some("aether")), ("What should we build in ".to_string(), Some("aether".to_string()), "?".to_string()));
    assert_eq!(start_headline(true, None), ("What should we build?".to_string(), None, String::new()));
    assert_eq!(start_headline(false, Some("aether")), ("What can I help with?".to_string(), None, String::new()));
}

#[test]
fn recents_skip_the_draft_and_keep_the_newest_four() {
    let cards: Vec<Card> = (1..=6).map(|i| card(i, i * 10)).collect(); // helper: id, activity_ms
    let ids: Vec<i64> = recent_chats(cards, 6, 4).iter().map(|c| c.id).collect();
    assert_eq!(ids, [5, 4, 3, 2]);
}
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p aether start`
Expected: FAIL.

- [ ] **Step 3: Implement `render_start`**

Agent chat:
- `Hero` with a 30px normal-weight heading built from `start_headline`.
- The project name is a text-styled `Button` with a dashed underline, using gpui's dashed border if available, otherwise a 1px `muted_foreground` underline.
- That button is the trigger of `ChoiceMenu::new(&self.project_menu, ..)`.
- Then the composer.
- Then `RecentChats` from `recent_chats(self.project_cards(&dir), chat.id, 4)`.
  - Row icon: `provider_icon(card.provider)`.
  - Branch: `card.place`.
  - Time: `relative_time(card.activity_ms, now, &Local)`.
  - Click: `select(card.ix)`.
  - "View all": `open_project_page(dir)`.
  - Hidden when the list is empty.
- The group is vertically centered.

Direct chat: "What can I help with?" with no recents. Drop the old subtitle.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether-ui && cargo test -p aether`
Expected: PASS.

### Task 10: Cmd+N starts in the current Project

**Files:**
- Modify: `crates/app/src/app/chat/create.rs`
  - `new_chat` (:86-137);
  - add `#[cfg(test)] #[path = "../../../tests/unit/app/chat/create.rs"] mod tests;`.
- Create: `crates/app/tests/unit/app/chat/create.rs`.

**Interfaces:**
- Consumes: `set_draft_folder` (Task 8), `project_list` (Task 6), `chat_project` (header.rs:85) and `is_agent_chat` (chat/mod.rs:33).
- Produces: `pub(super) fn new_chat_folder(cli: bool, current: Option<PathBuf>, recent: Option<PathBuf>) -> Option<PathBuf>`. It returns `None` when `!cli`, otherwise `current.or(recent)`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn new_chats_start_in_the_current_or_latest_project() {
    let cur = Some(PathBuf::from("/w/a"));
    let rec = Some(PathBuf::from("/w/b"));
    assert_eq!(new_chat_folder(true, cur.clone(), rec.clone()), cur);
    assert_eq!(new_chat_folder(true, None, rec.clone()), rec);
    assert_eq!(new_chat_folder(false, cur, rec), None);
    assert_eq!(new_chat_folder(true, None, None), None);
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test -p aether new_chats_start_in_the_current_or_latest_project`
Expected: FAIL.

- [ ] **Step 3: Implement**

Before switching, `new_chat` computes:
- `cli = CLIS.contains(&self.settings.provider)`;
- `current = chat_project(active)` if the active chat is an Agent chat;
- `recent` = the first `project_list(..)` entry with `last_activity.is_some()`.

After the existing reuse-or-create logic and `select`, if `new_chat_folder(..)` is `Some(f)`, call `set_draft_folder(self.active, f, cx)`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether`
Expected: PASS.

- [ ] **Step 5: Phase 2 gate**
  1. Run `cargo clippy --workspace`.
  2. Run the app and check:
     - the headline switcher moves an empty Draft;
     - "Add project…" works from both the switcher and the sidebar;
     - an empty saved Project shows in the sidebar;
     - recents open chats, and "View all" opens the project page;
     - Cmd+N lands in the current Project;
     - Direct chats show "What can I help with?".
  3. Ask the user to approve Phase 2, then commit.

## Phase 3: Attachments and image paste (after Phase 1)

### Task 11: Attachment type, limits and delivery rules (core)

**Files:**
- Create: `crates/core/src/attachment.rs` and `crates/core/tests/unit/attachment.rs`.
- Modify: `crates/core/src/lib.rs`:
  - add `mod attachment; pub use attachment::*;`;
  - add `#[serde(default)] pub attachments: Vec<Attachment>` to `Message` (:275);
  - set `attachments: Vec::new()` in `Message::user` (:314) and `Message::assistant` (:338).
- Test: `crates/core/tests/unit/lib.rs`.

**Interfaces:**
- Produces:
  - `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct Attachment { pub name: String, pub path: PathBuf, pub mime: String, pub size: u64 }`.
  - Constants:
    - `pub const MAX_ATTACHMENTS: usize = 10;`
    - `pub const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;`
    - `pub const DIRECT_INLINE_BYTES: u64 = 10 * 1024 * 1024;`
    - `pub const CLAUDE_IMAGE_BYTES: u64 = 5 * 1024 * 1024;`
    - `pub const INLINE_TEXT_BYTES: u64 = 100 * 1024;`
  - `pub fn mime_for(path: &Path) -> &'static str`. The extension is lowercased first.

    | Extension | Mime |
    |---|---|
    | png | image/png |
    | jpg, jpeg | image/jpeg |
    | gif | image/gif |
    | webp | image/webp |
    | tif, tiff | image/tiff |
    | bmp | image/bmp |
    | heic | image/heic |
    | pdf | application/pdf |
    | json | application/json |
    | md | text/markdown |
    | csv | text/csv |
    | html | text/html |
    | txt, rs, ts, tsx, js, jsx, py, toml, yaml, yml, sh, go, swift, java, c, h, cpp, css, sql, log | text/plain |
    | anything else | application/octet-stream |

  - `pub fn is_image(mime: &str) -> bool` is true for `image/*`.
  - `pub fn is_text(mime: &str) -> bool` is true for `text/*` and `application/json`.
  - `pub fn check_attachment(count_before: usize, name: &str, size: u64, mime: &str, direct: bool) -> Result<(), String>`, using the error copy from Global Constraints. Checks run in this order:
    1. count
    2. 20 MB
    3. direct type (image, text or PDF)
    4. direct 10 MB
  - `pub fn attached_files_block(files: &[Attachment]) -> String`. It is empty when `files` is empty. Otherwise it is `"\n\nAttached files:"` followed by `"\n- {path} ({mime})"` for each file.
  - `pub fn native_image_split(provider: Provider, files: &[Attachment]) -> (Vec<Attachment>, Vec<Attachment>)`, used only for CLI providers. It returns `(native, by_path)`.
    - Claude: images in png, jpeg, gif or webp of at most `CLAUDE_IMAGE_BYTES` go native.
    - Codex: all images go native.
    - Pi, OpenCode and Antigravity: nothing goes native. Pi's `images` field has no documented element format, so Pi gets paths.
    - Order is kept within each side.

- [ ] **Step 1: Write the failing tests**

```rust
fn att(name: &str, mime: &str, size: u64) -> Attachment {
    Attachment { name: name.into(), path: PathBuf::from("/c/attachments/1").join(name), mime: mime.into(), size }
}

#[test]
fn limits_and_direct_chat_types_are_checked_on_add() {
    assert_eq!(check_attachment(10, "a.png", 1, "image/png", false), Err("Up to 10 attachments".into()));
    assert_eq!(check_attachment(0, "big.mov", 21 * 1024 * 1024, "application/octet-stream", false), Err("big.mov is over 20 MB".into()));
    assert_eq!(check_attachment(0, "x.zip", 10, "application/zip", true), Err("x.zip: this chat can only take images, PDFs and text files".into()));
    assert_eq!(check_attachment(0, "shot.png", 15 * 1024 * 1024, "image/png", true), Err("shot.png is over 10 MB".into()));
    assert_eq!(check_attachment(9, "x.zip", 10, "application/zip", false), Ok(()));
}

#[test]
fn mime_comes_from_the_extension() {
    assert_eq!(mime_for(Path::new("a/Shot.PNG")), "image/png");
    assert_eq!(mime_for(Path::new("doc.pdf")), "application/pdf");
    assert_eq!(mime_for(Path::new("main.rs")), "text/plain");
    assert_eq!(mime_for(Path::new("blob")), "application/octet-stream");
}

#[test]
fn claude_takes_small_images_natively_and_the_rest_by_path() {
    let (small, big, pdf) = (att("a.png", "image/png", 1024), att("b.png", "image/png", 6 * 1024 * 1024), att("c.pdf", "application/pdf", 10));
    let (native, paths) = native_image_split(Provider::Claude, &[small.clone(), big.clone(), pdf.clone()]);
    assert_eq!(native, vec![small.clone()]);
    assert_eq!(paths, vec![big, pdf.clone()]);
    let (native, paths) = native_image_split(Provider::OpenCode, &[small.clone(), pdf.clone()]);
    assert!(native.is_empty());
    assert_eq!(paths, vec![small, pdf]);
}

#[test]
fn path_block_lists_each_file() {
    assert_eq!(attached_files_block(&[att("a.png", "image/png", 3)]), "\n\nAttached files:\n- /c/attachments/1/a.png (image/png)");
    assert_eq!(attached_files_block(&[]), "");
}

// crates/core/tests/unit/lib.rs, same JSON shape as chats_saved_before_sent_time_load_without_it
#[test]
fn chats_saved_before_attachments_load_without_them() {
    // parse that test's user-message JSON (it has no "attachments" key)
    // assert!(message.attachments.is_empty());
}
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p aether-core attachment chats_saved_before_attachments`
Expected: FAIL.

- [ ] **Step 3: Implement the functions and the `Message` field**

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether-core`
Expected: PASS.

### Task 12: Attachment store and lifecycle (app)

**Files:**
- Modify: root `Cargo.toml` `[workspace.dependencies]`: add the `image` entry from Tech Stack.
- Modify: `crates/app/Cargo.toml`: add `image.workspace = true`.
- Create: `crates/app/src/attachments.rs` (add `mod attachments;` in main.rs or lib root) and `crates/app/tests/unit/attachments.rs`.
- Modify: `crates/agent/src/store.rs`: add `ids()`. Test in `crates/agent/tests/unit/store.rs`.
- Modify: `crates/app/src/app/chat/manage.rs`: in `delete_chat` (:84), after `store.delete`, run `remove_chat` on the background executor.
- Modify: `crates/app/src/app/new.rs`: after `restore_chats` (:456), run `sweep` on the background executor with the ids from `store.ids()`.
- Modify: `crates/app/src/app/chat/create.rs`: in `duplicate_chat` (:14), copy the attachments of the copied messages with `copy_all` on the background executor, then patch the new chat's message paths and `persist`.

**Interfaces:**
- Consumes: `Attachment` and `mime_for` (Task 11), and `config_dir` (core paths.rs:7).
- Produces:
  - `pub(crate) fn attachments_root() -> Option<PathBuf>`, which is `config_dir()?.join("attachments")`.
  - `pub(crate) fn store_file(root: &Path, chat: i64, src: &Path) -> io::Result<Attachment>`.
  - `pub(crate) fn store_bytes(root: &Path, chat: i64, name: &str, bytes: &[u8]) -> io::Result<Attachment>`.
    - Both write to `root/<chat>/<n>-<name>`, where `n` is 1 plus the number of entries already in that folder.
    - A `image/tiff` or `image/bmp` input is decoded with `image` and saved as PNG, with the name's extension changed to `.png` and the mime set to `image/png`.
    - `size` is the stored file's size.
  - `pub(crate) fn pasted_name(n: usize, ext: &str) -> String`.
  - `pub(crate) fn remove_chat(root: &Path, chat: i64)`.
  - `pub(crate) fn sweep(root: &Path, live: &HashSet<i64>)`. It removes every numeric folder not in `live`.
  - `pub(crate) fn copy_all(root: &Path, to_chat: i64, files: &[Attachment]) -> Vec<Attachment>`. It skips files that are missing.
  - `pub fn ids(&self) -> sqlx::Result<Vec<i64>>` on `Store`: `SELECT id FROM chats`, including rows whose JSON no longer parses.

- [ ] **Step 1: Write the failing tests**

```rust
fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aether-att-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn stored_files_get_numbered_copies_under_their_chat() {
    let root = tmp("store");
    let src = root.join("in.txt");
    fs::write(&src, "hi").unwrap();
    let a = store_file(&root, 7, &src).unwrap();
    let b = store_file(&root, 7, &src).unwrap();
    assert_eq!(a.path, root.join("7").join("1-in.txt"));
    assert_eq!(b.path, root.join("7").join("2-in.txt"));
    assert_eq!((a.name.as_str(), a.mime.as_str(), a.size), ("in.txt", "text/plain", 2));
}

#[test]
fn tiff_is_stored_as_png() {
    let root = tmp("tiff");
    let mut bytes = Vec::new();
    image::RgbaImage::new(1, 1).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Tiff).unwrap();
    let a = store_bytes(&root, 1, "shot.tiff", &bytes).unwrap();
    assert_eq!(a.mime, "image/png");
    assert_eq!(a.name, "shot.png");
    assert!(a.path.extension().is_some_and(|e| e == "png"));
}

#[test]
fn sweep_keeps_only_live_chats() {
    let root = tmp("sweep");
    fs::create_dir_all(root.join("1")).unwrap();
    fs::create_dir_all(root.join("2")).unwrap();
    sweep(&root, &HashSet::from([1]));
    assert!(root.join("1").exists() && !root.join("2").exists());
}

#[test]
fn forks_get_their_own_copies() {
    let root = tmp("fork");
    let src = root.join("in.txt");
    fs::write(&src, "hi").unwrap();
    let a = store_file(&root, 1, &src).unwrap();
    let copies = copy_all(&root, 9, &[a]);
    assert_eq!(copies[0].path, root.join("9").join("1-in.txt"));
    assert!(copies[0].path.exists());
}

#[test]
fn pasted_images_are_numbered() {
    assert_eq!(pasted_name(3, "png"), "Pasted image 3.png");
}
```

Plus, in `crates/agent/tests/unit/store.rs`: `ids_include_every_row`. It saves two chats and asserts `store.ids()` returns both ids.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p aether attachments && cargo test -p aether-agent ids_include_every_row`
Expected: FAIL.

- [ ] **Step 3: Implement and wire up delete, sweep and fork**

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether && cargo test -p aether-agent`
Expected: PASS.

### Task 13: Pending Attachments in the composer

**Files:**
- Modify: `crates/app/src/app/mod.rs`:
  - `Chat` gets `pub(crate) pending: Vec<Attachment>` (update every literal listed in Task 1);
  - `ChatApp` gets `pub(crate) attach_error: Option<SharedString>`.
- Create: `crates/ui/src/chat/attachments.rs`, exported from chat/mod.rs, with `crates/ui/tests/unit/chat/attachments.rs`.
- Modify: `crates/ui/src/chat/composer.rs`:
  - `Composer::on_paste` forwards to `Textarea::new(..).on_paste(..)` (gpui-component input/textarea.rs:163);
  - `Composer::on_drop_paths` puts `.on_drop::<ExternalPaths>` and `.drag_over::<ExternalPaths>` on the card;
  - fill the `attachments` slot.
- Create: `crates/app/src/app/composer/attach.rs` (`mod attach;` in composer/mod.rs) with `crates/app/tests/unit/app/composer/attach.rs`.
- Modify: `crates/app/src/app/composer/mod.rs`:
  - Attach button (Paperclip `icon_chip`) in trailing;
  - the error line in the header stack;
  - send enabled when there is text or pending;
  - `draft_empty` counts pending.
- Modify: `crates/app/src/assets.rs`: add `Paperclip`, `Image` and `X` to `icon_assets!`, and add those three to `crates/app/tests/unit/assets.rs`.

**Interfaces:**
- Consumes: `check_attachment`, `mime_for` and `is_image` (Task 11), and `store_file`, `store_bytes`, `pasted_name` and `attachments_root` (Task 12).
- Produces:
  - `pub fn format_size(bytes: u64) -> String`, in ui attachments.rs, base 1024, one decimal for KB and MB: "512 B", "1.2 KB", "3.3 MB".
  - `pub struct AttachmentView { pub name: SharedString, pub path: PathBuf, pub image: bool, pub size: u64, pub missing: bool }`.
  - `pub fn attachment_caption(view: &AttachmentView) -> String`: "File missing" when missing, otherwise `format_size(size)`.
  - `pub fn attachment_strip(items: Vec<AttachmentView>, tile: Pixels, on_open: Option<Rc<dyn Fn(&Path, &mut Window, &mut App)>>, on_remove: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>, cx: &App) -> AnyElement`.
    - Image tiles use `img(path)`.
    - File chips show the FileText icon, name and caption, with a remove `X` when `on_remove` is set.
    - Missing items render muted.
  - `pub(crate) enum PasteKind { Text, Image(gpui_kit::Image), Files(Vec<PathBuf>) }`.
  - `pub(crate) fn paste_kind(item: &ClipboardItem) -> PasteKind`. The first `ClipboardEntry::Image` gives `Image`, `ExternalPaths` gives `Files`, and anything else gives `Text`.
  - `ChatApp::add_attachment_paths(&mut self, paths: Vec<PathBuf>, cx)`.
    - Runs `check_attachment` per file, counting pending.
    - Copies on the background executor and pushes into the pending list of the chat with the captured id.
    - Sets `attach_error` on the first refusal.
  - `ChatApp::add_pasted_image(&mut self, image: gpui_kit::Image, cx)`. The name is `pasted_name(n, ext)`, where `n` is the count of pasted images in pending plus 1, and the extension comes from the image format.
  - `ChatApp::remove_pending(&mut self, i: usize, cx)`. It removes the item and deletes its file on the background executor.
  - `ChatApp::pick_attachments(&mut self, window, cx)`. It opens `prompt_for_paths`: files, multiple selection, prompt "Attach".
  - `attach_error` clears on the composer's `Change` event (new.rs:164-172) and on the next add.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/ui/tests/unit/chat/attachments.rs
#[test]
fn sizes_read_like_finder() {
    assert_eq!(format_size(512), "512 B");
    assert_eq!(format_size(1_250), "1.2 KB");
    assert_eq!(format_size(3_500_000), "3.3 MB");
}

#[test]
fn missing_files_say_so() {
    let mut v = AttachmentView { name: "a.png".into(), path: "/x/a.png".into(), image: true, size: 1_250, missing: false };
    assert_eq!(attachment_caption(&v), "1.2 KB");
    v.missing = true;
    assert_eq!(attachment_caption(&v), "File missing");
}

// crates/app/tests/unit/app/composer/attach.rs
#[test]
fn text_paste_is_left_to_the_textarea() {
    assert!(matches!(paste_kind(&ClipboardItem::new_string("hi".into())), PasteKind::Text));
}
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p aether-ui chat::attachments && cargo test -p aether text_paste_is_left_to_the_textarea serves_`
Expected: FAIL.

- [ ] **Step 3: Implement**

The `on_paste` handler matches on `paste_kind`:
- `Image` → `add_pasted_image`, returns `true`;
- `Files` → `add_attachment_paths`, returns `true`;
- `Text` → returns `false`.

Other wiring:
- Drop calls `add_attachment_paths`. While files hover, the card border uses `theme.accent`.
- The Paperclip button calls `pick_attachments`.
- The composer's attachment row is `attachment_strip(.., px(56.), None, Some(remove))`, and its appearance animates with the card height.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether-ui && cargo test -p aether`
Expected: PASS.

### Task 14: Send path and delivery to each agent

**Files:**
- Modify: root `Cargo.toml`: add `base64 = "0.22"` to the workspace, and to `crates/driver/Cargo.toml` as `base64.workspace = true`.
- Modify: `crates/app/src/app/mod.rs`: `Chat.queue: Vec<String>` (:116) becomes `Vec<Queued>`.
- Modify: `crates/app/src/app/chat/send.rs`:
  - `send` (:26) takes the active chat's `pending` together with the text, and allows an empty text when pending is non-empty;
  - `next_queued` (:19), `steer_queued` (:76), `remove_queued` (:94), `submit` (:102), and `Message::user` at :150 sets `attachments`.
- Modify: `crates/app/src/app/composer/mod.rs`: queue rows (:72-99) show `text`, with a Paperclip count when there are attachments.
- Modify: `crates/app/src/app/worktree/send.rs` (:13, :84) and `crates/app/src/app/worktree/setup.rs` (`SetupJob` :12 gets `attachments`; `setup_cancel` restores them into pending).
- Modify: `crates/app/src/app/chat/edit.rs`:
  - `edit_message` (:34) puts `copy_all(root, chat.id, &msg.attachments)` into pending, on the background executor;
  - `retry_request` (:8) returns `(usize, String, Vec<Attachment>)`;
  - `retry` resends them.
- Modify: `crates/app/src/app/chat/mod.rs`: in `history` (:28), user messages with attachments get `"\n[attached: a.png, b.pdf]"` appended.
- Modify: `crates/agent/src/agent.rs`:
  - `Command::Prompt` (:68) and `Command::Steer` (:76);
  - `send` (:462), `send_cli` (:503) and `steer` (:676);
  - the worker at :584-599 builds `text = context_prompt(..) + &attached_files_block(&by_path)` and calls `driver.prompt(text, native)` / `driver.steer(text, native)`, using `native_image_split(provider, &attachments)`.
- Modify: `crates/agent/src/turn.rs`: `run` (:39-55).
- Modify: `crates/driver/src/lib.rs`: the trait (:61-66), plus every implementation:
  - `claude.rs`: `user_message` (:382), `launch_args` (:317-354);
  - `codex.rs`: `input` (:368), `start_turn` (:372), `steer_turn` (:438), `State.queued` (:57), `Call::Steer` (:30);
  - `pi.rs`;
  - `opencode/mod.rs`;
  - `antigravity.rs`;
  - the test `Fake` at `crates/agent/tests/unit/agent.rs:13`.
- Test:
  - `crates/driver/tests/unit/claude.rs`
  - `crates/driver/tests/unit/codex.rs`
  - `crates/agent/tests/unit/agent.rs`
  - `crates/app/tests/unit/app/chat/mod.rs`
  - `crates/app/tests/unit/app/chat/send.rs` (update the existing queue test to `Queued`)

**Interfaces:**
- Consumes: `native_image_split`, `attached_files_block`, `is_image`, `is_text` and `INLINE_TEXT_BYTES` (Task 11), and `copy_all` / `attachments_root` (Task 12).
- Produces:
  - `pub(crate) struct Queued { pub text: String, pub attachments: Vec<Attachment> }`.
  - `pub(crate) fn submit(&mut self, ix: usize, text: String, attachments: Vec<Attachment>, steer: bool, window: &mut Window, cx: &mut Context<Self>)`.
  - `send_in_new_worktree(.., text: String, attachments: Vec<Attachment>, window, cx)`, and the same new parameter for `send_in_existing_worktree` and `begin_setup_or_submit`.
  - `pub fn send(&self, session: &str, text: String, attachments: Vec<Attachment>, settings: &Settings, api_key: &str, skills: Vec<Skill>, history: &[(Role, String)], chat: &ChatSession) -> Reply`.
  - `pub fn steer(&self, text: String, attachments: Vec<Attachment>, settings: &Settings, history: &[(Role, String)], chat: &ChatSession) -> Reply`.
  - `Command::Prompt { text, attachments: Vec<Attachment>, history, resume, model, effort }`.
  - `Command::Steer(String, Vec<Attachment>, UnboundedSender<Update>, String)`.
  - The `Driver` trait:
    - `fn prompt(&mut self, text: String, images: Vec<Attachment>);`
    - `fn steer(&mut self, text: String, images: Vec<Attachment>) -> bool;`
    - Pi, OpenCode and Antigravity ignore `images`, which is always empty for them.
  - Claude:
    - `fn user_message(text: &str, images: &[Attachment]) -> Value`. Content is the existing text block followed by `{"type":"image","source":{"type":"base64","media_type":mime,"data":<base64 STANDARD of the file>}}` per image. An unreadable file is skipped.
    - `launch_args` adds `"--add-dir"` and `config_dir()/attachments` when `config_dir()` is `Some`.
  - Codex: `fn input(text: &str, images: &[Attachment]) -> Value`. It returns the existing text item followed by `{"type":"localImage","path":<path>}` per image.
  - OpenRouter: `pub async fn run(self, text: String, attachments: Vec<Attachment>, tx: ..) -> Result<()>`. The content starts with `Content::new("user").with_text(text)`, then for each attachment:
    - images and PDFs → `with_inline_data(mime, fs::read(path)?)`;
    - text files of at most `INLINE_TEXT_BYTES` → `with_text(format!("File: {name}\n```\n{contents}\n```"))`;
    - other text files → `with_inline_data`.

    Sizes are already capped at 10 MB by Task 11.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/driver/tests/unit/claude.rs
#[test]
fn user_message_puts_images_after_the_text() {
    let dir = std::env::temp_dir().join(format!("aether-claude-img-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.png");
    std::fs::write(&path, [1u8, 2, 3]).unwrap();
    let img = Attachment { name: "a.png".into(), path, mime: "image/png".into(), size: 3 };
    let v = user_message("look", &[img]);
    let content = /* the same JSON path user_message uses today for its content array */;
    assert_eq!(content[1], serde_json::json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"AQID"}}));
}

#[test]
fn launch_args_let_claude_read_attachments() {
    let (args, ..) = launch_args(/* the options used in full_access_args_start_fresh_session */);
    assert!(args.windows(2).any(|w| w[0] == "--add-dir" && w[1].ends_with("attachments")));
}

// crates/driver/tests/unit/codex.rs
#[test]
fn turn_input_adds_local_images() {
    let img = Attachment { name: "a.png".into(), path: "/c/a.png".into(), mime: "image/png".into(), size: 3 };
    let v = input("hi", &[img]);
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[1], serde_json::json!({"type":"localImage","path":"/c/a.png"}));
}

// crates/agent/tests/unit/agent.rs, following cli_prompt_carries_the_transcript_as_context
#[test]
fn cli_prompt_lists_files_the_agent_cannot_take_inline() {
    // send with an OpenCode ChatSession and one image attachment at /x/a.png;
    // assert the Fake driver's recorded prompt text ends with
    // "\n\nAttached files:\n- /x/a.png (image/png)" and its recorded images are empty.
}

// crates/app/tests/unit/app/chat/mod.rs
#[test]
fn history_names_earlier_attachments() {
    let mut m = Message::user("see this");
    m.attachments = vec![Attachment { name: "a.png".into(), path: "/x/a.png".into(), mime: "image/png".into(), size: 1 }];
    assert_eq!(history(&[m])[0].1, "see this\n[attached: a.png]");
}
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p aether-driver && cargo test -p aether-agent && cargo test -p aether history_names_earlier_attachments`
Expected: FAIL (compile errors, then assertions).

- [ ] **Step 3: Implement the changes in the Files list, crate by crate: core, driver, agent, app**

- [ ] **Step 4: Run every test and confirm it passes**

Run: `cargo test -p aether-core -p aether-driver -p aether-agent -p aether`
Expected: PASS.

### Task 15: Attachments in the transcript

**Files:**
- Modify: `crates/ui/src/chat/message.rs`: `UserMessage` (:33-132). Render `attachment_strip(.., px(64.), Some(open), None)` above the bubble at :114 when it isn't empty.
- Modify: `crates/app/src/app/composer/message.rs`: the user branch of `render_message` (:16-28).
- Modify: `crates/app/src/app/mod.rs`: add `pub(crate) missing_attachments: HashSet<PathBuf>`, refreshed in the background on `select` and after `restore_chats`.

**Interfaces:**
- Consumes: `AttachmentView` and `attachment_strip` (Task 13).
- Produces:
  - `pub fn attachments(mut self, items: Vec<AttachmentView>, on_open: Rc<dyn Fn(&Path, &mut Window, &mut App)>) -> Self` on `UserMessage`.
  - The app's `on_open` runs `std::process::Command::new("open").arg(path)` on the background executor.

- [ ] **Step 1: Write the failing test**

```rust
// crates/ui/tests/unit/chat/message.rs
#[test]
fn user_message_renders_its_attachment_strip() {
    let src = include_str!("../../../src/chat/message.rs");
    assert!(src.contains("attachment_strip("));
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test -p aether-ui user_message_renders_its_attachment_strip`
Expected: FAIL.

- [ ] **Step 3: Implement**

`render_message` maps `message.attachments` to `AttachmentView`, with `missing = self.missing_attachments.contains(&path)` and `image = is_image(&mime)`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether-ui && cargo test -p aether`
Expected: PASS.

- [ ] **Step 5: Phase 3 gate**
  1. Run `cargo clippy --workspace`.
  2. Run the app and check:
     - Paperclip, drop and screenshot paste (Cmd+Ctrl+Shift+4, then Cmd+V);
     - pasting files copied in Finder;
     - text paste still works;
     - limits and direct-chat refusals;
     - send to Claude, Codex, OpenCode and OpenRouter with an image;
     - queue, steer, edit and retry keep their attachments;
     - deleting a chat removes its folder;
     - the transcript thumbnails open in Preview.
  3. Ask the user to approve Phase 3, then commit.

## Phase 4: Ctrl+G Draft editor (after Phase 1; can run beside Phases 2 and 3)

### Task 16: Ctrl+G editor overlay

**Files:**
- Modify: `crates/app/src/actions.rs`:
  - `actions!` (:20-48) gets `EditDraft`, `DraftEditorDone` and `DraftEditorSend`;
  - add the constants `COMPOSER_CONTEXT = "Composer"` and `DRAFT_EDITOR_CONTEXT = "DraftEditor"` next to :51-57;
  - in `bind_keys` (:67), add a group after the input-key groups, with `let composer = format!("{COMPOSER_CONTEXT} > Input"); let draft = format!("{DRAFT_EDITOR_CONTEXT} > Input");`.
- Modify: `crates/app/src/app/composer/mod.rs`: the outer wrapper in `render_composer` (:32) always gets `.key_context(COMPOSER_CONTEXT)` and `.on_action(EditDraft → open_draft_editor)`.
- Create: `crates/app/src/app/draft_editor.rs` (`mod draft_editor;` in app/mod.rs).
- Modify: `crates/app/src/app/mod.rs`: add `pub(crate) draft_editor: Option<Entity<EditorState>>`.
- Modify: `crates/app/src/app/render.rs`: the chat column `v_flex` (:232) gets `.relative()`, and adds `.children(self.render_draft_editor(cx))` when `page == Page::Chat`.
- Test: `crates/app/tests/unit/actions.rs`.

**Interfaces:**
- Consumes: `ChatApp::send(window, cx)` (chat/send.rs:26) and the composer's `set_value` / `focus`.
- Produces:
  - `ChatApp::open_draft_editor(&mut self, window, cx)`.
    - No-op while the composer is disabled (`switching_folder`).
    - Creates `EditorState::new(window, cx).language("markdown").default_value(text)` (soft wrap is on by default), puts the cursor at the end with `set_cursor_position`, and focuses it.
  - `ChatApp::close_draft_editor(&mut self, apply: bool, window, cx)`. With `apply`, it runs `composer.set_value(editor text)`. Either way it drops the editor and focuses the composer with the cursor at the end.
  - `ChatApp::send_from_draft_editor(&mut self, window, cx)`: `close_draft_editor(true)` then `send`.
  - `fn render_draft_editor(&self, cx) -> Option<AnyElement>`:
    - an `absolute().inset_0()` sheet in `theme.background`;
    - `.key_context(DRAFT_EDITOR_CONTEXT)`;
    - `on_action`: `DraftEditorDone` → `close_draft_editor(true)`, `DraftEditorSend` → `send_from_draft_editor`;
    - a header row with "Edit draft", the muted hint "Esc or Ctrl+G to return · ⌘↩ to send", and a ghost "Discard changes" button that runs `close_draft_editor(false)`;
    - the body is `Editor::new(&editor).h(relative(1.)).appearance(false).bordered(false)` inside the content width.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn draft_editor_keybindings_are_registered() {
    let source = include_str!("../../src/actions.rs");
    for line in [
        r#"KeyBinding::new("ctrl-g", EditDraft, Some(composer.as_str()))"#,
        r#"KeyBinding::new("escape", DraftEditorDone, Some(draft.as_str()))"#,
        r#"KeyBinding::new("ctrl-g", DraftEditorDone, Some(draft.as_str()))"#,
        r#"KeyBinding::new("secondary-s", DraftEditorDone, Some(draft.as_str()))"#,
        r#"KeyBinding::new("secondary-enter", DraftEditorSend, Some(draft.as_str()))"#,
    ] {
        assert!(source.contains(line), "{line}");
    }
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test -p aether draft_editor_keybindings_are_registered`
Expected: FAIL.

- [ ] **Step 3: Implement**

The `"DraftEditor > Input"` bindings are more specific than the input's own escape and enter and than the root `SaveBuffer` and `BackToChat`, so they win. Register the group after the input's keys, like the existing context groups (:84-98).

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p aether actions`
Expected: PASS.

- [ ] **Step 5: Phase 4 gate**
  1. Run `cargo clippy --workspace`.
  2. Run the app and check, in both an Agent chat and a Direct chat:
     - Ctrl+G opens with the Draft and the cursor at the end;
     - Esc, Ctrl+G and Cmd+S each write the text back;
     - Cmd+Enter sends;
     - "Discard changes" keeps the old Draft;
     - Attachments are untouched;
     - Ctrl+G does nothing while the composer is disabled.
  3. Ask the user to approve Phase 4, then commit.
