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
        "AETHER_ORCH_SOCK": "<config dir>/orch/orchestrator.sock",
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
  On Unix it lives in a private 0700 `orch` directory under the config dir.
  On Windows the loopback address file remains `<config dir>/orchestrator.sock`.
  `$XDG_CONFIG_HOME/aether/orch/orchestrator.sock` on Linux, or
  `~/.config/aether/orch/orchestrator.sock` without XDG. On macOS the default
  config dir is `~/Library/Application Support/aether`, with an existing XDG
  installation still honored. If unset, the server uses the platform default.
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

Leads also have access to `list_chats`, `read_history`, `update_chat`,
`stop_chat`, `archive_chats`, `delete_chats`, and board tools (`list_cards`,
`read_card`, `add_card`, `move_card`, `update_card`, `delete_cards`).

### Chat and agent inspection (`list_chats`, `list_agents`)

`list_chats` lists Aether chats matching an optional filter. Each chat entry
includes:
- `id`, `title`, `project`, `provider`, `state`, `last_active`, `pinned`,
  `archived`, `lead`.
- `model`: the raw stored provider model string for the chat
  (`c.provider_model`), which may be empty; it does not resolve global or
  provider fallback models.
- `queued`: count of messages currently queued for the chat (`c.queue.len()`).
- `cost`: total spend across all messages in the chat in USD, rounded to
  cents (e.g. `2.23`).

`list_agents` lists available CLI providers with the models reported by their
installed CLI probes, plus the lead's non-archived direct `children` (including
idle, running, and failed chats). Each child entry includes `id`, `title`,
`provider`, `model`, `status`, and `cost` (total spend for that child chat in
USD, rounded to cents).

#### Cost calculation

Cost for both `list_chats` and `list_agents` children is computed across the
chat's messages:
- If a reply reports spend (`cost_micros > 0`), that exact value is used.
- If `cost_micros` is 0, cost is estimated from model token pricing
  (`prompt_tokens * prompt_price + completion_tokens * completion_price`)
  when the message's model is found in Aether's configured model prices. If
  pricing for that model is unavailable, the reply contributes 0 micros (which
  readers should note does not confirm zero spend).
- Total micros across all replies are converted to USD rounded to the nearest
  cent (`(micros / 1e4).round() / 100.0`).

### Peer messaging and loop guard (`message_agent`)

`message_agent` sends or queues a prompt for direct child agents as well as
other chats (peer leads or standalone chats; not the lead itself, and not
archived chats).

- **Parent-to-child messages:** Messages sent by a lead to its own direct child
  (`chat.parent == Some(lead.id)`) are delivered directly without tags and are
  not subject to access-level checks or the loop guard.

When messaging a chat that is **not** the caller's direct child:
- **Access restriction:** The send is refused if the destination chat runs with
  a higher access level than the calling lead.
- **Answers do not return automatically:** The tool call returns a status
  (`{"status": "sent"}` or `{"status": "queued"}`) along with the note:
  `"not your child: its answer won't be sent to you; read it later with read_history and this chat's id"`.
  Peer replies are not delivered back automatically; leads should inspect
  replies using `read_history` with the destination chat's `id`. (Ending the
  caller's turn after messaging is workflow guidance for the agent, not
  enforced tool behavior.)
- **Sender tagging:** Aether prepends `[from <title> #<id>] ` to the text,
  where `<id>` is the sending chat's id. The title is cleaned first: `[`, `]`,
  `#`, control characters, bidi and zero-width characters are removed and it
  is cut to 40 characters, so a chat cannot forge another sender by renaming
  itself. The id after `#` is what identifies the sender.
- **Loop guard:** a lead can send the same non-child chat at most 6 messages
  in a row. The count is the run of most recent messages (sent or queued) in
  that chat carrying this sender's id; a message from anyone else, from you,
  or a child result ends the run. Renaming the sender does not reset it. The
  seventh send fails with
  `agent-to-agent loop guard: you sent chat <id> <n> messages in a row with no reply from another sender or the user; ask the user before sending more`.
  A message you type that starts with the exact tag also counts.

`update_chat` (rename, pin) only reaches the lead's own chat and its direct
children, plus, for an organization's CEO, that organization's heads.

`read_history` reads earlier messages of this chat (or, for a lead, any chat)
with full tool calls and output. A negative `offset` counts back from the end,
so `-3` reads the last three messages. `list_chats` and the other chat filters
take `search`, words that must all appear in the title.

`request_worker` is for organization heads only (`title`, `task`, optional
`card`, `provider`, `model`, `why`). Aether checks the hiring rules (the
request has a card, the head is under its worker cap, the org pool has a
free slot) and applies the hiring policy from Settings: it replies
`hired` with the worker's id, `denied` with the failing rules, or
`pending` with a request id the user decides on in the Organization page;
that decision reaches the head as its next turn. A head's `spawn_agent`
also waits for a free slot under its worker cap and the org pool.

## Linux troubleshooting

If orchestration does not work on Linux, check in this order:

1. **The chat is actually a lead.** The Lead chip must be on, and toggling
   it on an existing chat restarts the driver (or starts a fresh one).
   Tools are injected at CLI launch; a chat started before the toggle, or
   resumed without a restart, has no `aether` tools. Start a new chat
   after toggling, or toggle before the first message.
2. **Only one Aether owns the socket.** The socket file is
   `~/.config/aether/orch/orchestrator.sock` (or under `$XDG_CONFIG_HOME`).
   A second Aether instance (another user, another `$HOME`, a `sudo`
   launch) fails to bind and prints `aether: orchestration off: another
   Aether owns the socket` — visible only if you launch from a terminal.
   Close the other instance, or launch Aether from a terminal to see the
   message.
3. **The socket exists.** `ls -l ~/.config/aether/orch/orchestrator.sock`
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
