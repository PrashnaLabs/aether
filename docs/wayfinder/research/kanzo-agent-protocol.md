# Kanzo Agent Protocol & Aether Mapping

Study of [emanuele-em/kanzo](https://github.com/emanuele-em/kanzo) (commit shallow clone) for agent-driven Kanban coordination and mapping to Aether's MCP orchestration server.

---

## 1. Exact Agent Interface

Kanzo provides three interfaces: MCP (primary for agents), CLI (human/process control), and HTTP REST (web UI).

### A. MCP Server (`src/mcp.rs`)
- **Transports**:
  - **stdio**: Launched via `kanzo mcp` using `rmcp::transport::stdio()` (`src/mcp.rs:53-62`).
  - **Streamable HTTP**: Mounted at `/mcp` when `mcp_http = true` (`src/mcp.rs:66-77`, `src/web.rs:147-148`).
  - **Networked / Shared**: A local `kanzo mcp` forwards tool calls to an upstream shared instance when joined (`src/mcp.rs:571-590`, `src/share.rs`).
- **All 10 MCP Tools**:
  1. `list_projects`: No args. Returns text list of projects: `<slug> ["<name>"] [(<repos>)]: <open> open of <total> [<- current repo]`.
  2. `create_project`: Args `{ slug: string, name?: string, repo?: string }`. Returns `"Created project <slug> (<name>)[, linked to <repo>]."`.
  3. `link_repo`: Args `{ project: string, repo: string }`. Returns `"Project <slug> is linked to <repos>."`.
  4. `list_tasks`: Args `{ project?: string, status?: string, filter?: string }`. Returns 1 line per card: `<slug>#<id> [<status>] <title> [{<tags>}] [(<n> comments)]` with summary footer for hidden human/done cards.
  5. `get_task`: Args `{ id: string | integer }` (e.g. `"api#12"` or `12`). Returns full plain-text card: status, metadata, description, images, comments.
  6. `create_task`: Args `{ title: string, description?: string, project?: string, tags?: string, status?: string, human?: bool }`. Returns `"Created <slug>#<id> in <status>: <title>"`.
  7. `update_task`: Args `{ id: string | integer, title?: string, description?: string, status?: string, tags?: string, human?: bool, url?: string }`. Returns `"Updated <slug>#<id>[, <old> -> <new>]: <title>"`.
  8. `add_comment`: Args `{ id: string | integer, body: string, author?: string }` (author defaults to MCP client name, e.g. Claude Code, Codex, or `"agent"`). Returns `"Comment added to <slug>#<id> as <author>."`.
  9. `next_task`: Args `{ project?: string, filter?: string }`. Atomically claims top `todo` card to `doing` and returns `"Moved to doing:\n<task_detail>"` or `"Nothing in todo for <project>."`.
  10. `delete_task`: Args `{ id: string | integer }`. Returns `"Deleted <slug>#<id>[ and its <n> comments]: <title>"`.

### B. CLI (`src/main.rs:21-106`)
Human/script commands: `kanzo` (starts web server at `:4242`), `kanzo mcp` (stdio MCP), `kanzo init`, `kanzo sync` (git sync), `kanzo export` (Markdown/JSON), `kanzo share on|off`, `kanzo join <host>`, `kanzo leave`, `kanzo update`, `kanzo service install|uninstall`. Flags: `--home <dir>`, `--port <u16>`, `--no-open`.

### C. HTTP REST Endpoints (`src/web.rs:151-175`)
Used by browser UI: `GET /api/board`, `GET /api/changes` (long-polling revision counter), `GET /api/search`, `POST /api/projects`, `PATCH/DELETE /api/projects/{slug}`, `POST/DELETE /api/projects/{slug}/repos`, `POST /api/tasks`, `GET/PATCH/DELETE /api/tasks/{id}`, `POST /api/tasks/{id}/move`, `POST /api/tasks/{id}/comments`, `POST /api/images`, `GET/POST /api/sync`, `PATCH /api/settings`.

---

## 2. Token-Economy Tricks (10–60 Token Replies)

Kanzo targets minimal context overhead (`README.md:226-227`: tool list ~1,000 tokens once; replies 10–60 tokens):
1. **Tool Schema Trimming (`src/mcp.rs:273-292`)**: Strips `$schema` URLs and unwraps JSON schema optional unions (`["string", "null"]` -> `"string"`), reducing initial handshake token weight across all 10 tools.
2. **Compact ID Shorthand (`src/mcp.rs:168-181`, `src/store.rs:117-120`)**: Composite identifier format `project#id` (e.g., `api#12`) or raw integer. Eliminates nested `{ "project": "...", "id": ... }` structures.
3. **Plain Text over Structured JSON (`src/mcp.rs:127-132`)**: Every tool returns raw formatted text blocks (`ContentBlock::text(...)`). Avoids JSON syntax markup (keys, quotes, brackets).
4. **Terse Action Diffs (`src/mcp.rs:477, 501-505, 522`)**: Mutation replies report only the change diff:
   - Status transition: `"Updated api#12, todo -> doing: Fix parser"` (~10 tokens).
   - Creation: `"Created api#12 in todo: Fix parser"` (~8 tokens).
   - Commenting: `"Comment added to api#12 as codex."` (~9 tokens).
5. **Condensed List Projections (`src/mcp.rs:406-444, 603-621`)**: `list_tasks` prints exactly 1 line per task: `api#12 [doing] Fix crash {bug} (2 comments)`. Omit descriptions and comments. `done` tasks are hidden by default (`keep = Some(0)`). Human-flagged tasks are hidden and aggregated into a single line: `(2 human tasks not shown: for the user, not for agents)`.
6. **Compound Workflow Tool (`next_task`, `src/mcp.rs:528-545`)**: Collapses 3 conversational round-trips (`list_tasks` -> `get_task` -> `update_task(status="doing")`) into a single atomic call that fetches and claims in ~40–60 tokens.
7. **Property Masking (`src/mcp.rs:417, 600-670`, `src/store.rs:207-230`)**: Properties disabled in settings (`tags`, `comments`, `images`, `url`, `created`, `updated`, `human`, `description`) are stripped from MCP serialization.

---

## 3. Concurrency & Failure Modes

### How Two Agents Avoid Grabbing the Same Card (`src/store.rs:725-753`)
- **Atomic Claim via Immediate Transactions**: `Store::write()` wraps writes in SQLite `BEGIN IMMEDIATE` (`src/store.rs:235-247`).
- **Busy Timeout**: Connections set `busy_timeout(5s)` (`src/store.rs:170`).
- **Claim Implementation**:
  ```rust
  // src/store.rs:733-750 (inside next_task_matching)
  let id = self.write(|c| {
      let sql = format!(
          "SELECT t.id FROM tasks t JOIN projects p ON p.id = t.project_id
           WHERE t.status = ?1 AND t.human = 0 AND (?2 IS NULL OR t.project_id = ?2){}
           ORDER BY t.position LIMIT 1", filter.sql
      );
      let id: Option<i64> = c.query_row(&sql, values, |r| r.get(0)).optional()?;
      if let Some(id) = id {
          let position = Self::end_of_column(c, to, id)?;
          c.execute("UPDATE tasks SET status = ?1, position = ?2, updated_at = ... WHERE id = ?3", params![to, position, id])?;
      }
      Ok(id)
  })?;
  ```
- **Serialization Guarantee**: When Agent A calls `next_task`, SQLite locks the database, selects the first `todo` card, moves it to `doing`, and commits. Agent B's concurrent call waits on `busy_timeout(5s)`; once unblocked, Card 1 is already in `doing`, so Agent B receives Card 2.
- **No Leases or Locks**: Kanzo has **no** lease timeouts, heartbeats, or per-card locks. (`kanzo.lock` via `flock` in `src/home.rs:124-145` only coordinates git syncs against the SQLite DB).

### What Happens if an Agent Dies Mid-Card
- The card remains in `status = 'doing'` indefinitely.
- There is no automatic rollback, watchdog timer, or reaper process.
- **Recovery paths**:
  1. Human board operator moves the stuck card back to `todo` or `review` via the Web UI.
  2. A supervisor agent inspects `list_tasks(status="doing")` and resets the card using `update_task`.
  3. Comments logged before death remain persisted in SQLite.

---

## 4. Data Schema

### Core Tables & Triggers (`src/schema.sql`, `src/store.rs:21-47`)
```sql
CREATE TABLE projects (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    slug       TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    repo       TEXT UNIQUE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
); -- Migration 1 adds: color TEXT

CREATE TABLE tasks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL,
    position    REAL NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
); -- Migrations add: tags TEXT DEFAULT '', human INTEGER DEFAULT 0, url TEXT DEFAULT ''

CREATE TABLE comments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author     TEXT NOT NULL,
    body       TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT INTO meta (key, value) VALUES ('changes', '0');

CREATE TRIGGER tasks_insert AFTER INSERT ON tasks BEGIN UPDATE meta SET value = value + 1 WHERE key = 'changes'; END;
CREATE TRIGGER tasks_update AFTER UPDATE ON tasks BEGIN UPDATE meta SET value = value + 1 WHERE key = 'changes'; END;
CREATE TRIGGER tasks_delete AFTER DELETE ON tasks BEGIN UPDATE meta SET value = value + 1 WHERE key = 'changes'; END;
```

*Note on Columns, Boards, and Events*:
- **Boards**: Mapped directly to rows in `projects`.
- **Columns**: No table; hardcoded in code: `const STATUSES: [&str; 4] = ["todo", "doing", "review", "done"];` (with optional `"backlog"` toggled in `meta`).
- **Events**: No event table; change notifications are broadcast via the monotonic `meta.changes` counter incremented by triggers.

### Core Rust Structs (`src/store.rs:88-148`)
```rust
#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub repos: Vec<String>,
    pub color: Option<String>,
    pub created_at: String,
    pub open: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: i64,
    pub project: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub url: String,
    pub human: bool,
    pub status: String,
    pub position: f64,
    pub created_at: String,
    pub updated_at: String,
    pub comments: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Comment {
    pub id: i64,
    pub task_id: i64,
    pub author: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub project_id: Option<i64>,
    pub tags: Option<String>,
    pub url: Option<String>,
    pub human: Option<bool>,
}
```

---

## 5. Mapping to Aether Orchestration

Aether already provides an MCP stdio server (`aether mcp`) connected via Unix domain socket (`AETHER_ORCH_SOCK`) to the desktop app (`docs/orchestration.md`, `crates/agent/src/orchestrator/mod.rs`), exposing:
`spawn_agent`, `list_agents`, `read_agent`, `message_agent`, `stop_agent`.

### Proposed Minimal Board Extension (5 Tools)
Adding task-board coordination alongside existing agent management requires only 5 concise tools:

| Tool | Arguments | Description & Response Shape |
| --- | --- | --- |
| `list_tasks` | `status?: string`, `filter?: string` | Terse 1-line list of active tasks: `#4 [doing] Refactor parser (agent-2) (1 comment)`. Excludes `done` by default (~20 tokens). |
| `create_task` | `title: string`, `description?: string`, `tags?: string` | Lead creates card in `todo`. Returns `"Created task #5 in todo: Refactor parser"` (~8 tokens). |
| `claim_task` | `id?: integer`, `filter?: string` | Atomically claims top `todo` card (or specified `#id`), sets status to `doing`, assigns child caller ID, returns full details (~40-60 tokens). Adopts Kanzo's `next_task` pattern. |
| `update_task` | `id: integer`, `status?: string`, `title?: string`, `description?: string`, `url?: string` | Updates status (`review`, `done`) or fields. Completing a task is `update_task(id, status="done")`. Returns `"Updated #4, doing -> review: Refactor parser"` (~10 tokens). |
| `comment_task` | `id: integer`, `text: string` | Appends progress report or PR link with caller agent attribution. Returns `"Comment added to #4 as agent-2."` (~8 tokens). |

### Addressing Kanzo's Dead-Agent Limitation in Aether
Unlike Kanzo, Aether tracks process states (`child_status` in `crates/app/src/app/orchestrate.rs:288`).
When `claim_task` binds task ID to `child_id`, Aether's orchestrator can detect when a child chat crashes or stops without completing the task, allowing Aether to automatically mark the card blocked or revert it from `doing` to `todo` with an auto-generated comment.
