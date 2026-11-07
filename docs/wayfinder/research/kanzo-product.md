# Product Research: Kanzo (Self-Hosted Kanban for Humans and AI Agents)

Research on [emanuele-em/kanzo](https://github.com/emanuele-em/kanzo) — a single-binary (~4.5MB) Rust kanban board and MCP server.

---

## 1. Board Model

### Columns / Statuses
- **Fixed Workflow**: Statuses are hardcoded to `["todo", "doing", "review", "done"]` (`src/store.rs#L150-L152`). Arbitrary custom columns are not supported.
- **Optional Backlog**: A `backlog` column placed before `todo` can be toggled on/off in Settings (`src/store.rs#L154`, `L178-L183`, `L189-L203`). Disabling it requires moving existing tasks out first (`src/store.rs#L191-L194`).

### Card Fields
Stored in SQLite table `tasks` (`src/schema.sql#L12-L21`, `src/store.rs#L21-L47`, `src/store.rs#L100-L114`):
- `id` (INTEGER PK): Numeric ID, presented and keyed as `<project>#<id>` (e.g. `api#12`) (`src/store.rs#L117-L120`).
- `project_id` (INTEGER FK): References `projects.id` with `ON DELETE CASCADE`.
- `title` (TEXT): Cleaned single-line string (`src/store.rs#L601`).
- `description` (TEXT): Freeform Markdown body (`src/schema.sql#L16`, `src/store.rs#L104`).
- `status` (TEXT): One of `backlog`, `todo`, `doing`, `review`, `done` (`src/store.rs#L577-L583`).
- `position` (REAL): Float64 for column ordering (`ORDER BY position ASC`). Midpoint insertion `(prev + next) / 2.0`; auto-renumbers when delta drops below `1e-9` (`src/store.rs#L661-L718`).
- `tags` (TEXT): Comma-separated normalized tag list (`src/store.rs#L24`, `L1069-L1078`).
- `url` (TEXT): Web link (PR, issue, CI) validated for HTTP/S (`src/store.rs#L46`, `L1080-L1094`).
- `human` (INTEGER/bool): Flag marking tasks reserved for humans; agents ignore them (`src/store.rs#L45`, `L108`).
- `created_at` / `updated_at` (TEXT): ISO-8601 UTC timestamps via SQLite `strftime('%Y-%m-%dT%H:%M:%SZ', 'now')` (`src/schema.sql#L19-L20`, `src/store.rs#L51`).
- **No Assignee**: Cards have no assignee column. Agents self-select tasks by moving them to `doing`.
- **No Priority Enum**: No P0–P4/High/Low field; ordering within a column dictates priority.
- **No Direct Session/Agent FK**: Cards do not link to agent sessions or process IDs.

### Comments & Activity Log
- Stored in `comments` table (`src/schema.sql#L26-L32`, `src/store.rs#L123-L129`): `task_id`, `author` (human uses `"me"`, agents use client name, e.g. `"claude-code"`), `body` (Markdown), `created_at`.
- Associated `images` table (`src/store.rs#L35-L44`) stores attachments on disk in `~/kanzo/images/<16hex>.<ext>`.
- No separate audit/event log; card history is reflected solely through comments and `updated_at`.

---

## 2. Human UX

### Card Creation, Movement & Editing
- **Creation**: Press `n` or click `+ New task` (`src/app.js#L791-L805`, `L1218`). Automatically creates a task titled `"New task"` in the active project and `todo` column, opens the right drawer, and selects `#p-title`. If canceled without changes, `dropFresh()` auto-deletes it (`src/app.js#L807-L816`).
- **Movement**: Native HTML5 drag-and-drop (`src/app.js#L753-L786`). Dragging shows `.drop-line`; drop computes the `before` sibling and calls `POST /api/tasks/{id}/move`. Alternatively, select a new column in the task detail `#p-status` dropdown (`src/app.js#L1027`).
- **Editing**: Right-side drawer panel (`#panel`, `src/index.html#L503-L537`, `src/app.js#L853-L932`). Clicking any card opens it. Blurring fields immediately autosaves via `PATCH /api/tasks/{id}` (`src/app.js#L871-L873`). Description switches between raw textarea and rendered Markdown view (`src/app.js#L1007-L1025`). Images can be pasted or dropped into description/comments (`src/app.js#L1032-L1093`).
- **Filtering**: Header filter box (`f`) debounced at 120ms (`src/app.js#L830-L837`). Supports GitHub-like qualifiers: `tag:`, `status:`, `project:`, `title:`, `description:`, `url:`, `comment:`, `id:`, `comments:`, `images:`, `created:`, `updated:`, `is:human`, `has:`, `no:`, and `-` negation (`src/store.rs#L978-L1019`). Live syntax highlighting via mirror overlay (`.query-mirror`, `src/index.html#L150-L166`). Pressing Enter pins the query as a chip; tag pills on cards are clickable to filter (`src/app.js#L734-L738`).

### Keyboard Shortcuts
- **Global**: `/` or `⌘K` / `Ctrl+K` (global search palette), `n` (new task), `f` (focus filter), `h` (toggle human-only filter), `Esc` (close modal/drawer/menu) (`src/app.js#L1209-L1221`).
- **Search Modal**: `ArrowUp` / `ArrowDown` (navigate results), `Enter` (open selected task), `Esc` (dismiss) (`src/app.js#L1205-L1207`).
- **Task Panel**: `Enter` or `Tab` in Title (jump to description), `⌘Enter` / `Ctrl+Enter` (save & close, or submit comment), `Esc` (revert field or close panel) (`src/app.js#L985-L1000`, `L1134-L1136`).
- **Filter Box**: `Enter` (commit chip), `Backspace` on empty (delete last chip), `Esc` (clear & blur) (`src/app.js#L832-L837`).

### Views Beyond the Board
Kanzo is strictly kanban-focused (no Gantt, calendar, or tabular spreadsheet views). Supplemental views include:
1. **Aggregated Multi-Project Board (`#all`)**: All project tasks across shared columns with color-coded project pills (`src/app.js#L687-L690`).
2. **Single-Project Board (`#<slug>`)**: Filtered board view for one project (`src/app.js#L644-L646`).
3. **Global Search Palette (`⌘K` / `/`)**: Modal overlay listing matching tasks with metadata across projects (`src/app.js#L1147-L1207`).
4. **Task Detail Drawer (`#panel`)**: Slide-over panel for editing, comments, and attachments (`src/index.html#L503-L537`).
5. **Settings Modal**: 3 tabs: Board (toggle backlog, hide properties), Sync (git remote), About (updates) (`src/index.html#L539-L579`).
6. **Network/Share Popover**: Overlay managing LAN sharing, IP endpoints, and connected guests (`src/index.html#L580-L599`).
7. **Collapsible Columns**: Vertical collapsed header mode (`writing-mode: vertical-rl`) (`src/index.html#L207-L211`).

---

## 3. What Makes It "For Agents"

### Agent Lifecycle & Workflow
Agents interface via Model Context Protocol (MCP) over stdio (`kanzo mcp`) or HTTP (`/mcp`) (`src/mcp.rs#L53-L77`):
1. **Auto Project Scope**: In stdio mode, Kanzo runs `git remote -v` from `std::env::current_dir()` to find the git repo (`src/mcp.rs#L142-L158`). If linked to a project, the agent does not need to supply `project`.
2. **Task Pickup**: Agent invokes `next_task(project, filter)` (`src/mcp.rs#L527-L545`). Kanzo atomically finds the top non-human card in `todo`, moves it to `doing`, and returns its details in one step (`src/store.rs#L725-L753`).
3. **Progress Reporting**: Agent invokes `add_comment(id, body)` (`src/mcp.rs#L510-L525`). If author is omitted, Kanzo pulls client identity from MCP metadata (`clientInfo.name`, e.g. `claude-code`) (`src/mcp.rs#L135-L140`, `L516-L517`).
4. **Handoff / Finishing**: Agent calls `update_task(id, status="review", url="<PR_URL>")` (`src/mcp.rs#L482-L508`). Kanzo instructions explicitly tell agents: *"Keep it current unasked: doing when you start a task, comments on what you did, review when you finish or get stuck, url for a PR you opened. Leave done and human tasks to the user."* (`src/mcp.rs#L33-L44`).

### The "10 to 60 Tokens Per Reply" Mechanism
Kanzo eliminates MCP token waste through three techniques:
- **Stripped Tool Schemas**: Kanzo mutates the JSON Schema before sending to LLM: drops `$schema` keys, collapses `["string", "null"]` down to `"string"` (since non-required already implies optional), and uses one-sentence descriptions (`src/mcp.rs#L270-L293`). This reduces the tool definition footprint to ~1,000 tokens once per session.
- **Terse Plain Text Tool Output (Zero JSON Overhead)**: Tool results are returned as raw, compact plain text strings wrapped in `ContentBlock::text(...)` rather than structured JSON dictionaries (`src/mcp.rs#L1, L127-L131`):
  - `update_task`: `"Updated api#12, doing -> review: Fix auth bug"` (~10 tokens) (`src/mcp.rs#L505`).
  - `add_comment`: `"Comment added to api#12 as claude-code."` (~9 tokens) (`src/mcp.rs#L522`).
  - `create_task`: `"Created api#12 in todo: Fix auth bug"` (~9 tokens) (`src/mcp.rs#L477`).
  - `next_task`: Concise plain-text summary block (`key title \n status \n created \n description`) consuming 30–60 tokens (`src/mcp.rs#L537`, `L623-L671`).
  - `list_tasks`: One line per card, e.g. `api#12 [doing] Fix auth bug {bug} (2 comments)` (`src/mcp.rs#L603-L615`).
- **Property Masking**: Users can hide fields (`description`, `tags`, `url`, `comments`, `images`, `created`, `updated`, `human`) in Settings; hidden properties are completely stripped from agent output (`src/store.rs#L206-L232`, `src/mcp.rs#L624-L652`).

---

## 4. Persistence

### Storage Format
- **Database**: Single SQLite file at `~/kanzo/kanzo.db` (`src/home.rs#L72-L100`).
- **Rollback Journal (`journal_mode = TRUNCATE`)**: Kanzo deliberately avoids SQLite WAL mode (`src/store.rs#L1-L4`, `L175`). Because CLI and agent MCP processes connect per-operation, WAL caused repeated `-wal`/`-shm` creation/teardown; `TRUNCATE` measured 3x faster and leaves no residual lock files.
- **Concurrency**: Transactions use `BEGIN IMMEDIATE` (`src/store.rs#L235-L247`) paired with a 5-second busy timeout (`src/store.rs#L170`). File locking on `kanzo.lock` (`src/home.rs#L123-L146`) protects database replacement during sync.
- **Attachments**: Stored in `~/kanzo/images/` as raw files keyed by 16-hex content hashes (`src/store.rs#L881-L895`).

### Multi-Board Support
- Kanzo supports **Projects** within a single database (`projects` table with unique lowercase slugs) (`src/schema.sql#L4-L10`).
- Multiple repositories can link to the same project (`project_repos` table, `src/store.rs#L26-L34`).
- Completely separate boards require pointing to different directories via `--home <dir>` or `KANZO_HOME` (`src/home.rs#L72-L81`, `src/main.rs#L24-L26`).

### Sharing Mechanisms
1. **Live Network Sharing (`kanzo share on` / `kanzo join <ip>`)** (`src/share.rs`, `src/web.rs#L182-L297`):
   - One host machine shares its HTTP server on LAN/VPN (binds to `0.0.0.0` or Tailscale IP).
   - Guests join via IP; the guest's local Kanzo server transparently reverse-proxies all API and MCP requests to the host (`src/web.rs#L285-L290`, `src/share.rs#L273-L373`).
   - Guest MCP requests pass headers `x-kanzo-folder`, `x-kanzo-repos`, and `x-kanzo-computer` so host project resolution continues to work relative to guest worktrees (`src/share.rs#L24-L27`, `src/mcp.rs#L310-L322`).
   - Completely unauthenticated (intended strictly for trusted networks/VPNs).
2. **Git Asynchronous Sync (`kanzo sync`)** (`src/sync.rs`):
   - Turns `~/kanzo` into a git repository syncing against a private remote (e.g. GitHub).
   - Stages `kanzo.db` and images while holding an exclusive lock on `kanzo.lock` (`src/sync.rs#L131-L137`).
   - Non-interactive fast-forward pull or push. If diverged, git cannot merge binary SQLite files; user must resolve with `--keep-local` or `--keep-remote` (old DB backed up to `kanzo.db.bak`) (`src/sync.rs#L197-L226`).

---

## 5. Desktop Agent Workspace Takeaways

### 5 Features to Copy
1. **Git Remote Auto-Scoping (`git_repos` remote sniffing)**: Automatically maps the agent's active repository directory to the matching board project without requiring manual board/project ID configuration (`src/mcp.rs#L95-L124`, `L142-L158`).
2. **Atomic `next_task` Transition**: Combines selecting the highest-priority `todo` item and claiming it (`status -> doing`) in a single atomic SQL transaction to prevent agent race conditions (`src/store.rs#L725-L753`).
3. **Terse Plain-Text Tool Outputs with Stripped Schemas**: Returning single-line plain text strings and stripped schema definitions saves 80–90% of token overhead compared to standard JSON MCP tools (`src/mcp.rs#L270-L293`, `L505`).
4. **First-Class Human Task Barrier (`human` flag / `is:human`)**: A dedicated boolean column and query qualifier that unconditionally filters human-only cards away from autonomous agent dispatchers (`src/store.rs#L45`, `L736`).
5. **Contractual "Review" Handoff with `url`**: Explicit protocol directing agents to move completed tasks to `review` with a PR URL, keeping final `done` state transitions strictly under human control (`src/mcp.rs#L33-L44`).

### 3 Features to Skip
1. **Binary Database Git Sync (`kanzo sync`)**: Merging binary SQLite files over git is inherently non-mergeable and forces destructive conflict overwrites (`--keep-local`/`--keep-remote`); desktop workspaces should use CRDTs, append-only JSONL event logs, or standard cloud DB sync (`src/sync.rs#L197-L226`).
2. **Unauthenticated Open LAN Proxying (`kanzo share`)**: Opening an unauthenticated HTTP/MCP server across network interfaces exposes raw code execution / task injection vulnerabilities without token or session validation (`src/share.rs#L90-L96`).
3. **Ephemeral Draft Card Creation (`newTask` + `dropFresh`)**: Allocating real database IDs for empty new tasks and relying on `blur` and `pagehide` delete handlers is brittle; drafts should remain client-side UI state until explicitly committed (`src/app.js#L791-L816`).
