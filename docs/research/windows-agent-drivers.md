# Windows Support: Agent Drivers and Orchestration Layer

## Executive Summary

Aether currently targets macOS and Linux. Porting the agent driver and orchestration subsystems to Windows involves bridging fundamental OS differences in binary resolution, process lifecycle management, IPC transport, filesystem path semantics, and configuration storage.

All five supported CLI agents (**Claude Code**, **OpenAI Codex**, **OpenCode**, **Pi**, and **Google Antigravity**) provide native Windows support without requiring WSL. However, Aether's current driver and orchestration code contains Unix-exclusive assumptions:
- Hardcoded Unix shell commands (`sh -l`, `/bin/kill`, `/bin/ps`, `lsof`, `/usr/bin/sqlite3`).
- POSIX-only standard library APIs (`std::os::unix::net::{UnixListener, UnixStream}`).
- File paths split on `:` (which shatters Windows drive letters like `C:\`).
- Unhandled npm batch wrappers (`.cmd`, `.bat`) and absence of `PATHEXT` resolution.
- Omission of `CREATE_NO_WINDOW`, causing console windows to flash on every spawn in a GUI app.
- Tree-kill failures when using `Child::kill()` (which leaves child subprocesses orphaned).
- Reads from `/dev/urandom` that fail on Windows, causing degenerate UUID generation and service startup crashes.

This report details each issue citing `file:line`, outlines technical solutions, provides an issue table with effort sizing, and delivers a recommended implementation roadmap.

---

## Per-Issue Summary Table

| File:Line | Problem | Proposed Fix | Effort |
| :--- | :--- | :--- | :---: |
| `crates/core/src/paths.rs:61-67` | `find_binary` searches for exact binary name without Windows extensions (`.exe`, `.cmd`, `.bat`, `.ps1`), missing npm/native CLIs. | Iterate over `PATHEXT` extensions (default `.COM;.EXE;.BAT;.CMD`) for each directory. | S |
| `crates/core/src/paths.rs:77-79` | `home_dir()` reads `HOME`, which is unset on native Windows. | Fall back to `USERPROFILE`, `HOMEDRIVE`+`HOMEPATH`, or `dirs::home_dir()`. | S |
| `crates/core/src/paths.rs:81-96, 149-162` | `search_dirs` & `repaired_path` split and join `PATH` on `:`, destroying drive letters (e.g., `C:\dir`). | Use `std::env::split_paths` and `std::env::join_paths` (`:` on Unix, `;` on Windows). | S |
| `crates/core/src/paths.rs:107-132` | `run_login_shell` executes `sh -l -c` to extract `PATH`, which fails or hangs on Windows. | `#[cfg(windows)]` bypasses shell execution and queries system + user registry environment (`HKCU\Environment`, `HKLM\...\Environment`). | M |
| `crates/core/src/paths.rs:11-30` | `config_dir()` uses XDG / macOS paths; Windows standard `%APPDATA%` is ignored. | Add `#[cfg(windows)]` resolving `%APPDATA%\aether` (via `known_folders` or `dirs::config_dir()`). | S |
| `crates/driver/src/spawn.rs:12-24, 26-29` | Console window flashing: spawns console processes from GUI GPUI app without `CREATE_NO_WINDOW`. | Apply `CommandExt::creation_flags(0x08000000)` (`CREATE_NO_WINDOW`) on Windows. | S |
| `crates/driver/src/spawn.rs:12-24` | Executing `.cmd` / `.bat` npm wrappers directly via `CreateProcessW` fails or has quote injection hazards. | Route batch scripts through `cmd.exe /c` with sanitized quoting, or execute `.exe` targets directly. | M |
| `crates/driver/src/claude/spawn.rs:392-401` | `new_uuid()` reads `/dev/urandom`. On Windows, read fails, generating all-zero UUID `00000000-0000-4000-8000-000000000000`. | Use `uuid::Uuid::new_v4()` or `getrandom` / `rand` crate (backed by `BCryptGenRandom`). | S |
| `crates/driver/src/opencode/service.rs:298-302` | `password()` reads `/dev/urandom`, hard-failing `connect_lead` on Windows. | Replace with `getrandom` or Windows crypto random API. | S |
| `crates/driver/src/claude/mod.rs:515, 560`<br>`crates/driver/src/codex/mod.rs:367, 434`<br>`crates/driver/src/pi.rs:500, 584, 601`<br>`crates/driver/src/antigravity.rs:230, 493` | `Child::kill()` uses `TerminateProcess`, killing only the parent wrapper process and orphaning tool/compiler child processes. | Assign child processes to a Win32 **Job Object** configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. | M |
| `crates/driver/src/antigravity.rs:336, 497-503`<br>`crates/driver/src/opencode/service.rs:232-237` | Cancellation executes `Command::new("/bin/kill").args([sig, pid])`, which does not exist on Windows. | Use Win32 process termination / Job Object termination for agy and lead servers on Windows. | M |
| `crates/agent/src/orchestrator/server.rs:3-5, 22-34`<br>`crates/agent/src/orchestrator/mod.rs:5, 52-54, 260-262` | Built-in MCP orchestrator uses `UnixListener` and `UnixStream`, which do not compile on Windows. | Adopt Localhost TCP (`127.0.0.1:<random_port>`) with token authentication or Windows Named Pipes (`\\.\pipe\...`). | L |
| `crates/driver/src/pi_lead.ts:8` | `net.createConnection(sock)` in Node.js does not support AF_UNIX filesystem paths on Windows. | Support Windows Named Pipe (`\\.\pipe\...`) or TCP port (`port, "127.0.0.1"`) in `pi_lead.ts`. | M |
| `crates/agent/src/external.rs:56-66` | `scan()` runs Unix `ps` and `lsof` to enumerate processes and cwds; fails immediately on Windows. | Implement Windows process enumeration via Toolhelp32 API / `sysinfo` and query process cwd. | L |
| `crates/agent/src/external.rs:561-570` | `stop()` executes `/bin/ps` and `/bin/kill` to signal external agent processes. | Use Win32 `OpenProcess` / `GenerateConsoleCtrlEvent` or `taskkill.exe`. | M |
| `crates/agent/src/external.rs:683, 691` | `agy_rows` shells out to `/usr/bin/sqlite3`, which does not exist on Windows. | Query SQLite via `rusqlite` / `sqlx` (already in workspace) instead of external CLI binary. | M |
| `crates/agent/src/external.rs:169-171` | `pi_bucket()` only replaces `/` and `:`, leaving Windows backslashes `\` unescaped in directory names. | Replace `['/', '\\', ':']` with `-`. | S |
| `crates/driver/src/opencode/service.rs:115-132`<br>`crates/agent/src/usage_history/discovery.rs:38-43, 126-130` | OpenCode discovery only scans Unix XDG directories (`.local/state`, `.local/share`, `.config`). | Include `%LOCALAPPDATA%\opencode` and `%APPDATA%\opencode` in search paths. | S |
| `crates/git/src/worktree.rs:770-785`<br>`crates/agent/src/store.rs:59-63` | Path comparison mismatch: `canonicalize()` emits `\\?\C:\...`, git emits `C:/...`, and NTFS is case-insensitive. | Strip `\\?\` prefix via `dunce::canonicalize` and perform case-normalized path comparisons on Windows. | M |
| `crates/agent/src/skills.rs:9-21, 29-64` | Skill folder discovery hardcodes `~/.config/...` and `~/.local/...` Unix paths for several sources. | Resolve `%APPDATA%`, `%LOCALAPPDATA%`, and `%USERPROFILE%` equivalents for each provider. | S |

---

## 1. CLI Discovery and Process Spawning

### Binary Locating (`find_binary` & PATHEXT)
In [`crates/core/src/paths.rs:61-67`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/core/src/paths.rs#L61-L67):
```rust
pub fn find_binary(name: &str, override_path: &str) -> Option<PathBuf> {
    if !override_path.is_empty() {
        let p = expand_tilde(override_path);
        return p.is_file().then_some(p);
    }
    search_dirs().into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}
```
On Windows, executable files almost always have a file extension (`.exe`, `.cmd`, `.bat`, `.ps1`). Node-based tools installed via `npm install -g` (such as `@anthropic-ai/claude-code`, `@openai/codex`, `@earendil-works/pi-coding-agent`, or `opencode`) generate shim files in `%APPDATA%\npm\`:
- `claude.cmd` (batch wrapper for cmd.exe)
- `claude.ps1` (PowerShell script)
- `claude` (bash shell script for Git Bash/Cygwin)

Native installers (like `install.ps1` for Claude, Codex, Pi, and Antigravity) place `claude.exe`, `codex.exe`, or `agy.exe` into user directories (e.g., `%LOCALAPPDATA%\Programs\...` or `%USERPROFILE%\.local\bin` or `%LOCALAPPDATA%\agy\bin`).

Calling `d.join("claude")` looks for an extensionless file, which fails on Windows.
**Required Fix**: Inspect the `PATHEXT` environment variable (defaulting to `.COM;.EXE;.BAT;.CMD;.VBS;.JS`). If `name` lacks an extension, test `d.join(format!("{name}{ext}"))` for each candidate extension in `PATHEXT`.

### PATH Delimiters & Drive Letter Preservation
In [`crates/core/src/paths.rs:81-83, 149`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/core/src/paths.rs#L81-L83):
```rust
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        login_path().split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect();
```
On Windows, path entries are delimited by semicolons (`;`), not colons (`:`). Colons are reserved for drive letters (`C:\Program Files\...`). Splitting on `:` splits `C:\Users\name\bin` into `C` and `\Users\name\bin`, completely corrupting the path list.
**Required Fix**: Use `std::env::split_paths` and `std::env::join_paths`.

### Shell Invocation & Environment Resolution
In [`crates/core/src/paths.rs:107-132`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/core/src/paths.rs#L107-L132):
`run_login_shell` spawns `Command::new("sh").args(["-l", "-c", "printf %s \"$PATH\""])`.
On Windows, `sh.exe` is absent unless Git for Windows or MSYS2 is installed and present in `%PATH%`. If missing, this causes a 2-second timeout or immediate error.
**Required Fix**: On Windows (`#[cfg(windows)]`), bypass `run_login_shell`. Windows GUI applications do not inherit a login shell. Instead, read the composite environment PATH directly from:
1. `HKLM\System\CurrentControlSet\Control\Session Manager\Environment` (`Path`)
2. `HKCU\Environment` (`Path`)
3. User profile tools folders (`%LOCALAPPDATA%\Programs\...`, `%USERPROFILE%\.cargo\bin`, `%APPDATA%\npm`, `%LOCALAPPDATA%\agy\bin`).

### Console Window Flashing (`CREATE_NO_WINDOW`)
In [`crates/driver/src/spawn.rs:26-29`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/spawn.rs#L26-L29):
```rust
pub fn spawn_logged(cmd: &mut Command) -> std::io::Result<Child> {
    eprintln!("spawning: {}", describe(cmd));
    cmd.spawn()
}
```
Aether is a GPUI application compiled with `#![windows_subsystem = "windows"]`. By default on Windows, when a non-console GUI process spawns a console application (such as `claude.exe`, `node.exe`, `git.exe`), the Windows kernel creates and displays a new console window (`conhost.exe`). This causes persistent black command prompt windows to pop up and disappear every time a CLI, background server, or git command is invoked.
**Required Fix**:
```rust
#[cfg(windows)]
{
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}
```

### Batch File Quoting & `CreateProcess` (CVE-2024-24576)
If `find_binary` resolves a `.cmd` or `.bat` file (e.g. `claude.cmd`), Win32 `CreateProcessW` cannot execute it directly without `cmd.exe /c`. Furthermore, Rust's standard library enforces strict escaping rules for batch files to prevent argument injection (CVE-2024-24576).
**Required Fix**: Whenever a resolved binary has extension `.cmd` or `.bat`, invoke it via `cmd.exe /c` with arguments properly guarded, or prioritize finding the underlying `.exe` binary (e.g. `node.exe` executing the main entrypoint script) to bypass `cmd.exe` completely.

### Process-Tree Termination & Job Objects
Across all drivers:
- `crates/driver/src/claude/spawn.rs:128`: `child.kill()`
- `crates/driver/src/claude/mod.rs:515, 560`: `child.kill()`
- `crates/driver/src/codex/mod.rs:367, 434`: `child.kill()`
- `crates/driver/src/pi.rs:500, 560, 601`: `child.kill()`
- `crates/driver/src/antigravity.rs:230, 493`: `child.kill()`

On Windows, `Child::kill()` maps to `TerminateProcess(hProcess, 1)`. In Windows, `TerminateProcess` terminates **only** the target process handle; it does **not** terminate child processes spawned by that process. When `claude.cmd` or `node.exe` spawns a subagent, a shell tool, `rg.exe`, or a build command, calling `child.kill()` terminates only the top-level process. The child processes remain active in the background as orphans, keeping file locks active and holding ports open.
**Required Fix**: Use Windows **Job Objects**.
When spawning a driver process on Windows:
1. Create a Win32 Job Object with `CreateJobObjectW`.
2. Configure `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
3. Assign the spawned process to the job with `AssignProcessToJobObject`.
4. When Aether kills the child or drops the driver, closing the job handle or calling `TerminateJobObject` guarantees atomic termination of the entire descendant process tree.

### Cryptographic Randomness on Windows (`/dev/urandom`)
In [`crates/driver/src/claude/spawn.rs:392-401`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/claude/spawn.rs#L392-L401):
```rust
pub(crate) fn new_uuid() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    ...
```
And in [`crates/driver/src/opencode/service.rs:298-302`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/opencode/service.rs#L298-L302):
```rust
fn password() -> std::io::Result<String> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}
```
`/dev/urandom` does not exist on Windows.
- In `ClaudeDriver`, `new_uuid()` fails silently to open `/dev/urandom`. The buffer `b` remains all zeroes, generating the identical UUID `00000000-0000-4000-8000-000000000000` on every session launch.
- In `OpencodeDriver`, `password()` returns `Err`, completely breaking `connect_lead`.
**Required Fix**: Use standard crates `getrandom` or `uuid::Uuid::new_v4()`, which call Windows `BCryptGenRandom`.

---

## 2. Stdio, JSON Streaming, and Signals

### Line Delimiters & Streaming
- Output from CLI tools on Windows often emits CRLF (`\r\n`).
- `BufReader::lines()` in Rust strips both `\r` and `\n`, so line-by-line reading works transparently.
- In [`crates/agent/src/orchestrator/mcp.rs:69`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/orchestrator/mcp.rs#L69), `line.trim_end_matches(['\n', '\r'])` is already applied.
- In [`crates/agent/src/orchestrator/server.rs:61-62`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/orchestrator/server.rs#L61-L62), `read_line` includes `\r\n`; `serde_json::from_str` ignores trailing whitespace.

### Signal Emulation & Cancellation Semantics
Windows has no POSIX signals (`SIGINT`, `SIGTERM`, `SIGKILL`). The drivers interact with cancellation as follows:
1. **Claude Code** ([`crates/driver/src/claude/mod.rs:501-505`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/claude/mod.rs#L501-L505)): Sends `{"type": "control_request", "request": {"subtype": "interrupt"}}` over stdin JSON. 100% portable.
2. **OpenAI Codex** ([`crates/driver/src/codex/mod.rs:558-561`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/codex/mod.rs#L558-L561)): Sends `turn/interrupt` over stdin JSON-RPC. 100% portable.
3. **OpenCode** ([`crates/driver/src/opencode/mod.rs:327`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/opencode/mod.rs#L327)): Issues HTTP POST to `/api/session/{session}/interrupt`. 100% portable.
4. **Pi** ([`crates/driver/src/pi.rs:700-702`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/pi.rs#L700-L702)): Sends `clear_queue` and `abort` over stdin JSON. 100% portable.
5. **Antigravity (`agy`)** ([`crates/driver/src/antigravity.rs:336, 497-503`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/antigravity.rs#L497-L503)):
   ```rust
   fn signal(sig: &str, pid: u32) {
       let _ = Command::new("/bin/kill")
           .args([sig, &pid.to_string()])
           .status();
   }
   ```
   Invokes `/bin/kill`, which fails immediately on Windows. Note that `AntigravityDriver::cancel` notes that SIGINT terminates the process and respawns on the next prompt.
   **Required Fix**: Replace `/bin/kill` with Job Object termination or `TerminateProcess`.
6. **Lead OpenCode Servers** ([`crates/driver/src/opencode/service.rs:229-238`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/opencode/service.rs#L229-L238)):
   `kill_lead_servers()` calls `/bin/kill -KILL <pid>`.
   **Required Fix**: On Windows, iterate `LEAD_PIDS` and terminate each process handle directly via Win32 `OpenProcess` + `TerminateProcess`.
7. **External Agents** ([`crates/agent/src/external.rs:557-572`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/external.rs#L557-L572)):
   `stop()` calls `/bin/ps` and `/bin/kill`.
   **Required Fix**: Use Windows APIs for process validation and termination.

---

## 3. Built-in MCP Orchestrator Server Transport

### Current Implementation
The orchestrator server in [`crates/agent/src/orchestrator/server.rs`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/orchestrator/server.rs) uses Unix domain sockets:
- Lines 3–4: `use std::os::unix::fs::PermissionsExt;` and `use std::os::unix::net::{UnixListener, UnixStream};`
- Line 32: `let listener = UnixListener::bind(path)?;`
- Line 33: `std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;`
- [`crates/agent/src/orchestrator/mod.rs:52-54`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/orchestrator/mod.rs#L52-L54): `socket_path()` returns `config_dir().join("orchestrator.sock")`.
- [`crates/agent/src/orchestrator/mod.rs:260-262`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/orchestrator/mod.rs#L260-L262): `forward()` calls `UnixStream::connect(sock)`.

### How Agent CLIs Reach the Orchestrator
1. **Claude Code, Codex, OpenCode, Antigravity**:
   These CLIs are configured with standard stdio MCP transport pointing to `aether mcp` (which runs [`crates/agent/src/orchestrator/mcp.rs:52-77`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/orchestrator/mcp.rs#L52-L77)):
   - CLI spawns `aether mcp` as a subprocess with environment variables `AETHER_ORCH_SOCK` and `AETHER_ORCH_TOKEN`.
   - CLI speaks JSON-RPC over stdin/stdout with `aether mcp`.
   - `aether mcp` runs `forward()`, connecting to `AETHER_ORCH_SOCK` and forwarding the call to the running Aether GUI app.
2. **Pi (`pi`)**:
   Pi does not use `aether mcp`. It uses [`crates/driver/src/pi_lead.ts`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/pi_lead.ts#L8):
   - An in-process TypeScript extension loaded by Pi.
   - Line 8: `const c = net.createConnection(sock);`
   - Connects directly to `AETHER_ORCH_SOCK`.

### Windows Transport Analysis
On Windows, `std::os::unix::net` does not compile. Furthermore, in Node.js on Windows (`pi_lead.ts`), `net.createConnection(path)` **does not support AF_UNIX socket paths**. In libuv on Windows, `net.createConnection(path)` only supports Named Pipes (format `\\.\pipe\...`).

#### Transport Options:
1. **Localhost TCP Loopback (`127.0.0.1:<ephemeral-port>`)** — *(Recommended)*:
   - Server: Uses standard `std::net::TcpListener::bind("127.0.0.1:0")`. Port is assigned dynamically.
   - Authentication: Already enforced via `AETHER_ORCH_TOKEN`.
   - `aether mcp`: Uses standard `std::net::TcpStream::connect(addr)`. No external dependencies or platform-specific CFGs needed.
   - `pi_lead.ts`: `net.createConnection(port, "127.0.0.1")` works identically on Windows, macOS, and Linux.
   - `AETHER_ORCH_SOCK` environment variable: Can hold `127.0.0.1:<port>`.
2. **Windows Named Pipes (`\\.\pipe\aether-orch-<hash>`)**:
   - Server: Requires `tokio::net::windows::named_pipe` or the `interprocess` crate.
   - `aether mcp`: Requires Named Pipe client code on Windows.
   - `pi_lead.ts`: `net.createConnection("\\\\.\\pipe\\aether-orch-...")` is natively supported by Node.js.
   - More complex in Rust than standard TCP loopback.

---

## 4. Storage Locations, Discovery, and Handoff

### Per-CLI Windows Storage Paths

| CLI | Default Path on Windows | Environment Variable | Current Aether Assumption | Changes Needed |
| :--- | :--- | :--- | :--- | :--- |
| **Claude Code** | `%USERPROFILE%\.claude` | `CLAUDE_CONFIG_DIR` | `~/.claude` ([`spawn.rs:381`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/claude/spawn.rs#L381)) | Ensure `home_dir()` maps to `USERPROFILE`. Verify project directory naming (`C-path` vs `/path`). |
| **OpenAI Codex** | `%USERPROFILE%\.codex` | `CODEX_HOME` | `~/.codex` ([`external.rs:90`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/external.rs#L90)) | Ensure `home_dir()` maps to `USERPROFILE`. Session directory layout is identical (`sessions/YYYY/MM/DD`). |
| **OpenCode** | Config: `%USERPROFILE%\.config\opencode`<br>Storage: `%LOCALAPPDATA%\opencode` or `%USERPROFILE%\.local\share\opencode` | `XDG_CONFIG_HOME`, `XDG_DATA_HOME` | Only checks `.local/share` & `.local/state` ([`discovery.rs:41`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/usage_history/discovery.rs#L41)) | Add `%LOCALAPPDATA%\opencode\storage` and `%APPDATA%\opencode` to search roots. |
| **Pi** | `%USERPROFILE%\.pi\agent` | `PI_CODING_AGENT_DIR` | `~/.pi/agent` ([`pi.rs:272`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/pi.rs#L272)) | Fix `pi_bucket()` in `external.rs:170`: must escape backslashes `\` in addition to `/` and `:`. |
| **Antigravity** | `%USERPROFILE%\.gemini\antigravity-cli`<br>Bin: `%LOCALAPPDATA%\agy\bin` | None | `~/.gemini/antigravity-cli` ([`antigravity.rs:400`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/driver/src/antigravity.rs#L400)) | Replace `/usr/bin/sqlite3` call with internal SQLite engine in `agy_rows` ([`external.rs:683`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/external.rs#L683)). |

### External Agent Discovery (`crates/agent/src/external.rs`)
In [`crates/agent/src/external.rs:56-66`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/external.rs#L56-L66):
```rust
pub fn scan(own_pid: u32, own_sessions: &HashSet<String>) -> Option<Vec<ExternalAgent>> {
    let ps = parse_ps(&run("ps", &["-axww", "-o", "pid=,ppid=,etime=,lstart=,command="])?);
    ...
    let cwds = (!pids.is_empty())
        .then(|| run("lsof", &["-a", "-p", &pids.join(","), "-d", "cwd", "-Fpn"]))
```
`ps` and `lsof` do not exist on Windows. `scan()` will always return `None`, meaning external agent discovery, live status tracking, and session handoff are completely inoperative on Windows.
**Required Fix**:
- Implement a Windows process scanner module using Win32 Toolhelp32 snapshot APIs (`CreateToolhelp32Snapshot`, `Process32FirstW`, `Process32NextW`).
- Query the command line for target processes (`claude`, `codex`, `pi`, `opencode`, `agy`) using `NtQueryInformationProcess` (`ProcessCommandLineInformation`).
- Read working directory via Win32 Process Environment Block (PEB) or use Windows PowerShell / WMI fallback.

---

## 5. Path Handling

### Backslashes, Drive Letters, and Colons
- **Path Separation**: Windows paths use `\` or `/` interchangeably in Win32 APIs, but CLI output and scripts often mix them. Any string replacement logic that assumes `/` (e.g., `pi_bucket` in `external.rs:170`) breaks unless both `\` and `/` are handled.
- **Drive Letters**: Paths like `C:\repo` contain a colon at index 1. Any code splitting by `:` (such as `login_path().split(':')` in `paths.rs:83` and `repaired_path()` in `paths.rs:149`) must be replaced with `std::env::split_paths`.

### Extended-Length Path Prefix (`\\?\`)
- In Rust, calling `std::fs::canonicalize(path)` on Windows prepends the verbatim UNC prefix `\\?\` (e.g. `\\?\C:\Users\username\project`).
- Git for Windows, Claude Code, and various other tools do not accept or normalize `\\?\` verbatim paths.
- In [`crates/git/src/worktree.rs:770-785`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/git/src/worktree.rs#L770-L785), `git worktree list --porcelain` outputs standard paths (e.g. `worktree C:/path/to/worktree`).
- Direct equality checks (`path == canonicalized_path`) fail because `C:\path` does not equal `\\?\C:\path`.
**Required Fix**: Use the standard `dunce::canonicalize` crate (or an equivalent helper) instead of `std::fs::canonicalize`. `dunce` produces canonical paths without the `\\?\` prefix whenever paths fit within standard Windows limits.

### Case-Insensitivity
- Windows NTFS and ReFS filesystems are case-insensitive (`C:\Project` and `c:\project` reference the exact same file).
- Git, user input, and different tools may report the drive letter or folders in uppercase or lowercase.
- In [`crates/agent/src/external.rs:476`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/external.rs#L476): `s.1 == a.cwd` tests exact byte equality of `PathBuf`.
- In [`crates/agent/src/store.rs`](file:///Users/iris/Downloads/xzx/PERSONAL/aether/crates/agent/src/store.rs): SQLite queries match `cwd` and `project` using standard SQL `WHERE cwd = ?`.
**Required Fix**: Normalize paths to lowercase when comparing or hashing them for caching, or use case-insensitive path comparisons on Windows.

---

## 6. Native Windows Support vs WSL Verification

All five CLIs are supported **natively** on Windows. None require WSL for operation:

1. **Anthropic Claude Code (`claude`)**:
   - **Status**: Native Windows support.
   - **Installation**: Native PowerShell installer (`irm https://claude.ai/install.ps1 | iex`) or npm (`npm install -g @anthropic-ai/claude-code`).
   - **Documentation**: [Anthropic Claude Code Docs](https://docs.anthropic.com/en/docs/agents-and-tools/claude-code/overview)
2. **OpenAI Codex CLI (`codex`)**:
   - **Status**: Native Windows support.
   - **Installation**: Native PowerShell installer (`irm https://chatgpt.com/codex/install.ps1 | iex`), npm (`npm install -g @openai/codex`), or `winget`.
   - **Documentation**: [OpenAI Codex Documentation](https://platform.openai.com/docs/codex)
3. **OpenCode (`opencode`)**:
   - **Status**: Native Windows support.
   - **Installation**: Native Windows binary release, npm package, or PowerShell script.
   - **Documentation**: [OpenCode Installation & Configuration](https://opencode.ai/docs)
4. **Pi (`@earendil-works/pi-coding-agent`)**:
   - **Status**: Native Windows support.
   - **Installation**: PowerShell installer (`irm https://pi.dev/install.ps1 | iex`) or npm (`npm install -g @earendil-works/pi-coding-agent`).
   - **Details**: Uses Git Bash (`bash.exe`) or PowerShell natively for shell commands on Windows.
   - **Documentation**: [Pi Coding Agent Documentation](https://pi.dev/docs)
5. **Google Antigravity (`agy`)**:
   - **Status**: Native Windows support.
   - **Installation**: PowerShell installer (`irm https://antigravity.google/cli/install.ps1 | iex`) or CMD script (`install.cmd`). Installs to `%LOCALAPPDATA%\agy\bin`.
   - **Documentation**: [Antigravity CLI Reference](https://antigravity.google/docs/cli/reference)

---

## 7. Recommended Implementation Order

To port the driver and orchestration layer cleanly with minimal risk and maximum testability, work should proceed in five distinct phases:

### Phase 1: Core Foundation & Path Normalization (Effort: S–M)
1. **Fix `crates/core/src/paths.rs`**:
   - Replace `:` path splitting with `std::env::split_paths` / `join_paths`.
   - Fix `home_dir()` to check `%USERPROFILE%`.
   - Replace `run_login_shell` with Windows Registry environment resolution.
   - Update `find_binary` to iterate `PATHEXT` extensions (`.exe`, `.cmd`, `.bat`).
   - Configure `config_dir()` to return `%APPDATA%\aether`.
2. **Replace `/dev/urandom`**:
   - Fix `new_uuid` in `claude/spawn.rs` and `password` in `opencode/service.rs` using `getrandom` or `uuid::Uuid::new_v4`.

### Phase 2: Process Spawning & Job Objects (Effort: M)
1. **Console Flashing Fix**:
   - Add `CREATE_NO_WINDOW` creation flag in `crates/driver/src/spawn.rs` and all child process commands.
2. **Windows Job Objects for Clean Tree Termination**:
   - Create a `JobObject` wrapper in `crates/driver/src/spawn.rs` (or `crates/core`).
   - Attach all spawned driver children to a Job Object configured with `KILL_ON_JOB_CLOSE`.
   - Replace POSIX `signal` / `/bin/kill` invocations in `antigravity.rs` and `opencode/service.rs` with Win32 process termination.

### Phase 3: Built-in MCP Orchestrator Transport (Effort: L)
1. **Migrate Transport to Localhost TCP**:
   - Replace `UnixListener` / `UnixStream` in `crates/agent/src/orchestrator/server.rs` and `mod.rs` with `TcpListener` bound to `127.0.0.1:0`.
   - Update `socket_path` / `AETHER_ORCH_SOCK` to provide `127.0.0.1:<port>`.
   - Update `crates/agent/src/orchestrator/mcp.rs` to connect via TCP.
   - Update `crates/driver/src/pi_lead.ts` to parse `host:port` and connect via `net.createConnection(port, host)`.
   - This eliminates all OS-specific socket shims across both Rust and Node.js.

### Phase 4: Storage Discovery & Path Normalization (Effort: M)
1. **Update Provider Storage Roots**:
   - Extend `provider_roots()` in `discovery.rs` to include `%LOCALAPPDATA%` and `%APPDATA%` paths for OpenCode.
   - Fix `pi_bucket()` in `external.rs` to strip drive prefixes and replace backslashes.
   - Update `skills.rs` directories to search `%APPDATA%` / `%USERPROFILE%`.
2. **Dunce / Canonical Path Normalization**:
   - Strip `\\?\` verbatim prefixes on canonicalized paths to maintain compatibility with Git worktree commands and transcript parsers.

### Phase 5: External Agent Scanning & Ingestion (Effort: M–L)
1. **Windows Process Enumerator**:
   - Implement `scan()` in `crates/agent/src/external.rs` for Windows using Toolhelp32 process snapshotting or `sysinfo`.
   - Replace `/usr/bin/sqlite3` invocations in `agy_rows` with internal `rusqlite` / `sqlx` database queries.
