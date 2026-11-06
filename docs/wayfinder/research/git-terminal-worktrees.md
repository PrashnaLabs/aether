# Research: git side panel, terminal drawer, worktree chats

Sources: Waku (GPL-3.0, design only, nothing copied) at `reference/`; T3 Code (MIT) read-only. Aether today: no git, terminal, or worktree code; chats already carry a `cwd` column (`crates/agent/src/store.rs`), which is the natural binding point. `alacritty_terminal` 0.26.0 is already in the local cargo registry.

## 1. Git panel

### How the references do it

Waku has no panel-level git library. Its right panel is a tabbed surface (diff, files, browser, terminal), width clamped 280..1000 px, per-session open/closed state restored on chat switch. The "diff" tab is a review of a chosen source: last agent turn (checkpoint refs), uncommitted, unstaged, staged, committed, branch. Staging is shown as a source filter, not as per-file buttons. Commit/push live in a separate modal (`commit_dialog.rs`) with a message box, an "include unstaged" checkbox, and Commit / Commit and push / Push buttons. All git work runs on the background executor; the UI paints cached state.

T3 Code: `GitVcsDriverCore.ts` gathers status, `VcsStatusBroadcaster.ts` caches and pushes it, `DiffPanel.tsx` renders it (per-turn or whole-thread selection, per-file diffs via a diff library), `GitActionsControl.tsx` is the commit/push/PR button.

### Git commands used (both references converge)

| Need | Command | Notes |
|---|---|---|
| Branch, ahead/behind, per-file state | `git status --porcelain=2 --branch -z` | T3 uses v2 with `--branch`; adds `--no-ahead-behind` when it wants a cheap pass. Waku uses `--porcelain=v1 --untracked-files=all`. v2 gives rename origin and head/upstream/ab header lines. |
| +/- per file, tracked | `git diff --numstat -z HEAD --` (staged: `--cached`) | Binary files report `-` `-`; treat as binary, no counts. T3 uses plumbing `diff-index -M --numstat HEAD` because porcelain `diff` can rewrite the index. |
| Untracked +/- | not in numstat | Waku counts lines itself from `ls-files --others --exclude-standard -z`, capped by file count, bytes per file, and total bytes. |
| Per-file unified diff | `git diff --no-color --no-ext-diff --no-textconv --find-renames [--cached] -- <path>` | `--no-ext-diff` and `--no-textconv` are a safety rule: repo config must not run programs. T3 adds `--minimal`. |
| Empty repo | diff against the empty tree hash `4b825dc6...` | Waku and T3 both special-case unborn HEAD. |
| Ahead count for push | `git rev-list --count <upstream>..HEAD`, or `--left-right --count HEAD...@{u}` | |
| Stage / unstage | `git add -- <path>`; `git restore --staged -- <path>` | Neither reference exposes per-file stage buttons prominently; Waku commits via `add -A` when "include unstaged" is ticked. |
| Commit | `git diff --cached --quiet` (exit 1 means something staged), then `git commit -m <msg>` | Waku refuses with a message when nothing is staged. |
| Push | `git push` if upstream set, else `git push --set-upstream <remote> <branch>`; refuses detached HEAD | |

All commands run with `current_dir` set to the chat cwd, never `-C` on user input, paths after `--`. Waku wraps spawn in a helper that sets a sane PATH and bounds time (120 s) and captured output (1 MiB stdout, 256 KiB stderr). T3 sets `GIT_OPTIONAL_LOCKS=0` style behavior and detects `index.lock` before reading status so a refresh never fights a running git.

### Refresh triggers

- Waku: on demand (panel open, source change, after checkpoint capture at turn end). No file watcher, no timer for local status.
- T3: refresh at turn end; a 30 s periodic poll with exponential backoff on failure (30 s base, 15 min cap); an upstream `fetch` throttled to every 15 s with a 5 s timeout and failure cooldown; per-cwd cache with a broadcaster so many viewers share one result.

### Performance limits worth copying

- Waku caps rendered diff at 50,000 lines and truncates with a flag; requests full-context diff only if the patch is under 32 MiB, else falls back to 3 lines of context. Virtualised line list with a binary search over line offsets per frame.
- Diff and status work never on the UI thread.
- Binary: show "binary file", no hunks. Large untracked: bounded line count.

### Aether recommendation (git panel)

Minimum valuable version, read-mostly:

- Right-side drawer toggled from the chat header and a shortcut; fixed 360 px default, drag clamp 280..720; open state remembered per chat.
- Header: branch name, ahead/behind chips, refresh button. Detached HEAD shows short SHA.
- Two sections, Staged and Changes (plus Untracked inside Changes): rows with status letter (M A D R ? with colors), path, +N/-M. Click a row for a unified diff in the panel (or a modal), added/removed line tinting, hunk headers, "binary" and "too large" placeholders.
- Actions: stage / unstage per file and per section, commit with a message box (disabled when nothing staged or message empty), push (button only when ahead or no upstream). Skip PR creation, branch switching, agent-generated messages for v1.
- Data: one `git status --porcelain=2 --branch -z` plus `git diff --numstat -z` (and `--cached`) per refresh, off-thread via the existing tokio/background executor; diff text fetched only for the selected file.
- Refresh: on open, on chat switch, when an agent turn finishes, on window focus, and the manual button. No watcher and no timer in v1 (Waku gets by without both). Debounce to one in-flight refresh; drop stale results by generation counter.
- Limits: truncate a single file diff at about 2,000 lines or 512 KiB with a "show anyway" cap-lift; skip counting untracked files beyond 200 files or 1 MiB.

Safety rules:

- No discard / `restore` of worktree files, no `reset --hard`, `clean`, `checkout --`, force push, or `--no-verify`. If discard is added later, it needs a confirm dialog naming the file and warning it is irreversible.
- Commit does not auto `add -A` unless the user ticks an explicit "stage everything" box.
- Always pass `--` before paths; reject paths that are absolute or contain `..`; run from the chat cwd.
- Surface git stderr (bounded, trimmed) in the panel; never swallow errors.
- Hooks run on commit (user config); surface their failure text. Do not skip hooks.

## 2. Terminal drawer

### Waku recipe (`src/terminal.rs`, ~2000 lines, plus a daemon-side variant)

Crates and features: `alacritty_terminal = "0.26"` with `default-features = false` (drops serde). Transitive: polling, vte, parking_lot, libc.

alacritty APIs used:

| Area | API |
|---|---|
| Emulator | `Term<T: EventListener>` built from `term::Config` (only `scrolling_history` set, 10,000) and a `Dimensions` impl (own struct with columns and rows). Held in `Arc<FairMutex<Term>>`. |
| PTY | `tty::Options { shell: Some(Shell::new(path, args)), working_directory, env, drain_on_exit: false }`, then `tty::new(&options, WindowSize, window_id)`. |
| IO thread | `EventLoop::new(term, listener, pty, drain_on_exit, ref_test)`, `event_loop.channel()` gives an `EventLoopSender`; `event_loop.spawn()` starts alacritty's own reader/writer thread. Input is `Msg::Input(bytes)`, resize is `Msg::Resize(WindowSize)`, teardown is `Msg::Shutdown`. No hand-written read loop. |
| Events | custom `EventListener::send_event` matching `Event::Wakeup`, `Title`, `ResetTitle`, `ClipboardStore/Load`, `PtyWrite`, `ColorRequest`, `TextAreaSizeRequest`, `Bell`, `Exit`, `ChildExit`. |
| Render data | `term.renderable_content()` gives `display_iter` (indexed cells), `cursor`, `selection`, `colors`, `display_offset`. Cell flags from `term::cell::Flags` (BOLD, ITALIC, DIM, INVERSE, underline set, STRIKEOUT, WIDE_CHAR_SPACER, HIDDEN). |
| Colors | `vte::ansi::{Color, NamedColor, Rgb}`; resolve `Color::Spec`, `Indexed`, `Named` against `content.colors` with a fallback palette keyed by theme darkness. |
| Modes | `TermMode` flags: `APP_CURSOR`, `BRACKETED_PASTE`, mouse modes. |
| Scroll/select | `term.scroll_display(Scroll::Delta(n))`, `Selection::new(SelectionType, Point, Side)`, `selection_to_string`. |
| Links | `RegexSearch`, `RegexIter` for URL detection, plus OSC 8 hyperlinks on cells. |

Spawn details:

- Shell: account default shell from passwd (outranks inherited `SHELL`), else env candidates, else `/bin/sh`; launched with `-l` so login files set PATH.
- Env: `TERM=xterm-256color`, `COLORTERM=truecolor`, PATH replaced by the app's resolved login-shell PATH (GUI apps launched from Finder have a thin PATH).
- cwd: `working_directory` on `Options`. Spawn happens on the background executor (it can block), result delivered back to the view.
- Waku's daemon variant (headless) uses `tty::new` and the `EventedReadWrite`/`OnResize` traits directly, with its own reader thread.

Output to UI: the PTY thread only sets an `AtomicBool` dirty flag via the listener. The view runs a 24 ms timer loop on the executor that swaps the flag and calls `cx.notify()` only when dirty. Other events (title, clipboard, exit) go through a channel drained in the same poll. This avoids a notify per read chunk.

Rendering in GPUI: each frame, `snapshot()` locks the term, walks `display_iter`, fills a `columns * rows` cell vector (blank-initialised), applies inverse, dim, selection, cursor colors, then builds one string per row plus style runs (merge adjacent cells with equal style). Each row is one fixed-height div with `StyledText::with_runs`, so per-row shaping is cached by GPUI's text system. Row height fixed (18 px), font size 12.5, monospace font; cell width is measured once from the font advance of `m` and used for column count and cursor overlay. Cursor: solid cell inversion when focused and blink phase visible; hollow outline overlay div when unfocused; hidden when the program hides it; 500 ms blink with a 300 ms pause after keystrokes. A `canvas` element records the grid bounds for mouse hit-testing.

Resize: columns/rows computed in render from panel width/height minus padding over cell size, min 20x8. If changed: `term.resize(dimensions)`, store new `WindowSize` in a shared mutex (used to answer size-report requests), send `Msg::Resize`.

Key mapping (own function, tested): enter is CR, tab is HT, shift-tab is `ESC [ Z`, backspace is DEL (0x7f), escape is ESC. Arrows use `ESC O x` under `APP_CURSOR`, else `ESC [ x`; with modifiers `ESC [ 1 ; m x` where m = 1 + shift + 2*alt + 4*ctrl. Home/End/Insert/Delete/PageUp/PageDown/F1..F12 follow xterm sequences with the same modifier parameter. Ctrl+letter maps to 1..26, plus `@ [ \ ] ^ _ ?` specials. Alt prefixes ESC. On macOS, Cmd+Left/Right/Backspace map to Ctrl-A / Ctrl-E / Ctrl-U. Printable text comes from the keystroke's `key_char`.

Paste: bracketed paste markers `ESC [ 200 ~ ... ESC [ 201 ~` only when `BRACKETED_PASTE` is set, else raw text. Copy: Cmd+C on macOS (Ctrl+Shift+C elsewhere so Ctrl+C stays SIGINT, with a test). Any typed key clears selection.

Scrollback: 10,000 lines; wheel accumulates fractional deltas into integer line scrolls; scrollbar adapter maps `display_offset` to an overlay scrollbar. New output while scrolled up stays put until the user returns to the bottom.

Close/kill: dropping the session sends `Msg::Shutdown`. The headless variant goes further: alacritty's PTY drop sends SIGHUP then waits without timeout, which can hang if a shell ignores SIGHUP, so Waku sends SIGHUP itself, waits a 250 ms grace, then SIGKILL, and joins the reader. On `Exit`/`ChildExit` the view flags `exited` and shows state instead of respawning.

Pitfalls Waku handles: dim and bold-italic flag unions (use `contains`, not `intersects`); wide-char spacer cells render as spaces; zero-width combining chars appended to the cell text; OSC color/size queries answered from the PTY thread using atomics (no UI access from that thread); clipboard OSC 52 bridged to the UI thread; panel width in columns must come from the measured font advance, not a constant; PATH inheritance; spawn off the UI thread; copy shortcut must not steal SIGINT on non-mac; soft-wrapped URLs detected across lines (cap 100 lines of search).

### T3 Code approach (for comparison)

Server-side terminal manager with node-pty, xterm.js in the browser. Drawer bottom docked: min 180 px, max 75% of window height, drag to resize, per-thread height remembered. Multiple terminals per thread, grouped, split horizontal or vertical, with a cap per group (`MAX_TERMINALS_PER_GROUP`). Closing asks for confirmation when a process is running. Resize RPC pushes cols/rows to the PTY; history replay keeps scrollback across reconnects. Terminal output can be attached to the chat as context.

### Aether recommendation (terminal)

Minimum valuable version:

- Add `alacritty_terminal = { version = "0.26", default-features = false }`. Use the `EventLoop` path (alacritty owns the reader thread); do not hand-roll PTY reads. Unix only for v1; gate with `cfg(unix)`.
- New `crates/ui` view (`TerminalView`) plus a thin session struct in `crates/app` holding `Arc<FairMutex<Term>>`, `EventLoopSender`, a dirty `AtomicBool`, and a channel for title/exit/clipboard. Poll at 24 to 30 ms only while the drawer is visible.
- Spawn on a background task: user's shell from `$SHELL` (fallback `/bin/sh`) with `-l`, cwd = chat cwd (worktree path when bound), env `TERM=xterm-256color`, `COLORTERM=truecolor`, and the PATH the agent drivers already resolve.
- Render: per-row `StyledText` with merged style runs, fixed cell height, font advance measured once; cursor overlay; colors from a 16-color theme palette plus xterm 256 cube and truecolor; selection with mouse drag, Cmd+C copy, Cmd+V paste with bracketed mode.
- Key mapping per the table above, implemented once in a pure function with unit tests (arrows, ctrl-letters, alt, enter, backspace, function keys, app-cursor).
- Drawer: bottom dock, default 260 px, drag clamp 140..75% of window; toggle shortcut; focus goes to the terminal on open and back to the composer on close. Key events must stop propagation so composer shortcuts do not fire.
- Lifecycle: one terminal per chat for v1 (map chat id to session; keep alive when drawer is hidden or chat is switched; kill on chat delete or app quit). Tabs for multiple terminals are phase 2.
- Kill: send `Msg::Shutdown`; also SIGHUP then SIGKILL after about 250 ms on a detached thread to avoid the hang Waku documents. On shell exit show "process exited, press Enter to restart".
- Skip in v1: link detection, search, split panes, mouse reporting to apps, image protocols, context-menu.

## 3. Worktrees

### Waku (`crates/waku-core/src/worktree.rs`)

- Opt-in at draft time ("New worktree" chip next to the local-checkout chip, with a base branch picker). Creation is deferred: the draft records only the choice, and the worktree is created on first submit, so empty drafts leave nothing on disk.
- Location: `~/.waku/worktrees/<project-uuid>/<slug>`. Slug derived from the first prompt (lowercased, dash-joined, max 48 bytes, default `new-worktree`), collisions resolved by a numeric suffix up to 100 tries, then a short session-id suffix.
- Branch: `waku/<slug>`, created with `git worktree add -b <branch> <path> <base>`. Existing branches or paths are skipped, never reused.
- Base: user-picked branch, else local branch named by `origin/HEAD`, else current branch, else `HEAD`. Verified with `rev-parse --verify <base>^{commit}` first. No fetch, so the user's checkout is untouched.
- Monorepo aware: the project path may be a subdirectory of the repo; the session cwd is the same relative subpath inside the worktree.
- Binding: the session stores `Worktree { path, branch }`; every file, git, terminal, and provider operation resolves its cwd from that. Sidebar shows the branch label. Branch picker marks branches checked out elsewhere as disabled for checkout.
- Listing: branch snapshot lists local branches with the worktree that owns each.
- Removal/cleanup: none found in the reference (`rg` for remove/prune/cleanup returns nothing). Worktrees accumulate.
- No setup script, no env file copying, no merge or PR flow tied to worktrees. Commit/push goes through the same commit dialog.

### T3 Code

- Location: `<worktrees dir>/<repo-name>/<branch-with-slashes-replaced>`; the dir defaults to `<app home>/worktrees`, configurable (must be absolute, not a filesystem root), with a list of previous directories remembered so older worktrees still count as managed.
- Create: `git worktree add -b <new> <path> <base>` with `checkout.workers` config passed through, a long timeout, and forced progress output (`GIT_PROGRESS_DELAY=0`) streamed to a setup card. Optional submodule handling by setting.
- Setup: a per-project "setup script" runs in the new worktree via a PTY before the first turn, with env pointing at the project root and worktree path, so users copy `.env` files, install deps, etc. A tracker shows stages (checkout, setup script) with tail lines, cancellable until the agent starts; cancel rolls the thread back.
- Thread binding: the thread stores `worktreePath` and branch; "Local / New worktree" env-mode selector and a base-branch picker sit in the branch toolbar. PR threads can reuse an existing worktree for the same branch and refuse when the branch is checked out in the main repo.
- Remove: `git worktree remove [--force] <path>` run with `status.showUntrackedFiles=normal` so untracked files count as dirty (otherwise a user's `showUntrackedFiles=no` would let git delete them). Without force, git refuses on dirty trees; force is a separate explicit choice. If git says it is already gone and the path is missing, run `git worktree prune` and treat as success. Long timeout (dependency-heavy trees).
- Shared worktrees: before removing on thread delete, a helper checks whether any other thread uses the same path; only an orphan is removed.
- Listing: `git worktree list --porcelain -z` (falls back to non-`-z` on git older than 2.36).
- Merge/PR: PR creation and "prepare PR thread" flows exist in the git manager; the worktree is just where the branch lives.

### Aether recommendation (worktrees)

Minimum valuable version:

- A "Worktree" toggle on the new-chat composer (local checkout vs new worktree) with a base branch dropdown defaulting to the repo's default branch. Create lazily on the first send.
- Path: `~/.local/share/aether/worktrees/<repo-name>-<short-hash-of-repo-path>/<slug>` (or the platform data dir aether already uses). Branch: `aether/<slug>`, slug from the first prompt, collision suffix, never reuse an existing branch or path.
- Command: `git worktree add -b <branch> <path> <base>` run in the repo root, with `rev-parse --verify` on the base first and no fetch. Set the chat's `cwd` to the worktree path (plus the same relative subdir if the project is not the repo root); the agent drivers, git panel, and terminal then follow `cwd` with no other changes. Store branch and worktree path on the chat row (two new nullable columns, migrated like `cwd`).
- Show branch name in the sidebar row; keep a "worktree" badge.
- Setup: none in v1. Optional later: a per-project command string run once in the new worktree (copying `.env` is the main real-world need); show its output in the terminal drawer rather than building a progress card.
- Cleanup: on chat delete or archive, offer "Remove worktree" (default off). Before removing: refuse if another chat shares the path; run `git status --porcelain` in it and, if dirty or the branch has commits not on any other branch or upstream, show a confirm naming the changes; use plain `git worktree remove <path>` (no `--force`) and only add `--force` after a second explicit confirm. If the path is already missing, run `git worktree prune`. Never delete the branch automatically.
- Listing: settings or palette entry reading `git worktree list --porcelain` filtered to the aether worktree root, to find orphans.
- Skip in v1: PR flows, merge-back, auto-fetch, submodules, shared worktrees across chats.

Safety rules:

- Only ever remove paths under the aether worktree root, after canonicalising (symlink-safe), and never the root or the repo itself.
- Never `rm -rf` as a fallback if git refuses.
- Worktree creation failure must leave the chat on its previous cwd with the error shown; no half-bound state.
- Bare/non-git folders: hide the toggle.

## 4. Summary and suggested order

| Feature | v1 scope | Biggest risk |
|---|---|---|
| Git panel | status, per-file diff, stage/unstage, commit, push; refresh on turn end, focus, manual | diff size on huge files; keep everything off the UI thread and capped |
| Terminal | one shell per chat in a bottom drawer, alacritty EventLoop, row StyledText rendering, key map, paste, scrollback, resize | GPUI key routing vs composer shortcuts; PTY teardown hanging |
| Worktrees | lazy create at first send, `cwd` rebind, remove with dirty check | orphan accumulation and unsafe deletes; guard with root check and confirms |

Order: worktrees first (tiny, only touches `cwd` and a git call), then git panel (uses the chat cwd so it works with worktrees), then terminal (largest, independent). All three read `cwd` from the chat, so no cross-feature coupling is needed.

Open questions for the user: Linux/Windows support for the terminal in v1 (Unix-only is assumed)? Worktree root location preference? Whether push belongs in v1 given credentials prompts (a PTY-less `git push` fails on interactive auth; the terminal drawer is the fallback).
