use std::path::Path;

use aether_core::{AccessMode, Message, Provider};
use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedSender};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tokio::runtime::Handle;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS chats (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    messages TEXT NOT NULL,
    pinned INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    provider TEXT NOT NULL DEFAULT 'openrouter',
    provider_model TEXT NOT NULL DEFAULT '',
    cli_session TEXT NOT NULL DEFAULT '',
    access TEXT NOT NULL DEFAULT 'ask',
    effort TEXT NOT NULL DEFAULT '',
    cwd TEXT NOT NULL DEFAULT '',
    project TEXT NOT NULL DEFAULT '',
    worktree_path TEXT NOT NULL DEFAULT '',
    worktree_branch TEXT NOT NULL DEFAULT '',
    parent INTEGER,
    lead INTEGER NOT NULL DEFAULT 0,
    lead_seen INTEGER,
    pending_deliveries TEXT NOT NULL DEFAULT '[]',
    handoff_from INTEGER
)";

const CARDS_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS cards (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    col TEXT NOT NULL DEFAULT 'todo',
    position REAL NOT NULL DEFAULT 0,
    chat INTEGER,
    project TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL DEFAULT 0
)";

// Added after the first release; existing files gain them with zero values.

/// A chat as kept on disk. `id` also orders chats, oldest first.
pub struct SavedChat {
    pub id: i64,
    pub title: String,
    pub messages: Vec<Message>,
    pub pinned: bool,
    pub archived: bool,
    pub provider: Provider,
    pub provider_model: String,
    pub cli_session: String,
    pub access: AccessMode,
    pub effort: String,
    pub cwd: String,
    /// Folder the chat was started in, when it now runs elsewhere (a worktree).
    pub project: String,
    pub worktree_path: String,
    pub worktree_branch: String,
    /// Lead chat that spawned this one.
    pub parent: Option<i64>,
    pub lead: bool,
    /// Newest reply of this child its lead has read with read_agent.
    pub lead_seen: Option<usize>,
    /// Child results queued for this lead but not yet delivered as a turn.
    pub pending_deliveries: Vec<SavedDelivery>,
    /// Chat this one was handed off from.
    pub handoff_from: Option<i64>,
}

/// A child result waiting to be handed to its lead.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedDelivery {
    pub child: i64,
    /// `None` for setup failures, which are never dropped as read.
    pub reply_ix: Option<usize>,
    pub text: String,
}

/// A task-board card as kept on disk. `col` is one of backlog|todo|doing|review|done.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedCard {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub col: String,
    pub position: f64,
    pub chat: Option<i64>,
    pub project: String,
    pub updated_at: i64,
}

/// Chats in a SQLite file. Saves queue up and are written in order.
pub struct Store {
    pool: SqlitePool,
    rt: Handle,
    writes: UnboundedSender<Write>,
}

enum Write {
    Save(Box<SavedChat>),
    SaveCard(SavedCard),
    DeleteCard(i64),
    /// Answered once every write queued before it is done.
    Flush(std::sync::mpsc::Sender<()>),
}

impl Store {
    pub(crate) fn open(path: &Path, rt: Handle) -> sqlx::Result<Self> {
        #[cfg(unix)]
        if !path.exists() {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
            match opts.open(path) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(sqlx::Error::Io(e)),
            }
        }
        let options = SqliteConnectOptions::new().filename(path).create_if_missing(true);
        let pool = rt.block_on(async {
            let pool = SqlitePool::connect_with(options).await?;
            sqlx::query(SCHEMA).execute(&pool).await?;
            sqlx::query(CARDS_SCHEMA).execute(&pool).await?;
            let cols: Vec<String> = sqlx::query("PRAGMA table_info(chats)")
                .fetch_all(&pool)
                .await?
                .iter()
                .map(|row| row.get::<String, _>("name"))
                .collect();
            if !cols.iter().any(|col| col == "pinned") {
                sqlx::query("ALTER TABLE chats ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0")
                    .execute(&pool)
                    .await?;
            }
            if !cols.iter().any(|col| col == "archived") {
                sqlx::query("ALTER TABLE chats ADD COLUMN archived INTEGER NOT NULL DEFAULT 0")
                    .execute(&pool)
                    .await?;
            }
            if !cols.iter().any(|col| col == "provider") {
                sqlx::query(
                    "ALTER TABLE chats ADD COLUMN provider TEXT NOT NULL DEFAULT 'openrouter'",
                )
                .execute(&pool)
                .await?;
            }
            if !cols.iter().any(|col| col == "provider_model") {
                sqlx::query("ALTER TABLE chats ADD COLUMN provider_model TEXT NOT NULL DEFAULT ''")
                    .execute(&pool)
                    .await?;
            }
            if !cols.iter().any(|col| col == "cli_session") {
                sqlx::query("ALTER TABLE chats ADD COLUMN cli_session TEXT NOT NULL DEFAULT ''")
                    .execute(&pool)
                    .await?;
            }
            if !cols.iter().any(|col| col == "access") {
                sqlx::query("ALTER TABLE chats ADD COLUMN access TEXT NOT NULL DEFAULT 'ask'")
                    .execute(&pool)
                    .await?;
            }
            if !cols.iter().any(|col| col == "effort") {
                sqlx::query("ALTER TABLE chats ADD COLUMN effort TEXT NOT NULL DEFAULT ''")
                    .execute(&pool)
                    .await?;
            }
            for (col, add) in [
                ("cwd", "ALTER TABLE chats ADD COLUMN cwd TEXT NOT NULL DEFAULT ''"),
                ("project", "ALTER TABLE chats ADD COLUMN project TEXT NOT NULL DEFAULT ''"),
                (
                    "worktree_path",
                    "ALTER TABLE chats ADD COLUMN worktree_path TEXT NOT NULL DEFAULT ''",
                ),
                (
                    "worktree_branch",
                    "ALTER TABLE chats ADD COLUMN worktree_branch TEXT NOT NULL DEFAULT ''",
                ),
                ("parent", "ALTER TABLE chats ADD COLUMN parent INTEGER"),
                ("lead", "ALTER TABLE chats ADD COLUMN lead INTEGER NOT NULL DEFAULT 0"),
                ("lead_seen", "ALTER TABLE chats ADD COLUMN lead_seen INTEGER"),
                (
                    "pending_deliveries",
                    "ALTER TABLE chats ADD COLUMN pending_deliveries TEXT NOT NULL DEFAULT '[]'",
                ),
                ("handoff_from", "ALTER TABLE chats ADD COLUMN handoff_from INTEGER"),
            ] {
                if !cols.iter().any(|c| c == col) {
                    sqlx::query(add).execute(&pool).await?;
                }
            }
            Ok::<_, sqlx::Error>(pool)
        })?;
        let (writes, mut queue) = mpsc::unbounded::<Write>();
        let writer = pool.clone();
        rt.spawn(async move {
            while let Some(write) = queue.next().await {
                match write {
                    Write::Save(chat) => {
                        if let Err(e) = save(&writer, &chat).await {
                            eprintln!("aether: couldn't save chat {}: {e}", chat.id);
                        }
                    }
                    Write::SaveCard(card) => {
                        if let Err(e) = save_card(&writer, &card).await {
                            eprintln!("aether: couldn't save card {}: {e}", card.id);
                        }
                    }
                    Write::DeleteCard(id) => {
                        if let Err(e) = sqlx::query("DELETE FROM cards WHERE id = ?")
                            .bind(id)
                            .execute(&writer)
                            .await
                        {
                            eprintln!("aether: couldn't delete card {id}: {e}");
                        }
                    }
                    Write::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        Ok(Self { pool, rt, writes })
    }

    /// Every chat whose row and messages still parse, oldest first.
    pub fn load(&self) -> sqlx::Result<Vec<SavedChat>> {
        let rows = self.rt.block_on(
            sqlx::query("SELECT id, title, messages, pinned, archived, provider, provider_model, cli_session, access, effort, cwd, project, worktree_path, worktree_branch, parent, lead, lead_seen, pending_deliveries, handoff_from FROM chats ORDER BY id")
                .fetch_all(&self.pool),
        )?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let text = |col: &str| row.try_get::<String, _>(col).ok();
                let flag = |col: &str| row.try_get::<i64, _>(col).ok().map(|v| v != 0);
                Some(SavedChat {
                    id: row.try_get("id").ok()?,
                    title: text("title")?,
                    messages: parse_messages(&text("messages")?)?,
                    pinned: flag("pinned")?,
                    archived: flag("archived")?,
                    provider: parse_provider(&text("provider")?),
                    provider_model: text("provider_model")?,
                    cli_session: text("cli_session")?,
                    access: parse_access(&text("access")?),
                    effort: text("effort")?,
                    cwd: text("cwd")?,
                    project: text("project")?,
                    worktree_path: text("worktree_path")?,
                    worktree_branch: text("worktree_branch")?,
                    parent: row.try_get("parent").ok()?,
                    lead: flag("lead")?,
                    lead_seen: row
                        .try_get::<Option<i64>, _>("lead_seen")
                        .ok()?
                        .and_then(|v| usize::try_from(v).ok()),
                    pending_deliveries: parse_pending_deliveries(&text("pending_deliveries")?),
                    handoff_from: row.try_get("handoff_from").ok()?,
                })
            })
            .collect())
    }

    /// Every chat id, including rows whose messages no longer parse.
    pub fn ids(&self) -> sqlx::Result<Vec<i64>> {
        let rows = self.rt.block_on(sqlx::query("SELECT id FROM chats").fetch_all(&self.pool))?;
        Ok(rows.iter().filter_map(|r| r.try_get("id").ok()).collect())
    }

    /// Inserts or replaces the chat in the background.
    pub fn save(&self, chat: SavedChat) {
        let id = chat.id;
        if let Err(e) = self.writes.unbounded_send(Write::Save(Box::new(chat))) {
            eprintln!("aether: couldn't queue save for chat {id}: {e}");
        }
    }

    /// Every card, ordered by column then position.
    pub fn load_cards(&self) -> sqlx::Result<Vec<SavedCard>> {
        let rows = self.rt.block_on(
            sqlx::query("SELECT id, title, body, col, position, chat, project, updated_at FROM cards ORDER BY col, position")
                .fetch_all(&self.pool),
        )?;
        rows.iter()
            .map(|row| {
                Ok(SavedCard {
                    id: row.try_get("id")?,
                    title: row.try_get("title")?,
                    body: row.try_get("body")?,
                    col: row.try_get("col")?,
                    position: row.try_get("position")?,
                    chat: row.try_get("chat")?,
                    project: row.try_get("project")?,
                    updated_at: row.try_get("updated_at")?,
                })
            })
            .collect()
    }

    /// Inserts or replaces the card in the background.
    pub fn save_card(&self, card: SavedCard) {
        let id = card.id;
        if let Err(e) = self.writes.unbounded_send(Write::SaveCard(card)) {
            eprintln!("aether: couldn't queue save for card {id}: {e}");
        }
    }

    /// Deletes the card in the background; a missing row is fine.
    pub fn delete_card(&self, id: i64) {
        if let Err(e) = self.writes.unbounded_send(Write::DeleteCard(id)) {
            eprintln!("aether: couldn't queue delete for card {id}: {e}");
        }
    }

    /// Blocks until every save queued so far is written.
    pub fn flush(&self) {
        let (done, wait) = std::sync::mpsc::channel();
        if self.writes.unbounded_send(Write::Flush(done)).is_ok() {
            let _ = wait.recv();
        }
    }

    /// Deletes the chat; a missing row is fine.
    pub fn delete(&self, id: i64) {
        let pool = self.pool.clone();
        self.rt.block_on(async move {
            if let Err(e) =
                sqlx::query("DELETE FROM chats WHERE id = ?").bind(id).execute(&pool).await
            {
                eprintln!("aether: couldn't delete chat {id}: {e}");
            }
        });
    }
}

fn parse_pending_deliveries(json: &str) -> Vec<SavedDelivery> {
    serde_json::from_str(json).unwrap_or_default()
}

fn parse_messages(json: &str) -> Option<Vec<Message>> {
    let values: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    Some(
        values
            .into_iter()
            .map(|v| serde_json::from_value(v).unwrap_or_else(|_| Message::unreadable()))
            .collect(),
    )
}

async fn save(pool: &SqlitePool, chat: &SavedChat) -> sqlx::Result<()> {
    let messages =
        serde_json::to_string(&chat.messages.iter().map(Message::for_save).collect::<Vec<_>>())
            .map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))?;
    let pending_deliveries = serde_json::to_string(&chat.pending_deliveries)
        .map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))?;
    sqlx::query(
        "INSERT INTO chats (id, title, messages, pinned, archived, provider, provider_model, cli_session, access, effort, cwd, project, worktree_path, worktree_branch, parent, lead, lead_seen, pending_deliveries, handoff_from) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET title = excluded.title, messages = excluded.messages,
         pinned = excluded.pinned, archived = excluded.archived,
         provider = excluded.provider, provider_model = excluded.provider_model,
         cli_session = excluded.cli_session, access = excluded.access,
         effort = excluded.effort, cwd = excluded.cwd, project = excluded.project,
         worktree_path = excluded.worktree_path, worktree_branch = excluded.worktree_branch,
         parent = excluded.parent, lead = excluded.lead, lead_seen = excluded.lead_seen,
         pending_deliveries = excluded.pending_deliveries, handoff_from = excluded.handoff_from",
    )
    .bind(chat.id)
    .bind(&chat.title)
    .bind(messages)
    .bind(i64::from(chat.pinned))
    .bind(i64::from(chat.archived))
    .bind(provider_name(&chat.provider))
    .bind(&chat.provider_model)
    .bind(&chat.cli_session)
    .bind(access_name(chat.access))
    .bind(&chat.effort)
    .bind(&chat.cwd)
    .bind(&chat.project)
    .bind(&chat.worktree_path)
    .bind(&chat.worktree_branch)
    .bind(chat.parent)
    .bind(i64::from(chat.lead))
    .bind(chat.lead_seen.map(i64::try_from).transpose().ok().flatten())
    .bind(pending_deliveries)
    .bind(chat.handoff_from)
    .execute(pool)
    .await?;
    Ok(())
}

async fn save_card(pool: &SqlitePool, card: &SavedCard) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO cards (id, title, body, col, position, chat, project, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET title = excluded.title, body = excluded.body,
         col = excluded.col, position = excluded.position, chat = excluded.chat,
         project = excluded.project, updated_at = excluded.updated_at",
    )
    .bind(card.id)
    .bind(&card.title)
    .bind(&card.body)
    .bind(&card.col)
    .bind(card.position)
    .bind(card.chat)
    .bind(&card.project)
    .bind(card.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

fn provider_name(provider: &Provider) -> &'static str {
    match provider {
        Provider::OpenRouter => "openrouter",
        Provider::Claude => "claude",
        Provider::Codex => "codex",
        Provider::Pi => "pi",
        Provider::OpenCode => "opencode",
        Provider::Antigravity => "antigravity",
    }
}

/// Unknown names fall back to OpenRouter, as do legacy rows migrated in with
/// the column default.
fn parse_provider(name: &str) -> Provider {
    match name {
        "claude" => Provider::Claude,
        "codex" => Provider::Codex,
        "pi" => Provider::Pi,
        "opencode" => Provider::OpenCode,
        "antigravity" | "agy" => Provider::Antigravity,
        _ => Provider::OpenRouter,
    }
}

fn access_name(access: AccessMode) -> &'static str {
    match access {
        AccessMode::Ask => "ask",
        AccessMode::AutoEdit => "autoedit",
        AccessMode::Auto => "auto",
        AccessMode::ReadOnly => "readonly",
        AccessMode::Full => "full",
    }
}

fn parse_access(name: &str) -> AccessMode {
    match name {
        "autoedit" => AccessMode::AutoEdit,
        "auto" => AccessMode::Auto,
        "readonly" => AccessMode::ReadOnly,
        "full" => AccessMode::Full,
        _ => AccessMode::Ask,
    }
}

#[cfg(test)]
#[path = "../tests/unit/store.rs"]
mod tests;
