# Native Agent Chat, Composer & Live Usage Cockpit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform Aether's chat into The Native Cockpit: fix the Antigravity input bug, eliminate composer input focus lockup, add a dedicated `/` slash command button with an expanded command catalog, enable a live circular context gauge for all agent sessions, and provide an interactive account quota popover with live rate-limit lanes and token breakdown.

**Architecture:** 
1. Fix Antigravity CLI invocation in `crates/agent/src/driver/antigravity.rs` by passing the prompt text directly via `-p <prompt>` rather than `-p -` with ignored stdin.
2. Delegate card-wide mouse clicks in `crates/ui/src/chat/composer.rs` directly to `TextareaState::focus` and prevent probe canvas hit-test interference.
3. Expose a `/` command button in the composer toolbar and enrich the slash command engine in `crates/app/src/suggest.rs` with descriptions, argument hints, and agent actions.
4. Remove provider restrictions on the composer's context ring in `crates/app/src/app.rs`, map model context limits across all providers, and integrate an account quota fetcher (reading Claude OAuth usage via keychain/config) into an interactive `ContextRing` popover in `crates/ui/src/chat/context_ring.rs`.
5. Enhance tool execution presentation in `crates/ui/src/chat/tool_row.rs` with elapsed execution timers and friendly labels.

**Tech Stack:** Rust (GPUI, `gpui-kit`), Tokio, Reqwest, Keyring, Chrono, Serde.

**Spec:** [`docs/superpowers/specs/2026-10-17-native-agent-chat-and-usage-design.md`](../specs/2026-10-17-native-agent-chat-and-usage-design.md)

## Global Constraints

- Never pass `-p -` to `agy`: `agy` requires the prompt as an argument string to `-p` (`-p <prompt>`).
- The circular context ring must measure **active context window occupancy** (`latest context_tokens / model_context_limit`), never the cumulative sum of historic turns.
- Rate-limit lanes (5h rolling, 7d rolling) must source from Claude OAuth credentials when available and gracefully omit empty quota lanes for providers without rolling rate-limit APIs.
- All credential lookups and HTTP calls must run asynchronously in background tasks without blocking the UI thread.
- Clicking anywhere inside the composer box must reliably focus `TextareaState`.

## Review Focus

1. **Multiline / Escaped Antigravity Prompts**: Prompts with newlines, spaces, or quotes must be passed verbatim to `agy` via `Command::args` without shell truncation or syntax errors.
2. **Context Gauge for Unknown Models**: Models not in the explicit context length map must fall back gracefully to a sensible default (e.g., 200,000 tokens) rather than hiding the gauge or dividing by zero.
3. **Missing or Expired Claude Credentials**: If macOS Keychain has no `Claude Code-credentials` or the OAuth token has expired, the usage popover must cleanly show context tokens and an unobtrusive prompt to run `claude login`, rather than erroring or spinning forever.
4. **Composer History Key Conflicts**: When the slash command suggestion menu is open, arrow keys and `Enter` must navigate the menu and must not trigger `ComposerHistoryPrev` / `ComposerHistoryNext`.
5. **Rapid Chat Switching Focus**: Switching between chats while a query is running must not drop keyboard focus or point the composer to a stale chat.

---

### Task 1: Fix Antigravity Input Dispatch

**Files:**
- Modify: `crates/agent/src/driver/antigravity.rs:210-245`
- Test: `crates/agent/src/driver/antigravity.rs:340-390`

**Interfaces:**
- Consumes: `DriverStartOptions`, `Update`
- Produces: `fn launch_args(opts: &DriverStartOptions, session_id: Option<&str>, prompt: &str) -> Vec<String>`

- [ ] **Step 1: Update launch_args tests to verify prompt is passed directly to `-p`**

In `crates/agent/src/driver/antigravity.rs`, update `launch_args_smoke` and `launch_args_resume` tests to assert that `launch_args` includes `-p` followed by the actual prompt string:
```rust
#[test]
fn launch_args_smoke() {
    let opts = DriverStartOptions {
        binary: String::new(),
        cwd: std::path::PathBuf::from("/tmp"),
        model: "gemini-2.5-pro".into(),
        effort: "high".into(),
        access: AccessMode::Full,
        resume: String::new(),
    };
    let args = launch_args(&opts, None, "hello world");
    assert_eq!(
        args,
        vec![
            "--output-format",
            "stream-json",
            "--model",
            "gemini-2.5-pro",
            "--dangerously-skip-permissions",
            "--effort",
            "high",
            "-p",
            "hello world",
        ]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-agent driver::antigravity::tests::launch_args_smoke`
Expected: FAIL due to signature mismatch or assertion difference.

- [ ] **Step 3: Update `launch_args` and `prompt` in `crates/agent/src/driver/antigravity.rs`**

Update `launch_args` to accept `prompt: &str` and push `prompt.to_string()` after `-p`. In `prompt(&mut self, text: String)`, pass `&text` into `launch_args`. Configure `cmd.stdin(Stdio::null())` since `agy` reads the prompt directly from flag `-p`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-agent driver::antigravity::tests`
Expected: PASS (all 3 tests pass).

- [ ] **Step 5: Commit**

```bash
git add crates/agent/src/driver/antigravity.rs
git commit -m "fix(agent): pass prompt directly to agy -p flag instead of stdin hyphen"
```

---

### Task 2: Composer Card-Wide Click-to-Focus & Hit-Testing

**Files:**
- Modify: `crates/ui/src/chat/composer.rs:90-185`
- Modify: `crates/app/src/app.rs:1700-1750`

**Interfaces:**
- Consumes: `Entity<TextareaState>`, `Window`, `App`
- Produces: Full card hit-testing and reliable textarea activation.

- [ ] **Step 1: Write test for composer click-to-focus and bounds non-interception**

In `crates/ui/src/chat/composer.rs`, add a test verifying that `Composer` exposes card focus delegation and handles input interaction.

- [ ] **Step 2: Implement card-wide click delegation in `crates/ui/src/chat/composer.rs`**

In `Composer::render`:
1. On the inner container card `v_flex()` (line 138), attach `.on_mouse_down(gpui_kit::MouseButton::Left, cx.listener(...))` or click handler that immediately calls `self.input.update(cx, |s, cx| s.focus(window, cx))`.
2. Ensure the bounds probe canvas (line 125) does not eat mouse events: place it with `pointer_events: false` or order it cleanly so child interaction is unblocked.
3. In `crates/app/src/app.rs` in `select(ix, window, cx)`, ensure `self.composer.update(cx, |s, cx| s.focus(window, cx))` reliably requests focus and triggers `cx.notify()`.

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test -p aether-ui`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/ui/src/chat/composer.rs crates/app/src/app.rs
git commit -m "fix(ui): delegate composer card clicks to textarea and assert active focus"
```

---

### Task 3: Native Slash Command Engine & Dedicated `/` Trigger Button

**Files:**
- Modify: `crates/ui/src/chat/composer.rs:30-80, 150-170`
- Modify: `crates/app/src/suggest.rs:130-350`
- Modify: `crates/app/src/app.rs:2510-2540, 2980-3020`
- Test: `crates/app/src/suggest.rs:600-750`

**Interfaces:**
- Consumes: `SuggestionMenu`, `SlashCommand`, `Pick`
- Produces: Dedicated `/` button in composer toolbar, enriched slash command catalog.

- [ ] **Step 1: Write unit test for enriched slash commands catalog**

In `crates/app/src/suggest.rs`, add a unit test checking that all standard commands (`compact`, `review`, `diff`, `doctor`, `usage`, `init`, `clear`, `stop`) have descriptive explanatory text and correct enum variants.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-app suggest::tests`
Expected: FAIL until commands and descriptions match.

- [ ] **Step 3: Implement dedicated `/` button and enriched catalog**

1. In `crates/ui/src/chat/composer.rs`, add `on_commands: Option<ClickHandler>` to `Composer` with builder `.on_commands(...)`. In the leading toolbar, render a discrete `/` button chip (`composer_chip("slash", false, cx)`). Clicking it calls `on_commands`.
2. In `crates/app/src/app.rs`, connect `.on_commands(...)` on `composer_card`:
   When clicked, set composer text to `"/"` (if empty) or open suggestions directly via `this.open_slash_menu(cx)`, and focus the composer.
3. In `crates/app/src/suggest.rs`, enrich `slash_commands`:
   Ensure clear command descriptions and hints for all agent and driver actions.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-app suggest::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ui/src/chat/composer.rs crates/app/src/suggest.rs crates/app/src/app.rs
git commit -m "feat(ui): add dedicated slash command trigger button and rich command catalog"
```

---

### Task 4: Circular Context Gauge for All Agent Chats

**Files:**
- Modify: `crates/app/src/app.rs:2945-2985`
- Modify: `crates/ui/src/chat/context_ring.rs:1-120`
- Test: `crates/app/src/app.rs:5450-5490`

**Interfaces:**
- Consumes: `Chat::messages`, `context_tokens`
- Produces: Real-time circular context gauge in composer for all providers.

- [ ] **Step 1: Write test for provider-wide context limits and active occupancy**

In `crates/app/src/app.rs`, add a unit test verifying that `context_limit_for_model(provider, model)` correctly returns:
- Claude models: 200,000
- Codex / GPT-4o models: 128,000 / 200,000
- Antigravity / Gemini models: 1,000,000
- Fallback: 200,000

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-app context_limit`
Expected: FAIL.

- [ ] **Step 3: Implement provider-wide context ring in `crates/app/src/app.rs`**

1. Define `pub fn default_context_limit(provider: Provider, model: &str) -> u64` in `crates/agent/src/models.rs` or `crates/app/src/app.rs`.
2. In `composer_card` in `crates/app/src/app.rs`:
   Remove `.filter(|_| openrouter)`. Calculate `total` using `self.context_lengths.get(&chat_model).copied().unwrap_or_else(|| default_context_limit(chat.provider, &chat_model))`.
   Read `used` from the latest message's `context_tokens` (falling back to prompt + completion tokens if `context_tokens` is 0).
   Attach `ContextRing::new(used, total).model(chat_model)`.
3. In `crates/ui/src/chat/context_ring.rs`, ensure smooth color transitions: Green (< 60%), Yellow (60-80%), Orange (80-90%), Red (> 90%).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/app.rs crates/ui/src/chat/context_ring.rs crates/agent/src/models.rs
git commit -m "feat(ui): enable live circular context gauge for all agent provider sessions"
```

---

### Task 5: Account Plan Quota Telemetry & Usage Popover

**Files:**
- Create: `crates/agent/src/usage.rs`
- Modify: `crates/agent/src/lib.rs`
- Modify: `crates/ui/src/chat/context_ring.rs`
- Modify: `crates/app/src/app.rs`
- Test: `crates/agent/src/usage.rs`

**Interfaces:**
- Consumes: macOS Keychain `Claude Code-credentials`, `~/.claude/.credentials.json`, `reqwest`
- Produces: `pub struct PlanUsage { pub primary: Option<PlanWindow>, pub secondary: Option<PlanWindow> }`, `pub struct PlanWindow { pub used_percent: f64, pub resets_at: Option<DateTime<Utc>> }`

- [ ] **Step 1: Write failing unit test for `PlanUsage` parsing**

In `crates/agent/src/usage.rs`, write a unit test testing parsing of Anthropic `/api/oauth/usage` JSON payload containing 5-hour and 7-day rate limits.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-agent usage::tests`
Expected: FAIL.

- [ ] **Step 3: Implement `crates/agent/src/usage.rs`**

1. Implement `read_claude_credentials()`: reads from macOS Keychain service `Claude Code-credentials` or fallback `~/.claude/.credentials.json`.
2. Implement `fetch_claude_plan_usage()`: makes asynchronous GET request to `https://api.anthropic.com/api/oauth/usage` with header `anthropic-beta: oauth-2025-04-20`.
3. In `crates/ui/src/chat/context_ring.rs`, integrate an anchored `Popover`:
   When clicked, displays:
   - **Context Window**: progress bar with `used / total tokens` and breakdown (input, output, cache read).
   - **Account Limits**: 5-hour and 7-day limit progress bars with reset countdown timer strings (e.g., *"Resets in 2h 15m"*).
   - **Cost**: estimated session cost.
4. In `ChatApp`: periodic background refresh of `PlanUsage` (every 5 minutes or on turn completion).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-agent usage::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent/src/usage.rs crates/agent/src/lib.rs crates/ui/src/chat/context_ring.rs crates/app/src/app.rs
git commit -m "feat(telemetry): implement account quota fetcher and interactive usage popover"
```

---

### Task 6: Descriptive Tool Execution Badges & End-to-End Verification

**Files:**
- Modify: `crates/ui/src/chat/tool_row.rs:80-140, 300-380`
- Modify: `crates/ui/src/chat/message.rs:180-240`
- Test: Workspace integration tests

**Interfaces:**
- Consumes: `ToolCall`, `ToolStatus`, `ToolDetail`
- Produces: Polished, descriptive execution badges with elapsed timers.

- [ ] **Step 1: Write test for friendly tool name formatting**

In `crates/ui/src/chat/tool_row.rs`, test that tool titles are formatted descriptively (e.g., `bash` with `cargo check` displays `"Running cargo check"`, `edit` displays `"Editing <path>"`).

- [ ] **Step 2: Implement friendly execution badges in `crates/ui/src/chat/tool_row.rs`**

1. Display live elapsed time badge on tool rows (e.g. `· 0.4s` or `· 2.1s`).
2. Add clear success/failure exit code pills.
3. Clean up spacing and font weights for a native, polished feel.

- [ ] **Step 3: Run full workspace test suite to verify everything compiles and passes**

Run: `cargo test --workspace`
Expected: PASS (all tests in `aether-core`, `aether-agent`, `aether-ui`, and `aether-app` pass).

- [ ] **Step 4: Commit**

```bash
git add crates/ui/src/chat/tool_row.rs crates/ui/src/chat/message.rs
git commit -m "feat(ui): add descriptive tool execution pills with elapsed timers"
```

---

### Task 7: Force Terminal Always in Dark Mode

**Files:**
- Modify: `crates/app/src/terminal/view.rs:915-925`
- Modify: `crates/app/src/terminal/palette.rs:184-210`
- Test: `crates/app/src/terminal/view.rs:1280-1310`

**Interfaces:**
- Consumes: `ThemePalette::resolve`, `TerminalTheme`
- Produces: Guaranteed dark terminal palette regardless of app theme.

- [ ] **Step 1: Write test for terminal always-dark resolution**

In `crates/app/src/terminal/view.rs`, add a test verifying that even when the app theme is light (`is_dark = false`), terminal palette resolves to dark background (`#1e1e2e` or `#0a0a0a`) and light text (`#cdd6f4` or `#fafafa`).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-app terminal_always_dark`
Expected: FAIL.

- [ ] **Step 3: Update `TerminalView::render` to always resolve dark mode**

In `crates/app/src/terminal/view.rs`, ensure `ThemePalette::resolve` is always invoked with `is_dark = true` and dark fallback colors (`DARK` ANSI colors, dark background and light foreground):
```rust
let resolved = ThemePalette::resolve(
    self.terminal_theme,
    true, // Terminal is always in dark mode
    if theme.is_dark() { rgb(theme.foreground) } else { rgb(0xfafafa) },
    if theme.is_dark() { rgb(theme.background) } else { rgb(0x0a0a0a) },
);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/terminal/view.rs crates/app/src/terminal/palette.rs
git commit -m "feat(terminal): ensure terminal always renders in dark mode"
```
