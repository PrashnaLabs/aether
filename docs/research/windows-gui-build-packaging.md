# Windows support: GUI stack, native deps, build and packaging

Scope: Aether (Rust 2024 workspace, GPUI app) today ships macOS and Linux only.
Question: what does it take to build and ship on Windows, from the GUI stack down
to installer and CI. Sources are upstream docs and crate pages for the exact
versions in `Cargo.toml` and `Cargo.lock`, checked against how this repo uses them.

## Verdicts at a glance

| Piece | Verdict |
|---|---|
| GPUI via gpui-kit 0.7.3 (gpui-pre 0.3.9) | needs work, no blocker |
| wry 0.56.1 browser panel | needs work, no blocker |
| cpal 0.18.2 voice capture | works |
| whisper-rs 0.16 (whisper-rs-sys 0.15) | needs work, no blocker |
| keyring 4.2 (windows-native-keyring-store) | works |
| sqlx 0.9 sqlite-bundled via libsqlite3-sys 0.37 | works |
| alacritty_terminal 0.26 terminal | needs work, no blocker |
| Touch Bar / objc2 / macOS code | works (already gated), needs work around edges |
| Process and shell glue (`/bin/kill`, `sh`, `gio`, `open`) | needs work, the largest code change |
| Packaging (MSI, NSIS, MSIX, zip) | needs work, greenfield |
| CI matrix | needs work, no blocker |

Nothing on this list is a hard blocker that forces a crate swap. The real risk is
volume: a lot of small Unix assumptions in path, shell, process, and trash code that
compile today and fail at runtime on Windows.

## 1. GPUI / gpui-kit

Upstream: https://docs.rs/gpui-pre/0.3.9 states the platform split directly.
Windows windowing is Win32 and text is DirectWrite, with no features required.
Rendering on macOS is Metal; Linux needs `wayland` and `x11` features. GPUI is
still pre-1.0 and breaking changes between versions are expected. The kit crate
https://docs.rs/crate/gpui-kit/0.7.3 lists gpui-pre =0.3.9 and
gpui-pre-platform as normal dependencies, so Aether gets the same backend Zed runs.

Zed's own Windows build doc https://zed.dev/docs/development/windows describes the
production shape: Visual Studio with C++ desktop workload, Windows SDK 10.0.20348
or newer, CMake on PATH, and a developer shell before compiling. It also notes
that Zed uses Vulkan on Windows and that "NoSupportedDeviceFound" crashes are
usually stale GPU drivers or overlay software. That Vulkan warning is about Zed's
renderer path, but it is the best signal for what GPUI-on-Windows feels like in
the field: the backend ships and is used by a real editor, driver edge cases
remain.

Aether repo state:

- Root `Cargo.toml` pins `gpui-kit = { version = "0.7.3", features = [...tree-sitter...] }`.
  No platform feature gating, so Windows gets the default upstream backend.
- `Cargo.lock` shows gpui-pre 0.3.9 with a `windows = "0.62"` dependency and
  `embed-resource` as a build dep. That is the Win32 path, not a stub.
- `crates/app/src/main.rs` opens windows through `gpui_kit::application()` and
  `gpui_kit::open_window`, sets an icon from `crates/app/assets/app-icons/256.png`,
  and quits instead of closing on `on_window_should_close`. The quit-instead-of-close
  rule exists because dropping a window aborts on Touch Bar Macs. On Windows that
  means the main window can never be closed to the tray or quit normally; the
  semantics need a deliberate choice there.
- `crates/app/src/session_workspace/windows.rs` keeps detached session windows
  alive via `retain_closed_window`: macOS hides the NSWindow, everything else
  calls `window.minimize_window`. Minimize is already portable, so secondary
  windows mostly work. The macOS-only `hide_macos_window` reads the AppKit handle
  through `wry::raw_window_handle` and is cleanly `cfg(target_os = "macos")`.

Open question: docs.rs built gpui-kit 0.7.3 only for
x86_64-unknown-linux-gnu. That does not prove Windows is unsupported, but it
means the kit's Windows enablement is unverified from docs.rs alone. The
gpui-pre dependency table includes `windows 0.62`, which points the other way.
First Windows smoke test should confirm `gpui-pre-platform` actually enables
the Win32 backend under gpui-kit, rather than assuming docs.rs coverage equals
support.

## 2. wry 0.56 browser panel

Upstream: https://docs.rs/wry/0.56.1/wry/ says wry is cross-platform, requires a
windowing library exposing `HasWindowHandle`, and carries `webview2-com 0.38`,
`windows 0.61`, `windows-core 0.61`, and `windows-version 0.1` in its dependency
list. Platform notes: Linux uses WebKitGTK, macOS uses WebKit, Windows uses
WebView2 provided by Microsoft Edge Chromium, covering Windows 7, 8, 10 and 11.
Child webviews through `WebViewBuilder::build_as_child` are supported on macOS,
Windows, and X11 Linux. Microsoft's distribution doc
https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution
adds the deployment rule: a production app must check for the Evergreen WebView2
Runtime and install it if missing, and should not depend on the Edge Stable
browser being present. Windows 11 ships the runtime; most Windows 10 machines
have it, but the installer cannot assume it.

Aether repo state:

- `crates/app/Cargo.toml` gates wry behind
  `cfg(any(target_os = "macos", target_os = "linux"))`. Windows gets no webview
  until that line changes. The macOS deps it pulls
  (`objc2-app-kit`, `objc2-foundation`, `objc2-user-notifications`, `block2`)
  are `cfg(target_os = "macos")` only, and `gtk` is `cfg(target_os = "linux")`
  only, so the manifest is already structured for adding Windows without
  dragging either in.
- `crates/app/src/browser.rs` is the whole surface. The real host is
  `cfg(any(target_os = "macos", target_os = "linux"))`; the fallback host
  `cfg(not(any(target_os = "macos", target_os = "linux")))` returns an error
  string ("The browser panel isn't available on this platform yet"). Adding
  Windows means widening both cfgs and the Cargo gate, not rewriting the panel.
- `Host::create` calls `gtk::init()` only under `cfg(target_os = "linux")`,
  picks Cmd or Ctrl for the tab shortcut modifier with an `else` branch that
  already treats non-macOS as Ctrl, then builds the webview with
  `WebViewBuilder::new()...build_as_child(window)`. On Windows this receives
  GPUI's Win32 `HWND` through `HasWindowHandle` and wry parents WebView2 to it.
  No GTK or Cocoa involved. The existing bounds, visibility, load, reload,
  evaluate, focus, and focus_parent calls are wry APIs, not platform APIs.
- Two macOS-only blocks use `wry::WebViewExtMacOS` to detach and show/hide the
  WKWebView inspector. Both are `cfg(target_os = "macos")`. The generic
  `devtools()` path calls `self.webview.open_devtools()` for
  `not(macos)` debug builds. wry docs mark `close_devtools` as unsupported on
  Windows, but the code never calls it, so nothing to fix unless the toolbar
  starts closing DevTools.
- `USER_AGENT` is a hardcoded Safari string ("M... Safari/605.1.15"). Sites
  keying off the UA will serve Aether the desktop Safari treatment on Windows,
  which is wrong. Needs a Windows Chrome/Edge UA in a Windows branch.
- The browser panel matches the crate the repo pins: `wry 0.56` in workspace
  deps, `wry 0.56.1` in `Cargo.lock` with WebKitGTK, objc2, and webview2
  transitive deps all resolved. No version bump needed for Windows.

## 3. Native dependencies

### cpal 0.18 (crates/voice)

Verdict: works.

https://docs.rs/crate/cpal/0.18 lists x86_64-pc-windows-msvc among built
platforms, WASAPI as the default Windows backend, and Windows 8 as the minimum
OS. `Cargo.lock` resolves cpal 0.18.2 with `windows = "0.62"` and
`windows-core = "0.62"` plus `asio-sys` optional. Aether's
`crates/voice/src/capture.rs` only uses `cpal::default_host()` and
`default_input_device()`, input stream construction for F32/I16/U16, and
`play()`. No ALSA, JACK, or PipeWire specifics leak into the app code. The
optional `asio` feature for low-latency audio is not enabled and is not needed.
ASIO would need LLVM and vendor drivers per cpal docs; WASAPI avoids that.

### whisper-rs 0.16 (crates/voice)

Verdict: needs work, no blocker. Build toolchain is the gate, not the crate.

https://docs.rs/crate/whisper-rs/0.16 says Windows and macOS builds have
instructions, Linux should work out of the box, and non-maintained platforms
cannot be supported. The BUILDING file at
https://raw.githubusercontent.com/tazz4843/whisper-rs/master/BUILDING.md gives
two Windows paths:

- MSYS2 MinGW-w64 with the GNU Rust toolchain and a custom `.cargo/config.toml`
  linker pointing at `gcc.exe` and `ar.exe`.
- Visual Studio C++ with CMake and LLVM/clang on PATH, `LIBCLANG_PATH` set to
  the VS LLVM bindir, then cargo build. CUDA builds add the CUDA toolkit.

https://docs.rs/crate/whisper-rs-sys/0.15.0 confirms the build script depends
on `cmake`, `bindgen`, `fs_extra`, `semver`, `build` env flags, and
`WHISPER_DONT_GENERATE_BINDINGS` as a fallback if binding generation panics.
Feature flags on the safe crate are `cuda`, `hipblas` (Linux only), `openblas`,
`metal`, `vulkan`. Aether enables only `metal` under `cfg(target_os = "macos")`
in `crates/voice/Cargo.toml`, so Windows would build CPU-only whisper.cpp.
That matches `build.py` already setting `GGML_NATIVE=OFF`, which avoids CPU
tuning that crashes on lesser machines; `PKGBUILD` sets `GGML_NATIVE=ON` for
Arch because it controls that package. Windows should keep `OFF`.

For Aether this means the Windows build machine needs MSVC C++ tools, CMake,
and LLVM/clang for bindgen, not just `hlsl` or Windows SDK. The MSYS2 path
exists but switches Rust to the GNU target, which would complicate the rest of
the workspace (GPUI, wry, sqlx). The MSVC path is the only one consistent
with a single `x86_64-pc-windows-msvc` workspace.

### keyring 4.2 (crates/app, crates/agent)

Verdict: works.

https://docs.rs/crate/keyring/4.2 describes the default v1 feature as native
secure stores on Mac, Windows, and Linux with the same API. `Cargo.lock`
already resolves `keyring 4.2.0` with `windows-native-keyring-store`,
`apple-native-keyring-store`, and `zbus-secret-service-keyring-store`,
plus `keyring-core 1.0.0`. The Windows store crate at
https://docs.rs/crate/windows-native-keyring-store/1.0.0 is a thin wrapper
over the Windows credential store using `windows-sys 0.61`, and it warns that
same-entry access from multiple threads does not sequence reliably. Aether
reads and writes one entry, `("aether", "openrouter-api-key")` through
`crates/app/src/settings.rs` and `"aether"` / `"Claude Code-credentials"`
through `crates/agent/src/usage.rs`, both behind `keyring::Entry::new`. That
shape maps directly to the Windows Credential Manager. No manifest change
needed. The only caveat is that store: concurrent reads and writes to the
same entry, or tests racing on it, can see `NoEntry` on Windows in a way
macOS Keychain tolerates.

### sqlx 0.9 sqlite-bundled (crates/agent)

Verdict: works.

https://docs.rs/crate/sqlx/0.9.0 documents the SQLite driver as going through
the libsqlite3 C library, and https://docs.rs/crate/libsqlite3-sys/0.37.0 lists
x86_64-pc-windows-msvc and i686-pc-windows-msvc among built platforms, with
`cc` and `pkg-config` as build dependencies and `bundled` described as "a good
option ... when linking to SQLite is complicated, such as Windows". Aether
enables `["runtime-tokio", "sqlite-bundled"]` with `default-features = false`,
so the bundled path that matters on Windows is already on. The build needs a C
compiler on the machine, which MSVC provides. Do not switch to system
`sqlite3.dll` linkage; bundled avoids DLL search order problems and matches
the current manifest.

### alacritty_terminal 0.26 (crates/terminal)

Verdict: needs work, no blocker beyond shell policy.

https://docs.rs/alacritty_terminal/ lists `windows-sys`, `miow`, `piper`,
and `polling` among dependencies for 0.26.0, and upstream
https://github.com/alacritty/alacritty describes itself as cross-platform
with Windows requiring ConPTY support (Windows 10 1809 or higher). The
`tty` module is where the PTY lives, and Aether already abstracts over it:
`crates/terminal/src/session.rs` builds `alacritty_terminal::tty::Options`
with a `Shell`, creates the PTY with `tty::new`, and drives it with
`EventLoop`. The `Options` struct differs per platform (the code even
comments "windows Options has extra fields (escape_args)").

What breaks is not the crate. Aether currently assumes Unix shells and
signals everywhere:

- `user_shell()` reads `account_shell()` (a `cfg(any(macos, linux))` `getpwuid_r`
  helper that returns `None` elsewhere) or `$SHELL`, requires an absolute
  path, and falls back to `/bin/sh`. On Windows that always lands on
  `/bin/sh`, which does not exist.
- `Session::spawn` passes `vec!["-l"]`, and `launch()` prefixes
  `/usr/bin/env -u NAME ...` to strip inherited terminal variables. Neither
  exists on Windows. ConPTY launch needs `COMSPEC` (`cmd.exe`), `pwsh.exe`,
  or `powershell.exe`, and env removal has a different spelling than
  `env -u`.
- `signal()` and `signal_group()` are no-ops on `not(unix)`, backing
  `terminate()` and the `Drop` path that sends `SIGHUP`. On Windows there is
  no SIGHUP, killpg, or process group the same way. A shell that ignores
  close will need Job Object termination or `GenerateConsoleCtrlEvent`.
- `running_job()` is Unix-only ioctl (`tcgetpgrp`) with a `not(unix)` stub.
  Leaving it returning `None` is acceptable for a first Windows build.
- Workspace pin is `alacritty_terminal = { version = "0.26", default-features = false }`.
  Confirm which default features this disables and whether the Windows PTY
  path needs one of them; the docs.rs page does not describe feature flags
  in the excerpt, so that check belongs to the first Windows build, not to
  this report. Nothing in `crates/terminal` enables GPU or X11 features
  that would be Windows-incompatible.

Settings gating follows the terminal: `crates/app/src/settings_search.rs`
hides terminal theme, font size, font family, and cursor style behind
`cfg(unix)` items. Those will stay hidden on Windows until the terminal
lands, which is consistent if the terminal is sequenced after the shell work.

## 4. macOS-only code and cfg gates

The good news: the heavy macOS pieces are already `cfg` gated in the right
places. The bad news: `cfg(unix)` is doing work that means "macOS or Linux"
and `cfg(not(macos))` is doing work that means "XDG". Windows falls into the
else branches and is mostly safe but sometimes silently degraded.

### Already clean

- `crates/app/Cargo.toml`: `objc2` workspace dep plus `objc2-app-kit` with
  `NSTouchBar`/`NSTouchBarItem`/`NSCustomTouchBarItem`/`NSDockTile`,
  `objc2-foundation`, `objc2-user-notifications`, `block2`, and `resvg` are
  all `cfg(target_os = "macos")`. Nothing to ungate.
- Touch Bar: `crates/app/src/app/touch_bar.rs` gates `mod native` behind
  `cfg(target_os = "macos")` and `sync_touch_bar` behind the same cfg.
  Call sites in `crates/app/src/app/render.rs` and
  `crates/app/src/session_workspace/windows.rs` are also `cfg(target_os = "macos")`.
  Aether on Windows simply has no Touch Bar. No work.
- Notifications: `crates/app/src/notify.rs` keeps the whole
  `UNUserNotificationCenter` path in a `cfg(target_os = "macos")` module and
  the `osascript` fallback behind the same cfg. Non-macOS falls through to
  `cx.show_system_notification`. `authorize()` and `dock_badge()` are
  macOS-only with no Windows equivalent yet. Provider logo rendering uses
  `resvg` only inside the macOS module, so the dependency stays macOS-only.
- Dev bundle relaunch in `notify.rs` (`relaunch_in_dev_bundle`, `DEV_PLIST`,
  `dev_bundle`) is `cfg(all(target_os = "macos", debug_assertions))`. No
  Windows equivalent needed.
- `crates/app/src/sysinfo.rs`: `sample()` returns `None` on
  `not(any(macos, linux))`. Windows status bar will show no CPU or memory.
  That is a stub, not a crash. A real implementation would use PDH or the
  Windows `GlobalMemoryStatusEx` / performance counters, or add a small
  `sysinfo` crate.
- `crates/app/src/open_in.rs`: `discover()` returns `Vec::new()` when
  `!cfg!(target_os = "macos")`, so the feature is dead but safe on Windows.
  `open()` and `open_url()` hardcode `/usr/bin/open -a`, and icon extraction
  shells out to `plutil` and `sips`. Windows needs a different discover
  (Start Menu `.lnk`, App Paths, `shell:AppsFolder`) and `explorer.exe` /
  `ShellExecute` to launch. Keep the stub until then.

### Needs cfg work

- `crates/core/src/paths.rs` is the biggest silent-degradation:
  - `config_dir()` is Application Support on macOS and XDG elsewhere. On
    Windows it resolves to `$XDG_CONFIG_HOME/aether` or `$HOME/.config/aether`.
    `%APPDATA%` is the expected base, and `%USERPROFILE%` should back `home_dir()`,
    which currently reads only `$HOME` and returns an empty path when unset.
  - `search_dirs()` and `repaired_path()` split `PATH` on `:` and append
    `.local/bin`, `.cargo/bin`, `/opt/homebrew/bin`, `/usr/local/bin`. On
    Windows, separator is `;`, binaries often live under
    `Program Files`, `%USERPROFILE%\.cargo\bin`, and user installs, and CLI
    tools are `.exe` / `.cmd` / `.bat` chosen by `PATHEXT`, not extensionless
    names. `find_binary` only tests `is_file()`, so a Windows lookup can
    "find" a file that the shell cannot run.
  - `run_login_shell()` runs `sh -l -c 'printf %s "$PATH"'` with a 2 s
    deadline. On Windows there is no `sh`; the function returns `None` and
    `repaired_path()` falls back to the process `PATH`, which for a GUI app
    launched from Explorer is often minimal. The fallback is correct behavior,
    but the login-shell idea does not transfer. Use `where.exe` / `PATHEXT`
    discovery instead.
  - `create_private_dir()` is `0o700` on Unix and `create_dir_all` elsewhere.
    On Windows that means default ACLs. Acceptable, but be explicit that the
    0600 / 0700 invariants from `crates/app/src/settings.rs` become
    best-effort. A Windows ACL pass would use `SetSecurityInfo`, not file modes.
- `crates/app/src/settings.rs`: `#[cfg(unix)] mode(0o600)` when writing
  `settings.json`. Fine as-is on Windows (no-op).
- `crates/explorer/src/ops.rs`: `trash()` uses `/usr/bin/trash` on macOS
  and `gio trash` elsewhere. Windows has no `gio trash` by default. This
  needs `IFileOperation` (modern per-file `IFileOperation::DeleteItem`) or
  the Windows Recycle Bin via `SHFileOperation`, or a `trash` crate that
  already abstracts it. Error copy still promises "No trash command found
  (/usr/bin/trash or gio)", which will be wrong on Windows.
- Notes under `crates/app/src/notes/`: `external.rs` already splits Unix and
  non-Unix executable checks and temp-file modes, which is good. `shell_command()`
  and `quote()` do POSIX single-quote escaping. That is wrong for `cmd.exe`
  and for PowerShell. Notes "open in terminal" would need Windows quoting
  (`CommandLineToArgvW` rules) and a `pwsh -Command` wrapper, not a
  `sh -c` string.
- Driver and agent process management: `/bin/kill` appears in
  `crates/driver/src/antigravity.rs`, `crates/driver/src/catalog.rs`, and
  `crates/driver/src/opencode/service.rs`; `/bin/ps -p ... -o lstart=`
  and `/usr/bin/sqlite3` appear in `crates/agent/src/external.rs`;
  `process_group` in `catalog.rs` is `cfg(unix)`. All of those are no-ops or
  missing-binary failures on Windows. Killing CLI children on Windows
  needs Job Objects (assign at spawn) or `taskkill /PID`. SQLite reads of
  external CLI databases should switch to the bundled SQLx path or embed
  SQLite rather than shelling to `/usr/bin/sqlite3`.
- Keybindings: `crates/terminal/src/lib.rs` binds `cmd-*` (Cmd+J/C/V/A/K/R,
  Cmd+Shift+backtick, Cmd+backslash, Cmd+W, Cmd+Alt+arrows, Cmd+Shift+] / [,
  Cmd+F). On Windows the platform modifier is Ctrl, and some of these
  collide with browser and shell conventions. GPUI maps platform keys, so
  the fix is a Windows binding set, not a framework change.
- `crates/app/src/workspace_popover/mod.rs` already does the right shape:
  `if cfg!(target_os = "macos") { run("scutil", ...) } else { None }`. That
  is the pattern to copy for every new Windows branch.

## 5. Packaging

### What exists today

- `scripts/build.py`: macOS builds `Aether.app` with `Info.plist`, `Aether.icns`,
  ad-hoc or identity codesign via `codesign` and `security`, and optional
  notarization into a DMG. Linux builds a `tar.gz` staging tree with
  `usr/bin/aether`, desktop file, and PNG icons, plus an optional `deb` via
  `dpkg-shlibdeps` / `dpkg-deb`. It pins `GGML_NATIVE=OFF` unless overridden,
  picks `x86_64` or `aarch64` from `platform.machine()`, and explicitly sets
  `--target` to `apple-darwin` or `unknown-linux-gnu`.
- `PKGBUILD` and `Makefile`: Arch Linux only (`makepkg`, pacman deps incl.
  `webkit2gtk-4.1`, `libsecret`, `vulkan-icd-loader`, `clang`, `cmake`).
- Icons: `crates/app/assets/app-icons/` holds PNGs at 16, 32, 48, 64, 128,
  256, 512, 1024 plus `aether.svg` and a hand-packed `Aether.icns`.
  `scripts/generate-icons.sh` renders them from the SVG with ImageMagick
  and packs the ICNS by hand. `scripts/tests/test_packaging.py` checks PNG
  signatures and dimensions and ICNS chunks, but has no Windows case.

### Windows choices

- **MSI via WiX** (https://wixtoolset.org/): Windows Installer packages,
  per-machine elevation, enterprise deployment, rollback, and Add/Remove
  Programs entries. FireGiant now markets extended support for older WiX
  majors and HeatWave in Visual Studio, with WiX v7 current. For Aether the
  fit is a per-machine `Aether.msi` when IT distribution matters. Cost is a
  real `.wixproj` / `wax` authored payload, product codes, upgrade codes,
  and publisher identity that has to stay stable across releases.
- **NSIS** (https://nsis.sourceforge.io/Docs/): scriptable, free, small,
  per-user is the norm, MUI2 setup UI, and the fastest path from
  `build.py` because the whole installer is a script file. No Windows
  Installer transaction, no downgrade safety. For a first release, NSIS is
  the most forgiving choice if you do not need per-machine policy.
- **MSIX** (https://learn.microsoft.com/en-us/windows/msix/overview): the
  modern package format with package identity, containerized execution,
  99.96% install success claim from Microsoft, differential 64 KB block
  updates, Intune/ConfigMgr deployment, and Store support. All MSIX
  packages must be signed. Identity is what unlocks Windows features that
  need a package identity. Best long-term story if Aether wants Store
  listing, auto-update, and enterprise install, but it is the most demanding:
  `AppxManifest.xml`, publisher matching, capability declarations, and a
  trusted signing cert from the start.
- **Portable zip**: copy `aether.exe` and `LICENSE` into
  `Aether-<version>-windows-x86_64.zip` with a `.sha256`, mirroring the Linux
  tar. Zero installer work, zero elevation story, no Add/Remove entry. Shippable
  on day one and then upgraded to an installer without changing the binary.

### Icons

Aether already has the inputs. Windows wants `Aether.ico` (an ICO container
holding 16, 32, 48, 64, 128, 256 PNG entries) plus a 256x256 PNG for the
taskbar and a 48x48 for the Explorer grid if you want it crisp. Generate it
in `scripts/generate-icons.sh` from the same committed PNGs rather than
adding a second source: ImageMagick (`magick convert *.png Aether.ico`) or
Python/Pillow pack the same files that `test_packaging.py` already checks.
ZLGPUI windows code in Zed embeds an `app-icon.ico` through a winresource
build script and needs a valid RC path under the Windows SDK; Aether will
need the equivalent `winresource` or `embed_resource` step in a build script
or `build.rs` for taskbar and Explorer icons, not just a file on disk.

### Code signing and auto-update

No updater exists today on either platform. macOS ships DMG plus notarization,
Linux ships tar or deb plus manual install. Windows has no precedent to
inherit, so the choice stays open. What every Windows distribution needs
regardless of format:

- Sign the binary and the installer with a code-signing certificate and a
  timestamp (`signtool sign /fd SHA256 /tr <timestamp> /td SHA256`), and
  publish an EV certificate if SmartScreen reputation matters. CI signing
  on `windows-latest` usually goes through Azure Trusted Signing rather
  than an exported PFX in secrets.
- If bundling WebView2, decide Evergreen bootstrapper versus Fixed Version
  runtime. Evergreen keeps the webview patched with Edge and is the default
  Microsoft recommends; Fixed Version ships a private runtime copy with the
  app at the cost of size and your own servicing.
- For auto-update, the smallest addition after zip is a release manifest
  with `0.1.x` checks and a download of the zip. WiX/NSIS/MSIX each have
  their own update story (MSI is reinstall, NSIS with a helper such as
  Squirrel or Velopack can delta-update, MSIX gives differential updates
  from Windows Update / App Installer). Pick after the first signed MSI
  or NSIS ships; doing it before the installer is stable creates a second
  product to maintain.

### How build.py extends

Add a `windows()` branch beside `macos()` and `linux()`:

- Allow `platform.system() == "Windows"` instead of erroring out.
- Map target to `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`.
- Add formats `zip`, `nsis`, `msi`, `msix` alongside the existing `app`,
  `dmg`, `tar`, `deb`.
- Keep `GGML_NATIVE=OFF` and the Cargo `--locked --release` invocation.
- Add `aether.ico` generation from `ICONS/*.png`, a `packaging/windows/`
  directory with `Aether.nsis`, `Product.wxs`, or `AppxManifest.xml`, and a
  `signtool` step when `WINDOWS_SIGN_PFX` or Azure Trusted Signing env is
  set. Keep the sha256 sidecar logic unchanged.
- Treat WebView2 runtime presence as a packaging step: fail the build if
  neither the Evergreen bootstrapper nor a Fixed Version runtime directory
  is staged, matching Microsoft's guidance to check before first WebView2
  creation.

## 6. CI

Current `.github/workflows/build.yml` matrix:

- `macos-15` arm64, `macos-15-intel` x86_64, `ubuntu-24.04` x86_64.
- Steps: checkout, `setup-python` from `.python-version`, Linux apt deps
  via `scripts/setup-linux.sh` when `runner.os == 'Linux'`, pinned rustup
  toolchain 1.99.0 with `rustfmt` and `clippy`, `Swatinem/rust-cache`,
  `cargo fmt --check`, `cargo test --locked --workspace -- --skip hackernews::client`,
  packaging tests via `python3 -m unittest discover -s scripts/tests`,
  desktop-file validation on Linux, `python3 scripts/build.py`, deb on
  Linux, artifact upload.

GitHub's hosted runner table
(https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
lists standard public runners as `windows-latest`, `windows-2025`,
`windows-2025-vs2026`, `windows-2022` on x64 and `windows-11-arm`,
`windows-11-vs2026-arm` on arm64, each with 4 CPUs, 16 GB RAM, 14 GB SSD.
`windows-latest` follows the latest stable image; for a pinned toolchain
1.99.0 that is usually fine, but `windows-2025` pins the image and is safer
if the MSVC/SDK version ever matters.

Needed for the Windows leg:

- Add a matrix entry `{ os: windows-latest, arch: x86_64 }` and extend
  `key: ${{ matrix.os }}-${{ matrix.arch }}` to cover it. The rust-cache key
  already includes both fields, so no other cache changes.
- MSVC environment: `windows-latest` images include Visual Studio with C++
  build tools, Windows SDK, and CMake, but the step that activates the
  developer prompt is not implicit in a bare `cargo build`. In practice:
  add `Setup MSVC` via `ilammy/msvc-dev-cmd` with `arch: x64`, or invoke
  `vswhere` / `VsDevCmd.bat` before the cargo step. Zed's Windows doc also
  requires an initialized developer shell and CMake on PATH; the runner has
  CMake, but the PATH entry depends on which VS component layout the image
  ships. Verify with `where.exe cmake` and `where.exe clang` in the job.
- Prebuild steps for whisper: `cmake`, `llvm` / `clang`, and `libclang`
  reachable for bindgen. `windows-latest` ships CMake and LLVM; whisper's
  BUILDING file for Windows MSVC expects `LIBCLANG_PATH` to point at the VS
  LLVM bindir. Set that env in the workflow or the bindgen step fails even
  though cargo syntax is right.
- `cargo fmt --check` and packaging unit tests are platform-neutral and
  already run. `desktop-file-validate` is Linux-only and should stay that
  way (`if: runner.os == 'Linux'`).
- Path and shell differences in `cargo test`: tests that assume Unix modes,
  `PATH` split, or `sh` will fail on Windows even when the library code is
  right. `hackernews::client` is already skipped for flake; the Windows leg
  will add its own `--skip` for Unix-only tests until they are gated.
- `python3 scripts/build.py` on Windows will error until the Windows branch
  lands. Phase the job: first a `cargo test` + `cargo build` smoke job that
  uploads `target/.../aether.exe` as an artifact, then a packaging job.
- Windows ARM (`windows-11-arm`, `aarch64-pc-windows-msvc`) is available on
  GitHub runners. Do not add it to the same PR as x86_64. GPUI, wry WebView2
  on ARM, whisper.cpp SIMD tuning with `GGML_NATIVE=OFF`, and llvms in CI are
  each plausible but unproven for this workspace. Add x86_64 first, then
  use ARM as a follow-up after the MSVC whisper path works.

## 7. Other deps in the tree

No Windows action needed for the rest of the workspace deps: `gpui-kit`
tree-sitter features, `markdown`, `spellbook`, `reqwest` with `rustls`,
`image`, `chrono`, `dotenvy`, `serde`, `tokio`, `regex`, `sha2`, `base64`,
`scraper`, `schemars`, and `adk-rust` are all pure Rust or already
cross-platform C. The C surface is exactly the five named above plus
GPUI/wry themselves. `resvg` is macOS-only in this repo because it exists
only for the notification logo path.

## 8. Recommended order of work

1. Spike the build: clone onto `windows-latest`, activate MSVC, install
   rustup 1.99.0, run `cargo check --locked -p aether --target x86_64-pc-windows-msvc`
   with CMake, clang, and `LIBCLANG_PATH` set. Fix nothing in app code until
   the linker succeeds. This validates GPUI, wry transitive deps, whisper
   bindgen, sqlx bundled sqlite, and alacritty_terminal in one pass.
2. Widen the wry gate (`crates/app/Cargo.toml`, `crates/app/src/browser.rs`),
   add the Windows UA branch, run the browser panel under WebView2. Stub the
   DevTools close path only if it errors. Ship with the fallback error string
   in place for the first build and smoke the panel before anything else.
3. Fix `crates/core/src/paths.rs` for Windows: `%APPDATA%` config dir,
   `%USERPROFILE%` home, `;`-split PATH, `PATHEXT` lookup, and no `sh`
   login shell. Add `where.exe`-based binary discovery. This unlocks every
   CLI driver and settings save.
4. Terminal shell: `COMSPEC` / `pwsh.exe` detection, ConPTY launch args,
   env stripping without `/usr/bin/env`, Job Object termination. Ungate
   terminal settings items in `settings_search.rs` and add Windows
   keybindings when the shell runs. Leave `running_job()` returning `None`.
5. Process glue: replace `/bin/kill`, `/bin/ps`, `/usr/bin/sqlite3` in
   `crates/driver` and `crates/agent` with Windows equivalents (Job Objects,
   `taskkill`, bundled SQLite). Add Recycle Bin trash in
   `crates/explorer/src/ops.rs` and Windows quoting in
   `crates/app/src/notes/external.rs`.
6. Sysinfo and open-in: implement PDH/Win32 sysinfo, replace `open_in.rs`
   `/usr/bin/open` with `explorer.exe` / `ShellExecute`, or keep the stubs
   and hide the UI until they land.
7. Packaging: portable zip first (`build.py` windows branch, `Aether.ico`
   from existing PNGs, sha256 sidecar, WebView2 bootstrapper check), then
   NSIS, then WiX MSI, then MSIX if Store/enterprise matters. Sign with
   Azure Trusted Signing once the binary name and layout stop changing.
8. CI: add `windows-latest` x86_64 to `build.yml` after the zip ships.
   Keep Linux desktop validation and macOS bundling untouched. Add
   `windows-11-arm` only after x86_64 is green.

## 9. Open questions

- Does `gpui-kit 0.7.3` with its default features actually enable the
  Win32 backend on `x86_64-pc-windows-msvc`, or does it need a
  `gpui-pre-platform` feature the docs.rs page does not name?
- Is Zed's Vulkan device-init path an issue for Aether, or does plain
  `gpui_kit::application()` pick a DirectX path that does not hit the same
  `NoSupportedDeviceFound` failure? Aether does not use Zed's editor chrome,
  only the framework.
- Does `alacritty_terminal` with `default-features = false` include the
  Windows PTY code, or does the workspace need to enable a feature that
  pulls `ConPTY` support?
- Should whisper stay CPU-only on Windows, or is Vulkan support worth the
  CUDA/Vulkan SDK and CI cost? The crate exposes `vulkan` and `cuda`
  features; Aether currently uses neither.
- Does `close_devtools` matter for the Windows browser toolbar? The code
  never calls it today. If DevTools docking is wanted on Windows, wry
  currently reports it unsupported.
- Which installer identity will Aether use across releases: WiX product and
  upgrade codes, NSIS AppUserModelID, or MSIX publisher and package family?
  That identity becomes hard to change once users upgrade in place. It also
  decides whether auto-update is manual re-download, a third-party helper,
  or MSIX differential updates.
- What is the minimum Windows version Aether will support? wry claims
  Windows 7 support, cpal needs Windows 8, ConPTY needs Windows 10 1809,
  WebView2 Evergreen is cheapest on Windows 10/11. ConPTY makes the honest
  floor Windows 10 1809, so supporting older Windows would need a terminal
  other than the current one.
- Should `windows-latest` be pinned to `windows-2025` in CI? The rolling
  `windows-latest` label moves with GitHub images while `rust-toolchain.toml`
  pins Rust at 1.99.0. Pinning the image removes one variable when a build
  breaks.
