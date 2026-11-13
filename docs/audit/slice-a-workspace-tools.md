# Slice A Audit: Workspace Tools

**Auditor:** Slice A (Workspace Tools Agent)  
**Date:** 2026-11-10  
**Scope:**
- `crates/git`, `crates/explorer`, `crates/terminal`, `crates/voice`
- `crates/app/src/terminal.rs`, `crates/app/src/explorer/`, `crates/app/src/git_panel/`
- Packaging and CI: `scripts/`, `packaging/`, `PKGBUILD`, `.github/workflows/`

---

## Method

1. **Static Analysis & Control-Flow Tracing:** Traced end-to-end inputs from user interactions, git references, remote branches, filesystems, and environment variables into external command executions (`git`, shells, `/usr/bin/env`, `powershell`, `trash`), looking for argument injection, missing `--` separators, unescaped path variables, or unsanitized inputs.
2. **Environment & Secrets Leakage:** Audited PTY terminal initialization (`crates/terminal/src/session.rs`), environment inheritance, and driver execution parameters against the orchestration socket threat model (`AETHER_ORCH_*`).
3. **Filesystem Traversal & Symlink Handling:** Audited `crates/explorer/src/` (`ops.rs`, `tree.rs`, `search.rs`, `index.rs`) and `crates/app/src/explorer/` for arbitrary path traversal, symlink loop amplification, and unvalidated file modifications outside the workspace root.
4. **Performance & UI Thread Blocking:** Profiled hot execution paths on GPUI render loops, scrollback buffers, transcription audio processing, model integrity checking, and synchronous directory I/O.
5. **Packaging & CI Pipeline Review:** Audited release build automation, archive permissions, dependencies, and GitHub Actions workflows for unsigned artifacts or privilege leakage.
6. **Verification & Testing:** Verified all findings and local surgical fixes against automated unit test suites (`cargo test --locked`, `cargo clippy --locked`, `python3 -m unittest`) using the warm shared target cache.

---

## Findings Summary

| ID | Severity | Component / Location | Summary | Status |
|---|---|---|---|---|
| **A-SEC-1** | High | `crates/git/src/lib.rs:1058` | Git push argument injection via branch or remote names starting with `-` | Fixed |
| **A-SEC-2** | Medium | `crates/git/src/refs.rs:212`, `worktree.rs:355` | Missing `--` separator before branch/commit arguments in `git switch` and `git branch -d` | Fixed |
| **A-SEC-3** | High | `crates/terminal/src/session.rs:544` | Terminal PTY leaks orchestration socket credentials (`AETHER_ORCH_*`) | Fixed |
| **A-SEC-4** | High | `crates/app/src/explorer/tree.rs:516` | Notes new file creation bypassed canonical path allowed checks | Fixed |
| **A-PERF-1** | Medium | `crates/voice/src/model.rs:40`, `transcribe.rs:42` | Whisper transcription re-hashes 148 MB model binary on every turn | Fixed |
| **A-PERF-2** | Medium | `crates/explorer/src/search.rs:167` | Explorer grep reads symlinks, special files, and huge files without bounds | Fixed |
| **A-PERF-3** | Medium | `crates/terminal/src/grid.rs:375` | Terminal find allocates unbounded matches over 10k scrollback lines on UI thread | Fixed |
| **A-PERF-4** | Medium | `crates/app/src/explorer/editor.rs:448` | Markdown preview re-allocates and re-parses full document per render frame | Documented only |
| **A-PERF-5** | Low | `crates/app/src/explorer/mod.rs:161` | Explorer refresh executes synchronous directory read tree walking on UI thread | Fixed in follow-up |
| **A-PACK-1** | Low | `scripts/build.py:222`, `test_packaging.py:102` | Linux release tarballs preserve build host UID/GID; test runner hardcoded python3 | Fixed |

---

## Detailed Findings

### A-SEC-1: Git push argument injection via branch or remote names starting with `-`
- **Severity:** High
- **Location:** `crates/git/src/lib.rs:1058`, `crates/git/src/lib.rs:489`
- **What is wrong:** When publishing a new branch to a remote (`push(top)`), `args.extend(["--set-upstream", &remote, &branch])` constructed arguments without a `--` separator, and without validating that `branch` or `remote` does not start with `-`.
- **Evidence:**
  ```rust
  remote = pick_remote(&run_args(top, &["remote"], QUICK)?.stdout).ok_or(...)?;
  args.extend(["--set-upstream", &remote, &branch]);
  ```
  If a branch name starts with `-d`, `--delete`, or `--force`, git interprets the branch name as a CLI flag, executing unintended branch deletion or forced pushes. Furthermore, if a single remote exists named `--upload-pack=...`, `pick_remote` would select it as the remote target.
- **Impact:** Maliciously crafted branch names (e.g., in a cloned repository or agent checkout) can delete remote refs or execute arbitrary git flags when the user clicks "Publish branch".
- **Fix:** Validated that `branch` does not start with `-` (returning an error early), passed `--` separator before `&branch`, and filtered out remotes starting with `-` in `pick_remote`.
- **Status:** Fixed. Regression test `push_refuses_dash_prefixed_branch_name` and unit tests in `crates/git/tests/unit/lib.rs` verify rejection.

---

### A-SEC-2: Missing `--` separator before arguments in `git switch` and `git branch -d`
- **Severity:** Medium
- **Location:** `crates/git/src/refs.rs:212, 240`, `crates/git/src/worktree.rs:355`
- **What is wrong:** In `create_branch`, the `base` commit/ref argument was appended to `git switch -c <name>` without a `--` separator. Similarly, `detach` invoked `git switch --detach <sha>` without `--`, and `remove_worktree_with_branch` invoked `git branch -d <branch>` without `--`.
- **Evidence:**
  In `crates/git/src/refs.rs`:
  ```rust
  let mut args = vec!["switch", "--no-track", "-c", name];
  if let Some(base) = base {
      ...
      args.push(base);
  }
  ```
- **Impact:** If `base` or `branch` started with `-` (e.g., an orphaned ref or malicious commit tag), git could treat it as an option flag rather than a positional target ref.
- **Fix:** Added `--` separators before `base`, `sha`, and `branch`, and validated `!base.starts_with('-')`.
- **Status:** Fixed.

---

### A-SEC-3: Terminal PTY leaks orchestration socket credentials (`AETHER_ORCH_*`)
- **Severity:** High
- **Location:** `crates/terminal/src/session.rs:544-553`
- **What is wrong:** `foreign_terminal_vars` strips emulator-specific variables (`ITERM_*`, `KITTY_*`, `VSCODE_*`), but did not strip internal orchestration variables (`AETHER_ORCH_SOCK`, `AETHER_ORCH_TOKEN`, `AETHER_ORCH_SPECS`, `OPENCODE_CONFIG_CONTENT`).
- **Evidence:**
  In `crates/driver/src/spawn.rs`, spawned agents explicitly remove `AETHER_ORCH_SOCK`, `AETHER_ORCH_TOKEN`, etc. to prevent unauthorized lead token access. However, in `crates/terminal/src/session.rs`, when opening a terminal drawer or tab, the PTY process inherited the host process environment without filtering `AETHER_ORCH_*`.
- **Impact:** Any terminal command, untrusted script, or child process executed inside the embedded terminal could read `AETHER_ORCH_TOKEN` and communicate with `AETHER_ORCH_SOCK`, gaining full lead agent capabilities and bypassing permission prompts.
- **Fix:** Added `"AETHER_ORCH_SOCK"`, `"AETHER_ORCH_TOKEN"`, `"AETHER_ORCH_SPECS"`, `"OPENCODE_CONFIG_CONTENT"`, and the `"AETHER_ORCH_"` prefix to `foreign_terminal_vars` so `/usr/bin/env -u` unsets them prior to shell launch.
- **Status:** Fixed. Regression test `orchestration_secrets_are_dropped_from_pty_env` added to `crates/terminal/tests/unit/session.rs`.

---

### A-SEC-4: Notes new file creation bypassed canonical path allowed checks
- **Severity:** High
- **Location:** `crates/app/src/explorer/tree.rs:516`
- **What is wrong:** `ask_new_folder` and `ask_rename_path` guard against symlink path traversal via `self.notes_tree_path_allowed(&dir, cx)`. However, `ask_new_file` was missing this check.
- **Evidence:**
  ```rust
  fn ask_new_file(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
      // Missing: if !self.notes_tree_path_allowed(&dir, cx) { return; }
      let op: NameOp = |this, dir, name, window, cx| {
          let made = aether_explorer::create_file(dir, name);
          ...
  ```
- **Impact:** Allowed arbitrary file creation outside the canonical notes directory if a symlinked directory or traversed path was selected.
- **Fix:** Added `if !self.notes_tree_path_allowed(&dir, cx) { return; }` to `ask_new_file`.
- **Status:** Fixed.

---

### A-PERF-1: Whisper transcription re-hashes 148 MB model binary on every turn
- **Severity:** Medium
- **Location:** `crates/voice/src/model.rs:40`, `crates/voice/src/transcribe.rs:42`
- **What is wrong:** Every audio transcription turn invokes `verify_model(model)`, which directly invoked `file_verified`, reading and SHA-256 hashing the entire 147,964,211 byte file from disk on every speech transcription.
- **Evidence:**
  In `crates/voice/src/model.rs`:
  ```rust
  pub(crate) fn verify_model(path: &Path) -> bool {
      file_verified(path, SHA256, MODEL_BYTES)
  }
  ```
  `installed()` maintained a cached `CHECKED: Mutex<Option<(PathBuf, u64, Option<SystemTime>, bool)>>` verifying mtime and file size, but `verify_model` completely bypassed the cache.
- **Impact:** Significant CPU spikes (100–300 ms) and disk I/O latency each time the user speaks a voice command.
- **Fix:** Routed `verify_model` through the `CHECKED` cache, re-hashing only when file size or mtime differs, and updated `installed()` to share the implementation.
- **Status:** Fixed. Regression test `verify_model_rejects_wrong_size_immediately` added to `crates/voice/tests/unit/model.rs`.

---

### A-PERF-2: Explorer grep reads symlinks, special files, and huge files without bounds
- **Severity:** Medium
- **Location:** `crates/explorer/src/search.rs:167-178`
- **What is wrong:** In `grep_files`, files returned from the index were read using `std::fs::read_to_string(&path)` without size or file-type checks.
- **Evidence:**
  ```rust
  for rel in files {
      let path = root.join(rel);
      let Ok(content) = std::fs::read_to_string(&path) else { continue; };
      ...
  ```
- **Impact:** If the repository contains a symlink to `/dev/zero`, a FIFO named pipe, or a multi-gigabyte generated file, `read_to_string` causes indefinite thread hangs, multi-gigabyte memory consumption, or process out-of-memory crashes.
- **Fix:** Checked `std::fs::symlink_metadata(&path)` prior to reading: skipped non-regular files (including symlinks, FIFOs, devices) and files exceeding 10 MB (`10 * 1024 * 1024`).
- **Status:** Fixed. Regression test `grep_files_skips_symlinks_and_non_regular_files` added to `crates/explorer/tests/unit/search.rs`.

---

### A-PERF-3: Terminal find allocates unbounded matches over 10k scrollback lines on UI thread
- **Severity:** Medium
- **Location:** `crates/terminal/src/grid.rs:375-388`, `crates/terminal/src/view/find.rs:66`
- **What is wrong:** `search_lines` collected all substring matches across 10,000 lines of scrollback history synchronously on the UI thread without any result cap.
- **Evidence:**
  When searching common characters like `" "` or `"e"` across a full terminal buffer, `search_lines` allocated hundreds of thousands of `SearchMatch` structs and vectors in the UI loop.
- **Impact:** Severe frame drops, UI freezing, and memory thrashing when typing queries in terminal search.
- **Fix:** Capped matches in `search_lines` at 1,000 (`MAX_MATCHES`), terminating search loops early once reached.
- **Status:** Fixed. Regression test `search_lines_caps_matches_at_limit` added to `crates/terminal/tests/unit/grid.rs`.

---

### A-PERF-4: Markdown preview re-allocates and re-parses full document per render frame
- **Severity:** Medium
- **Location:** `crates/app/src/explorer/editor.rs:448-465`
- **What is wrong:** When the file preview mode is active for markdown files in the project or notes explorer, `render` extracts `editor.as_ref().map(|e| e.read(cx).text().to_string())` and instantiates a new `Markdown::new(...)` GPUI element on every frame render.
- **Evidence:**
  ```rust
  .child(if preview {
      let text = editor.as_ref().map(|e| e.read(cx).text().to_string()).unwrap_or_default();
      ...
      Markdown::new(format!("explorer-markdown-{}", buffer_id.unwrap_or_default()), text.into())
  ```
- **Impact:** Substantial CPU allocation and markdown AST re-parsing per 60/120 Hz render tick, causing UI stutter while scrolling or typing in adjacent panels.
- **Proposed Approach:** Cache the rendered markdown element or parsed document tied to the buffer's edit generation/version ID so re-parsing occurs only upon content modification.
- **Status:** Documented only (requires structural GPUI element caching redesign).

---

### A-PERF-5: Explorer refresh executes synchronous directory read tree walking on UI thread
- **Severity:** Low
- **Location:** `crates/app/src/explorer/mod.rs:161`
- **What is wrong:** `refresh_explorer` calls `explorer.update(cx, |exp, _| exp.refresh_tree());`. `refresh_tree()` runs `std::fs::read_dir` synchronously for all expanded folders on the main GPUI thread.
- **Evidence:**
  While `load_explorer_index` runs off-thread on line 177, `refresh_tree` executes synchronously on line 161. On networked filesystems or deep directory structures with dozens of open folders, directory reads block the UI thread.
- **Proposed Approach:** Dispatch `refresh_tree()` folder scanning to `cx.background_executor()` and apply updated tree nodes back on the UI thread asynchronously.
- **Status:** Fixed in follow-up. Manual and completed-tool refreshes capture a tree snapshot and scan it on the background executor. Create/rename reveal runs inside the same background job after refresh. Only the latest generation applies, and merging listings preserves selections and expansion changes made during the scan. A pending reveal carries forward if another refresh supersedes its job. Regression tests in `crates/explorer/tests/unit/view.rs` cover deferred reads, reveal, stale results and intervening interactions.

---

### A-PACK-1: Linux release tarballs preserve build host UID/GID; test runner hardcoded python3
- **Severity:** Low
- **Location:** `scripts/build.py:222`, `scripts/tests/test_packaging.py:102`
- **What is wrong:** `tarfile.open(artifact, "w:gz")` added files without a filter, preserving the build machine's local user UID, GID, username, and unnormalized file modes in distributed release archives. Additionally, `test_packaging.py` executed `["python3", SCRIPT]`, failing on systems where `python3` defaults to Python < 3.11.
- **Evidence:**
  ```python
  with tarfile.open(artifact, "w:gz") as archive:
      for child in sorted(stage.iterdir()):
          archive.add(child, arcname=f"{stem}/{child.name}")
  ```
- **Impact:** Release tarballs leak build infrastructure user names and IDs and risk incorrect file permissions when unpacked without `-p`.
- **Fix:** Added `tar_filter` normalizing UID/GID to 0 (`root:root`) and modes (`0o755` for directories and executables, `0o644` for files). Updated `test_packaging.py` to use `sys.executable`.
- **Status:** Fixed. Unit test `test_tar_filter_normalizes_ownership_and_permissions` added to `scripts/tests/test_packaging.py`.

---

## Seen Outside My Slice

The following issues were observed during audit tracing in code owned by other slices:

1. **Synchronous Git on UI Thread in Orchestration (`crates/app/src/app/orchestrate.rs:236`):**
   ```rust
   // ponytail: sync git on the UI thread (3s timeout)
   let branch = aether_git::branch_info(project).map(|b| b.branch);
   ```
   `worktree_base` invokes git command execution with a 3-second blocking timeout on the GPUI UI thread during orchestration flow setup. Should be dispatched to `cx.background_executor().spawn(...)`.

2. **Synchronous Git History Loading on UI Thread (`crates/app/src/project_page/data.rs:23`):**
   ```rust
   aether_git::history(&dir)
   ```
   Directly called during project page data initialization, synchronously executing `git log` on the UI thread.

3. **Git Refresh during UI Render (`crates/app/src/app/render.rs:47`):**
   ```rust
   self.refresh_git(false, cx);
   ```
   Invoked directly inside `Render::render` when changing active chats, triggering git status checks during layout rendering.
