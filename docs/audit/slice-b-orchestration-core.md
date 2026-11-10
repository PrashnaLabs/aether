# Audit slice B: orchestration and core

## Scope

- `crates/agent`: orchestrator socket server (`orchestrator/server.rs`), MCP stdio server (`orchestrator/mcp.rs`), tool specs, OpenRouter agent (`agent.rs`, `turn.rs`, `gate.rs`, `tools/*`), `usage.rs`, `usage_history/*`, `external.rs`, `store.rs`, `skills.rs`
- `crates/core`: `paths.rs`, `sys.rs`, `settings.rs`, `lib.rs`, `request.rs`, `attachment.rs`
- `crates/app/src/app/orchestrate.rs`, `crates/app/src/app/control.rs`, `crates/app/src/settings.rs`, `crates/app/src/hitl.rs`

## Method

Read each file end to end and traced the call path from the socket to the UI handler: `listen` -> `serve` -> `answer` (token lookup) -> `try_queue` -> `ChatApp::orchestrate` -> tool handler. Followed token minting (`Agent::lead_token`, `random_bytes`) to every place the token reaches (driver env and config, outside the slice, read only). Traced the approval path both ways: `Gate::approve` for Aether's own tools and `answer_request`/`hitl.rs` for CLI asks. Checked what the transcript and chat database keep of an answer. Ran the affected crate test suites after the fixes.

What was checked and found sound:

- **Socket file mode.** Bound then chmod 0600 (`server.rs`). A stale file or symlink at the path is removed with `remove_file`, which never follows a symlink. A live socket owned by another Aether makes `listen` fail instead of stealing it.
- **Token entropy.** 128 bits from `getrandom` (`core/sys.rs`), hex encoded. One token per chat key, stable for the app lifetime.
- **Token comparison.** The token is a `HashMap` key (randomly keyed SipHash). There is no byte-wise compare to time, so constant-time comparison isn't needed.
- **Token scoping.** The token only maps to a chat key. `orchestrate` re-reads `chat.lead` on every call, so a non-lead (or a lead demoted after minting) gets only the board tools and its own `read_history`. `read_agent`, `stop_agent` go through `own_child`: a lead can't read or stop another lead's children. `move_card`/`update_card`/`spawn_agent(card)` are scoped to the lead's project. Child access is clamped to the lead's (`child_access`).
- **Request limits.** 64 KiB request line (`take(MAX_LINE + 1)`), 32 connections, 16 in-flight calls, 5 s socket timeouts, malformed JSON gives a generic error, no `unwrap` on agent JSON in `answer`, `mcp::handle` or `check_args`. MCP stdin line capped at 1 MiB, replies at 1 MiB.
- **Token exposure.** The token is never in argv: Claude gets a config file path, Codex uses `env_vars`, others use process env. The driver's `describe()` logs argv only. `command()` in `driver/src/spawn.rs` strips the orchestrator env from every non-lead spawn.
- **Keys.** The OpenRouter key lives in the keyring (env fallback); `Settings` has no key field and `settings.json` is written 0600 through a temp file and rename. No key, token, or `Authorization` header reaches any `eprintln!`. Usage fetches go to fixed https hosts with a 4 s timeout.
- **OpenRouter agent tools.** It has no file or shell tools: `ask_user`, `web_search` (fixed DuckDuckGo endpoint), `load_skill` (in memory map lookup), `propose_plan`, and the orchestrator tools. So there is no path traversal or command sandbox surface. Skill names are validated (`valid_skill_name`) before any path join; `delete` only touches Aether-owned skills.
- **Chat database.** Created 0600.

## Findings

| ID | Severity | Where | Summary | Status |
|---|---|---|---|---|
| B-SEC-1 | Medium | `app/src/hitl.rs` `answer_request` | Answers to secret questions/form fields stored and shown in plaintext | Fixed |
| B-SEC-2 | Medium | `agent/src/external.rs` `agy_rows` | Antigravity DB copied to predictable `/tmp/aether-agy-<pid>`, dir left behind | Fixed |
| B-SEC-3 | Medium | `app/src/app/orchestrate.rs` `message_agent` | A low-access lead can drive a higher-access chat, bypassing its approvals | Fixed |
| B-SEC-4 | Low | `app/src/app/orchestrate.rs` `spawn_agent` | `spawn_queue` grows without bound | Fixed (spawn queue); message queue Documented only |
| B-SEC-5 | Low | `agent/src/orchestrator/server.rs` | Socket is chmod 0600 after `bind`; parent dir mode not enforced | Fixed |
| B-SEC-6 | Low | `app/src/app/orchestrate.rs` `read_card`, `control.rs` `matching_cards` | Non-lead chats can read cards of other projects | Fixed |
| B-SEC-7 | Low | `agent/src/orchestrator/server.rs` | Per-read (not total) 5 s timeout lets 32 slow clients pin the listener | Fixed |
| B-SEC-8 | Low | `agent/src/orchestrator/server.rs` `answer` | A call that times out after 30 s still runs later, and frees its pending slot | Fixed |
| B-SEC-9 | Low | `agent/src/tools/mod.rs` | `web_search` client has no timeout, body is unbounded | Fixed (15 s total timeout) |
| B-SEC-10 | Info | `app/src/app/orchestrate.rs` `SpawnArgs::parse` | `effort` (and `model` when the probe lists none) reach CLI argv unvalidated | Documented only |
| B-SEC-11 | Info | `app/src/hitl.rs` `enter_submits` | Enter in an Aether-owned approval line means Allow | Documented only |
| B-PERF-1 | Low | `agent/src/usage_history/discovery.rs` | `load_opencode_session_dirs` re-reads every OpenCode session JSON on every scan | Fixed |
| B-PERF-2 | Low | `agent/src/usage_history/aggregate.rs` | Scan cache never evicts, and the global lock is held for the whole scan | Fixed (eviction); lock Documented only |

## B-SEC-1: secret answers kept in clear (Medium, Fixed)

`hitl.rs` masks a secret question or form field while typing (`InputState::masked(q.secret)`), but `answer_request` then did `msg.answer(id, answer.clone())`. That stores `RequestStatus::Answered(Answer::Answers/Form)` with the typed value in the message. Two consequences:

1. `resolved_text` renders it in the transcript: `"<question> -> <answer>"` for `Ask::Questions`.
2. `Message` is serialized into the chat database (`store.rs` `messages TEXT`), so the secret sits on disk and is replayed on restart, in `read_history`-style exports and in any handoff built from messages.

Impact: an API key or password the user typed into a masked prompt (Codex `isSecret`, OpenCode `secret` fields, MCP elicitation) becomes visible plaintext in the UI and persists in `aether.db`. SECURITY.md lists "leaking credentials into transcripts" as in scope.

Fix: new `redact_secrets(ask, answer)` in `hitl.rs`. `answer_request` records the redacted answer in the message and still sends the real answer to the agent (`self.agent.answer(id, answer)`). Masked values keep their count so "answered / skipped" detection is unchanged. Test: `secret_answers_are_masked_before_they_are_kept` in `crates/app/tests/unit/hitl.rs`.

Limit: transcripts written by the provider CLI itself (its own session files) are out of Aether's reach.

## B-SEC-2: predictable temp dir for the Antigravity DB copy (Medium, Fixed)

`agy_rows` copies a live Antigravity conversation database and its WAL (full chat content) to `temp_dir()/aether-agy-<pid>` with `create_dir_all` (default umask, no ownership check), and leaves the directory behind. On Linux `/tmp` is shared: another local user can create `/tmp/aether-agy-<pid>` first (the pid is guessable) and then read the copy, or plant a symlink at `<dir>/<id>.db` so `fs::copy` overwrites a file the victim can write. macOS `$TMPDIR` is per-user, so it is mostly a Linux issue.

Fix: the directory name now has 64 random bits (`random_bytes::<8>`), is created with `create_private_dir` (0700), and is removed after the copy. Test: `agy_rows_reads_with_and_without_a_wal` now also asserts no `aether-agy-<pid>-*` directory is left.

## B-SEC-3: `message_agent` reaches chats with more access than the lead (Medium, Fixed)

`message_agent` accepts any non-archived chat id, by design ("also reaches chats that aren't the lead's children", `docs/orchestration.md`). `spawn_agent` is clamped (`child_access`: a child never exceeds its lead), but the message path had no equivalent. A lead in Ask or Read-only mode could `list_chats`, pick a Full or Auto chat (any project), and `message_agent` it a task. That chat runs it with its own mode and no prompts, so the lead's approval gate is bypassed by proxy. A prompt-injected lead is the realistic route.

Fix: for a target that is not the lead's child, `message_agent` refuses when `rank(target.access) > rank(lead.access)`, the same ordering `child_access` uses. Equal or lower access, and the lead's own children, behave as before. Test: `message_agent_refuses_a_chat_with_more_access_than_the_lead`.

Behaviour change to be aware of: a Full-access lead can still message anything; a lower lead gets `chat N runs with more access than this lead; only the user can message it`. Update `docs/orchestration.md` if you want it stated.

## B-SEC-4: unbounded `spawn_queue` (Low, Fixed in part)

When a lead is at `max_children`, every `spawn_agent` without a matching `nonce` pushes its args (up to 64 KiB) onto `ChatApp::spawn_queue`. A looping lead can grow it without limit; nothing evicts until slots free.

Fix: at most `MAX_QUEUED_SPAWNS` (32) queued spawns per lead; the next gets an error. A retried `nonce` still reuses its entry. Test: `spawn_queue_is_capped_per_lead`.

Not fixed: `message_agent` to a busy chat also appends to `chat.queue` with no cap (same shape, 64 KiB per entry, and it is persisted). Proposed approach: cap queued entries per chat (for example 50) and return the error the same way.

## B-SEC-5: socket bind then chmod (Low, Fixed)

`UnixListener::bind` creates the socket with `0777 & ~umask`, then `set_permissions(0o600)`. The config dir is created 0700 only when it doesn't exist (`create_private_dir` keeps the mode of an existing dir). With a umask of 022 (default on macOS) nobody else can connect in the gap, because connecting needs write permission. Only a umask of 000 leaves a window. The per-chat token is still required for every call, so a connection alone gets nothing.

Fix: Unix sockets now bind inside `<config dir>/orch/`, created atomically with mode 0700. Existing non-private directories and symlinks are rejected; no parent directory is chmod'd. Socket mode remains 0600. Defaults and `docs/orchestration.md` use the new Unix path; Windows keeps its existing address-file path. Tests cover private creation and rejection without changing shared parent permissions.

## B-SEC-6: cross-project board reads from non-lead chats (Low, Documented only)

`read_card` finds a card by id with no project filter, and `list_cards` honours `filter.project = "all"` for any chat that holds a token (every chat gets board tools). `move_card`/`update_card` are project scoped; reads are not. Card ids are millisecond timestamps, so guessable. A prompt-injected agent in project A can read project B's task text and notes. The same cross-project reach is intended for leads (documented `project: "all"`), so this is scope, not a bug in the lead path.

Proposed: for non-lead chats, ignore `project` and `ids` outside the chat's own project in `matching_cards`, and add the project check to `read_card` when `!chat.lead`. Left alone because it changes documented board behaviour and needs a product call.

Status: Fixed in follow-up, after the user decided non-lead chats stay in their own project. `matching_cards` ignores `project` and `ids` outside the chat's project when the chat isn't a lead, and `read_card` adds the same project check. Leads keep `project: "all"`. Test: `non_lead_chats_only_read_their_own_projects_cards`. `docs/task-board.md` is updated.

## B-SEC-7: slow clients (Low, Fixed)

Fix: request reads share one absolute five-second deadline. Each buffered read uses only the remaining time, so trickling bytes cannot renew the budget. The 64 KiB cap remains. Regression test: `trickling_request_cannot_extend_total_read_deadline`.

## B-SEC-8: late execution after a timeout (Low, Fixed)

Fix: socket and in-process calls carry a 30-second deadline. The UI skips expired calls before executing any tool, and only the UI consumer releases queued pending slots, so a stalled UI cannot accumulate calls beyond the existing cap. Tests cover retained slots after timeout and a skipped late mutation. Calls already executing when their deadline passes are not interrupted.

## B-SEC-9: `web_search` client without timeout (Low, Documented only)

`Tools::new` uses `reqwest::Client::new()`; `duckduckgo::search` reads `response.text()` without limit. The endpoint is fixed (no agent controlled URL), so risk is a hung turn. Proposed: `Client::builder().timeout(Duration::from_secs(15))`.

Status: Fixed in backlog follow-up. `Tools::new` builds the client with a 15 s total timeout, which also bounds how long the body read can take.

## B-SEC-10: unvalidated `effort` and `model` (Info, Documented only)

`SpawnArgs::parse` checks `model` only when the probe lists models, and never checks `effort`. Both go into argv as the value of `--model`/`--effort` (`driver/src/claude/spawn.rs`, `antigravity.rs`). They are single argv elements and option values are consumed greedily by the CLIs, so no flag injection was demonstrated. Defence in depth: reject values starting with `-` or containing control characters.

## B-SEC-11: Enter approves an Aether-owned approval (Info, Documented only)

`enter_submits` maps Enter in the approval line to Allow for Aether's own chats (the `web_search` ask) and to Deny for CLI chats. Focus only jumps to a new card when the composer is empty (`takes_focus`), which limits accidental approvals. Consider making Enter a no-op for approvals.

## B-PERF-1: OpenCode session map rebuilt on every scan (Low, Documented only)

`scan` calls `load_opencode_session_dirs()` unconditionally, which walks `opencode/storage/session` and parses every session JSON with `since_ms = 0`. Cost grows with session count and is repeated for every scan, even when no OpenCode file changed. Proposed: keep the map in a `static` keyed on the newest mtime of the session dir, or build it lazily only when an OpenCode message file is actually (re)parsed.

Status: Fixed in follow-up. The map is now a `OnceCell` that is built only when an OpenCode message file misses the cache.

## B-PERF-2: usage cache growth and lock scope (Low, Documented only)

`scan_cached` keeps a process-wide `Mutex<ScanCache>` locked for the whole scan, so concurrent callers queue behind a full pass. The cache entry key is the path; entries for deleted files are never dropped, and each entry holds every parsed record. A growing active session JSONL is re-read in full each scan because the cache key includes size and mtime. Proposed: prune entries not seen in the current listing, and for append-only JSONL resume from the cached length.

Status: Eviction fixed in follow-up. After each scan, cache entries are dropped when the file wasn't listed and no longer exists. Files outside the time window stay cached, so widening the window doesn't re-read them. The lock scope and append-only resume are left as they are; the scan runs off the UI thread.

## Seen outside my slice

- `crates/driver/src/claude/spawn.rs:364` `write_mcp_config`: writes the lead token to `temp_dir()/aether-mcp-<hash>.json` (0600, good) but the file is never deleted, so the token stays on disk after the app exits (until the OS clears temp). On Linux `/tmp` is shared; the name is a SipHash of the token so it can't be pre-created without knowing the token. Proposed: delete on session end, or put it under `config_dir()` (0700).
- `crates/driver/src/spawn.rs:31` `describe()` logs every argv element for each CLI spawn. Fine today (no secrets in argv), but any future secret argument would be logged.
- `crates/core/src/paths.rs` `run_login_shell`: stdout of `sh -l` is taken verbatim as `PATH`, so a profile that prints text corrupts it. Not a security issue.
- Windows: `listen` writes the loopback address with `std::fs::write` (default ACL) and any local process can connect to the TCP port. The token is still required, so this is Info.

## Commands run

See the final report in the task reply.
