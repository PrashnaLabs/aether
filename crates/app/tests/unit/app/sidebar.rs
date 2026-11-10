use std::collections::HashSet;

use crate::app::{Queued, chat_with};
use aether_core::Update;

use super::*;

#[test]
fn toggle_sidebar_action_and_method_bound() {
    let source =
        [include_str!("../../../src/app/render.rs"), include_str!("../../../src/app/sidebar.rs")]
            .map(|s| s.split("#[cfg(test)]").next().unwrap())
            .concat();
    assert!(source.contains("ToggleSidebar,"));
    assert!(source.contains("this.toggle_sidebar(window, cx)"));
    assert!(source.contains("fn toggle_sidebar(&mut self"));
    assert!(source.contains(".on_collapse("));
    assert!(source.contains("self.settings.sidebar_collapsed = !self.settings.sidebar_collapsed"));
}

#[test]
fn agent_state_follows_the_table() {
    use AgentState as S;
    use aether_core::Ask;
    let mut busy = chat_with("t", &["hi", "yo"]);
    busy.cancel = Some(aether_agent::Cancel::inert());
    assert_eq!(agent_state(&busy, false), Some(S::Working(0)));
    busy.queue = ["a", "b"]
        .map(|t| Queued {
            text: t.into(),
            attachments: vec![],
            delivering: false,
            from_children: false,
            deliveries: vec![],
        })
        .into();
    assert_eq!(agent_state(&busy, false), Some(S::Working(2)));
    busy.messages.last_mut().unwrap().apply(Update::Ask {
        id: 1,
        ask: Ask::Approval(aether_core::Approval {
            tool: "t".into(),
            title: "t".into(),
            input: "i".into(),
            ..Default::default()
        }),
    });
    assert_eq!(agent_state(&busy, false), Some(S::Waiting("Needs approval")));

    let mut idle = chat_with("t", &[]);
    idle.queue = vec![Queued {
        text: "a".into(),
        attachments: vec![],
        delivering: false,
        from_children: false,
        deliveries: vec![],
    }];
    assert_eq!(agent_state(&idle, false), Some(S::Queued(1)));
    idle.queue.clear();
    idle.unseen = ChatStatus::Failed;
    assert_eq!(agent_state(&idle, true), Some(S::Failed));
    idle.unseen = ChatStatus::Done;
    assert_eq!(agent_state(&idle, true), Some(S::Done));
    idle.unseen = ChatStatus::Idle;
    idle.provider = Provider::Claude;
    assert_eq!(agent_state(&idle, true), Some(S::Open));
    assert_eq!(agent_state(&idle, false), None);
    idle.provider = Provider::OpenRouter;
    assert_eq!(agent_state(&idle, false), None);
}

#[test]
fn detached_shows_short_sha_and_unborn_shows_no_commits_yet() {
    use crate::git_panel::sidebar_status_summary;
    use aether_git::{BranchInfo, Status};

    let mut branches = HashMap::new();
    branches.insert(
        PathBuf::from("/repo"),
        Some(BranchInfo { branch: "abc1234".into(), detached: true, worktree: false }),
    );
    assert_eq!(
        row_place(true, "", Path::new("/repo"), Path::new("/repo"), &branches),
        Some(RowPlace::Branch { name: "abc1234".into(), worktree: false, detached: true })
    );

    let unborn =
        Status { branch: Some("main".into()), ahead_behind: Some((1, 0)), ..Status::default() };
    assert_eq!(
        sidebar_status_summary(&unborn),
        (0, 0, 0, None),
        "no commits yet hides ahead/behind, keeps the branch name on the row"
    );
}

#[test]
fn sidebar_git_dedups_by_folder_and_counts_unique_paths() {
    use crate::git_panel::{Loaded, sidebar_pending, sidebar_status_summary};
    use aether_git::{Change, ChangeKind, Status};

    let a = PathBuf::from("/repo/a");
    let b = PathBuf::from("/repo/b");
    let statuses: HashMap<PathBuf, Loaded> = HashMap::new();
    let inflight: HashSet<PathBuf> = HashSet::new();
    assert_eq!(
        sidebar_pending(&statuses, &inflight, &[b.clone(), a.clone(), b.clone()]),
        vec![a, b]
    );
    assert!(sidebar_pending(&statuses, &inflight, &[]).is_empty());

    let change = |path: &str, staged: bool| Change {
        path: path.into(),
        from: None,
        kind: ChangeKind::Modified,
        staged,
        stat: aether_git::Stat::Lines(25, 10),
    };
    let status = Status {
        changes: vec![change("file.rs", false), change("file.rs", true), change("new.rs", true)],
        branch: Some("aether/fix".into()),
        commit: Some("abc1234".into()),
        upstream: Some("origin/aether/fix".into()),
        ahead_behind: Some((2, 1)),
        ..Default::default()
    };
    assert_eq!(sidebar_status_summary(&status), (2, 75, 30, Some((2, 1))));
    assert_eq!(sidebar_status_summary(&Status::default()), (0, 0, 0, None));
}

#[test]
fn ask_agent_file_tagging_formats_relative_path() {
    let root = Path::new("/workspace");
    let file = PathBuf::from("/workspace/src/lib.rs");
    let rel = file.strip_prefix(root).unwrap_or(&file).display().to_string();
    assert_eq!(rel, "src/lib.rs");
    let formatted = format!("@{rel} ");
    assert_eq!(formatted, "@src/lib.rs ");
}

#[test]
fn outside_agents_get_their_own_sidebar_view() {
    let source = include_str!("../../../src/app/sidebar.rs");
    let view = |name: &str| source.split(name).nth(1).unwrap().split("\n    fn ").next().unwrap();
    assert!(
        view("fn render_outside_sidebar").contains(".empty(\"No agents running outside aether\")")
    );
    assert!(!view("fn render_direct_chats_sidebar").contains("outside"));
}

#[test]
fn model_label_prefers_catalog_name() {
    let model = |id: &str, name: &str, default: bool| CliModel {
        id: id.into(),
        name: name.into(),
        default,
        ..Default::default()
    };
    let catalog = [model("ds/flash", "DeepSeek V4.1 Flash Free", false), model("big", "Big", true)];
    assert_eq!(model_label(&catalog, "ds/flash"), "DeepSeek V4.1 Flash Free");
    assert_eq!(model_label(&catalog, "raw/id"), "raw/id");
    assert_eq!(model_label(&catalog, ""), "Big");
    assert_eq!(model_label(&[], ""), "Default");
}

#[test]
fn rows_outside_their_group_show_the_branch_and_fall_back_to_the_folder() {
    use aether_git::BranchInfo;

    let mut branches = HashMap::new();
    branches.insert(
        PathBuf::from("/repo"),
        Some(BranchInfo { branch: "main".into(), detached: false, worktree: false }),
    );
    assert_eq!(
        row_place(false, "", Path::new("/repo"), Path::new("/repo"), &branches),
        Some(RowPlace::Branch { name: "main".into(), worktree: false, detached: false })
    );
    assert!(matches!(
        row_place(false, "", Path::new("/other"), Path::new("/other"), &branches),
        Some(RowPlace::Folder(_))
    ));
}

#[test]
fn outside_line_keeps_the_row_short_and_the_rest_on_hover() {
    let a = ExternalAgent {
        provider: Provider::Claude,
        pid: Some(42),
        started: String::new(),
        elapsed: "05:12".into(),
        cwd: "/code/aether".into(),
        session: "s".into(),
        title: "Fix tests".into(),
        state: ExternalState::Idle,
        exact: false,
        origin: "Terminal".into(),
    };
    let line = outside_line(&a);
    assert_eq!(line.id, "42");
    assert_eq!(line.title, "Fix tests");
    assert_eq!(line.detail, "aether · Terminal · 05:12");
    assert_eq!((line.state, line.label.as_ref()), (OutsideState::Idle, "Idle"));
    assert_eq!(line.tip, "Idle (estimated) · /code/aether");

    let waiting = ExternalAgent {
        pid: None,
        session: "ses_x".into(),
        title: String::new(),
        state: ExternalState::Waiting("approval".into()),
        exact: true,
        origin: String::new(),
        elapsed: String::new(),
        ..a.clone()
    };
    let line = outside_line(&waiting);
    assert_eq!((line.id.as_ref(), line.title.as_ref()), ("ses_x", "aether"));
    assert_eq!(line.detail, "aether");
    assert_eq!((line.state, line.label.as_ref()), (OutsideState::Waiting, "Needs approval"));
    assert_eq!(line.tip, "Needs approval · /code/aether · Busy in another OpenCode client");

    let bare = ExternalAgent { title: String::new(), cwd: PathBuf::new(), ..a };
    let line = outside_line(&bare);
    assert_eq!((line.title.as_ref(), line.detail.as_ref()), ("Untitled", "Terminal · 05:12"));
}

#[test]
fn hidden_same_basename_project_does_not_rename_visible_one() {
    let a = PathBuf::from("/x/app");
    let b = PathBuf::from("/y/app");
    let groups = || {
        vec![
            projects::Project { folder: a.clone(), chats: vec![0] },
            projects::Project { folder: b.clone(), chats: vec![1] },
        ]
    };
    let hidden = ["/y/app".to_string()];
    let names = |shown: &[projects::Project]| {
        disambiguated_names(&shown.iter().map(|g| g.folder.clone()).collect::<Vec<_>>())
    };
    let (shown, rest) = split_hidden(groups(), &hidden, 0);
    assert_eq!(rest.len(), 1);
    assert_eq!(names(&shown)[&a], "app");
    let (shown, rest) = split_hidden(groups(), &hidden, 1);
    assert!(rest.is_empty());
    assert_ne!(names(&shown)[&a], "app");
}

#[test]
fn projects_heading_offers_add_project() {
    let src = include_str!("../../../src/app/sidebar.rs");
    assert!(src.contains(".on_add_project("));
    assert!(src.contains("add_project_from_picker"));
}

#[test]
fn a_project_row_outside_git_says_so_once_checked() {
    let mut branches = HashMap::new();
    assert_eq!(row_place(true, "", Path::new("/plain"), Path::new("/plain"), &branches), None);
    branches.insert(PathBuf::from("/plain"), None);
    assert_eq!(
        row_place(true, "", Path::new("/plain"), Path::new("/plain"), &branches),
        Some(RowPlace::NoGit)
    );
}

#[gpui_kit::test]
fn every_sidebar_style_draws(cx: &mut gpui_kit::TestAppContext) {
    use aether_core::SidebarStyle;
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        let mut chat = crate::app::chat_with("Fix login", &["hi", "hello"]);
        chat.id = 77;
        chat.provider = Provider::Claude;
        chat.pinned = true;
        chat.lead = true;
        app.chats.push(chat);
        let mut child = crate::app::chat_with("Check login tests", &["hi", "hello"]);
        child.id = 78;
        child.provider = Provider::Pi;
        child.parent = Some(77);
        app.chats.push(child);
        app
    });
    for style in SidebarStyle::ALL {
        window.update(cx, |app, _, _| app.settings.sidebar_style = style).unwrap();
        cx.update_window(*window, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
    }
}

fn crew(id: i64, parent: Option<i64>, lead: bool) -> Chat {
    let mut c = chat_with("t", &[]);
    c.id = id;
    c.parent = parent;
    c.lead = lead;
    c
}

#[test]
fn children_follow_their_lead_in_group_order() {
    let chats = [
        crew(1, None, false),
        crew(5, Some(2), false),
        crew(2, None, true),
        crew(3, Some(2), false),
    ];
    assert_eq!(
        nest_children(&chats, &[0, 1, 2, 3]),
        [(0, false), (2, false), (3, true), (1, true)]
    );
}

#[test]
fn orphan_child_keeps_normal_position() {
    let chats = [crew(1, Some(9), false), crew(2, None, false), crew(3, Some(2), false)];
    assert_eq!(nest_children(&chats, &[0, 1, 2]), [(0, false), (1, false), (2, false)]);
}

#[test]
fn child_in_other_group_is_not_nested() {
    let chats = [crew(1, None, true), crew(2, Some(1), false)];
    assert_eq!(nest_children(&chats, &[1]), [(1, false)]);
    assert_eq!(nest_children(&chats, &[0]), [(0, false)]);
}

#[test]
fn nested_lead_keeps_its_row_and_its_children() {
    let chats = [crew(1, None, true), crew(2, Some(1), true), crew(3, Some(2), false)];
    assert_eq!(nest_children(&chats, &[0, 1, 2]), [(0, false), (1, false), (2, true)]);
}

#[test]
fn minimal_crews_follow_leads_across_sections_without_duplicates() {
    let chats = [
        crew(10, None, true),
        crew(12, Some(10), false),
        crew(11, Some(10), false),
        crew(20, None, false),
    ];
    let sections = vec![("Today".into(), vec![1, 3]), ("Yesterday".into(), vec![2, 0])];
    assert_eq!(
        nest_section_children(&chats, sections),
        vec![
            ("Today".into(), vec![(3, false)]),
            ("Yesterday".into(), vec![(0, false), (2, true), (1, true)]),
        ]
    );
}

#[test]
fn minimal_pinned_lead_keeps_crew_but_pinned_children_stay_independent() {
    let mut chats = [crew(10, None, true), crew(11, Some(10), false), crew(12, Some(10), false)];
    chats[0].pinned = true;
    chats[2].pinned = true;
    let sections = vec![("Pinned".into(), vec![2, 0]), ("Today".into(), vec![1])];
    assert_eq!(
        nest_section_children(&chats, sections),
        vec![("Pinned".into(), vec![(2, false), (0, false), (1, true)])]
    );
}

#[test]
fn minimal_missing_leads_and_normal_chats_keep_their_section_order() {
    let chats = [crew(1, Some(9), false), crew(2, None, false), crew(3, Some(2), false)];
    let sections = vec![("Project B".into(), vec![2, 0]), ("Project A".into(), vec![1])];
    assert_eq!(
        nest_section_children(&chats, sections),
        vec![
            ("Project B".into(), vec![(2, false), (0, false)]),
            ("Project A".into(), vec![(1, false)]),
        ]
    );
}

#[gpui_kit::test]
fn native_lead_chats_get_the_lead_marker(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        app.chats.clear();
        for (id, lead) in [(1, true), (2, false)] {
            let mut chat = crew(id, None, lead);
            chat.provider = Provider::OpenRouter;
            app.chats.push(chat);
        }
        app.sidebar_view = SidebarView::DirectChats;
        app
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("chat-lead-0").is_some());
    assert!(visual.debug_bounds("chat-lead-1").is_none());
}

#[gpui_kit::test]
fn direct_and_archive_nav_buttons_toggle_back(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        app.chats.clear();
        let mut chat = crew(1, None, false);
        chat.provider = Provider::OpenRouter;
        app.chats.push(chat);
        app.active = 0;
        app
    });
    window
        .update(cx, |app, window, cx| {
            app.sidebar_view = SidebarView::DirectChats;
            app.toggle_archive(cx);
            assert_eq!(app.sidebar_view, SidebarView::Archived);
            app.toggle_archive(cx);
            assert_eq!(app.sidebar_view, SidebarView::DirectChats);
            app.toggle_direct_chats(window, cx);
            assert_eq!(app.sidebar_view, SidebarView::Projects);
            app.toggle_direct_chats(window, cx);
            assert_eq!(app.sidebar_view, SidebarView::DirectChats);
        })
        .unwrap();
}

#[gpui_kit::test]
fn bulk_actions_apply_to_picked_chats_and_clear_picks(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        app.chats.clear();
        for id in 1..=4 {
            let mut chat = crew(id, None, false);
            chat.provider = Provider::OpenRouter;
            chat.archived = id <= 3;
            app.chats.push(chat);
        }
        app.active = 3;
        app
    });
    window
        .update(cx, |app, window, cx| {
            app.archive_picks.extend([1, 2, 3]);
            app.apply_bulk(Bulk::Unarchive, &[1], window, cx);
            assert!(!app.chats.iter().find(|c| c.id == 1).unwrap().archived);
            app.apply_bulk(Bulk::Delete, &[2, 3], window, cx);
            let left: Vec<i64> = app.chats.iter().map(|c| c.id).collect();
            assert_eq!(left, vec![1, 4]);
            assert!(app.archive_picks.is_empty());
            app.project_picks.insert(1);
            app.apply_bulk(Bulk::Archive, &[1], window, cx);
            assert!(app.chats.iter().find(|c| c.id == 1).unwrap().archived);
            assert!(app.project_picks.is_empty());
        })
        .unwrap();
}

#[test]
fn shared_folder_row_counts_only_the_files_its_chat_edited() {
    use crate::git_panel::{sidebar_chat_summary, sidebar_status_summary};
    use aether_git::{Change, ChangeKind, Stat, Status};

    let change = |path: &str, add: u32, rem: u32| Change {
        path: path.into(),
        from: None,
        kind: ChangeKind::Modified,
        staged: false,
        stat: Stat::Lines(add, rem),
    };
    let status = Status {
        changes: vec![
            change("src/a.rs", 10, 2),
            change("src/b.rs", 5, 0),
            change("docs/notes.md", 100, 0),
        ],
        ahead_behind: Some((1, 0)),
        commit: Some("abc1234".into()),
        upstream: Some("origin/main".into()),
        ..Default::default()
    };
    let folder_wide = sidebar_status_summary(&status);
    assert_eq!(folder_wide, (3, 115, 2, Some((1, 0))));

    let edited = |paths: &[&str]| paths.iter().map(|p| p.to_string()).collect::<HashSet<_>>();
    assert_eq!(
        sidebar_chat_summary(&status, &edited(&["src/a.rs"])),
        (1, 10, 2, Some((1, 0))),
        "one chat sees its own file, not the folder's other changes"
    );
    assert_eq!(
        sidebar_chat_summary(&status, &edited(&["src/b.rs", "docs/notes.md"])),
        (2, 105, 0, Some((1, 0))),
        "a second chat in the same folder gets different numbers"
    );
    assert_eq!(
        sidebar_chat_summary(&status, &edited(&["xa.rs"])),
        (0, 0, 0, Some((1, 0))),
        "a name that only ends the same is not a match"
    );
    assert_eq!(
        sidebar_chat_summary(&status, &edited(&["already_committed.rs"])).0,
        0,
        "an edit to an already committed file leaves nothing to show"
    );
    assert_eq!(sidebar_chat_summary(&status, &HashSet::new()).0, 0, "no edits, no chip");
}

#[test]
fn chat_in_a_subfolder_matches_repo_rooted_changes() {
    use crate::git_panel::sidebar_chat_summary;
    use aether_git::{Change, ChangeKind, Stat, Status};

    let status = Status {
        changes: vec![Change {
            path: "crates/app/src/lib.rs".into(),
            from: None,
            kind: ChangeKind::Modified,
            staged: false,
            stat: Stat::Lines(3, 1),
        }],
        ..Default::default()
    };
    let edited: HashSet<String> = ["src/lib.rs".to_string()].into();
    assert_eq!(sidebar_chat_summary(&status, &edited), (1, 3, 1, None));
}

#[gpui_kit::test]
fn chats_sharing_a_folder_show_their_own_diff_not_the_folders(cx: &mut gpui_kit::TestAppContext) {
    use aether_core::{Step, ToolCall, ToolDetail, ToolStatus};
    use aether_git::{Change, ChangeKind, Stat, Status};

    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        let folder = "/tmp/shared-proj";
        let edited = |path: &str| {
            let detail =
                ToolDetail::Edit { path: path.into(), diff: String::new(), added: 0, removed: 0 };
            let mut call = ToolCall::new("e", "Edit file", detail);
            call.status = ToolStatus::Done;
            Step::Tool(call)
        };
        let mut chats = Vec::new();
        for (id, title, file) in [
            (1, "writes a", Some("src/a.rs")),
            (2, "writes b", Some("src/b.rs")),
            (3, "idle", None),
        ] {
            let mut chat = crate::app::chat_with(title, &["hi", "hello"]);
            chat.id = id;
            chat.provider = Provider::Claude;
            chat.cwd = folder.into();
            if let Some(file) = file {
                chat.messages[1].steps.push(edited(&format!("{folder}/{file}")));
            }
            chats.push(chat);
        }
        app.chats = chats;
        let change = |path: &str, add: u32| Change {
            path: path.into(),
            from: None,
            kind: ChangeKind::Modified,
            staged: false,
            stat: Stat::Lines(add, 0),
        };
        let status = Status {
            changes: vec![change("src/a.rs", 10), change("src/b.rs", 5), change("docs/x.md", 100)],
            ..Default::default()
        };
        app.git.insert_sidebar_status(PathBuf::from(folder), Ok(status));
        app
    });
    window
        .update(cx, |app, _, _| {
            let folder = Path::new("/tmp/shared-proj");
            let rows: Vec<_> = (0..3).map(|ix| app.chat_git_summary(ix, folder)).collect();
            assert_eq!(rows[0], (1, 10, 0, None), "writes a");
            assert_eq!(rows[1], (1, 5, 0, None), "writes b");
            assert_eq!(rows[2], (0, 0, 0, None), "a chat that edited nothing shows nothing");

            app.chats.truncate(1);
            assert_eq!(
                app.chat_git_summary(0, folder),
                (3, 115, 0, None),
                "alone in its folder, a chat owns the folder's whole diff"
            );
        })
        .unwrap();
}
