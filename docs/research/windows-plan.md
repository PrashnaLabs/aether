# Windows support: merged plan

Merges three reports in this folder:

- `windows-gui-build-packaging.md` (Pi): GUI, native deps, packaging, CI.
- `windows-agent-drivers.md` (Antigravity): CLI spawning, orchestrator, session storage.
- `windows-terminal-git-fs.md` (Claude): terminal, git, filesystem, notes, tests.

None of the three ran anything on Windows. Every "works" is from docs or source reading, so phase 0 exists to turn those into facts.

## Verdict

No blocker, no crate swap. The cost is volume: many small Unix assumptions (PATH split on `:`, `HOME`, `sh`, `/bin/kill`, `/dev/urandom`, unix sockets, `canonicalize`) that compile today and fail at runtime. All five agent CLIs (Claude Code, Codex, OpenCode, Pi, Antigravity) run natively on Windows, no WSL needed.

## Hard compile blockers (agreed by two or more reports)

| Where | Problem |
|---|---|
| ~~`mod browser` ungated~~ | Not a blocker: `browser.rs:339` already has a stub for non-macOS/Linux. WebView2 is a feature, not a fix |
| `crates/agent/src/orchestrator/server.rs:3-4`, `mod.rs:5`, `mcp.rs` | unconditional `std::os::unix::net` |
| ungated tests in driver, agent, git | import `std::os::unix` without `cfg` (list in terminal report, section 7B) |

## Shared primitives (do once, every report depends on them)

All in `aether-core`:

1. `home_dir()` via `std::env::home_dir()` (today `HOME` only, empty on Windows).
2. `find_binary` honoring `PATHEXT`; `search_dirs` and `repaired_path` using `env::split_paths` / `join_paths`.
3. `simplify()` (or `dunce`, already in `Cargo.lock`) replacing every `canonicalize()`, so `\\?\C:\...` never reaches git, PTY cwd, nvim or `starts_with` checks.
4. `quiet(&mut Command)` setting `CREATE_NO_WINDOW`, applied to all ~20 spawn sites, plus `windows_subsystem = "windows"`.
5. `config_dir` = `%APPDATA%\aether`; cache/data/attachments = `%LOCALAPPDATA%\aether`.
6. Random bytes via `getrandom`/`uuid` instead of `/dev/urandom` (`claude/spawn.rs:394`, `opencode/service.rs:300`). Today this yields all-zero UUIDs on Windows.
7. Job Object wrapper (`KILL_ON_JOB_CLOSE`) for process-tree kill, replacing `/bin/kill`, `killpg` and `Child::kill()` orphaning.

## Decisions the reports disagreed on or left open

- **Orchestrator transport:** drivers report says loopback TCP, terminal report says named pipe or TCP. Recommend loopback TCP on `127.0.0.1:<port>` with the existing auth token, on all platforms. Reason: `pi_lead.ts` (Node) connects with `net.createConnection(port, host)` and needs no shim. Security note: any local process can reach the port, so the token must be required on every request and generated from a real CSPRNG (see item 6 above).
- **nvim integration:** needs its own listen address (`\\.\pipe\...`); tmux integration does not exist on Windows, hide its settings.
- **Default shell:** pwsh, then powershell, then `%ComSpec%`. Git Bash and WSL opt-in. Terminal needs `-l` dropped, `/usr/bin/env -u` rewrite skipped, `terminate()` implemented, AltGr fix in `keys.rs:41`.
- **Min Windows version:** Windows 10 1809 (ConPTY floor).

## Phases

**Phase 0: spike (S).** `windows-latest`, MSVC, CMake, LLVM (`LIBCLANG_PATH`), `cargo check --locked -p aether`. Answers the open GPUI/alacritty/whisper questions with facts. Fix nothing in app code until the linker succeeds.

**Phase 1: primitives + compile (S/M).** Shared primitives above. Gate `mod browser` or add the WebView2 `wry` target. Replace unix-socket orchestrator transport. Gate or port the ungated tests. Add `windows-latest` CI running `cargo check`, then `cargo test`.

**Phase 2: spawning (M).** `CREATE_NO_WINDOW`, Job Objects, `.cmd`/`.bat` npm shims with safe argument quoting (CVE-2024-24576 class: never pass untrusted args through `cmd.exe`), replace `/bin/kill`, `/bin/ps`, `lsof`, `/usr/bin/sqlite3` (`agy_rows` to bundled sqlx/rusqlite).

**Phase 3: git (S).** `git.exe` discovery and error text, `-c core.longpaths=true`, `GCM_INTERACTIVE=never`, `taskkill`-based `stop`, worktree removal retry on open handles (close Aether's own PTYs in that folder first).

**Phase 4: terminal MVP (M).** Shell selection, per-shell args and quoting, ConPTY, flip the `cfg!(unix)` UI gates. Defer `running_job`.

**Phase 5: orchestrator + storage (L).** TCP transport across `server.rs`, `mod.rs`, `mcp.rs`, `pi_lead.ts`. Provider roots for `%APPDATA%`/`%LOCALAPPDATA%` (OpenCode), `pi_bucket()` backslash/drive handling, `skills.rs` dirs, Windows process enumeration in `external.rs scan()` (Toolhelp32 or `sysinfo`).

**Phase 6: explorer, notes, browser (M).** `trash` crate for Recycle Bin, case-only rename, "Show in Explorer" label, `open_in.rs` via `ShellExecute`/`explorer.exe`, PowerShell `quote` for notes, WebView2 UA branch, bundle or check the WebView2 runtime. `settings::save` rename retry on sharing violations.

**Phase 7: packaging + release (M).** Portable zip first (`build.py` Windows branch, `.ico` from existing PNGs, sha256 sidecar). Then NSIS, WiX MSI, MSIX only if Store or enterprise matters. Sign with signtool or Azure Trusted Signing once layout stops changing. No updater yet.

**Phase 8: polish (M/L).** Notifications (depends on gpui-kit having a Windows backend, unverified), sysinfo, taskbar badge, `running_job`, whisper CPU-only build tuning (`GGML_NATIVE` off), ARM64.

## Open questions needing a real Windows machine or your call

1. Does gpui-kit 0.7.3 default features enable the Win32/DirectX backend, or is a feature flag needed? (phase 0)
2. Does `alacritty_terminal` with `default-features = false` include its Windows PTY code? Local source says ConPTY is there; confirm by building.
3. Installer identity (WiX upgrade code, NSIS AppUserModelID, MSIX publisher). Hard to change after users upgrade in place, and it decides the update story.
4. Whisper: CPU-only, or accept Vulkan/CUDA SDK and CI cost?
5. Pin CI to `windows-2025` instead of the rolling `windows-latest`?
6. AltGr key_char reporting in gpui-pre, `reveal_path` and notification implementations on Windows.
7. `git apply --reverse` on CRLF worktrees with `autocrlf=true` (predicted failure).
