# Performance and security audit, 2026-11-10

Branch: `feat/misc` (base `8fe1868`). Changes are uncommitted in the working tree.

## How it ran

The codebase (~106k lines of Rust across 9 crates) was split into four slices.
Each slice went to one agent, spawned by an Aether lead in its own git worktree.
Every agent audited its slice against the threat model in `SECURITY.md`, wrote a
report, fixed the confirmed issues, and added regression tests. The lead then
reviewed each diff and changed or dropped some fixes before merging them into
`feat/misc`.

| Slice | Agent | Scope | Report |
|---|---|---|---|
| A | Antigravity, gemini-3.8-flash-high | git, explorer, terminal, voice, explorer and git panels, packaging scripts | [slice-a-workspace-tools.md](slice-a-workspace-tools.md) |
| B | Claude Code, sonnet, high effort | orchestrator socket, `aether mcp`, lead tokens, core, approvals (HITL), settings | [slice-b-orchestration-core.md](slice-b-orchestration-core.md) |
| C | Codex, gpt-6.1-sol, low effort | provider CLI drivers, chat send path | [slice-c-drivers.md](slice-c-drivers.md) |
| D | Pi, openrouter/stealth/crouton-alpha, max effort | ui crate, browser panel, links, notifications, attachments, rest of the app | [slice-d-ui-app.md](slice-d-ui-app.md) |

Board: epic card #74, slice cards #75 to #78, and merge card #79. Open items
are filed as backlog cards.

## Fixed

### Security

| ID | Severity | Fix |
|---|---|---|
| D-SEC-1 | High | The browser panel only loads `http`, `https` and `about:blank`. The omnibox, HN links and `window.open` used to accept `file:`, `data:`, `mailto:` and any `scheme://`. |
| D-SEC-2 | High | Links in agent replies only reach the OS opener for `http`, `https` and `mailto`. Before, any scheme was passed on, including custom app URL handlers like `vscode://`. |
| A-SEC-1 | Medium (agent said High) | `git push --set-upstream` now puts `--` before the branch, and refuses branch and remote names that start with `-`. |
| A-SEC-2 | Medium | Added `--` before the ref arguments in `git switch -c`, `git switch --detach` and `git branch -d/-D`. |
| A-SEC-4 | Medium (agent said High) | Creating a new file in the Notes explorer now goes through the same symlink and root check as the other note operations. |
| B-SEC-1 | Medium | Answers to secret questions and form fields are masked in the transcript and chat DB. The agent still receives the real value. |
| B-SEC-2 | Medium | The temp copy of the Antigravity DB now uses a random, private (0700) directory that is removed afterwards. Before, it went to a predictable `/tmp/aether-agy-<pid>` that was never removed. |
| B-SEC-3 | Medium | A lead can no longer message a chat that is not its child and has more access than the lead. Doing so let it skip that chat's approval prompts. |
| A-SEC-3 | Low (agent said High) | The embedded terminal's shell no longer inherits `AETHER_ORCH_*` or `OPENCODE_CONFIG_CONTENT`. This only matters when Aether itself was started from a lead's shell. |
| B-SEC-4 | Low | At most 32 spawns can be queued per lead. |
| C-SEC-2 | Low | `AETHER_RECORD` recordings are created 0600 inside a 0700 directory. |
| D-SEC-3 | Low | `store_bytes` rejects attachments over `MAX_ATTACHMENT_BYTES`, both before decoding and after PNG conversion. |
| A-PACK-1 | Low | Release tarballs are written with root:root owners and normalized file modes. |

### Performance

| ID | Severity | Fix |
|---|---|---|
| D-PERF-1 | Medium | Markdown blocks are only re-parsed when their text changes. Before, `set_text` ran on every render for every visible block. This also fixes most of A-PERF-4 (explorer markdown preview), which uses the same component. |
| A-PERF-1 | Medium | The 148 MB Whisper model is hashed once per (path, size, mtime) instead of on every dictation turn. |
| A-PERF-2 | Medium | Explorer grep skips symlinks, special files and files over 10 MB. |
| A-PERF-3 | Medium | Terminal find stops after 1000 matches. |
| C-PERF-3 | Medium, partial | Pi `toolcall_delta` only re-parses the argument buffer when it ends with `}`. Most deltas now skip the JSON parse. Fixed by the lead. |
| C-PERF-4 | Low | Claude subagent activity copies at most `ACTIVITY_CHARS` characters instead of the whole buffer. |

## Changes made in lead review

- **D-SEC-1 navigation handler removed.** The agent also passed
  `with_navigation_handler(navigable)` to wry. On macOS, wry calls that handler
  for every frame, including iframes, so it would cancel `about:srcdoc`, `blob:`
  and `data:` iframes and break ordinary sites. WebKit already blocks web pages
  from navigating to `file:`, and the only IPC message a page can send is "cycle
  tab", so the risk it covered was small. The entry-point allowlist stays.
- **Slice D false positive.** Its "seen outside my slice" note on `NoticeLink::Open`
  is not a bug: `notice_link` only builds that variant for `http://` and `https://`.
- **Severity changes.** Porcelain git refuses branch names that start with `-`, so
  A-SEC-1 needs a crafted repo, made with plumbing like `git update-ref` or
  `git remote add -- -name`. That makes it Medium defense in depth, not High.
  A-SEC-3 needs Aether to have been started from an environment that already
  holds a lead token, so it is Low.
- I checked that git 2.54 accepts every `--` form used in the A-SEC-1 and A-SEC-2
  fixes, using a scratch repo.

## Open items (documented, not fixed)

Ordered by priority. Each one is a backlog card on the board.

1. **C-PERF-5, Medium: fixed in follow-up.** When a streamed text delta only
   extends the last text row of the live reply (`extend_tail`), that row is
   updated in place. Anything that could change the row structure still triggers
   a full rebuild: steps, requests, a possible code fence, a reply that isn't
   live, or the first text.
2. **D-PERF-2, Medium: fixed in follow-up.** The save step compares the
   current layout and drafts with the last saved ones, and serializes only when
   they differ.
3. **C-PERF-1, Medium.** CLI stdout/stderr readers use `BufReader::lines` with no
   cap on line length, and update channels are unbounded. Fixing it needs a
   per-protocol decision on what to do on overflow: kill the session and show an
   error, without losing a pending approval.
4. **Lead token left on disk: won't fix.** `claude/spawn.rs` `write_mcp_config`
   leaves `aether-mcp-<hash>.json` in the temp dir. The token is random, minted
   per run (`agent.rs` `lead_token`), and held only in memory, so it stops
   working when the app exits. The file is 0600, and its name is a hash of the
   token, so nobody can guess it in advance. Moving the file somewhere else
   would gain nothing.
5. **Orchestrator socket hardening (B-SEC-5, B-SEC-7, B-SEC-8): fixed in
   follow-up.** On Unix the socket is now bound inside a private 0700 `orch/`
   directory under the config dir. Each request has one 5 s total read deadline.
   The UI skips a call whose 30 s deadline has passed, and only the UI frees
   pending slots. `docs/orchestration.md` is updated with the new socket path.
6. **B-SEC-6: fixed in follow-up.** You decided chats that aren't leads only see
   board cards in their own project. Leads keep `project: "all"`.
7. **Smaller items.** B-SEC-9 is now fixed (`web_search` has a 15 s timeout). Still open: B-PERF-1 and
   B-PERF-2 (usage scan cache), A-PERF-5 (synchronous explorer refresh),
   D-PERF-3 (GTK pump keeps running while the browser is hidden) and the
   remaining part of C-PERF-3.

## Verification

Run by the lead on the merged tree, 2026-11-10:

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --locked --workspace --all-targets`: passes. The 5 warnings are
  all `field_reassign_with_default`, in test files this audit didn't touch.
- `make test` (workspace, skips `hackernews::client`): all pass. That is 2125
  tests in total: aether 843, aether-agent 140, aether-core 113, aether-driver
  566, aether-explorer 28, aether-git 69, aether-terminal 72, aether-ui 283 and
  aether-voice 11. 10 are ignored.
- `uv run -m unittest discover -s scripts/tests`: 8 tests pass.
- The GUI was not launched. Two fixes should get a manual check in the running
  app: the browser panel with ordinary sites, and markdown re-render in chat and
  in the explorer preview.
