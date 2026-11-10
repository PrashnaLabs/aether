# Slice D audit: UI (`crates/ui`) and the rest of `crates/app`

## Scope
Owns all of `crates/ui` and `crates/app/src` except `terminal.rs`,
`explorer/`, `git_panel/`, `app/orchestrate.rs`, `app/control.rs`,
`settings.rs`, `hitl.rs`, `app/chat/`, `app/providers.rs` (other slices).

Audited end-to-end (callers included): `crates/app/src/browser.rs`
(wry/webview, omnibox, IPC, NewWindow), `open_in.rs`, `notify.rs`,
`attachments.rs`, `notes/` (`store.rs`, `external.rs`, `mod.rs`, `search*`),
`image_viewer{,.rs}/`, `hackernews/` (`client.rs`, `view.rs`, `types.rs`,
`mod.rs`), `session_workspace/` (`persistence.rs`, `model.rs`, `render.rs`,
`pane.rs`, `windows.rs`), `board/`, `project_page/`, `usage_view/`, `palette.rs`,
`side_panel.rs`, `sysinfo.rs`, `app/render.rs`, `app/sidebar.rs`, `app/links.rs`,
and `crates/ui/src/chat/` (transcript `markdown.rs`, `message.rs`, `live.rs`,
`scroller.rs`, `cache.rs`, `links.rs`, `sidebar/`).

## Method
Read each hot spot's code path end to end, then confirmed or rejected each
candidate. High-risk checks: process spawns (`open -a` / `explorer.exe` /
`osascript` / `git` / nvim+tmux), webview navigation and IPC exposure,
attachment/notes path handling (ADR 0001), image decode limits, untrusted
JSON parsing, and per-frame GPUI render costs (markdown `set_text`, workspace
serialization, list/scroller behavior). Ran `cargo clippy` and `cargo test`
for both touched crates (results below). No GUI launched.

## Findings

| ID | Severity | Location | One line | Status |
|----|----------|----------|----------|--------|
| D-SEC-1 | High | `crates/app/src/browser.rs` | Webview loaded `file://`/`data:`/`mailto:` from any source (omnibox, HN, `window.open`) | Fixed |
| D-SEC-2 | High | `crates/ui/src/chat/links.rs` | Agent-controlled markdown links fell through to `cx.open_url` for any OS scheme | Fixed |
| D-PERF-1 | Medium | `crates/ui/src/chat/markdown.rs` | `set_text` re-parsed every markdown block on every frame | Fixed |
| D-PERF-2 | Medium | `crates/app/src/session_workspace/persistence.rs`, `app/render.rs` | Workspace+drafts JSON serialized on every render | Fixed |
| D-SEC-3 | Low | `crates/app/src/attachments.rs` | `store_bytes` itself had no size cap; conversion could exceed 20 MB | Fixed |
| D-PERF-3 | Low | `crates/app/src/browser.rs` | Linux GTK pump loops every 16 ms even when hidden | Documented only |

### Reviewed, no finding
- `open_in.rs`: `open` requires absolute folders; `open_url` allowlists
  `http[s]` via `web_url`; command execution uses argv arrays
  (`open -a <app> <target>`, Windows `command(app).arg(target)`), absolute
  targets can't read as flags. Icon extraction uses fixed `/usr/bin/plutil` /
  `/usr/bin/sips` with argv args and static ids for cache names.
- `notify.rs`: `quoted()` escapes `\`, `"` and CR/LF before building the
  `osascript` string. Native-path `tag` is the numeric chat id
  (`app/chat/send.rs`), and the logo file uses `create_new` + `0o600`.
- `notes/store.rs`: `checked()` rejects `..`, requires the notes root
  prefix, and rejects symlinks on every existing ancestor; writes append,
  moves are atomic `hard_link` without overwrite, restore/trash re-check
  through `checked()`. `notes/external.rs`: shell quoting `'...'` with
  `'\''` escapes, control-char refusal, session-name dashes/colons blocked,
  stale-socket removal only for sockets with matching dev/ino proof.
- `image_viewer/data.rs` + `attachments.rs` decode path: extension allowlist,
  64 MB file cap, `image::Limits` (`max_alloc` = pixels*4), post-decode
  dimension/pixel/total-bytes checks. Viewer loads on the background
  executor.
- `hackernews/client.rs`: 8 s timeout, bounded parallelism (`buffered(8)`),
  `u64` id interpolation, deduped page-0 ids; titles/authors render as plain
  text, never HTML. Story `url` reaches the browser only through
  `browser.go`, so the D-SEC-1 allowlist now covers it. `text` field from
  the API is not rendered (only sample data uses it).
- `session_workspace/model.rs` `repair()`: sanitizes non-finite rects/ratios,
  prunes unknown chats, clamps `next_id`, and `decode_layout` rejects
  unknown versions. `atomic_write` uses a same-dir `.json.tmp` + rename.

### D-SEC-1 — High — `crates/app/src/browser.rs` — Fixed
What: the omnibox accepted `file://`, `data:`, `mailto:` and any `scheme://`
as webview URLs, and `NewWindow` (`window.open` / `target=_blank`)
bypassed even that check by calling `open_url` directly. wry had no
navigation handler, so page-initiated navigations were unrestricted.
Evidence: old `resolve_address` allowlist `about|data|mailto|file` plus any
`scheme + //`; `Native::NewWindow(url) => self.open_url(url, cx)` in
`drain`; `WebViewBuilder` had no `with_navigation_handler`. `file://` pages
run JS in a local origin; `data:text/html` runs attacker HTML/JS in the
panel with the IPC bridge installed.
Impact: a malicious web page (or a crafted HN story URL clicked via
`open_hn_link` -> `browser.go`) could load a local file or script page in
the embedded webview — code execution from opening a web page, in scope.
Fix: `resolve_address` now only passes `http[s]`/`about:` (all else becomes
a search); `navigable()` guards `Browser::open_url` (reject + user-visible
error otherwise), which also covers `window.open` / `target=_blank` and HN
story URLs. Existing `open_in::web_url` is reused.
Lead review: the agent also added `.with_navigation_handler(|url| navigable(&url))`.
The lead removed it before merging. On macOS wry calls that handler for every
frame, so it would cancel `about:srcdoc`, `blob:` and `data:` iframes and break
ordinary sites. WebKit already blocks web-origin navigation to `file:`, and the
only IPC message the page can send is "cycle tab", so in-page navigation is left
to WebKit's own policy.
Regression tests in `crates/app/tests/unit/browser.rs`:
`dangerous_schemes_become_searches`, `only_web_pages_are_navigable`.
Status: Fixed.

### D-SEC-2 — High — `crates/ui/src/chat/links.rs` — Fixed
What: `open_link` tried `OpenWebLink` (which only accepts `web_url`,
i.e. `http[s]`) and then fell back to `cx.open_url(href)` unconditionally,
so any other scheme (`javascript:`, `vscode://`, arbitrary protocol
handlers) reached the OS opener when a user clicked a markdown link from
agent output.
Evidence: `app/links.rs` installs `OpenWebLink` returning `false` for
non-`web_url` hrefs; `ui/src/chat/links.rs` (old) ended with
`cx.open_url(href)`. `markdown.rs` forwards every `TextView` link click to
`open_link`, and hrefs are agent-controlled.
Impact: one click on an agent-authored link launches any installed
protocol handler — code execution from opening a link, in scope.
Fix: new `openable()` helper (only `http`, `https`, `mailto`,
case-insensitive) guards the fallback; other schemes are dropped.
Regression test `only_web_and_mail_schemes_reach_the_os` in
`crates/ui/tests/unit/chat/links.rs`.
Status: Fixed.

### D-PERF-1 — Medium — `crates/ui/src/chat/markdown.rs` — Fixed
What: `markdown()` called `state.set_text(&source, cx)` for every block on
every `RenderOnce`, even when the source hadn't changed — i.e. a full
re-parse/re-highlight of every transcript block per frame while streaming
or under any root `notify()`.
Evidence: the old code computed `source` and unconditionally called
`set_text`. The file header notes `>4KB re-parses async`, making repeated
sets load-bearing.
Impact: O(total transcript) work per frame; long chats with large replies
re-parse megabytes per second.
Fix: `MarkdownEntry` tracks `shown`; `set_text` runs only when the source
differs. The async file-link path updates `shown` alongside `linked` so
the next render is skipped too.
Status: Fixed. (No rendering test added: the guard needs a live
`TextViewState`; `cargo test -p aether-ui` passes 283/283.)

### D-PERF-2 — Medium — `crates/app/src/session_workspace/persistence.rs`, `crates/app/src/app/render.rs` — Fixed
What: `ChatApp::render` calls `schedule_workspace_save` every frame, which
serializes the whole `WorkspaceModel` plus all session drafts to JSON
`String`s and a `format!` signature on every render, deduplicating only
after the work.
Evidence: `self.schedule_workspace_save(cx)` in `render()` line ~61;
`workspace_payload()` clones `self.workspace.model`, calls
`session_snapshots(cx)`, and `serde_json::to_string`s both payloads before
`save.last == signature` is compared.
Impact: O(workspace + drafts) allocation/copy/serialize per frame at
60 fps for the app's lifetime; grows with open panes and composer content.
Fix: `SaveState` caches the last saved `(WorkspaceModel, drafts)` and compares
the current model and `session_snapshots()` against it with `PartialEq`. Only
when something differs does it serialize to JSON and schedule a write.
`WindowLayout` equality ignores the transient `shown` field, which isn't
persisted. The snapshots are still cloned on each render, but the JSON
serialization and `format!` signature are gone. The 300 ms debounce and the
shutdown flush are unchanged. Unit tests cover an unchanged state, layout
changes, draft changes, and a live composer edit.
Lead review: codex's version also skipped building the snapshots, using a
second 67-line function that repeated the `session_snapshots` field logic.
The lead dropped it, because if the two ever drifted apart, saves would be
silently skipped.
Status: Fixed.

### D-SEC-3 — Low — `crates/app/src/attachments.rs` — Fixed
What: `store_bytes` had no size check of its own; the 20 MB cap lives in
the composer (`check_attachment`). Direct callers (`copy_one` re-reading
files) and the TIFF/BMP -> PNG path were uncovered, and a PNG re-encode
could still exceed 20 MB after an in-range source.
Evidence: `store_file` -> `store_bytes` reads and writes without a length
check; conversion writes `png` then `bytes = &png` unconditionally.
Impact: unbounded disk/memory write from files that bypass the composer
check; oversized PNGs stored contrary to the product limit. Low: all
current call paths check first.
Fix: reject `bytes.len() > MAX_ATTACHMENT_BYTES` on entry (before
decoding) and again after PNG conversion.
Regression test `oversized_bytes_are_rejected_before_writing` in
`crates/app/tests/unit/attachments.rs`.
Status: Fixed.

### D-PERF-3 — Low — `crates/app/src/browser.rs` — Documented only
What: `pump_gtk` loops every 16 ms calling `gtk::main_iteration_do` for the
life of the browser entity, even when the panel is hidden.
Evidence: `cx.spawn` loop with `timer(16ms)` and no visibility check.
Impact: constant wakeups/battery on Linux; no correctness issue.
Fix (proposed, not applied): only pump while `native_visible()` (or after
`host.set_visible(true)`), waking on show. Not applied: Linux-only path,
hard to verify here.
Status: Documented only.

## Seen outside my slice (not edited)
- `crates/app/src/app/chat/manage.rs:678` — `NoticeLink::Open(url) =>
  cx.open_url(&url)` opens a notice link through the OS with no scheme
  check; same class as D-SEC-2 but owned by the `app/chat/` slice.
- `crates/app/src/app/chat/manage.rs` — `cx.open_url` fallback for notice
  links likewise bypasses `web_url`; worth the same `openable` allowlist
  when that slice lands.

Lead review: false positive. `NoticeLink::Open` is only built in
`crates/ui/src/chat/notice.rs` `notice_link`, which accepts `http://` and
`https://` only. No change needed.

## Verification
- `cargo clippy --locked -p aether-ui --all-targets`: pass (1 pre-existing
  warning in `tests/unit/chat/spellcheck.rs`, unrelated).
- `cargo test --locked -p aether-ui`: 283 passed, 0 failed.
- `cargo clippy --locked -p aether --all-targets`: pass (pre-existing
  warnings only, none in changed files).
- `cargo test --locked -p aether -- --skip hackernews::client`: 840 passed,
  0 failed (4 filtered).
- `cargo fmt --all -- --check` filtered to changed files: clean.
