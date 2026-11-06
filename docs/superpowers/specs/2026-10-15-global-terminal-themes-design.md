# Global Terminal with Themes (tmux & Neovim Compatibility) — Design

Date: 2026-10-15
Status: draft for review

## Intent and why

The user heavily relies on a daily workflow built around the terminal, `tmux`, and `Neovim`. Compatibility across these tools is a hard requirement.

Currently, Aether's terminal implementation (`crates/app/src/terminal/`) has two key limitations:
1. **Chat-bound lifecycle**: Terminal sessions are strictly 1:1 with chats (`ChatApp.terminals` keyed by `chat.id`), only render on `Page::Chat`, and close or swap when navigating between chats, agents, skills, or settings. This kills long-running tmux sessions, Neovim buffers, or background dev commands when the user switches tasks.
2. **Hardcoded terminal colors**: ANSI 0–15 colors are hardcoded to a single static dark and light palette in `crates/app/src/terminal/grid.rs`. There is no theme selection or customization. When running themed tools like tmux with `catppuccin/tmux` or Neovim with `catppuccin/nvim`, the terminal's default background, selection, and ANSI colors conflict with editor highlights and statusline contrast.

The goal is to introduce a **Global Terminal** that remains persistently alive across the entire application alongside contextual project/chat terminals, powered by a configurable **Terminal Theme Engine** with first-class **Catppuccin** support and full tmux/Neovim escape sequence compatibility.

## Goals

- **Persistent Global Terminal**:
  - A persistent, workspace-rooted terminal session that lives across all pages (Chat, Skills, Agents, Settings) without being dropped or restarted.
  - Spawns in `workspace_root` (falling back to user home directory if no workspace is open).
  - Keeps shells, tmux attachments, Neovim instances, and background processes alive even when the terminal drawer is closed or when navigating between pages.
- **Coexistence with Project/Chat Terminals**:
  - Project/chat-specific terminals continue to exist alongside the Global Terminal, scoped to the active chat's folder or worktree.
  - The terminal drawer provides a tabbed header: a persistent `[Global]` tab always present, plus a contextual `[Project: <name>]` (or `[Chat]`) tab when inside a chat with a valid folder.
  - Switching tabs preserves shell process states and scrollback in both sessions.
  - `Ctrl+`` toggles the terminal drawer globally, keeping focus in the currently selected terminal tab.
  - `Cmd+J` (macOS) continues to cycle focus between terminal and composer when on the chat page.
- **Terminal Theme System**:
  - Dedicated `terminal_theme` setting in `Settings` (`crates/core/src/settings.rs`), configurable in Settings UI (`crates/app/src/settings_view.rs`).
  - First-class support for the **Catppuccin** suite:
    - **Catppuccin Mocha** (dark, high contrast)
    - **Catppuccin Macchiato** (dark, medium contrast)
    - **Catppuccin Frappé** (dark, soft contrast)
    - **Catppuccin Latte** (light)
    - **Auto (Catppuccin)**: Automatically pairs Mocha for dark appearance and Latte for light appearance based on system/app theme.
    - Default Aether Dark / Light preserved as standard fallback.
- **Tmux and Neovim Protocol Compatibility**:
  - Exact 16 ANSI color registers (0–7 standard, 8–15 bright), default foreground (`Text`), background (`Base`), selection (`Surface2`), and cursor colors matching official Catppuccin color definitions.
  - Accurate PTY responses to OSC queries in `crates/app/src/terminal/session.rs`:
    - **OSC 10** (foreground query): reports the active theme's foreground RGB.
    - **OSC 11** (background query): reports the active theme's background RGB (enables Neovim `set background=dark`/`light` auto-detection).
    - **OSC 4** (palette query): reports the active theme's indexed colors 0–15.
  - Environment variables preserved: `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=aether`.
  - Full support for 24-bit true color SGR escape sequences (`\e[38;2;r;g;bm` and `\e[48;2;r;g;bm`), cursor shaping (DECSCUSR), OSC 52 clipboard copy, and bracketed paste.

## Non-goals

- Implementing an internal tmux multiplexer or window/pane manager inside Aether (tmux already manages its own panes/windows; Aether acts as a robust, high-fidelity terminal host).
- User-authored custom CSS or arbitrary theme script parsers at this stage (themes are structured presets in Rust with full ANSI + UI color specifications).
- Replacing `alacritty_terminal` with a different backend engine.

## Anti-goals

- Do not disrupt or remove per-chat worktree terminal capabilities.
- Do not kill or restart the global terminal session when closing the drawer or switching chats/pages.
- Do not allow themes with illegible contrast (faint/dim SGR 2 and contrast adjustment helpers in `grid.rs` must stay calibrated).

## Constraints

- Built with existing Rust + GPUI (`gpui-kit`) architecture and `alacritty_terminal`.
- Thread safety in `session.rs`: Alacritty's PTY thread and event loop take terminal locks before shared mutexes; the UI thread must never acquire terminal locks while holding the palette mutex.
- Keyboard ergonomics: Global `Ctrl+`` must work everywhere in the window, even when no chat is open or when on settings/skills pages.

## Decided how (from conversation)

- **Workflow Model**: Mostly global terminal for daily driving with persistent tmux/neovim sessions, while keeping project/chat terminals available per project.
- **Theme Choice**: Catppuccin family as primary theme ecosystem, ensuring statuslines and syntax highlighting match across terminal, tmux, and Neovim.
- **Drawer Integration**: Tabbed header in the terminal drawer to seamlessly switch between Global and Project sessions.
- **Settings**: Terminal theme dropdown in Settings view, persisting to `Settings.terminal_theme`.

## Left to builder

- Exact header tab styling: compact pill tabs or segmented button in the drawer header next to the existing clear and hide buttons.
- Global terminal default cwd when no workspace root is set (default to `$HOME`).
- Optional inclusion of other popular preset themes (Tokyo Night, Gruvbox, Nord) alongside the Catppuccin suite using the same palette abstraction.
- Title tracking in header tab: showing foreground job name (e.g. `tmux`, `nvim`, `zsh`) or folder name on the tab chip.
