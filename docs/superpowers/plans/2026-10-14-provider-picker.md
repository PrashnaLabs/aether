# Provider Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pick OpenRouter / Claude / Pi / OpenCode per chat from the composer, pick a discovered model within it, and configure CLIs and the workspace folder in Settings.

**Architecture:** Chat owns `provider` + `provider_model`; picks also update last-used defaults in `Settings`. `aether-agent` probes each CLI on a blocking tokio thread (`ProviderProbe`), the app keeps results in a map guarded by a per-provider generation counter, and the composer swaps the shared model select's items to the active chat's provider. Provider locks after the first message.

**Tech Stack:** Rust 2024, gpui-kit 0.7.3 (`gpui_kit::component::{menu, select, input}`), tokio, std::process.

**Spec:** `docs/superpowers/specs/2026-10-14-provider-picker-design.md`

## Global Constraints

- Never block the UI thread: no subprocess, filesystem walk, or login-shell call reachable from `render` or a UI handler. Probes go through `Agent::probe_provider` only.
- Empty CLI model = no `--model` flag. The picker row for it is the literal `"Default"` (`aether_ui::CLI_DEFAULT`).
- Provider switch allowed only when `chat.messages.is_empty() && chat.cancel.is_none()`.
- Never encode meaning in color alone: disabled menu rows carry text (`(not installed)`, `Locked to … in this chat`).
- No new dependencies.
- **No commits.** The user commits by hand. Each task ends with tests passing and the diff left unstaged.
- Comments: none unless a block is genuinely non-obvious; one line max, match file style.

## Review Focus

- A CLI list command that hangs (network, auth prompt) must not leave "Checking…" forever: probe lands with no models after the timeout. Test in Task 2.
- `pi --list-models` output with warning lines above the table must not yield garbage ids. Test in Task 2.
- Legacy OpenRouter chats (empty `provider_model`) must keep sending with `Settings.model`. Test in Task 4.
- Picking `Default` on a CLI chat must reach the driver as an empty model even when the global map holds one. Test in Task 4.
- The chat's current provider must stay selectable/checked in the menu even when its CLI is missing; only other rows disable. Test in Task 4.

---

### Task 1: `Provider` helpers in core

**Files:**
- Modify: `crates/core/src/settings.rs` (the `Provider` enum, ~line 40-48; tests module at the end)

**Interfaces:**
- Consumes: nothing.
- Produces: `Provider: Hash`, `Provider::ALL: [Provider; 4]` (OpenRouter first), `Provider::key(self) -> &'static str`, `Provider::label(self) -> &'static str`. Used by Tasks 2, 4, 5.

- [ ] **Step 1: Write the failing test** (append inside `mod tests` in `crates/core/src/settings.rs`)

```rust
    #[test]
    fn provider_keys_match_their_saved_names() {
        for p in Provider::ALL {
            assert_eq!(serde_json::to_string(&p).unwrap(), format!("\"{}\"", p.key()));
        }
        assert_eq!(Provider::ALL[0], Provider::OpenRouter);
        assert_eq!(Provider::Claude.label(), "Claude Code");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-core provider_keys_match -- --nocapture`
Expected: FAIL to compile, `no associated item named ALL`.

- [ ] **Step 3: Implement**

Add `Hash` to the derive list of `Provider`:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
```

Add right after the enum:

```rust
impl Provider {
    pub const ALL: [Self; 4] = [Self::OpenRouter, Self::Claude, Self::Pi, Self::OpenCode];

    /// Saved name, also the `Settings.provider_models` key.
    pub fn key(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Claude => "claude",
            Self::Pi => "pi",
            Self::OpenCode => "opencode",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::Claude => "Claude Code",
            Self::Pi => "Pi",
            Self::OpenCode => "OpenCode",
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p aether-core`
Expected: PASS (all).

- [ ] **Step 5: Stop.** No commit; leave diff for the user.

---

### Task 2: CLI discovery in `aether-agent` + home cwd

**Files:**
- Create: `crates/agent/src/driver/catalog.rs`
- Modify: `crates/agent/src/driver/mod.rs` (add `pub mod catalog;` + re-export)
- Modify: `crates/agent/src/driver/spawn.rs` (`fn home_dir` → `pub fn home_dir`)
- Modify: `crates/agent/src/agent.rs` (`opencode_hub_reachable` → `pub(crate)`; new `Agent::probe_provider`; `send_cli` cwd default)
- Modify: `crates/agent/src/lib.rs` (export `ProviderProbe`)

**Interfaces:**
- Consumes: Task 1 `Provider` (no new items needed; `Provider::Hash` not required here).
- Produces:
  - `aether_agent::ProviderProbe { pub installed: bool, pub path: Option<PathBuf>, pub models: Vec<String> }` (`Clone, Debug, Default, PartialEq`).
  - `Agent::probe_provider(&self, provider: Provider, settings: &Settings) -> impl Future<Output = ProviderProbe> + 'static`.
  - `aether_agent::driver::catalog::{probe, parse_pi_list, parse_opencode_list}`.

- [ ] **Step 1: Write the failing tests** — create `crates/agent/src/driver/catalog.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use aether_core::{Provider, Settings};

    use super::*;
    use crate::driver::DriverStartOptions;

    #[test]
    fn pi_list_reads_provider_and_model_columns_after_the_header() {
        let out = "Warning: update available\n\
                   provider    model            context  max-out\n\
                   opencode    big-pickle       200K     32K\n\
                   anthropic   claude-opus-4-6  1M       128K\n\n";
        assert_eq!(
            parse_pi_list(out),
            ["opencode/big-pickle", "anthropic/claude-opus-4-6"]
        );
        assert!(parse_pi_list("no table here").is_empty());
    }

    #[test]
    fn opencode_list_keeps_slash_ids_only() {
        let out = "google/gemini-2.5-pro\n\n  openai/gpt-5  \nINFO 2026/10/14 started\n";
        assert_eq!(
            parse_opencode_list(out),
            ["google/gemini-2.5-pro", "openai/gpt-5"]
        );
    }

    #[test]
    fn claude_lists_its_aliases_only_when_found() {
        let missing = Settings {
            claude_binary: "/definitely/not/here-xyz".into(),
            ..Settings::default()
        };
        assert_eq!(probe(Provider::Claude, &missing), ProviderProbe::default());
        let found = Settings {
            claude_binary: "/bin/sh".into(),
            ..Settings::default()
        };
        let probe = probe(Provider::Claude, &found);
        assert!(probe.installed);
        assert_eq!(probe.models, ["opus", "sonnet", "haiku"]);
    }

    #[test]
    fn hung_list_command_times_out() {
        let opts = DriverStartOptions {
            binary: "/bin/sh".into(),
            cwd: "/".into(),
            model: String::new(),
        };
        let start = Instant::now();
        assert_eq!(
            list_output(&opts, "sh", &["-c", "sleep 5"], Duration::from_millis(200)),
            None
        );
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(
            list_output(&opts, "sh", &["-c", "echo a/b"], Duration::from_secs(5)).as_deref(),
            Some("a/b\n")
        );
    }
}
```

Add to `crates/agent/src/driver/mod.rs` after `pub mod opencode;`:

```rust
pub mod catalog;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p aether-agent catalog`
Expected: FAIL to compile, `cannot find function parse_pi_list`.

- [ ] **Step 3: Implement** — put above the test module in `catalog.rs`:

```rust
use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use aether_core::{Provider, Settings};

use super::DriverStartOptions;
use super::spawn::{command, find_binary, home_dir};

/// Whether a CLI is usable and which model ids it offers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProviderProbe {
    pub installed: bool,
    pub path: Option<PathBuf>,
    pub models: Vec<String>,
}

const CLAUDE_ALIASES: [&str; 3] = ["opus", "sonnet", "haiku"];
const LIST_TIMEOUT: Duration = Duration::from_secs(20);

/// Blocking: searches PATH and may run the CLI. Keep off the UI thread.
pub fn probe(provider: Provider, settings: &Settings) -> ProviderProbe {
    let (name, args): (&str, &[&str]) = match provider {
        Provider::Claude => ("claude", &[]),
        Provider::Pi => ("pi", &["--list-models"]),
        Provider::OpenCode => ("opencode", &["models"]),
        Provider::OpenRouter => return ProviderProbe::default(),
    };
    let opts = DriverStartOptions::for_provider(settings, provider, home_dir());
    let path = find_binary(name, &opts.binary);
    let models = match (provider, &path) {
        (_, None) => Vec::new(),
        (Provider::Claude, Some(_)) => CLAUDE_ALIASES.map(String::from).to_vec(),
        (_, Some(_)) => list_output(&opts, name, args, LIST_TIMEOUT)
            .map(|out| match provider {
                Provider::Pi => parse_pi_list(&out),
                _ => parse_opencode_list(&out),
            })
            .unwrap_or_default(),
    };
    let installed = path.is_some()
        || (provider == Provider::OpenCode && crate::agent::opencode_hub_reachable());
    ProviderProbe {
        installed,
        path,
        models,
    }
}

/// Stdout of a successful run, or `None` on failure or after `timeout`.
fn list_output(
    opts: &DriverStartOptions,
    name: &str,
    args: &[&str],
    timeout: Duration,
) -> Option<String> {
    let mut cmd = command(opts, name)?;
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if name == "pi" {
        cmd.env("PI_SKIP_VERSION_CHECK", "1");
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        stdout.read_to_string(&mut out).map(|_| out)
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return reader.join().ok()?.ok(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// `pi --list-models` table rows as `provider/model`; lines above the
/// header are noise.
pub fn parse_pi_list(out: &str) -> Vec<String> {
    out.lines()
        .skip_while(|line| !line.trim_start().starts_with("provider"))
        .skip(1)
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            Some(format!("{}/{}", cols.next()?, cols.next()?))
        })
        .collect()
}

/// `opencode models` prints one `provider/model` per line.
pub fn parse_opencode_list(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|line| line.contains('/') && !line.contains(char::is_whitespace))
        .map(String::from)
        .collect()
}
```

Re-export in `crates/agent/src/driver/mod.rs` next to the other `pub use` lines:

```rust
pub use catalog::ProviderProbe;
```

In `crates/agent/src/driver/spawn.rs`, change `fn home_dir() -> PathBuf {` to `pub fn home_dir() -> PathBuf {`.

In `crates/agent/src/lib.rs`, add after `pub use agent::{Agent, Cancel, Reply};`:

```rust
pub use driver::ProviderProbe;
```

In `crates/agent/src/agent.rs`:

1. Change `fn opencode_hub_reachable() -> bool {` to `pub(crate) fn opencode_hub_reachable() -> bool {`.
2. Change the import line `use crate::driver::spawn::find_binary;` to `use crate::driver::spawn::{find_binary, home_dir};` and `use crate::driver::{ClaudeDriver, Driver, DriverStartOptions, OpencodeDriver, PiDriver};` to `use crate::driver::{ClaudeDriver, Driver, DriverStartOptions, OpencodeDriver, PiDriver, ProviderProbe, catalog};`.
3. Add after `list_models`:

```rust
    /// Whether `provider`'s CLI is usable and which models it offers.
    pub fn probe_provider(
        &self,
        provider: Provider,
        settings: &Settings,
    ) -> impl Future<Output = ProviderProbe> + 'static {
        let settings = settings.clone();
        let task = self
            .rt
            .spawn_blocking(move || catalog::probe(provider, &settings));
        async move { task.await.unwrap_or_default() }
    }
```

4. In `send_cli`, replace

```rust
        let cwd = if settings.workspace_root.is_empty() {
            std::path::PathBuf::from(".")
        } else {
```

with

```rust
        let cwd = if settings.workspace_root.is_empty() {
            home_dir()
        } else {
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p aether-agent`
Expected: PASS (all, including existing driver/store tests).

- [ ] **Step 5: Stop.** No commit.

---

### Task 3: CLI model groups + disabled picker in `aether-ui`

**Files:**
- Modify: `crates/ui/src/chat/model_picker.rs`
- Modify: `crates/ui/src/chat/mod.rs:19` (export)

**Interfaces:**
- Consumes: nothing.
- Produces: `aether_ui::CLI_DEFAULT: &str = "Default"`, `aether_ui::cli_model_groups(ids: impl IntoIterator<Item = String>, current: &str) -> ModelGroups`, `ModelPicker::disabled(self, bool) -> Self`.

- [ ] **Step 1: Write the failing test** (append inside `mod tests` in `model_picker.rs`)

```rust
    #[test]
    fn cli_lists_default_first_and_keeps_a_custom_current() {
        let ids = ["anthropic/opus", "opus"].map(String::from);
        let groups = cli_group_ids(ids, "my/custom");
        let titles: Vec<&str> = groups.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(titles, ["CLI", "my", "anthropic", "Other"]);
        assert_eq!(groups[0].1, [CLI_DEFAULT]);
        assert_eq!(cli_group_ids(Vec::<String>::new(), CLI_DEFAULT).len(), 1);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-ui cli_lists_default_first`
Expected: FAIL to compile, `cannot find function cli_group_ids`.

- [ ] **Step 3: Implement**

Replace `model_groups` with a shared builder and add the CLI variant:

```rust
/// Picker row standing for an empty CLI model: no `--model` flag.
pub const CLI_DEFAULT: &str = "Default";

/// Picker items: free models first, then one group per provider. `current`
/// is always listed, even if the catalog doesn't include it.
pub fn model_groups(ids: impl IntoIterator<Item = String>, current: &str) -> ModelGroups {
    searchable(group_ids(ids, current))
}

/// CLI picker items: `Default` first, then ids grouped like `model_groups`.
pub fn cli_model_groups(ids: impl IntoIterator<Item = String>, current: &str) -> ModelGroups {
    searchable(cli_group_ids(ids, current))
}

fn searchable(groups: Vec<(String, Vec<String>)>) -> ModelGroups {
    SearchableVec::new(
        groups
            .into_iter()
            .map(|(title, ids)| {
                SearchableGroup::new(title).items(ids.into_iter().map(SharedString::from))
            })
            .collect::<Vec<_>>(),
    )
}

fn cli_group_ids(ids: impl IntoIterator<Item = String>, current: &str) -> Vec<(String, Vec<String>)> {
    let current = if current == CLI_DEFAULT { "" } else { current };
    let mut groups = vec![("CLI".to_string(), vec![CLI_DEFAULT.to_string()])];
    groups.extend(group_ids(ids, current));
    groups
}
```

Add `disabled` to `ModelPicker`:

```rust
pub struct ModelPicker {
    id: ElementId,
    state: Entity<ModelList>,
    bordered: bool,
    disabled: bool,
}
```

In `new`, add `disabled: false,`. Add the builder after `bordered`:

```rust
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
```

In `render`, chain `.disabled(self.disabled)` onto the `Select::new(&self.state)` builder (before `.id(self.id)`).

In `crates/ui/src/chat/mod.rs` change the export line to:

```rust
pub use model_picker::{CLI_DEFAULT, ModelGroups, ModelList, ModelPicker, cli_model_groups, model_groups};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p aether-ui`
Expected: PASS.

- [ ] **Step 5: Stop.** No commit.

---

### Task 4: Per-chat model state, provider chip, probes, palette (`crates/app`)

**Files:**
- Modify: `crates/app/src/app.rs`
- Modify: `crates/app/src/palette.rs` (model rows ~line 268-283, `run_palette` Model arm ~line 88)

**Interfaces:**
- Consumes: Task 1 `Provider::{ALL, key, label}` + `Hash`; Task 2 `aether_agent::ProviderProbe`, `Agent::probe_provider`, `aether_agent::driver::DriverStartOptions`; Task 3 `CLI_DEFAULT`, `cli_model_groups`, `ModelPicker::disabled`.
- Produces (used by Task 5 and palette):
  - `pub(crate) const CLIS: [Provider; 3]`
  - `ChatApp.probes: HashMap<Provider, ProviderProbe>` (pub(crate)), `ChatApp.probe_runs: HashMap<Provider, u64>`
  - `ChatApp::probe_providers(&mut self, providers: &[Provider], window, cx)` (pub(crate))
  - `ChatApp::sync_model_picker(&self, window, cx)`, `ChatApp::pick_model(&mut self, value: String, window, cx)` (pub(crate)), `ChatApp::pick_provider(&mut self, Provider, window, cx)`, `ChatApp::model_ids(&self, Provider) -> Vec<String>` (pub(crate))
  - `Chat::model(&self, &Settings) -> String`, `Chat::provider_locked(&self) -> bool`
  - free fns `pub(crate) fn picker_value(Provider, &str) -> String`, `fn stored_model(Provider, &str) -> String`, `fn set_default_model(&mut Settings, Provider, String)`, `fn menu_entry(Provider, Provider, bool, Option<bool>) -> (String, bool)`
  - `initial_model` stays, body simplified; `provider_key` and `ChatApp::set_model` are deleted.

- [ ] **Step 1: Write the failing tests** (append inside `mod tests` in `app.rs`)

```rust
    #[test]
    fn legacy_openrouter_chat_uses_the_global_model() {
        let settings = Settings {
            model: "x/global".into(),
            ..Settings::default()
        };
        let mut chat = chat_with("t", &[]);
        assert_eq!(chat.model(&settings), "x/global");
        chat.provider_model = "x/own".into();
        assert_eq!(chat.model(&settings), "x/own");
        chat.provider = Provider::Claude;
        chat.provider_model = String::new();
        assert_eq!(chat.model(&settings), "");
    }

    #[test]
    fn default_row_means_no_model_flag() {
        assert_eq!(picker_value(Provider::Pi, ""), CLI_DEFAULT);
        assert_eq!(stored_model(Provider::Pi, CLI_DEFAULT), "");
        assert_eq!(picker_value(Provider::OpenRouter, "a/b"), "a/b");
        let mut s = Settings::default();
        s.provider_models.insert("claude".into(), "opus".into());
        set_default_model(&mut s, Provider::Claude, String::new());
        let opts = aether_agent::driver::DriverStartOptions::for_provider(&s, Provider::Claude, "/");
        assert_eq!(opts.model, "");
    }

    #[test]
    fn provider_locks_after_the_first_message() {
        let mut chat = chat_with("t", &[]);
        assert!(!chat.provider_locked());
        chat.messages.push(Message::user("hi"));
        assert!(chat.provider_locked());
    }

    #[test]
    fn provider_menu_disables_missing_and_locked_but_never_current() {
        let row = |p, current, locked, installed| menu_entry(p, current, locked, installed);
        assert_eq!(
            row(Provider::Pi, Provider::OpenRouter, false, Some(false)),
            ("Pi (not installed)".to_string(), true)
        );
        assert_eq!(row(Provider::Pi, Provider::OpenRouter, false, None), ("Pi".to_string(), false));
        assert_eq!(
            row(Provider::Claude, Provider::OpenRouter, true, Some(true)),
            ("Claude Code".to_string(), true)
        );
        assert_eq!(
            row(Provider::Pi, Provider::Pi, true, Some(false)),
            ("Pi (not installed)".to_string(), false)
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p aether -- provider_ default_row legacy_openrouter`
Expected: FAIL to compile, `no method named model found for struct Chat`.

- [ ] **Step 3: Implement state helpers in `app.rs`**

Imports: extend

```rust
use aether_agent::{Agent, Cancel, ProviderProbe, Reply, SavedChat, Store};
use aether_ui::{
    AssistantMessage, CLI_DEFAULT, ChatScroller, ChatScrollerState, ChatSidebar, ChatStatus, Chip,
    Composer, ContextRing, Hero, ModelList, ModelPicker, SendButton, TurnUsage, UserMessage,
    cli_model_groups, format_tokens, model_groups,
};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
```

Add after the `Page` enum:

```rust
pub(crate) const CLIS: [Provider; 3] = [Provider::Claude, Provider::Pi, Provider::OpenCode];
```

Extend `impl Chat` (next to `status`):

```rust
    /// Model this chat sends with. Legacy OpenRouter chats saved none and
    /// use the global one.
    pub(crate) fn model(&self, settings: &Settings) -> String {
        match self.provider {
            Provider::OpenRouter if self.provider_model.is_empty() => settings.model.clone(),
            _ => self.provider_model.clone(),
        }
    }

    /// Provider can change only before the first message.
    fn provider_locked(&self) -> bool {
        !self.messages.is_empty() || self.cancel.is_some()
    }
```

Replace `initial_model` and delete `provider_key`:

```rust
/// Model a fresh chat on `provider` starts with: the last one picked there.
fn initial_model(settings: &Settings, provider: Provider) -> String {
    match provider {
        Provider::OpenRouter => settings.model.clone(),
        p => settings.provider_models.get(p.key()).cloned().unwrap_or_default(),
    }
}

fn set_default_model(settings: &mut Settings, provider: Provider, model: String) {
    match provider {
        Provider::OpenRouter => settings.model = model,
        p => {
            settings.provider_models.insert(p.key().into(), model);
        }
    }
}

/// Picker row for a stored model; a CLI's empty model shows as `Default`.
pub(crate) fn picker_value(provider: Provider, model: &str) -> String {
    if provider != Provider::OpenRouter && model.is_empty() {
        CLI_DEFAULT.into()
    } else {
        model.into()
    }
}

fn stored_model(provider: Provider, value: &str) -> String {
    if provider != Provider::OpenRouter && value == CLI_DEFAULT {
        String::new()
    } else {
        value.into()
    }
}

/// Label and disabled state of `provider`'s row in the provider menu.
/// `installed` is `None` while its probe runs.
fn menu_entry(
    provider: Provider,
    current: Provider,
    locked: bool,
    installed: Option<bool>,
) -> (String, bool) {
    let missing = installed == Some(false);
    let label = if missing {
        format!("{} (not installed)", provider.label())
    } else {
        provider.label().to_string()
    };
    (label, provider != current && (locked || missing))
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p aether -- provider_ default_row legacy_openrouter`
Expected: the four new tests PASS (other code may still warn about unused fns; fine until Step 6).

- [ ] **Step 5: Wire probes and picker sync in `app.rs`**

Fields on `ChatApp` (after `models`):

```rust
    pub(crate) probes: HashMap<Provider, ProviderProbe>,
    probe_runs: HashMap<Provider, u64>,
```

Initialize in `Self { .. }`: `probes: HashMap::new(), probe_runs: HashMap::new(),`. After `app.new_chat(window, cx);` add `app.probe_providers(&CLIS, window, cx);`.

Composer model subscription — replace the `cx.subscribe_in(&model, ...)` closure body:

```rust
            cx.subscribe_in(&model, window, |this, _, ev: &SelectEvent<_>, window, cx| {
                if let SelectEvent::Confirm(Some(id)) = ev {
                    this.pick_model(id.to_string(), window, cx);
                }
            }),
```

In `load_models`, replace the block from `let current = ...` through the `this.model.update(...)` call with:

```rust
                this.context_lengths = models
                    .iter()
                    .filter_map(|m| Some((m.id.clone(), m.context_length?)))
                    .collect();
                this.model_prices = models
                    .iter()
                    .filter_map(|m| Some((m.id.clone(), (m.prompt_price?, m.completion_price?))))
                    .collect();
                this.models = models.into_iter().map(|m| m.id).collect();
                this.sync_model_picker(window, cx);
```

Delete `set_model`. Add these methods in the first `impl ChatApp` block (after `load_models`):

```rust
    /// Rechecks each CLI in `providers` in the background; a newer run
    /// supersedes an older one still in flight.
    pub(crate) fn probe_providers(
        &mut self,
        providers: &[Provider],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for &provider in providers {
            let run = self.probe_runs.entry(provider).or_default();
            *run += 1;
            let run = *run;
            self.probes.remove(&provider);
            let probe = self.agent.probe_provider(provider, &self.settings);
            cx.spawn_in(window, async move |this, cx| {
                let probe = probe.await;
                this.update_in(cx, |this, window, cx| {
                    if this.probe_runs.get(&provider) != Some(&run) {
                        return;
                    }
                    this.probes.insert(provider, probe);
                    if this.chats[this.active].provider == provider {
                        this.sync_model_picker(window, cx);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    /// Ids the picker offers for `provider`, before the chat's own model.
    pub(crate) fn model_ids(&self, provider: Provider) -> Vec<String> {
        if provider == Provider::OpenRouter {
            return self.models.clone();
        }
        let mut ids = self
            .probes
            .get(&provider)
            .map(|probe| probe.models.clone())
            .unwrap_or_default();
        let custom = initial_model(&self.settings, provider);
        if !custom.is_empty() && !ids.contains(&custom) {
            ids.insert(0, custom);
        }
        ids
    }

    /// Points the composer's model list at the active chat's provider.
    fn sync_model_picker(&self, window: &mut Window, cx: &mut Context<Self>) {
        let chat = &self.chats[self.active];
        let provider = chat.provider;
        let current = picker_value(provider, &chat.model(&self.settings));
        let ids = self.model_ids(provider);
        let items = match provider {
            Provider::OpenRouter => model_groups(ids, &current),
            _ => cli_model_groups(ids, &current),
        };
        let current = SharedString::from(current);
        self.model.update(cx, |select, cx| {
            select.set_items(items, window, cx);
            select.set_selected_value(&current, window, cx);
        });
    }

    /// Sets the active chat's model and makes it its provider's default.
    pub(crate) fn pick_model(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.chats[ix].cancel.is_some() {
            return self.sync_model_picker(window, cx);
        }
        let provider = self.chats[ix].provider;
        let model = stored_model(provider, &value);
        self.chats[ix].provider_model = model.clone();
        if !self.chats[ix].messages.is_empty() {
            self.persist(ix);
        }
        self.update_settings(cx, |s| set_default_model(s, provider, model));
        self.sync_model_picker(window, cx);
    }

    /// Switches the active chat's provider; only before its first message.
    fn pick_provider(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        let chat = &mut self.chats[self.active];
        if chat.provider_locked() || chat.provider == provider {
            return;
        }
        chat.provider = provider;
        chat.provider_model = initial_model(&self.settings, provider);
        self.update_settings(cx, |s| s.provider = provider);
        self.sync_model_picker(window, cx);
    }
```

In `select`, add `self.sync_model_picker(window, cx);` right before the final `cx.notify();`.

- [ ] **Step 6: Send path in `submit`**

Replace

```rust
        let provider = chat.provider;
        let provider_model = chat.provider_model.clone();
```

with

```rust
        let provider = chat.provider;
        let model = chat.model(&self.settings);
```

Replace

```rust
        reply.model = match provider {
            Provider::OpenRouter => Some(self.settings.model.clone()),
            _ if provider_model.is_empty() => None,
            _ => Some(provider_model.clone()),
        };
```

with

```rust
        reply.model = (!model.is_empty()).then(|| model.clone());
```

Replace

```rust
        // Per-chat provider wins; its model overrides the global map entry,
        // and an empty model is omitted from the CLI flags.
        let mut effective = self.settings.clone();
        effective.provider = provider;
        if provider != Provider::OpenRouter && !provider_model.is_empty() {
            effective
                .provider_models
                .insert(provider_key(provider).into(), provider_model);
        }
```

with

```rust
        // Chat's own provider and model win; an empty CLI model omits `--model`.
        let mut effective = self.settings.clone();
        effective.provider = provider;
        set_default_model(&mut effective, provider, model);
```

- [ ] **Step 7: Provider chip + OpenRouter-only controls in `composer_card`**

Add method in the second `impl ChatApp` block:

```rust
    fn provider_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let chat = &self.chats[self.active];
        let (current, locked) = (chat.provider, chat.provider_locked());
        let installed: HashMap<Provider, bool> = self
            .probes
            .iter()
            .map(|(p, probe)| (*p, probe.installed))
            .collect();
        let view = cx.entity();
        Button::new("provider")
            .ghost()
            .small()
            .label(current.label())
            .tooltip("Provider")
            .dropdown_menu(move |menu, _, _| {
                let menu = if locked {
                    menu.label(format!("Locked to {} in this chat", current.label()))
                } else {
                    menu
                };
                Provider::ALL.into_iter().fold(menu, |menu, p| {
                    let (label, disabled) =
                        menu_entry(p, current, locked, installed.get(&p).copied());
                    let view = view.clone();
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(p == current)
                            .disabled(disabled)
                            .on_click(move |_, window, cx| {
                                view.update(cx, |this, cx| this.pick_provider(p, window, cx));
                            }),
                    )
                })
            })
    }
```

In `composer_card`:

1. After `let busy = ...;` add:

```rust
        let openrouter = self.chats[self.active].provider == Provider::OpenRouter;
        let chat_model = self.chats[self.active].model(&self.settings);
```

2. Replace the `context` computation's lookup so it reads:

```rust
        let context = self
            .context_lengths
            .get(&chat_model)
            .filter(|_| openrouter)
            .map(|&total| {
                ContextRing::new(used, total)
                    .model(chat_model.clone())
                    .cost(chat_usage)
            });
```

3. Replace the three `.leading(...)` calls (Search chip, Plan chip, ModelPicker) with:

```rust
        let provider_menu = self.provider_menu(cx);
        Composer::new(&self.composer)
            .when(openrouter, |composer| {
                composer
                    .leading(
                        Chip::new("web-search", "Search")
                            .icon(IconName::Globe)
                            .selected(self.settings.web_search)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_settings(cx, |s| s.web_search = !s.web_search);
                            })),
                    )
                    .leading(
                        Chip::new("plan-mode", "Plan")
                            .icon(IconName::Check)
                            .selected(self.settings.plan_mode)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_settings(cx, |s| s.plan_mode = !s.plan_mode);
                            })),
                    )
            })
            .leading(provider_menu)
            .leading(ModelPicker::new("model", &self.model).disabled(busy))
```

(the rest of the chain, `.when_some(editing, ...)` onward, is unchanged; `Composer::new` is already the start of the returned expression, so `let provider_menu` goes right before it).

- [ ] **Step 8: Palette** (`crates/app/src/palette.rs`)

Change the `run_palette` arm to:

```rust
            Some(PaletteCommand::Model(id)) => self.pick_model(id, window, cx),
```

Replace the `"Switch model"` block with:

```rust
        let active = &self.chats[self.active];
        if !self.palette.read(cx).query(cx).is_empty() && active.cancel.is_none() {
            let provider = active.provider;
            let current = picker_value(provider, &active.model(&self.settings));
            let mut ids = self.model_ids(provider);
            if provider != Provider::OpenRouter {
                ids.insert(0, CLI_DEFAULT.to_string());
            }
            groups.push((
                "Switch model",
                ids.into_iter()
                    .map(|id| {
                        (
                            CommandItem::new().label(id.clone()).checked(id == current),
                            Some(PaletteCommand::Model(id)),
                        )
                    })
                    .collect(),
            ));
        }
```

Imports in `palette.rs`: `use aether_core::{Appearance, Message, Provider, Reasoning, Role};`, `use aether_ui::{CLI_DEFAULT, CommandMenu};`, `use crate::app::{ChatApp, chat_markdown, picker_value};`.

- [ ] **Step 9: Build and test**

Run: `cargo build -p aether && cargo test -p aether`
Expected: build clean with no new warnings, all tests PASS.

- [ ] **Step 10: Stop.** No commit.

---

### Task 5: Settings "Providers" tab + OpenRouter default select

**Files:**
- Modify: `crates/app/src/app.rs` (fields, constructor, `load_models`, `pick_model`, `binary_override` helper)
- Modify: `crates/app/src/settings_view.rs`

**Interfaces:**
- Consumes: Task 4 `CLIS`, `probes`, `probe_providers`, `pick_model`, `sync_model_picker`, `model_ids`; Task 2 `ProviderProbe`, `aether_agent::driver::DriverStartOptions`.
- Produces: `ChatApp.default_model: Entity<ModelList>`, `ChatApp.binary_inputs` / `ChatApp.model_inputs: HashMap<Provider, Entity<InputState>>`, `fn binary_override(&mut Settings, Provider) -> Option<&mut String>`, `fn provider_status(Option<&ProviderProbe>, &str) -> String`, `ChatApp::choose_workspace`, `SettingsTab::Providers`.

- [ ] **Step 1: Write the failing test** — add at the end of `settings_view.rs`:

```rust
#[cfg(test)]
mod tests {
    use aether_agent::ProviderProbe;

    use super::*;

    #[test]
    fn provider_status_explains_each_probe_state() {
        assert_eq!(provider_status(None, ""), "Checking…");
        let found = ProviderProbe {
            installed: true,
            path: Some("/bin/pi".into()),
            models: vec!["a/b".into()],
        };
        assert_eq!(provider_status(Some(&found), ""), "Found at /bin/pi · 1 model");
        let hub = ProviderProbe {
            installed: true,
            ..ProviderProbe::default()
        };
        assert_eq!(provider_status(Some(&hub), ""), "Running as a background service");
        let missing = ProviderProbe::default();
        assert_eq!(provider_status(Some(&missing), "~/bin/pi"), "Not found at ~/bin/pi");
        assert_eq!(provider_status(Some(&missing), ""), "Not found on PATH");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether provider_status`
Expected: FAIL to compile, `cannot find function provider_status`.

- [ ] **Step 3: Implement `provider_status` + tab** in `settings_view.rs`

Add free fn:

```rust
/// Settings status line for a CLI: where it was found, or why it wasn't.
fn provider_status(probe: Option<&ProviderProbe>, binary: &str) -> String {
    match probe {
        None => "Checking…".into(),
        Some(ProviderProbe {
            path: Some(path),
            models,
            ..
        }) => {
            let noun = if models.len() == 1 { "model" } else { "models" };
            format!("Found at {} · {} {noun}", path.display(), models.len())
        }
        Some(ProviderProbe { installed: true, .. }) => "Running as a background service".into(),
        Some(_) if !binary.is_empty() => format!("Not found at {binary}"),
        Some(_) => "Not found on PATH".into(),
    }
}
```

Imports: `use aether_agent::ProviderProbe;`, `use aether_agent::driver::DriverStartOptions;`, add `IconName` to the `gpui_kit::component::{...}` list, and `use gpui_kit::{Context, PathPromptOptions, SharedString, Window, div, prelude::*, px};`, `use crate::app::{CLIS, ChatApp};`.

`SettingsTab`: add `Providers` after `Model`; `ALL: [Self; 5] = [Self::Appearance, Self::Model, Self::Providers, Self::Tools, Self::Account]`; label `Self::Providers => "Providers"`.

Model tab: change the "Default model" row to use `&self.default_model` and description `"Used for new OpenRouter chats. Also changeable from the composer."`.

Add the tab arm in `render_settings`:

```rust
            SettingsTab::Providers => {
                let root = settings.workspace_root.clone();
                page = page
                    .action(
                        Button::new("refresh-providers")
                            .ghost()
                            .small()
                            .icon(IconName::RefreshCw)
                            .label("Refresh")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.probe_providers(&CLIS, window, cx)
                            })),
                    )
                    .child(
                        SettingsSection::new("Workspace").child(
                            SettingsRow::new("Folder")
                                .description("Coding agents run here. Defaults to your home folder.")
                                .control(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            div()
                                                .max_w(px(280.))
                                                .truncate()
                                                .text_sm()
                                                .text_color(muted)
                                                .child(if root.is_empty() {
                                                    "Home folder".to_string()
                                                } else {
                                                    root.clone()
                                                }),
                                        )
                                        .when(!root.is_empty(), |row| {
                                            row.child(
                                                Button::new("reset-workspace")
                                                    .ghost()
                                                    .small()
                                                    .label("Reset")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.update_settings(cx, |s| {
                                                            s.workspace_root.clear()
                                                        })
                                                    })),
                                            )
                                        })
                                        .child(
                                            Button::new("choose-workspace")
                                                .small()
                                                .label("Choose…")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.choose_workspace(window, cx)
                                                })),
                                        ),
                                ),
                        ),
                    )
                    .child(CLIS.into_iter().fold(
                        SettingsSection::new("Coding agents"),
                        |section, p| {
                            let binary = DriverStartOptions::for_provider(settings, p, "").binary;
                            section.child(
                                SettingsRow::new(p.label())
                                    .description(provider_status(self.probes.get(&p), &binary))
                                    .stacked()
                                    .control(
                                        h_flex()
                                            .gap_3()
                                            .child(labeled_input("Binary path", &self.binary_inputs[&p], muted))
                                            .child(labeled_input("Default model", &self.model_inputs[&p], muted)),
                                    ),
                            )
                        },
                    ));
            }
```

Add helpers in `settings_view.rs`:

```rust
fn labeled_input(
    label: &'static str,
    input: &gpui_kit::Entity<gpui_kit::component::input::InputState>,
    muted: gpui_kit::Hsla,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .gap_1()
        .child(div().text_xs().text_color(muted).child(label))
        .child(Input::new(input))
}

impl ChatApp {
    /// Asks for a folder and makes it the coding agents' workspace.
    fn choose_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.pop() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.update_settings(cx, |s| s.workspace_root = path.display().to_string())
            })
            .ok();
        })
        .detach();
    }
}
```

(If `PathPromptOptions` in gpui-kit 0.7.3 has extra fields, fill them with their defaults — check `~/.cargo/registry/src/*/gpui-pre-0.3.9/src/platform.rs` `struct PathPromptOptions`.)

- [ ] **Step 4: Wire state in `app.rs`**

Fields (after `model`):

```rust
    pub(crate) default_model: Entity<ModelList>,
    pub(crate) binary_inputs: HashMap<Provider, Entity<InputState>>,
    pub(crate) model_inputs: HashMap<Provider, Entity<InputState>>,
```

Helper (next to `set_default_model`):

```rust
fn binary_override(settings: &mut Settings, provider: Provider) -> Option<&mut String> {
    match provider {
        Provider::Claude => Some(&mut settings.claude_binary),
        Provider::Pi => Some(&mut settings.pi_binary),
        Provider::OpenCode => Some(&mut settings.opencode_binary),
        Provider::OpenRouter => None,
    }
}
```

In `new`, after the `model` entity:

```rust
        let default_model = cx.new(|cx| {
            SelectState::new(
                model_groups([], &settings.model),
                Some(IndexPath::default()),
                window,
                cx,
            )
            .searchable(true)
        });
        let mut binary_inputs = HashMap::new();
        let mut model_inputs = HashMap::new();
        for p in CLIS {
            let binary = aether_agent::driver::DriverStartOptions::for_provider(&settings, p, "").binary;
            let model = initial_model(&settings, p);
            binary_inputs.insert(
                p,
                cx.new(|cx| {
                    let mut s = InputState::new(window, cx).placeholder("Search PATH");
                    s.set_value(binary, window, cx);
                    s
                }),
            );
            model_inputs.insert(
                p,
                cx.new(|cx| {
                    let mut s = InputState::new(window, cx).placeholder("CLI default");
                    s.set_value(model, window, cx);
                    s
                }),
            );
        }
```

Make `let subscriptions = vec![...]` into `let mut subscriptions = vec![...]`, add inside the vec:

```rust
            cx.subscribe_in(&default_model, window, |this, _, ev: &SelectEvent<_>, window, cx| {
                if let SelectEvent::Confirm(Some(id)) = ev {
                    let id = id.to_string();
                    this.update_settings(cx, |s| s.model = id);
                    this.sync_model_picker(window, cx);
                }
            }),
```

and after the vec:

```rust
        for (&provider, input) in &binary_inputs {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this, input, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Change => {
                        let value = input.read(cx).value().trim().to_string();
                        this.update_settings(cx, |s| {
                            if let Some(binary) = binary_override(s, provider) {
                                *binary = value;
                            }
                        });
                    }
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        this.probe_providers(&[provider], window, cx)
                    }
                    _ => {}
                },
            ));
        }
        for (&provider, input) in &model_inputs {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this, input, ev: &InputEvent, window, cx| {
                    if let InputEvent::Change = ev {
                        let value = input.read(cx).value().trim().to_string();
                        this.update_settings(cx, |s| {
                            s.provider_models.insert(provider.key().into(), value);
                        });
                        if this.chats[this.active].provider == provider {
                            this.sync_model_picker(window, cx);
                        }
                    }
                },
            ));
        }
```

Initialize the three fields in `Self { .. }`.

In `load_models`, after `this.sync_model_picker(window, cx);` add:

```rust
                let current = SharedString::from(this.settings.model.clone());
                let items = model_groups(this.models.clone(), &current);
                this.default_model.update(cx, |select, cx| {
                    select.set_items(items, window, cx);
                    select.set_selected_value(&current, window, cx);
                });
```

In `pick_model`, replace `self.update_settings(cx, |s| set_default_model(s, provider, model));` with:

```rust
        match provider {
            Provider::OpenRouter => {
                let value = SharedString::from(model.clone());
                self.default_model.update(cx, |select, cx| {
                    select.set_selected_value(&value, window, cx)
                });
            }
            p => {
                if let Some(input) = self.model_inputs.get(&p) {
                    let value = model.clone();
                    input.update(cx, |input, cx| input.set_value(value, window, cx));
                }
            }
        }
        self.update_settings(cx, |s| set_default_model(s, provider, model));
```

- [ ] **Step 5: Build and test the workspace**

Run: `cargo build && cargo test --workspace`
Expected: build clean with no new warnings, all tests PASS.

- [ ] **Step 6: Stop.** No commit.
