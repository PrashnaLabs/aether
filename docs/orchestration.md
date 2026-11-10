# Orchestration (lead agents)

A lead chat can spawn, message, read, and stop child agent chats through an
MCP server. The MCP server is Aether itself: the same binary, run as
`aether mcp`, speaking MCP over stdio. Tool calls travel over a local Unix
socket to the running Aether window, which executes them in the UI.

No manual setup is needed for normal use. Toggle a chat to **Lead** (composer
chip or command palette) and Aether wires the MCP server into that chat's
CLI at launch:

| Provider | How Aether injects it |
| --- | --- |
| Claude Code | `--mcp-config` pointing at a temp JSON file, plus `--allowedTools mcp__aether` |
| Codex | `-c mcp_servers.aether.command=<aether exe>` and `-c mcp_servers.aether.args=["mcp"]` on the app-server launch |
| Pi | A Pi extension (`pi-lead.ts` in Aether's config dir) loaded with `-e`; tools appear as `aether_*` |
| OpenCode | `OPENCODE_CONFIG_CONTENT` with the `aether` MCP entry for its local server |
| Antigravity | One-time global registration via `agy mcp add aether <aether exe> mcp` (see below); token reaches the server through `agy`'s process env |

OpenRouter chats cannot be leads; child chats cannot be leads. A lead can
run at most `max_children` children at once (default 8, configurable in
settings.json, clamped 1–50).

## Claude child auto-compaction

Claude children created by `spawn_agent` automatically summarize long contexts.
Change **Settings > Providers > Claude child agents**, or set these keys in
Aether's `settings.json` (`~/.config/aether/settings.json` on Linux by default):

```json
{
  "agent_compaction": {
    "enabled": true,
    "threshold_percent": 45
  }
}
```

Missing keys use these defaults; unknown keys are ignored. The threshold must
be an integer from **30 to 90**. Invalid values reject settings loading with a
clear diagnostic (Aether's existing loader falls back to default settings).
Set `enabled` to `false` to turn the guardrail off.

At Claude process launch, Aether sets only
`CLAUDE_CODE_AUTO_COMPACT_WINDOW=round(threshold_percent / 100 × 967000)` in
that child's environment. The default is **435,150 tokens**, without rounding
to thousands. Sonnet and Opus 5.x model IDs, and the `sonnet` / `opus` CLI
aliases (including `[1m]`), use the 967,000-token window. Haiku, older and
unknown models keep Claude's own defaults. Only 1M-context models are affected;
200k models retain their own limit (about 180k).

The lead can pass an optional `compaction_threshold_percent` integer to
`spawn_agent` to override the global percentage for that child. Omit it to use
the global setting; invalid values return a tool error. An override does not
enable a disabled guardrail. Overrides persist with the chat and apply to
resumed children through the same launch path. Running agents keep their old
environment; settings changes take effect on subsequent launches. Leads and
ordinary chats are unaffected. Codex, Pi, OpenCode and Antigravity (`agy`)
children have no equivalent guardrail in Aether.

Aether does not edit `~/.claude/settings.json` or the parent process environment.
An explicit child value takes precedence over the user's stopgap setting. When
off or unsupported, Aether adds nothing; any user-managed Claude environment
or settings still apply. Remove the old stopgap separately if you want it off
everywhere.

The choice of 45% comes from the owner's local Claude Code **2.1.286** experiment
and simulation of **45 sessions / 6,556 turns**: 30% produced 34 compactions,
45% produced 18, and 60% produced 9. It is at the low end of the roughly linear
savings stretch, before compactions rise sharply below 40%. These are simulated
counts, not measured production savings; summary size and relative token prices
were assumed, using one day's data from one project. The token-window variable
triggered compaction in stream-json experiments; `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`
did not, so Aether does not use it. Compaction can lose exact details and require
re-reading files; raise the threshold if summaries interrupt review work.

## Manual setup (advanced)

Only needed if you run an agent outside Aether and want it to call the
orchestration tools, or to debug what Aether injects. The server command is
always the Aether binary with the `mcp` argument:

```json
{
  "mcpServers": {
    "aether": {
      "command": "/usr/local/bin/aether",
      "args": ["mcp"],
      "env": {
        "AETHER_ORCH_SOCK": "<config dir>/orchestrator.sock",
        "AETHER_ORCH_TOKEN": "<lead token>"
      }
    }
  }
}
```

- `command`: the Aether executable you launch. On Linux installs from the
  tarball or `.deb` this is `/usr/local/bin/aether` (or wherever
  `which aether` points). A symlink is fine; it is resolved to the real
  binary.
- `AETHER_ORCH_SOCK`: the socket of the running Aether window.
  `$XDG_CONFIG_HOME/aether/orchestrator.sock` on Linux, or
  `~/.config/aether/orchestrator.sock` without XDG. If unset, the server
  defaults to that path.
- `AETHER_ORCH_TOKEN`: minted per lead chat by the running Aether, stable
  for the app's lifetime. There is no standalone token: without a valid
  token, `tools/list` returns an empty list by design, and `tools/call`
  answers `not an Aether lead`. Manual configs only work while the Aether
  window that minted the token is running.

Per-agent placement of the block above:

- **Claude Code**: pass it as `--mcp-config <file>` on the command line, or
  save it in `.mcp.json` in the project.
- **Codex**: put it in `~/.codex/config.toml` as
  `[mcp_servers.aether]` with `command`, `args = ["mcp"]`, and
  `env_vars = ["AETHER_ORCH_SOCK", "AETHER_ORCH_TOKEN"]`, exporting the
  two variables in the environment instead.
- **Pi**: Aether writes a `pi-lead.ts` extension implementing the tools
  directly; a manual `.pi/mcp.json` is not used for orchestration.
- **OpenCode**: pass the block as `OPENCODE_CONFIG_CONTENT`, or add it to
  `opencode.json` as `{"mcp": {"aether": {...}}}`.
- **Antigravity (`agy`)**: registration is global and one-time:
  `agy mcp add aether /usr/local/bin/aether mcp`, writing to
  `~/.gemini/config/mcp_config.json`. Aether does this itself the first
  time a lead starts; the token is passed through `agy`'s process
  environment at launch, not stored in the file.

Antigravity is the exception to "no setup": if `agy mcp add` failed (for
example `agy` was not installed yet, or the config file was hand-edited),
remove the file and let Aether retry, or run the command above yourself.
Aether prompts to register it when a lead first starts.

## Tools

`list_agents`, `spawn_agent` (with optional `nonce` idempotency key),
`read_agent`, `message_agent`, `stop_agent`. Child results are delivered
back to the lead automatically; leads should not poll `read_agent` for
completion.

`message_agent` also reaches chats that aren't the lead's children (not
the lead itself, not archived). Their answers don't come back on their own;
read them with `read_history` and the chat's `id`, which leads can pass to
read any chat. A negative `offset` counts back from the end, so `-3` reads
the last three messages. `list_chats` and the other chat filters take
`search`, words that must all appear in the title.

## Linux troubleshooting

If orchestration does not work on Linux, check in this order:

1. **The chat is actually a lead.** The Lead chip must be on, and toggling
   it on an existing chat restarts the driver (or starts a fresh one).
   Tools are injected at CLI launch; a chat started before the toggle, or
   resumed without a restart, has no `aether` tools. Start a new chat
   after toggling, or toggle before the first message.
2. **Only one Aether owns the socket.** The socket file is
   `~/.config/aether/orchestrator.sock` (or under `$XDG_CONFIG_HOME`).
   A second Aether instance (another user, another `$HOME`, a `sudo`
   launch) fails to bind and prints `aether: orchestration off: another
   Aether owns the socket` — visible only if you launch from a terminal.
   Close the other instance, or launch Aether from a terminal to see the
   message.
3. **The socket exists.** `ls -l ~/.config/aether/orchestrator.sock`
   while Aether runs. If it is missing, Aether could not create it:
   check `$HOME` is set for the desktop session and the config directory
   is writable. Launching from a terminal shows the error.
4. **Agent CLIs are on the desktop PATH.** GUI launches inherit the
   desktop environment's PATH, not your shell's. If `claude`/`codex`/`pi`/
   `opencode`/`agy` are installed under e.g. `~/.local/bin`, make sure
   that directory is in the desktop session PATH (or set the binary path
   in Aether's Settings).
5. **Verify the MCP server by hand.** With Aether running:
   `echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}' | /usr/local/bin/aether mcp`
   should print a JSON reply with `serverInfo.name == "aether"`. Without
   `AETHER_ORCH_TOKEN`, `tools/list` correctly returns `[]`.
6. **Antigravity:** check `~/.gemini/config/mcp_config.json` contains an
   `aether` entry whose command is your Aether binary and is not
   disabled. Re-run `agy mcp add aether /usr/local/bin/aether mcp` if
   stale.
7. **Version skew.** Orchestration changed in early versions (spawn
   `nonce`, socket caps, lead respawn on toggle). Update Aether and
   restart the lead chat so the driver picks up the new injection.
