Aether alpha-0.1.1 is the second alpha. It adds an optional Organization view for running teams of agents, makes helper agents pick the provider with the most quota left, and adds multiple accounts per provider and richer notifications.

## Highlights

### Organization view (off by default)
Turn it on in **Settings > Organization**.
- An org page with three views: Roster console, Org chart and Round table.
- A setup wizard creates pinned lead chats, role files and review routing.
- Department heads hire workers with the `request_worker` tool. Hiring follows rules and is capped by an org-wide pool.
- Moving a card to Review wakes the matching `review:<role>` head. Head notes wake the department head and the CEO.
- An org sidebar view and a status bar popover.

See [docs/organization.md](https://github.com/PrashnaLabs/aether/blob/alpha-0.1.1/docs/organization.md).

### Quota-aware orchestration
- `spawn_agent` takes `provider: "auto"` or no provider. Aether starts the helper on the installed provider with the most quota left, ranked on its tightest usage window.
- Bursts of spawns spread across providers based on how many chats each one is already running.
- A provider without usage data is skipped, not guessed. When no data is fresh, the helper starts on the lead's own provider.

### Accounts and usage
- Several accounts per provider in `settings.json` (`accounts`, `default_accounts`), mapped to `CLAUDE_CONFIG_DIR` and `CODEX_HOME`.
- The usage card shows account chips. Usage always comes from the active account.
- New chats default to the provider you picked. If you never picked one, they use a detected Claude or Codex login, then OpenRouter.

### Notifications
- Choose in-app toasts or OS notifications, with an optional sound.
- Optional webhook POST of `{title, body, chat, state}`.
- Rate limited to one per chat and kind every 10 seconds. No notification for the chat you are watching.

### Agent tools
- `list_cards` takes filters and can return card bodies. New `update_card` tool.
- `message_agent` can reach any chat except itself and archived chats. Messages carry a `[from <title> #<id>]` tag and a per-sender loop guard.
- `read_history` takes an `offset`, and leads can read any chat.
- `list_chats` and `list_agents` report model, queued messages and cost.
- The Claude lead token now travels in the environment. The MCP config file holds only `${VARS}`.

### Desktop
- macOS traffic lights sit in each page's top bar, so a collapsed sidebar no longer adds a title row.
- macOS: `ctrl-w` closes the window and `ctrl-q` quits. The terminal still gets `ctrl-w`.
- The go-to-chat palette filters by state (`state:blocked`, `state:working`, `state:done`, `state:idle`).
- Pin up to 4 projects and 3 chats per project. Pinned chats stay visible under collapsed projects.
- Settings: Direct Chat with an OpenRouter key, provider setup and Bottom Bar controls.
- Chat: a "By the way" panel, a draft dialog, selection actions and rotating composer tips.
- The browser panel keeps navigation, devtools and keyboard focus inside its panel.
- Resuming a provider session opens the CLI in a terminal tab without tmux.

### Fixes
- Chats that share a folder show only the changes in files they edited, not the whole folder's diff.
- A folded project no longer highlights twice when one of its pinned chats is open.
- Claude background tasks finish correctly when the completion arrives before the tool result.
- Windows-only test failures fixed (paths, JSON escaping, a tokio race).

## Downloads

| Platform | File |
|---|---|
| macOS, Apple Silicon | `Aether-0.1.1-alpha-macos-aarch64.dmg` |
| macOS, Intel | `Aether-0.1.1-alpha-macos-x86_64.dmg` |
| Linux x86_64, Debian/Ubuntu | `Aether-0.1.1-alpha-linux-x86_64.deb` |
| Linux x86_64, other distros | `Aether-0.1.1-alpha-linux-x86_64.tar.gz` |
| Windows x86_64 | `Aether-0.1.1-alpha-windows-x86_64.zip` |

Each file has a `.sha256` next to it. Check a download with `shasum -a 256 -c <file>.sha256`.

**This is an alpha.** Expect rough edges. The macOS build is not notarized. If macOS blocks it, run `xattr -dr com.apple.quarantine /Applications/Aether.app` after installing. On Windows, SmartScreen may warn about an unsigned app.

Install the agent CLIs you want to use (Claude Code, Codex and others) separately. Settings files from alpha-0.1.0 load unchanged, and new settings default to off.

