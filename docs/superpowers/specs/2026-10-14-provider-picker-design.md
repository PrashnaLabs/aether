# Provider picker and provider state

Date: 2026-10-14. Builds on `docs/superpowers/plans/2026-10-13-cli-providers.md` (backend shipped).

## Goal

Let the user pick OpenRouter, Claude, Pi, or OpenCode per chat from the composer, pick a model within that provider from a discovered list, and configure CLI binaries, default models, and the workspace folder in Settings. Reference: Waku (`reference/src/app/composer.rs` `render_provider_model_control`, `reference/crates/waku-core/src/model_catalog.rs`, `reference/src/app/settings.rs` `render_providers_settings`).

## Decisions

- Chat owns provider and model. Picks also update defaults for new chats (last-used).
- Provider locks after the chat's first message (Waku `can_choose_model`). Model may change within the provider while the chat is idle.
- CLI model lists are discovered in the background; Claude uses its stable aliases.
- Composer UI = provider chip with dropdown + existing searchable model select.
- Custom model ids are entered in Settings only.

## State

- `Chat.provider` and `Chat.provider_model` are the truth for that chat. OpenRouter chats keep their model in `provider_model` too. An OpenRouter chat with an empty `provider_model` (legacy row) falls back to `Settings.model`.
- For a CLI chat, empty `provider_model` means "CLI default": no `--model` flag.
- Defaults for new chats: `Settings.provider`, `Settings.model` (OpenRouter), `Settings.provider_models[key]` (each CLI). A provider pick writes `Settings.provider`; a model pick writes that provider's default.
- Lock rule: `chat.messages.is_empty() && chat.cancel.is_none()` allows a provider switch. Model switch requires `chat.cancel.is_none()`.
- Send path: OpenRouter sends use the chat's model (`effective.model`), not the global `Settings.model`. `reply.model` records the same value.

## Core (`crates/core/src/settings.rs`)

- `Provider::ALL: [Provider; 4]`, `Provider::key() -> &'static str` (`openrouter`, `claude`, `pi`, `opencode`, matching serde names), `Provider::label() -> &'static str` (`OpenRouter`, `Claude Code`, `Pi`, `OpenCode`).
- `crates/app` uses these in place of its local `provider_key`. `store.rs` and `driver/mod.rs` keep their own maps.

## Discovery (`crates/agent`)

- `pub struct ProviderProbe { pub installed: bool, pub path: Option<PathBuf>, pub models: Vec<String> }`, exported from the crate root. `installed` also covers an OpenCode hub with no binary.
- `Agent::probe_provider(&self, provider: Provider, settings: &Settings) -> impl Future<Output = ProviderProbe> + 'static`, run with `rt.spawn_blocking`, never on the UI thread.
  - Claude: `path = find_binary("claude", override)`; `models = ["opus", "sonnet", "haiku"]` when found.
  - Pi: `path = find_binary("pi", override)`; models = `pi --list-models` stdout through `parse_pi_list`, env `PI_SKIP_VERSION_CHECK=1`.
  - OpenCode: `path = find_binary("opencode", override)`; models = `opencode models` stdout through `parse_opencode_list`. Installed also when the hub answers (existing `opencode_hub_reachable`), with no models.
  - OpenRouter: not probed.
- Both commands are built with the shared `spawn::command` (repaired `PATH`), cwd = home.
- `parse_pi_list`: skip the header row (`provider model ...`), take the first two whitespace columns as `provider/model`, skip blank or malformed lines.
- `parse_opencode_list`: one trimmed non-empty line per id, kept only if it contains `/`.
- Failure (spawn error, non-zero exit) yields an empty list, never an error.

## Session cwd

`send_cli` uses `Settings.workspace_root` when set, else the home directory (was `"."`, which is `/` inside an app bundle).

## UI (`crates/ui/src/chat/model_picker.rs`)

- `pub const CLI_DEFAULT: &str = "Default"`: the picker row standing for an empty CLI model.
- `pub fn cli_model_groups(ids, current: &str) -> ModelGroups`: first group `"CLI"` holding `Default`, then the existing grouping by `provider/` prefix. `current` is listed when non-empty and missing.

## App (`crates/app`)

### Probe state

- `ChatApp.probes: HashMap<Provider, ProviderProbe>`; a missing key means "checking".
- `ChatApp.probe_runs: HashMap<Provider, u64>`: a generation counter per provider. A result whose generation is stale is dropped.
- Probe all three CLIs after construction. Re-probe one provider when its binary override is committed (Enter or blur). Re-probe all on Refresh.
- When a probe lands, resync the composer model list if the active chat uses that provider.

### Composer

- Provider chip: borderless `Button` labelled with `provider.label()`, `dropdown_menu` of `Provider::ALL` as `PopupMenuItem`s with `checked(p == chat.provider)`.
  - Uninstalled CLI (probe done, `path == None`, and not current): disabled, label `"{label} (not installed)"`.
  - Locked chat: other providers disabled, menu headed by a label `"Locked to {label} in this chat"`.
- Model select items:
  - OpenRouter: `model_groups(self.models, chat model)`.
  - CLI: `cli_model_groups(probe models + Settings default if custom, chat model)`, with `Default` selected for an empty model.
  - Resynced by `sync_model_picker(window, cx)` on chat select, provider pick, and probe landing.
- Model confirm maps `Default` to `""`, sets `chat.provider_model`, writes the provider default, persists the chat if it has messages, and syncs the Settings "Default model" input of that CLI.
- Search chip, Plan chip, and the context ring render only for OpenRouter chats. Context ring looks up the chat's model.
- Palette "Switch model" lists the active chat's model options and routes through the same pick path.

### Settings

- Model tab: "Default model" gets its own OpenRouter-only select entity; confirm writes `Settings.model` only.
- New tab "Providers":
  - Page action: Refresh button, re-probes all CLIs.
  - Section "Workspace": row "Folder", description "Coding agents run here. Defaults to your home folder.", shows the path or "Home folder", a "Choose…" button (`cx.prompt_for_paths` directories only), and "Reset" when set.
  - Section "Coding agents": one stacked row per CLI. Description = status: `Checking…`, `Found at {path} · {n} models`, `Not found on PATH` (or `Not found at {override}` when an override is set). Controls: "Binary path" input (placeholder "Search PATH"), "Default model" input (placeholder "CLI default"). Inputs save settings on change; binary path re-probes on Enter or blur.

## Out of scope

- Claude `initialize` probe, favorites, per-CLI effort chip, on-disk model cache, per-chat workspace.

## Tests

- core: `Provider::key` matches serde name for all variants.
- agent: `parse_pi_list`, `parse_opencode_list` on canned output; Claude aliases returned only when found.
- ui: `cli_model_groups` puts `Default` first and keeps a custom current id.
- app: `Default` maps to `""` and back; lock rule; OpenRouter legacy empty model falls back to `Settings.model`.
- `cargo test --workspace` and `cargo build` pass.
