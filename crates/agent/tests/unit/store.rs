use aether_core::{AccessMode, Update};
use tokio::runtime::Runtime;

use super::*;

#[test]
fn saved_chats_load_back_oldest_first_and_saving_again_replaces() {
    let path = std::env::temp_dir().join(format!("aether-store-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    let mut reply = Message::assistant();
    reply.apply(Update::Search { id: "c1".into(), query: "rust".into() });
    reply.apply(Update::Final("**Hi**".into()));
    let mut chat = SavedChat {
        id: 7,
        title: "Greeting".into(),
        messages: vec![Message::user("hello"), reply],
        pinned: true,
        archived: false,
        provider: Provider::Claude,
        provider_model: "sonnet".into(),
        cli_session: "sid".into(),
        access: AccessMode::Auto,
        effort: "high".into(),
        cwd: "/tmp/p".into(),
        project: "/code/p".into(),
        worktree_path: "/w/p-1/fix".into(),
        worktree_branch: "aether/fix".into(),
        parent: None,
        lead: false,
        lead_seen: None,
        pending_deliveries: vec![],
        handoff_from: Some(3),
    };
    let older = SavedChat {
        id: 3,
        title: "Older".into(),
        messages: vec![],
        pinned: false,
        archived: false,
        provider: Provider::OpenRouter,
        provider_model: String::new(),
        cli_session: String::new(),
        access: AccessMode::Ask,
        effort: String::new(),
        cwd: String::new(),
        project: String::new(),
        worktree_path: String::new(),
        worktree_branch: String::new(),
        parent: None,
        lead: false,
        lead_seen: None,
        pending_deliveries: vec![],
        handoff_from: None,
    };
    for chat in [&chat, &older] {
        rt.block_on(save(&store.pool, chat)).unwrap();
    }
    chat.title = "Renamed".into();
    rt.block_on(save(&store.pool, &chat)).unwrap();

    let chats = store.load().unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(chats.iter().map(|c| c.id).collect::<Vec<_>>(), [3, 7]);
    assert_eq!(chats[1].title, "Renamed");
    assert!(chats[1].pinned);
    assert_eq!(chats[1].provider, Provider::Claude);
    assert_eq!(chats[1].provider_model, "sonnet");
    assert_eq!(chats[1].cli_session, "sid");
    assert_eq!(chats[1].access, AccessMode::Auto);
    assert_eq!(chats[1].effort, "high");
    assert_eq!(chats[1].cwd, "/tmp/p");
    assert_eq!(chats[1].project, "/code/p");
    assert_eq!(chats[1].worktree_path, "/w/p-1/fix");
    assert_eq!(chats[1].worktree_branch, "aether/fix");
    assert_eq!(chats[1].handoff_from, Some(3));
    assert_eq!(chats[0].handoff_from, None);
    assert!(chats[0].project.is_empty() && chats[0].worktree_path.is_empty());
    let reply = &chats[1].messages[1];
    assert_eq!(reply.text, "**Hi**");
    assert_eq!(reply.steps.len(), 1);
    assert!(reply.thought_for.is_some());
}

#[test]
fn old_databases_gain_pin_and_archive_columns() {
    let path = std::env::temp_dir().join(format!("aether-store-legacy-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::query(
            "CREATE TABLE chats (id INTEGER PRIMARY KEY, title TEXT NOT NULL, messages TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO chats (id, title, messages) VALUES (1, 'Old', '[]')")
            .execute(&pool)
            .await
            .unwrap();
    });
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    let chats = store.load().unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(chats.len(), 1);
    assert!(!chats[0].pinned && !chats[0].archived);
}

#[test]
fn old_db_loads_as_openrouter() {
    let path =
        std::env::temp_dir().join(format!("aether-store-noprovider-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::query(
            "CREATE TABLE chats (id INTEGER PRIMARY KEY, title TEXT NOT NULL, messages TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO chats (id, title, messages) VALUES (1, 'Old', '[]')")
            .execute(&pool)
            .await
            .unwrap();
    });
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    let chats = store.load().unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].provider, Provider::OpenRouter);
    assert_eq!(chats[0].provider_model, "");
    assert!(
        chats[0].access == AccessMode::Ask
            && chats[0].cli_session.is_empty()
            && chats[0].cwd.is_empty()
    );
    assert!(
        chats[0].project.is_empty()
            && chats[0].worktree_path.is_empty()
            && chats[0].worktree_branch.is_empty()
    );
}

#[test]
fn ids_include_every_row() {
    let path = std::env::temp_dir().join(format!("aether-store-ids-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    for id in [4, 9] {
        let chat = SavedChat {
            id,
            title: String::new(),
            messages: vec![],
            pinned: false,
            archived: false,
            provider: Provider::Claude,
            provider_model: String::new(),
            cli_session: String::new(),
            access: AccessMode::Ask,
            effort: String::new(),
            cwd: String::new(),
            project: String::new(),
            worktree_path: String::new(),
            worktree_branch: String::new(),
            parent: None,
            lead: false,
            lead_seen: None,
            pending_deliveries: vec![],
            handoff_from: None,
        };
        rt.block_on(save(&store.pool, &chat)).unwrap();
    }
    let ids = store.ids().unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(ids, [4, 9]);
}

#[test]
fn flush_returns_once_queued_saves_are_written() {
    let path = std::env::temp_dir().join(format!("aether-store-flush-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    for title in ["first", "last"] {
        store.save(SavedChat {
            id: 1,
            title: title.into(),
            messages: vec![Message::user("hi")],
            pinned: false,
            archived: false,
            provider: Provider::Claude,
            provider_model: String::new(),
            cli_session: String::new(),
            access: AccessMode::Ask,
            effort: String::new(),
            cwd: String::new(),
            project: String::new(),
            worktree_path: String::new(),
            worktree_branch: String::new(),
            parent: None,
            lead: false,
            lead_seen: None,
            pending_deliveries: vec![],
            handoff_from: None,
        });
    }
    store.flush();
    let chats = store.load().unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(chats.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["last"]);
}

#[test]
fn a_bad_message_becomes_a_placeholder_not_a_lost_chat() {
    let good = serde_json::to_value(Message::user("hello")).unwrap();
    let json = serde_json::json!([good, {"role": "Robot"}]).to_string();
    let messages = parse_messages(&json).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].text, "hello");
    assert_eq!(messages[1].text, Message::unreadable().text);
    assert!(parse_messages("{}").is_none());
}

#[test]
fn parent_and_lead_round_trip() {
    let path = std::env::temp_dir().join(format!("aether-store-lead-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    store.save(SavedChat {
        id: 9,
        title: "Child".into(),
        messages: vec![],
        pinned: false,
        archived: false,
        provider: Provider::Claude,
        provider_model: String::new(),
        cli_session: String::new(),
        access: AccessMode::Ask,
        effort: String::new(),
        cwd: String::new(),
        project: String::new(),
        worktree_path: String::new(),
        worktree_branch: String::new(),
        parent: Some(5),
        lead: true,
        lead_seen: None,
        pending_deliveries: vec![],
        handoff_from: None,
    });
    store.flush();
    drop(store);
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    let chats = store.load().unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(chats[0].parent, Some(5));
    assert!(chats[0].lead);
}

#[test]
fn old_file_gains_parent_and_lead_as_none_false() {
    let path = std::env::temp_dir().join(format!("aether-store-nolead-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::query(
            "CREATE TABLE chats (id INTEGER PRIMARY KEY, title TEXT NOT NULL, messages TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO chats (id, title, messages) VALUES (1, 'Old', '[]')")
            .execute(&pool)
            .await
            .unwrap();
    });
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    let chats = store.load().unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(chats[0].parent, None);
    assert!(!chats[0].lead);
}

#[test]
fn lead_seen_and_pending_deliveries_round_trip() {
    let path = std::env::temp_dir().join(format!("aether-store-pending-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    store.save(SavedChat {
        id: 9,
        title: "Child".into(),
        messages: vec![],
        pinned: false,
        archived: false,
        provider: Provider::Claude,
        provider_model: String::new(),
        cli_session: String::new(),
        access: AccessMode::Ask,
        effort: String::new(),
        cwd: String::new(),
        project: String::new(),
        worktree_path: String::new(),
        worktree_branch: String::new(),
        parent: Some(5),
        lead: false,
        lead_seen: Some(3),
        pending_deliveries: vec![],
        handoff_from: None,
    });
    store.save(SavedChat {
        id: 5,
        title: "Lead".into(),
        messages: vec![],
        pinned: false,
        archived: false,
        provider: Provider::Claude,
        provider_model: String::new(),
        cli_session: String::new(),
        access: AccessMode::Ask,
        effort: String::new(),
        cwd: String::new(),
        project: String::new(),
        worktree_path: String::new(),
        worktree_branch: String::new(),
        parent: None,
        lead: true,
        lead_seen: None,
        pending_deliveries: vec![SavedDelivery {
            child: 9,
            reply_ix: Some(3),
            text: "[Aether child result] Agent 9 done".into(),
        }],
        handoff_from: None,
    });
    store.flush();
    drop(store);
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    let chats = store.load().unwrap();
    let _ = std::fs::remove_file(&path);
    let child = chats.iter().find(|c| c.id == 9).unwrap();
    let lead = chats.iter().find(|c| c.id == 5).unwrap();
    assert_eq!(child.lead_seen, Some(3));
    assert_eq!(lead.pending_deliveries.len(), 1);
    assert_eq!(lead.pending_deliveries[0].child, 9);
    assert_eq!(lead.pending_deliveries[0].reply_ix, Some(3));
    assert!(lead.pending_deliveries[0].text.starts_with("[Aether child result]"));
}
#[cfg(unix)]
#[test]
fn new_database_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!("aether-store-mode-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    drop(store);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let _ = std::fs::remove_file(&path);
    assert_eq!(mode, 0o600);
}

fn card(id: i64, col: &str, position: f64) -> SavedCard {
    SavedCard {
        id,
        title: format!("Card {id}"),
        body: "body".into(),
        col: col.into(),
        position,
        chat: Some(7),
        project: "/code/p".into(),
        updated_at: 99,
        by_agent: false,
        meta: String::new(),
    }
}

#[test]
fn cards_round_trip() {
    let path = std::env::temp_dir().join(format!("aether-store-cards-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    let mut a = card(1, "todo", 2.0);
    let b = SavedCard {
        chat: None,
        by_agent: true,
        meta: r#"{"labels":["ui"]}"#.into(),
        ..card(2, "doing", 0.5)
    };
    store.save_card(a.clone());
    store.save_card(b.clone());
    a.title = "Renamed".into();
    store.save_card(a.clone());
    store.flush();
    let cards = store.load_cards().unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(cards, [b, a]);
}

#[test]
fn card_delete() {
    let path = std::env::temp_dir().join(format!("aether-store-carddel-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    store.save_card(card(1, "todo", 0.0));
    store.delete_card(1);
    store.flush();
    let cards = store.load_cards().unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(cards.is_empty());
}

#[test]
fn old_db_gains_cards_table() {
    let path = std::env::temp_dir().join(format!("aether-store-nocards-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::query(
            "CREATE TABLE chats (id INTEGER PRIMARY KEY, title TEXT NOT NULL, messages TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
    });
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    let cards = store.load_cards();
    let _ = std::fs::remove_file(&path);
    assert_eq!(cards.unwrap(), vec![]);
}

#[test]
fn old_card_tables_gain_meta() {
    let path =
        std::env::temp_dir().join(format!("aether-store-cardmeta-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::query(
            "CREATE TABLE cards (id INTEGER PRIMARY KEY, title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '',
             col TEXT NOT NULL DEFAULT 'todo', position REAL NOT NULL DEFAULT 0, chat INTEGER,
             project TEXT NOT NULL DEFAULT '', updated_at INTEGER NOT NULL DEFAULT 0,
             by_agent INTEGER NOT NULL DEFAULT 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO cards (id, title, chat) VALUES (1, 'Old', 7)")
            .execute(&pool)
            .await
            .unwrap();
    });
    let store = Store::open(&path, rt.handle().clone()).unwrap();
    let cards = store.load_cards().unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!((cards[0].chat, cards[0].meta.as_str()), (Some(7), ""));
}
