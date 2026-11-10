<p align="center">
  <img src="crates/app/assets/app-icons/256.png" width="96" height="96" alt="Aether icon">
</p>

<h1 align="center">Aether</h1>

<p align="center">
  <strong>Coding agents side by side, in one native window.</strong><br>
  <a href="https://aether.prashnalabs.com">Website</a> ·
  <a href="https://aether.prashnalabs.com/docs/install/">Install</a> ·
  <a href="https://aether.prashnalabs.com/docs/quickstart/">Quickstart</a> ·
  <a href="https://aether.prashnalabs.com/docs/">Docs</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
  <a href="https://github.com/PrashnaLabs/aether/actions/workflows/build.yml"><img src="https://github.com/PrashnaLabs/aether/actions/workflows/build.yml/badge.svg" alt="Build"></a>
</p>

Aether is a native desktop app, written in Rust on [GPUI](https://www.gpui.rs),
that runs Claude Code, Codex, OpenCode, Pi, and Antigravity side by side, with
Git worktrees, terminals, and your project files in the same window. It drives
the agent CLIs you already have installed and signed in to.

## Features

- **Every agent in one place.** Chat with Claude Code, Codex, OpenCode, Pi, and
  Antigravity through their own CLIs, or with any OpenRouter model directly.
  Tool approvals, questions, plans, and subagents show up in one transcript.
- **Side by side.** Split, float, or detach sessions into their own windows.
  Layouts, drafts, and window positions come back after a restart.
- **Orchestration.** Turn a chat into a lead that spawns, messages, and stops
  child agents through Aether's built-in MCP server.
- **Git and worktrees.** Review changes, switch branches, and give each chat
  its own worktree.
- **Terminals and files.** Split terminals, a file explorer and editor with
  syntax highlighting, quick open, an image viewer, and a browser panel.
- **Usage at a glance.** Token and cost history across providers.
- **The rest of your desk.** Notes, a watch list for running agents, on-device
  dictation, a command palette, and Touch Bar support.

Aether runs on macOS 12+ (Apple Silicon and Intel) and on Linux (x86_64 and
aarch64, glibc, Vulkan). Install the agent CLIs you want to use separately.

## Session layouts

Agent sessions can run side by side in resizable splits, float over the workspace,
or move into their own desktop windows. Each session keeps its draft, transcript
position, running turn, and pending questions when you move it.

Use the pane header to open its layout menu. The close button appears when the
window contains multiple session panes. The menu offers split right/down,
float/dock, detach to a desktop window, maximize/restore, docking beside a chosen
pane, and opening an existing session beside the current one. The command
palette includes the same layout actions. The ⋯ menu retains the pane layout
controls. Right-click a non-interactive session area for pin/unpin, Stop generating,
Jump to, Split session, Terminal, and File Explorer, with icons and grouped
navigation/split submenus. Buttons, links, inputs, and embedded tools keep their
own right-click behavior.

- `Cmd+Option+[` / `Cmd+Option+]` on macOS, or `Ctrl+Alt+[` / `Ctrl+Alt+]`
  elsewhere, moves between panes.
- `Cmd+Option+Arrow` on macOS, or `Ctrl+Alt+Arrow` elsewhere, focuses a nearby
  pane. Terminal, editor, and request-card key bindings keep their existing behavior.
- Drag split dividers to resize; double-click a divider to reset it. Drag a
  floating pane's header to move it and its lower-right grip to resize it.
- Hiding a pane from the command palette keeps its agent running. Use **Stop** to stop the agent.
  Closing a detached window returns its sessions to the main workspace.

Layouts, window positions, and drafts restore on restart. Detached windows are
retained for reuse after their sessions return. On Linux, retained windows are
minimized; on macOS they are hidden.

## Chat layouts

Tabbed (default): chat in top tabs. Sidebar: chat center, right panel docked beside it. Split: chat center, right panel always docked and cannot be collapsed.

Toggle in **Settings > Appearance > Chat layout** or the command palette (**Chat layout: Tabbed/Sidebar/Split**). No default key; `CycleChatLayout` cycles Tabbed → Sidebar → Split.

Session panes still tile in Sidebar and Split. Monocle shows one pane at a time and keeps panel state; in Split, Monocle is off and panes tile beside the docked panel. See [docs/chat-layout.md](docs/chat-layout.md).

## Touch Bar

On supported MacBook Pro models, Aether provides **New Chat**,
**Terminal**, **Files**, **Git Changes**, **Commands**, and **Settings** buttons in the Touch Bar.
Terminal, Files, and Git Changes show icons only. Settings opens in the main window.
Session controls follow the focused pane in the main window or a detached
session window. Unavailable actions are disabled. New Chat opens in the main
workspace and uses its current project. The macOS Control Strip remains available.

## Image viewer

Open a local image from Explorer, Quick Open, a chat link, or an attachment to
preview it inside Aether. Pending image attachments are clickable too.

Use the toolbar, `+` / `-`, the mouse wheel, or a trackpad pinch to zoom. Drag to
pan, press `1` for actual size, and `0` to fit. Escape closes the viewer and
restores focus. Each window has its own viewer.

Supported formats are PNG, JPEG, WebP, GIF, TIFF, and BMP. Animated images show
their first frame. Previews allow files up to 64 MiB, 32 megapixels, and 16,384
pixels per side.

## Dictation

Click the mic in the composer, or press `Cmd+Shift+M` on macOS or `Ctrl+Shift+M`
elsewhere, and speak. Press it again or choose **Done**, and the text lands in the
chat you started from.

Speech turns into text on this device. The first use offers a one-time 141 MB
model download and asks for microphone permission. Dictation is English only.
**Settings > Composer** turns it off and downloads or removes the model.

## Notifications

Aether tells you when a chat needs an answer or finishes a turn while you are looking
elsewhere, never for the chat you are viewing. **Settings > Appearance > Notifications** picks
the delivery (off, in-app toast, or system notification; Linux uses `notify-send`) and **Notification sound** plays a sound on finish or on request. Repeats for the same
chat and kind are held back for 10 seconds.

For a webhook, set `notify_webhook` in `settings.json` to a URL. Each notification then POSTs
`{"title", "body", "chat", "state"}` there, where `state` is `done` or `request`. It is empty
(off) by default.

## Notes

Press `Alt+N` to open Notes. The default folder is `~/notes`; configure the
folder and editor under **Settings > Tools > Notes**. See [docs/notes.md](docs/notes.md).

## Claude child auto-compaction

**Settings > Providers > Claude child agents** controls auto-compaction for
Claude agents spawned by a lead. It defaults to **on at 45%** of the supported
1M models' 967,000-token compaction window (435,150 tokens). Choose any integer
from 30% to 90%, or turn the toggle off. Running agents keep their old value.
See [the settings, evidence and limits](docs/orchestration.md#claude-child-auto-compaction).

## Development

Install [Rust with rustup](https://rustup.rs/). The repository pins its compiler in
`rust-toolchain.toml`; rustup installs it when you run Cargo. Packaging runs on
[uv](https://docs.astral.sh/uv/), which uses the Python pinned in `.python-version`.

On macOS, install the Xcode command line tools and CMake, which whisper.cpp needs:

```sh
xcode-select --install
brew install cmake
```

On Ubuntu 24.04 or Debian 13, install the native build dependencies:

```sh
sudo bash scripts/setup-linux.sh
```

Linux uses GTK 3 and WebKitGTK 4.1 for the browser panel, plus a Vulkan driver
for GPUI. See the upstream [GPUI dependency list](https://github.com/longbridge/gpui-component/blob/main/script/install-linux.sh)
and [Wry Linux requirements](https://github.com/tauri-apps/wry#linux).
Other distributions need equivalent development packages.

```sh
cargo run --locked -p aether
cargo test --locked --workspace -- --skip hackernews::client
cargo fmt --all -- --check
```

The `Makefile` wraps these and packaging: `make build`, `run`, `test`,
`test-packaging`, `lint`, `fmt`, `fmt-check`, `package`, `pacman`, `app`, `clean`,
and on macOS `install`, which puts a release `Aether.app` in `/Applications`.
`make test` skips the flaky `hackernews::client` tests on macOS.

Configure providers in the app's settings. Lead-chat orchestration (spawning
child agents via MCP) is documented in [docs/orchestration.md](docs/orchestration.md).
Development builds can also read a
local `.env`. Packaging never copies `.env` or credentials. Install external
agent CLIs, Git, and any project tools separately and make them available on
the app's PATH. Finder launches inherit the desktop environment's PATH.

## Build installers

Run on the OS and CPU architecture you are packaging for:

```sh
uv run scripts/build.py
```

This builds with the locked dependencies and release profile, then writes
artifacts and SHA-256 checksum files to `dist/`.

| Host | Default output | Other formats |
| --- | --- | --- |
| macOS, Apple Silicon or Intel | `Aether-<version>-macos-<arch>.dmg` | `--format app`, `--format tar` |
| Linux, x86_64 or aarch64, glibc | `Aether-<version>-linux-<arch>.tar.gz` | `--format deb` |
| Windows, x86_64 | `Aether-<version>-windows-<arch>.zip` | |

The DMG includes an Applications shortcut. Drag Aether into Applications.
`--format app` writes `dist/Aether.app`, which can be launched with
`open dist/Aether.app`. The bundle includes its icon and version metadata.
macOS builds target macOS 12 or newer. Set `MACOSX_DEPLOYMENT_TARGET` before
building to choose a higher minimum version.

For Debian and Ubuntu:

```sh
uv run scripts/build.py --format deb
sudo apt install ./dist/Aether-*.deb
```

The Debian package derives shared library dependencies from the built binary.
Build on the oldest distribution you intend to support. Packages and archives
depend on compatible host libraries, including WebKitGTK. They are not
self-contained Linux binaries. The [Build workflow](.github/workflows/build.yml)
produces the same artifacts on GitHub Actions.

To install a Linux archive under `/usr/local`, extract it and copy its `usr/`
contents there. For example, from the repository root after building:

```sh
mkdir -p /tmp/aether-install
tar -xzf dist/Aether-0.1.0-linux-x86_64.tar.gz -C /tmp/aether-install
sudo cp -R /tmp/aether-install/Aether-0.1.0-linux-x86_64/usr/. /usr/local/
sudo update-desktop-database /usr/local/share/applications
sudo gtk-update-icon-cache -f -t /usr/local/share/icons/hicolor
```

Adjust the version and architecture to match your archive. The desktop entry,
Wayland app ID, X11 window identity, and installed icon use `dev.aether.Aether`.

## macOS signing

Local builds use an ad-hoc signature. Distribution outside your own machine
requires your Apple Developer ID certificate and notarization credentials.
Store a keychain profile using `xcrun notarytool store-credentials`, then run:

```sh
uv run scripts/build.py \
  --sign-identity 'Developer ID Application: Your Name (TEAMID)' \
  --notary-profile aether-notary
```

The build signs the app with the hardened runtime, creates and signs the DMG,
submits it for notarization, then staples and validates the ticket. You can
also set `APPLE_SIGNING_IDENTITY` and `APPLE_NOTARY_PROFILE` in the environment.
The Build workflow uses ad-hoc signatures and runs on the
[GitHub macOS and Linux runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

## Icons

The official icon is the `岚` character on the app's amber-to-pink brand gradient,
matching the sidebar's `BrandMark`. Its source is
`crates/app/assets/app-icons/aether.svg`. The glyph is stored as vector outlines
so builds do not depend on an installed CJK font. PNG sizes and the
macOS ICNS are checked in so normal builds need no image tools. After editing
the SVG, install ImageMagick and regenerate:

```sh
bash scripts/generate-icons.sh
```

Run the packaging checks with:

```sh
uv run -m unittest discover -s scripts/tests -v
```

## Credits

The app ships a few third-party assets, each under its own license:

- [Iconoir](https://iconoir.com) icons, MIT
- [Remix Icon](https://remixicon.com) icons, Remix Icon License v1.0
- [Solar](https://www.figma.com/community/file/1166831539721848736) icons by 480 Design, CC BY 4.0
- [Lucide](https://lucide.dev) icons as the fallback set, ISC, bundled by the
  `gpui-kit-assets` crate
- English spelling data from [LibreOffice dictionaries](https://github.com/LibreOffice/dictionaries)

Each icon pack folder in `crates/app/assets/icon-packs/` holds its license
file. `crates/app/assets/icons/SOURCES.md` lists sources and changes.

Aether's design draws on two open-source coding-agent apps, studied as
references for agent wiring, transcript rendering, git, and terminal UX:

- [Waku](https://github.com/egoist/waku), GPL-3.0. Design reference only, no
  Waku code is included.
- [T3 Code](https://github.com/pingdotgg/t3code), MIT

Claude, Codex, OpenCode, Pi, Antigravity, and OpenRouter are trademarks of
their respective owners. Their logos appear only to identify each provider.
They are not covered by Aether's license, and Aether is not affiliated with
or endorsed by them.

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) to get
started, and follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Security

Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md),
not in public issues.

## License

Aether is released under the [MIT License](LICENSE). Bundled third-party
assets keep their own licenses, listed under [Credits](#credits).

## Built by Prashna Labs

Aether is made by [Prashna Labs](https://prashnalabs.com/?utm_source=aether&utm_medium=readme),
an AI company in Kathmandu, Nepal. We also make
[DurbarAI](https://durbarai.prashnalabs.com/?utm_source=aether&utm_medium=readme),
a self-hosted runtime for running AI agents in production, with approvals and
an audit log.
