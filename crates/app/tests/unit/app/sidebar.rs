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
fn sidebar_search_fuzzy_matches_chat_titles_in_every_style(cx: &mut gpui_kit::TestAppContext) {
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
        for (id, title) in [(1, "Fix login bug"), (2, "Performance audit"), (3, "Old login notes")]
        {
            let mut chat = crate::app::chat_with(title, &["hi", "hello"]);
            chat.id = id;
            chat.provider = Provider::Claude;
            chat.archived = id == 3;
            app.chats.push(chat);
        }
        app
    });
    cx.update_window(*window, |_, window, cx| {
        let app = window.root::<ChatApp>().flatten().unwrap();
        app.update(cx, |app, cx| {
            app.sidebar_search.update(cx, |s, cx| s.set_value("flb", window, cx));
            let titles: Vec<_> = app
                .chat_search_results(cx)
                .into_iter()
                .map(|ix| app.chats[ix].title.to_string())
                .collect();
            assert_eq!(titles, ["Fix login bug"]);
            app.sidebar_search.update(cx, |s, cx| s.set_value("login", window, cx));
            assert_eq!(app.chat_search_results(cx).len(), 1, "archived chats stay out");
        });
    })
    .unwrap();
    for style in SidebarStyle::ALL {
        window.update(cx, |app, _, _| app.settings.sidebar_style = style).unwrap();
        cx.update_window(*window, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn search_row_opens_the_field_and_an_empty_blur_closes_it(cx: &mut gpui_kit::TestAppContext) {
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
        let mut chat = chat_with("t", &["hi", "hello"]);
        chat.id = 1;
        chat.provider = Provider::Claude;
        app.chats.push(chat);
        app.sidebar_view = SidebarView::Projects;
        app
    });
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let row = visual.debug_bounds("section-btn:Search").expect("Search row");
    visual.simulate_click(row.center(), gpui_kit::Modifiers::default());
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(app.read_with(&visual, |app, _| app.sidebar_search_open));
    assert!(visual.debug_bounds("section-btn:Search").is_none());
    let field = visual.debug_bounds("search-field").expect("search field");
    assert_eq!(
        (field.origin, field.size),
        (row.origin, row.size),
        "the field takes the row's slot"
    );
    visual.update(|window, cx| {
        let search = gpui_kit::Focusable::focus_handle(&app.read(cx).sidebar_search, cx);
        assert!(search.is_focused(window), "opening focuses the field");
        app.update(cx, |app, cx| {
            app.sidebar_search.update(cx, |s, cx| s.set_value("log", window, cx));
            app.close_empty_search(cx);
            assert!(app.sidebar_search_open, "a query keeps the field");
            app.sidebar_search.update(cx, |s, cx| s.set_value("", window, cx));
            app.close_empty_search(cx);
        });
        let _ = window.draw(cx);
    });
    assert!(!app.read_with(&visual, |app, _| app.sidebar_search_open));
    assert!(visual.debug_bounds("section-btn:Search").is_some());

    let row = visual.debug_bounds("section-btn:Search").unwrap();
    visual.simulate_click(row.center(), gpui_kit::Modifiers::default());
    visual.simulate_input("log");
    assert_eq!(
        app.read_with(&visual, |app, cx| app.sidebar_search.read(cx).value().to_string()),
        "log"
    );
    visual.simulate_keystrokes("escape");
    visual.update(|window, cx| {
        let search = gpui_kit::Focusable::focus_handle(&app.read(cx).sidebar_search, cx);
        assert!(!search.is_focused(window), "Esc leaves the field");
        assert!(app.read(cx).sidebar_search.read(cx).value().is_empty(), "Esc clears the query");
    });
}

#[gpui_kit::test]
fn finished_crew_skips_archived_and_counts_grandchildren(cx: &mut gpui_kit::TestAppContext) {
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
        for (id, parent, archived) in
            [(1, None, false), (2, Some(1), false), (3, Some(1), true), (4, Some(2), false)]
        {
            let mut chat = chat_with("t", &["hi", "hello"]);
            chat.id = id;
            chat.provider = Provider::Claude;
            chat.parent = parent;
            chat.lead = id == 1;
            chat.archived = archived;
            app.chats.push(chat);
        }
        app
    });
    window
        .update(cx, |app, _, _| {
            let mut ids = app.finished_crew(1);
            ids.sort();
            assert_eq!(ids, [2, 4]);
            assert!(app.finished_crew(4).is_empty());
        })
        .unwrap();
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
    assert_eq!(nest_children(&chats, &[0, 1, 2, 3]), [(0, 0), (2, 0), (3, 1), (1, 1)]);
}

#[test]
fn orphan_child_keeps_normal_position() {
    let chats = [crew(1, Some(9), false), crew(2, None, false), crew(3, Some(2), false)];
    assert_eq!(nest_children(&chats, &[0, 1, 2]), [(0, 0), (1, 0), (2, 0)]);
}

#[test]
fn child_in_other_group_is_not_nested() {
    let chats = [crew(1, None, true), crew(2, Some(1), false)];
    assert_eq!(nest_children(&chats, &[1]), [(1, 0)]);
    assert_eq!(nest_children(&chats, &[0]), [(0, 0)]);
}

#[test]
fn nested_lead_keeps_its_row_and_its_children() {
    let chats = [crew(1, None, true), crew(2, Some(1), true), crew(3, Some(2), false)];
    assert_eq!(nest_children(&chats, &[0, 1, 2]), [(0, 0), (1, 1), (2, 2)]);
}

#[test]
fn self_parent_lead_still_renders() {
    let chats = [crew(1, Some(1), true)];
    assert_eq!(nest_children(&chats, &[0]), [(0, 0)]);
}

#[test]
fn lead_cycle_renders_every_chat() {
    let chats = [crew(1, Some(2), true), crew(2, Some(1), true), crew(3, None, false)];
    let mut ixs: Vec<usize> = nest_children(&chats, &[0, 1, 2]).iter().map(|r| r.0).collect();
    ixs.sort();
    assert_eq!(ixs, [0, 1, 2]);
}

#[test]
fn great_grandchildren_cap_at_depth_two() {
    let chats = [
        crew(1, None, true),
        crew(2, Some(1), true),
        crew(3, Some(2), true),
        crew(4, Some(3), false),
    ];
    assert_eq!(nest_children(&chats, &[0, 1, 2, 3]), [(0, 0), (1, 1), (2, 2), (3, 2)]);
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
        vec![("Today".into(), vec![(3, 0)]), ("Yesterday".into(), vec![(0, 0), (2, 1), (1, 1)]),]
    );
}

#[test]
fn minimal_sections_keep_pinned_chats_in_their_section() {
    let mut chats = [crew(10, None, true), crew(11, Some(10), false), crew(12, Some(10), false)];
    chats[0].pinned = true;
    chats[2].pinned = true;
    let sections = vec![("Today".into(), vec![2, 0, 1])];
    assert_eq!(
        nest_section_children(&chats, sections),
        vec![("Today".into(), vec![(0, 0), (1, 1), (2, 1)])]
    );
}

#[test]
fn minimal_missing_leads_and_normal_chats_keep_their_section_order() {
    let chats = [crew(1, Some(9), false), crew(2, None, false), crew(3, Some(2), false)];
    let sections = vec![("Project B".into(), vec![2, 0]), ("Project A".into(), vec![1])];
    assert_eq!(
        nest_section_children(&chats, sections),
        vec![("Project B".into(), vec![(2, 0), (0, 0)]), ("Project A".into(), vec![(1, 0)]),]
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

fn pinned_sidebar(cx: &mut gpui_kit::TestAppContext) -> gpui_kit::VisualTestContext {
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
        for (id, project, pinned) in
            [(1, "/w/a", true), (2, "/w/b", true), (3, "/w/h", true), (4, "/w/a", false)]
        {
            let mut chat = chat_with("t", &["hi", "hello"]);
            chat.id = id;
            chat.provider = Provider::Claude;
            chat.project = project.into();
            chat.pinned = pinned;
            app.chats.push(chat);
        }
        app.active = 3;
        app.sidebar_view = SidebarView::Projects;
        app.settings.hidden_projects = vec!["/w/h".into()];
        app
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    visual
}

#[gpui_kit::test]
fn pinned_chat_in_hidden_project_shows_only_in_pinned(cx: &mut gpui_kit::TestAppContext) {
    let mut visual = pinned_sidebar(cx);
    assert!(visual.debug_bounds("pin:3").is_some());
    assert!(visual.debug_bounds("chat:3").is_none());
    assert!(visual.debug_bounds("chat:1").is_some());
}

#[gpui_kit::test]
fn default_pinned_lists_newest_first(cx: &mut gpui_kit::TestAppContext) {
    let mut visual = pinned_sidebar(cx);
    let y: Vec<_> =
        ["pin:3", "pin:2", "pin:1"].map(|k| visual.debug_bounds(k).unwrap().origin.y).into();
    assert!(y.is_sorted());
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
fn org_view_lasts_only_while_the_org_page_shows() {
    use SidebarView::*;
    assert_eq!(Org.off_org(Page::Org, Archived), Org);
    assert_eq!(Org.off_org(Page::Chat, Archived), Archived);
    assert_eq!(Org.off_org(Page::Board, Outside), Outside);
    assert_eq!(DirectChats.off_org(Page::Chat, Archived), DirectChats);
}

#[gpui_kit::test]
fn org_page_navigation_restores_the_page_and_sidebar(cx: &mut gpui_kit::TestAppContext) {
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    window
        .update(cx, |app, window, cx| {
            app.settings.org_enabled = true;
            let (a, b) = (PathBuf::from("/tmp/aether-org-a"), PathBuf::from("/tmp/aether-org-b"));
            app.new_chat_in(b.clone(), window, cx);
            app.chats[app.active].messages = vec![aether_core::Message::user("hi")];
            let in_b = app.active;
            app.open_page(Page::Board, cx);
            app.switch_sidebar_to_outside(cx);

            app.open_org_for(&a, true, cx);
            assert_eq!((app.page, app.sidebar_view), (Page::Org, SidebarView::Org));
            assert_eq!(app.org_project().as_deref(), Some("/tmp/aether-org-a"));
            assert!(matches!(app.org.overlay, crate::org::OrgOverlay::Wizard { .. }));

            app.open_org_for(&b, false, cx);
            assert_eq!(app.org_project().as_deref(), Some("/tmp/aether-org-b"));
            assert_eq!(app.org.overlay, crate::org::OrgOverlay::None);

            app.org_back(window, cx);
            assert_eq!((app.page, app.sidebar_view), (Page::Board, SidebarView::Outside));
            assert_eq!(app.org.project, None);

            app.sidebar_view = SidebarView::Projects;
            app.open_org_for(&a, false, cx);
            app.select(in_b, window, cx);
            assert_eq!((app.page, app.sidebar_view), (Page::Chat, SidebarView::Projects));
            assert_eq!(app.org.project, None);
        })
        .unwrap();
}
