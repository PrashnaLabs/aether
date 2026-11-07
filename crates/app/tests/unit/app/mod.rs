use crate::projects;

use super::*;

#[test]
fn workspace_changes_require_an_idle_draft() {
    let mut chat = chat_with("draft", &[]);
    assert!(chat.workspace_draft_ready());
    chat.worktree_job = Some(WorktreeJob::Switching);
    assert!(!chat.workspace_draft_ready());
    chat.worktree_job = None;
    chat.worktree_path = "/w/fix".into();
    assert!(!chat.workspace_draft_ready());
    chat.worktree_path.clear();
    chat.worktree_branch = "aether/fix".into();
    assert!(!chat.workspace_draft_ready());
    chat.worktree_branch.clear();
    chat.messages.push(Message::user("sent"));
    assert!(!chat.workspace_draft_ready());
}

#[test]
fn legacy_openrouter_chat_uses_the_global_model() {
    let settings = Settings { model: "x/global".into(), ..Settings::default() };
    let mut chat = chat_with("t", &[]);
    assert_eq!(chat.model(&settings), "x/global");
    chat.provider_model = "x/own".into();
    assert_eq!(chat.model(&settings), "x/own");
    chat.provider = Provider::Claude;
    chat.provider_model = String::new();
    assert_eq!(chat.model(&settings), "");
}


#[test]
fn activity_is_the_latest_send_reply_or_creation() {
    let mut chat = chat_with("t", &["hi", "yo", "again", "sure"]);
    chat.id = 1_000;
    assert_eq!(chat.activity(), 1_000);
    chat.messages[1].ended_at_ms = Some(5_000);
    chat.messages[3].ended_at_ms = Some(7_000);
    assert_eq!(chat.activity(), 7_000);
    chat.turn_at = 9_000;
    assert_eq!(chat.activity(), 9_000);
    let mut cli = chat_with("t", &[]);
    assert_eq!(cli.folder_name("/w"), None);
    cli.provider = Provider::Claude;
    assert_eq!(cli.folder_name("/code/w").as_deref(), Some("w"));
    cli.cwd = "/code/aether".into();
    assert_eq!(cli.folder_name("/code/w").as_deref(), Some("aether"));
}

#[test]
fn folder_is_fixed_at_first_send() {
    let mut chat = chat_with("t", &[]);
    assert_eq!(chat_cwd(&chat, "/w"), std::path::PathBuf::from("/w"));
    chat.cwd = "/fixed".into();
    assert_eq!(chat_cwd(&chat, "/w"), std::path::PathBuf::from("/fixed"));
    assert_eq!(folder_label("/Users/me/code/aether"), "aether");
}

#[test]
fn worktree_chats_group_under_the_project_they_started_in() {
    let mut local = chat_with("a", &["hi"]);
    local.provider = Provider::Claude;
    local.cwd = "/code/app".into();
    let mut forked = chat_with("b", &["hi"]);
    forked.id = 2;
    forked.provider = Provider::Claude;
    forked.cwd = "/w/app-1/fix/sub".into();
    forked.project = "/code/app".into();
    forked.worktree_path = "/w/app-1/fix".into();
    forked.worktree_branch = "aether/fix".into();
    let groups = projects::group_projects(
        [&local, &forked]
            .iter()
            .enumerate()
            .map(|(ix, c)| (ix, project_dir(c, "/home"), c.activity()))
            .collect(),
    );
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(groups[0].folder, PathBuf::from("/code/app"));
    assert_eq!(forked.folder_name("/home").as_deref(), Some("app"));
    assert_eq!(chat_cwd(&forked, "/home"), PathBuf::from("/w/app-1/fix/sub"));

    let mut missing = HashSet::new();
    assert!(forked.has_worktree(&missing, "/home") && !local.has_worktree(&missing, "/home"));
    missing.insert(PathBuf::from("/w/app-1/fix/sub"));
    assert!(!forked.has_worktree(&missing, "/home"));
    assert!(!forked.worktree_removed() && !local.worktree_removed());
    forked.worktree_path.clear();
    assert!(forked.worktree_removed(), "a removed worktree keeps its branch for the record");
    assert_eq!(project_dir(&forked, "/home"), PathBuf::from("/code/app"));
}

#[test]
fn explorer_page_toggles_and_preserves_previous_page() {
    let mut page = Page::Chat;
    let mut prev = Page::Chat;

    if page == Page::Explorer {
        page = prev;
    } else {
        prev = page;
        page = Page::Explorer;
    }
    assert_eq!(page, Page::Explorer);
    assert_eq!(prev, Page::Chat);

    if page == Page::Explorer {
        page = prev;
    } else {
        page = Page::Explorer;
    }
    assert_eq!(page, Page::Chat);
}

#[test]
fn chat_view_mode_defaults_to_chat_and_toggles() {
    let mut mode = ChatViewMode::default();
    assert_eq!(mode, ChatViewMode::Chat);
    mode = match mode {
        ChatViewMode::Chat => ChatViewMode::Explorer,
        ChatViewMode::Explorer => ChatViewMode::Chat,
    };
    assert_eq!(mode, ChatViewMode::Explorer);
}

#[test]
fn active_project_folder_resolves_chat_cwd_or_workspace_fallback() {
    let mut chat = chat_with("test", &[]);
    chat.cwd = "/tmp/my-project".to_string();
    let root = "/workspace";
    let resolved = chat_cwd(&chat, root);
    assert_eq!(resolved, PathBuf::from("/tmp/my-project"));
}

#[test]
fn chat_view_mode_toggle_flips_between_chat_and_explorer() {
    let mut chat = chat_with("test", &[]);
    assert_eq!(chat.view, ChatViewMode::Chat);
    chat.view = match chat.view {
        ChatViewMode::Chat => ChatViewMode::Explorer,
        ChatViewMode::Explorer => ChatViewMode::Chat,
    };
    assert_eq!(chat.view, ChatViewMode::Explorer);
    chat.view = match chat.view {
        ChatViewMode::Chat => ChatViewMode::Explorer,
        ChatViewMode::Explorer => ChatViewMode::Chat,
    };
    assert_eq!(chat.view, ChatViewMode::Chat);
}

#[test]
fn non_agent_chat_detection() {
    let mut openrouter_chat = chat_with("test", &[]);
    openrouter_chat.provider = Provider::OpenRouter;
    assert!(!openrouter_chat.is_agent());
    assert!(openrouter_chat.folder("/workspace").is_none());

    let mut agent_chat = chat_with("test", &[]);
    agent_chat.provider = Provider::Claude;
    assert!(agent_chat.is_agent());
    assert!(agent_chat.folder("/workspace").is_some());
}

#[test]
fn sidebar_view_transitions_and_segregation() {
    assert_eq!(SidebarView::default(), SidebarView::Projects);

    let mut normal_chat = chat_with("direct", &[]);
    normal_chat.provider = Provider::OpenRouter;
    normal_chat.id = 1;

    let mut agent_chat = chat_with("agent", &[]);
    agent_chat.provider = Provider::Claude;
    agent_chat.id = 2;

    let chats = [normal_chat, agent_chat];
    let mut view = SidebarView::Projects;

    // Selecting a non-agent chat switches view to DirectChats
    if !chats[0].is_agent() {
        view = SidebarView::DirectChats;
    }
    assert_eq!(view, SidebarView::DirectChats);

    // Selecting an agent chat switches view to Projects
    if chats[1].is_agent() {
        view = SidebarView::Projects;
    }
    assert_eq!(view, SidebarView::Projects);
}

#[test]
fn outside_view_keeps_chats_outside_agents_hold() {
    use SidebarView::*;
    assert_eq!(Outside.after_select(true, true), Outside);
    assert_eq!(Outside.after_select(true, false), Projects);
    assert_eq!(Outside.after_select(false, false), DirectChats);
    assert_eq!(Projects.after_select(true, true), Projects);
    assert_eq!(DirectChats.after_select(true, false), Projects);
}

#[test]
fn sidebar_status_precedence() {
    let facts = StatusFacts {
        busy: true,
        ask: false,
        started: true,
        retrying: false,
        last_event_ms: 100_000,
        turn_at: 40_000,
        background: 0,
        unseen: ChatStatus::Idle,
    };
    let now = 110_000;
    assert_eq!(
        chat_status(&StatusFacts { ask: true, retrying: true, ..facts }, 900_000),
        ChatStatus::Waiting
    );
    assert_eq!(chat_status(&StatusFacts { started: false, ..facts }, now), ChatStatus::Starting);
    assert_eq!(chat_status(&StatusFacts { retrying: true, ..facts }, now), ChatStatus::Retrying);
    assert_eq!(chat_status(&facts, 130_000), ChatStatus::Quiet { since_ms: 100_000 });
    assert_eq!(chat_status(&facts, now), ChatStatus::Working { since_ms: 40_000 });
    let idle = StatusFacts { busy: false, ..facts };
    assert_eq!(chat_status(&StatusFacts { background: 2, ..idle }, now), ChatStatus::Background(2));
    assert_eq!(
        chat_status(&StatusFacts { background: 2, unseen: ChatStatus::Failed, ..idle }, now),
        ChatStatus::Failed
    );
    assert_eq!(
        chat_status(&StatusFacts { unseen: ChatStatus::Done, ..idle }, now),
        ChatStatus::Done
    );
    assert_eq!(chat_status(&idle, now), ChatStatus::Idle);
}

#[test]
fn sidebar_quiet_flips_with_the_status_line_no_output() {
    use aether_ui::{Heartbeat, heartbeat};
    let facts = StatusFacts {
        busy: true,
        ask: false,
        started: true,
        retrying: false,
        last_event_ms: 100_000,
        turn_at: 40_000,
        background: 0,
        unseen: ChatStatus::Idle,
    };
    let at = 100_000 + SILENT_MS as i64;
    assert_eq!(heartbeat(SILENT_MS - 1), Heartbeat::Quiet(SILENT_MS - 1));
    assert_eq!(chat_status(&facts, at - 1), ChatStatus::Working { since_ms: 40_000 });
    assert_eq!(heartbeat(SILENT_MS), Heartbeat::Silent(SILENT_MS));
    assert_eq!(chat_status(&facts, at), ChatStatus::Quiet { since_ms: 100_000 });
}

#[test]
fn background_rows_keep_an_ended_turn_busy_looking() {
    let mut chat = chat_with("t", &["a", "b"]);
    assert_eq!(chat.status(0), ChatStatus::Idle);
    let mut call = aether_core::ToolCall::new("bg", "sleep", aether_core::ToolDetail::default());
    call.status = aether_core::ToolStatus::Background;
    chat.messages.last_mut().unwrap().apply(Update::Tool(call));
    assert_eq!(chat.status(0), ChatStatus::Background(1));
}

#[test]
fn silent_turns_go_quiet_from_the_later_of_last_event_and_send() {
    let facts = StatusFacts {
        busy: true,
        ask: false,
        started: false,
        retrying: false,
        last_event_ms: 0,
        turn_at: 40_000,
        background: 0,
        unseen: ChatStatus::Idle,
    };
    assert_eq!(chat_status(&facts, 60_000), ChatStatus::Starting);
    assert_eq!(chat_status(&facts, 70_000), ChatStatus::Quiet { since_ms: 40_000 });
    let ask = StatusFacts { ask: true, ..facts };
    assert_eq!(chat_status(&ask, 900_000), ChatStatus::Waiting);
}

#[test]
fn restored_background_rows_are_not_counted() {
    let mut chat = chat_with("t", &["a", "b"]);
    let mut call = aether_core::ToolCall::new("bg", "sleep", aether_core::ToolDetail::default());
    call.status = aether_core::ToolStatus::Background;
    let last = chat.messages.last_mut().unwrap();
    last.apply(Update::Tool(call));
    last.restored = true;
    assert_eq!(chat.status(0), ChatStatus::Idle);
}

#[test]
fn a_rate_limit_notice_never_reads_as_retrying() {
    let mut chat = chat_with("t", &["a", "b"]);
    chat.cancel = Some(aether_agent::Cancel::inert());
    chat.last_event_ms = 1_000;
    chat.turn_at = 1_000;
    let notice = |kind| {
        Update::Notice(aether_core::Notice {
            id: "n".into(),
            kind,
            text: String::new(),
            state: aether_core::NoticeState::Running,
            until_ms: Some(9_000),
            url: None,
            detail: None,
        })
    };
    chat.messages[1].apply(notice(NoticeKind::RateLimit));
    assert_eq!(chat.status(2_000), ChatStatus::Working { since_ms: 1_000 });
    chat.messages[1].apply(notice(NoticeKind::Retry));
    assert_eq!(chat.status(2_000), ChatStatus::Retrying);
}

#[test]
fn the_dead_bar_reads_the_cached_health_and_hides_while_locked() {
    let mut chat = chat_with("t", &["a"]);
    chat.provider = Provider::Claude;
    let mut reply = Message::assistant();
    reply.apply(Update::Error("claude exited".into()));
    reply.finish();
    chat.messages.push(reply);
    assert!(chat.dead_bar(false), "no live session: dead");
    chat.health = Some((SessionHealth { pid: Some(1), alive: true }, 0));
    assert!(!chat.dead_bar(false), "alive by the last check");
    chat.health = Some((SessionHealth { pid: Some(1), alive: false }, 0));
    assert!(chat.dead_bar(false));
    assert!(!chat.dead_bar(true), "a TUI holds the session");
}

mod orchestrate;
