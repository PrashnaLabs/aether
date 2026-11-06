# Agent orchestration: one lead chat, many child agents

## Intent

When working on a feature, one strong model (the lead) plans the work, splits it
into tasks, and hands each task to a cheaper agent (a child). A child can be any
harness Aether already drives (Claude Code, Codex, OpenCode, Pi, Antigravity) on
any model. The lead tracks its children, gets their results, and follows up.

Every child is a normal Aether chat. The user can open it, watch it stream,
answer its approvals, and type into it, exactly like a chat they created.

Prior art: T3 Code (an app-hosted MCP server with `delegate_task` and
`task_status`, children talked to over structured protocols) and Orca (a CLI
the agent shells out to, with workers in PTYs or chats and a mailbox). We follow
T3's shape. Orca's open bugs cluster around typing into PTYs and idle
detection, and Aether already drives every harness over structured protocols.

## Goals

- A lead chat can spawn, message, read, and stop child chats through tools.
- A child's result reaches the lead without the lead polling.
- Children show in the sidebar under their lead, and the lead shows a live strip
  of its children. One click opens a child in a split pane.
- Lead and child can be different harnesses and models.
- Optional fresh git worktree per child so parallel children don't collide.

## Non-goals (for now)

- Automatic merging of child worktrees. The lead has a shell and git; it merges
  or asks the user to. Neither T3 nor Orca automates this either.
- Task DAGs, gates, or a mailbox. The chat transcript is the record; the lead
  plans in its own context.
- Grandchildren. Children don't get the orchestration tools, so depth is 1.

## Anti-goals

- The lead approves a child's permission asks. Every ask still reaches the user
  (rule from the agent-compat spec). The lead only sees that a child is waiting.
- A child gets broader access than its lead.
- Spawning a child steals focus from what the user is looking at.
- Children silently pick expensive models. The lead names the model; unknown
  models are rejected, not substituted.
- Polling loops that burn lead tokens.

## Decisions (my calls, open to override)

1. **One orchestrator API in the app, several ways in.** The app listens on a
   Unix socket (`<config_dir>/orchestrator.sock`, mode 0600, stale file removed
   on startup) that takes one JSON line per tool call and answers with one JSON
   line. Stdlib only: `std::os::unix::net`, `serde_json`. Each harness reaches
   it through whatever that harness can load per session:
   - **MCP shim** for harnesses with MCP: `aether mcp` (the same binary, new
     first argument) speaks MCP JSON-RPC on stdio and forwards to the socket.
     The MCP subset is `initialize`, `notifications/initialized`, `ping`,
     `tools/list`, `tools/call`. No HTTP server dependency.
   - **Pi extension**: a small extension file Aether writes and loads with
     `-e <path>`; it registers the same tools and talks to the socket directly.
     T3 Code does the same for Pi.
   - **Native agent**: `FunctionTool`s in `Tools::enabled` that send straight to
     the app's channel; no socket.
2. **Identity: per-lead token.** When the app launches a lead's driver it mints
   a random token, maps it to the lead chat id in memory, and hands it to the
   harness's way in (MCP server `env`, or the lead process's env for the Pi
   extension). The socket rejects unknown tokens. A shim started without a
   token lists zero tools, so a globally registered shim stays inert in
   non-Aether and child sessions.
3. **Every harness can lead and every harness can be a child.** Children need
   no tools, so any driver works as a child today. Per-harness lead adapters:

   | Harness | How the tools get in | Status |
   |---------|----------------------|--------|
   | Claude Code | `--mcp-config <0600 temp file>` plus `--allowedTools mcp__aether__*` | flags exist in 2.1.318; phase 0 checks stream-json + resume |
   | Codex | `codex app-server -c mcp_servers.aether.command=... -c mcp_servers.aether.env=...` | `-c` confirmed in 0.166.0 |
   | Pi | `-e <aether extension>`; token in process env. Read-only mode's `--tools` allowlist must also list the Aether tool names | `-e` confirmed |
   | OpenCode | its service is shared, so MCP config is global. A lead gets its own `opencode serve` started with `OPENCODE_CONFIG_CONTENT` holding the shim entry and token. Children keep the shared service | phase 0 checks the env var on v2.2.5 |
   | Antigravity | first choice `--settings '<json>'` (session-only settings) if it accepts MCP servers; fallback is registering the shim once with `agy mcp add aether <exe> mcp` (after the user confirms) and passing the token through the agy process env | phase 0 checks both |
   | Native (OpenRouter) | in-process `FunctionTool`s | no unknowns; has no file tools, so it leads as a pure planner |

4. **Lead is a per-chat toggle**, persisted, offered for every harness.
   Toggling after the first message respawns the driver with resume (the
   command thread already respawns on option changes).
5. **Results are pushed, not polled.** When a child's turn ends (`finish_run`),
   the app queues a message into the lead with the child's final reply. If the
   lead is mid-turn, deliveries wait in the lead's queue and coalesce into one
   message when it goes idle. No `wait` tool.
6. **Children start clean.** A child gets only its task text plus a fixed
   suffix asking it to end with a short summary of what it did and which files
   it changed. No lead history (cost, and T3's issue #27011).
7. **Aether's own tools are pre-approved for the lead** (`--allowedTools
   mcp__aether__*` for Claude, the equivalent per harness, no `Gate` prompt for
   native). The children's own actions stay gated by their access mode.
8. **Caps:** at most 8 running children per lead, a constant marked with a
   `ponytail:` comment.

## Tools exposed to the lead

| Tool | Args | Returns |
|------|------|---------|
| `list_agents` | none | installed harnesses with their models and efforts (from the existing catalog), plus this lead's children with status |
| `spawn_agent` | `provider`, `model`, `effort?`, `title`, `task`, `worktree?` (bool, default false), `access?` | `{id}` immediately; the child starts its first turn |
| `message_agent` | `id`, `text` | queued or steered, using the chat's existing queue and steer path |
| `read_agent` | `id`, `last?` (default 1) | status (`running`, `idle`, `needs_you`, `failed`), cwd and branch, last N assistant replies |
| `stop_agent` | `id` | cancels the running turn; the chat stays |

Tool descriptions stay terse to save lead context. A tool may only touch the
calling lead's own children.

## How it fits the existing code

- **Driver launch:** `DriverStartOptions` (`crates/driver/src/lib.rs:23`) gains
  `lead: Option<LeadTools { exe, sock, token }>`. Each driver turns it into its
  own launch form from the table above (`claude.rs` launch args, `codex.rs`
  spawn args, `pi.rs` `-e` plus env, `opencode/service.rs` a dedicated
  instance, `antigravity.rs` settings or env). The native path passes the lead
  id to `Tools::enabled` instead.
- **Socket server:** a std thread in `crates/agent` accepts connections, checks
  the token, and forwards `(lead_id, ToolCall, oneshot reply)` over a channel to
  the UI loop, the same way `SideUpdate` reaches `ChatApp` (`crates/app/src/app/new.rs:731`).
  `ChatApp` handles calls with `cx` and the window, because chat creation and
  `submit` need them.
- **Shim:** branch at the top of `main` (`crates/app/src/main.rs:40`), before
  `relaunch_in_dev_bundle` and `Agent::new`, on `args().nth(1) == "mcp"`.
- **Child creation:** a variant of `new_chat_in` / `add_external_chat`
  (`crates/app/src/app/chat/create.rs`) that sets provider, model, effort,
  access, cwd and `parent`, does **not** call `select`, then `submit`s the task.
  With `worktree: true` it goes through `Agent::create_worktree`
  (`crates/agent/src/agent.rs:347`) and binds `cwd`, `worktree_path` and
  `worktree_branch` before the first message.
- **Storage:** `chats` gets `parent TEXT NULL` and `lead INTEGER NOT NULL
  DEFAULT 0` using the ALTER TABLE pattern in `crates/agent/src/store.rs:68`.
  Children are found by `parent == lead.id`. Pending deliveries live in memory.
  A restart delivers a child's turn only if reattach replays it (OpenCode
  children, once opened). A Claude or Codex child cut off by quitting doesn't
  deliver, and queued deliveries live in memory, so they're lost on quit
  (`ponytail:` persist both if it bites).
- **Access ceiling:** `spawn_agent` clamps `access` to the lead's mode, mapped
  through the existing per-provider allowed modes (`settings.rs:425`).
- **Approvals:** unchanged path (`hitl.rs`). The child's ask shows in the child
  chat and as `needs_you` on its chip in the lead's strip. `Approval.source`
  already labels asks from a child session.

## UI

- **Lead toggle** in the composer's agent menu, with a small "Lead" badge on the
  chat when on.
- **Crew strip** above the lead's composer: one chip per child with harness
  icon, title, model, and status dot (running, idle, needs you, failed). Click
  opens the child in a split via `workspace.model.open`
  (`crates/app/src/session_workspace/model.rs:352`).
- **Sidebar:** children nest under their lead with an indent and a
  collapse control (`crates/app/src/app/sidebar.rs`).
- **Child header:** "Delegated by <lead title>", linking back.
- **Tool rows** in the lead for `spawn_agent` render as a link to the child
  instead of a raw JSON tool row (mapped in each driver's tool mapping).

## Phases

Each phase ends with something runnable. Children of one phase can be built by
parallel subagents in worktrees; phases run in order.

0. **Spike (throwaway), one per CLI, run in parallel.** With a dummy stdio MCP
   server (or a dummy Pi extension), confirm against the installed versions
   that the tools appear and can be called in Aether's real launch mode:
   - Claude 2.1.318: `-p --input-format stream-json` with `--mcp-config` and
     `--allowedTools`, including across `--resume`.
   - Codex 0.166.0: `app-server -c mcp_servers...`.
   - Pi: `--mode rpc -e <file>`, including the Read-only `--tools` allowlist.
   - OpenCode 2.2.5: a second `opencode serve` with `OPENCODE_CONFIG_CONTENT`.
   - Antigravity: `--settings` with MCP servers, else `agy mcp add` plus env
     passthrough.
   For each, also record how it asks for approval on MCP or extension tools
   under every access mode.
   Verify: `docs/wayfinder/research/orchestration-adapters.md` with the exact
   flags that worked per harness.
1. **Bridge and spawn.** Socket server, token map, the `parent`/`lead` columns,
   `list_agents` and `spawn_agent`, MCP shim, with Claude as the first lead.
   Verify: unit tests for MCP framing and token rejection (a tokenless shim
   lists no tools); a fake-driver test (existing `Factory`) showing a spawn
   creates an unfocused child chat with `parent` set that runs its task.
2. **Track and deliver.** `read_agent`, `message_agent`, `stop_agent`, push
   delivery with coalescing. Verify: fake-driver tests for delivery while the
   lead is idle and while it is busy (one coalesced message), plus a test that
   a tool call on another lead's child is refused.
2b. **Remaining lead adapters, run in parallel** (one subagent per harness
   once phase 2 lands): Codex, Pi, OpenCode, Antigravity, native. Each uses
   the flags phase 0 recorded. Verify per harness: a driver unit test for the
   launch args/env when `lead` is set and unchanged args when it isn't, plus a
   manual run where the lead spawns one child.
3. **UI.** Lead toggle, crew strip, sidebar nesting, child header, spawn tool
   row. Verify: headless render PNGs of the strip and sidebar (see the headless
   render memory), then a desktop check by the user.
4. **Isolation and limits.** `worktree: true`, access ceiling, concurrency cap.
   Verify: tests for clamped access and the cap; a worktree child runs in its
   own branch.

## Risks and unknowns

- Harnesses may apply their own approval prompt to MCP or extension tools in
  some modes. Phase 0 settles it per harness.
- Antigravity may have no session-only way in. The fallback writes to the
  user's agy MCP config, so it needs an explicit one-time confirm in the UI.
- An OpenCode lead costs one extra `opencode serve` process per lead chat.
- Codex's default MCP tool timeout is about 60 s. All tools return immediately,
  so this only matters if a blocking tool is added later.
- Cost: a lead fanning out to many children multiplies spend. The cap and
  explicit model choice are the guard; the usage view already tracks per-chat
  cost.
- Lead deleted or archived while children run: children keep running and become
  ordinary chats. No cascade.
