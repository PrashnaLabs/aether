use super::super::{ChatStatus, ClickEvent};
use super::*;

#[test]
fn sidebar_custom_sections() {
    let mut sidebar = ChatSidebar::new("Direct Chats").chat_in_section(
        "Today",
        0,
        100,
        "Direct conversation",
        true,
        ChatStatus::Idle,
        false,
        None,
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
        None,
        None,
    );

    let rows = sidebar.rows();
    assert_eq!(rows.len(), 2);
    match &rows[0] {
        ChatRow::Header { title, .. } => assert_eq!(title.as_ref(), "Today"),
        _ => panic!("Expected Header row"),
    }
    match &rows[1] {
        ChatRow::Chat { chat, .. } => {
            assert_eq!(chat.title.as_ref(), "Direct conversation");
            assert_eq!(chat.id, 100);
        }
        _ => panic!("Expected Chat row"),
    }
}

#[test]
fn custom_section_children_keep_their_grouping_and_navigation() {
    let mut sidebar = ChatSidebar::new("Aether")
        .style(aether_core::SidebarStyle::Minimal)
        .child_agents(vec![100])
        .chat_in_section(
            "Today",
            0,
            100,
            "Worker",
            true,
            ChatStatus::Starting,
            false,
            None,
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            None,
            None,
        );
    let rows = sidebar.rows();
    assert_eq!(rows.len(), 2);
    assert!(rows[1].focusable());
    match &rows[1] {
        ChatRow::Chat { chat, .. } => {
            assert!(chat.child);
            assert!(chat.active);
            assert_eq!(chat.id, 100);
        }
        _ => panic!("Expected child chat row"),
    }
}

fn crew_sidebar(statuses: &[ChatStatus], selected: Option<usize>) -> ChatSidebar {
    let mut sidebar = ChatSidebar::new("Aether")
        .style(SidebarStyle::Minimal)
        .lead_agents(vec![10])
        .child_agents((0..statuses.len()).map(|i| 100 + i as i64).collect())
        .chat_in_section(
            "Today",
            0,
            10,
            "Lead",
            false,
            ChatStatus::Idle,
            false,
            None,
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            None,
            None,
        );
    for (i, &status) in statuses.iter().enumerate() {
        sidebar = sidebar.chat_in_section(
            "Today",
            i + 1,
            100 + i as i64,
            format!("Worker {i}"),
            selected == Some(i),
            status,
            false,
            None,
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            None,
            None,
        );
    }
    sidebar
}

#[test]
fn minimal_crew_preview_shows_three_recent_children_and_remaining_count() {
    let mut sidebar = crew_sidebar(&[ChatStatus::Done; 7], None);
    sidebar.collapsed_crews.insert(10);
    let rows = sidebar.rows();
    assert_eq!(
        rows.iter().map(ChatRow::key).collect::<Vec<_>>(),
        ["caption:Today", "chat:10", "chat:104", "chat:105", "chat:106", "crew-toggle:10"]
            .map(SharedString::from)
    );
    assert!(matches!(rows.last(), Some(ChatRow::CrewToggle { hidden: 4, expanded: false, .. })));
    assert!(rows.last().unwrap().focusable());
    assert!(
        !crew_sidebar(&[ChatStatus::Idle; 3], None)
            .rows()
            .iter()
            .any(|r| matches!(r, ChatRow::CrewToggle { .. }))
    );
}

#[test]
fn minimal_crew_never_hides_running_or_waiting_children() {
    let mut sidebar = crew_sidebar(
        &[
            ChatStatus::Starting,
            ChatStatus::Working { since_ms: 0 },
            ChatStatus::Waiting,
            ChatStatus::Retrying,
            ChatStatus::Quiet { since_ms: 0 },
            ChatStatus::Background(1),
            ChatStatus::Done,
            ChatStatus::Failed,
        ],
        None,
    );
    sidebar.collapsed_crews.insert(10);
    let rows = sidebar.rows();
    let ids: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            ChatRow::Chat { chat, .. } => Some(chat.id),
            _ => None,
        })
        .collect();
    assert_eq!(ids, [10, 100, 101, 102, 103, 104, 105]);
    assert!(matches!(rows.last(), Some(ChatRow::CrewToggle { hidden: 2, .. })));
}

#[test]
fn minimal_crew_keeps_selected_and_renamed_children_visible() {
    let mut sidebar = crew_sidebar(&[ChatStatus::Done; 7], Some(0));
    sidebar.rename_ix = Some(2);
    sidebar.collapsed_crews.insert(10);
    let rows = sidebar.rows();
    let ids: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            ChatRow::Chat { chat, .. } => Some(chat.id),
            _ => None,
        })
        .collect();
    assert_eq!(ids, [10, 100, 101, 106]);
}

#[test]
fn minimal_crews_start_expanded_and_can_collapse_independently() {
    let mut sidebar = crew_sidebar(&[ChatStatus::Done; 7], None);
    sidebar.collapsed_crews.insert(999);
    let rows = sidebar.rows();
    assert_eq!(rows.iter().filter(|r| matches!(r, ChatRow::Chat { .. })).count(), 8);
    assert!(matches!(rows.last(), Some(ChatRow::CrewToggle { expanded: true, hidden: 4, .. })));
    let mut collapsed = crew_sidebar(&[ChatStatus::Done; 7], None);
    collapsed.collapsed_crews.insert(10);
    assert_eq!(collapsed.rows().iter().filter(|r| matches!(r, ChatRow::Chat { .. })).count(), 4);
    let mut focus = crew_sidebar(&[ChatStatus::Done; 7], None).style(SidebarStyle::Focus);
    assert_eq!(focus.rows().iter().filter(|r| matches!(r, ChatRow::Chat { .. })).count(), 8);
}

fn default_crew_sidebar(statuses: &[ChatStatus], selected: Option<usize>) -> ChatSidebar {
    let mut sidebar = crew_sidebar(statuses, selected).style(SidebarStyle::Default);
    let (_, chats) = sidebar.custom_sections.pop().unwrap();
    sidebar = sidebar.project(
        ProjectHeader {
            name: "aether".into(),
            count: chats.len(),
            collapsed: false,
            status: ChatStatus::Idle,
            missing: false,
            path: "/w/aether".into(),
        },
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
    );
    sidebar.projects[0].chats = chats;
    sidebar
}

#[test]
fn default_project_crews_use_the_same_preview_and_expand_inside_the_project() {
    for expanded in [false, true] {
        let mut sidebar = default_crew_sidebar(&[ChatStatus::Done; 7], None);
        if !expanded {
            sidebar.collapsed_crews.insert(10);
        }
        let rows = sidebar.rows();
        let chats: Vec<_> = rows
            .iter()
            .filter_map(|r| match r {
                ChatRow::Chat { chat, nested, .. } => {
                    assert!(*nested);
                    Some(chat.id)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            chats,
            if expanded {
                vec![10, 100, 101, 102, 103, 104, 105, 106]
            } else {
                vec![10, 104, 105, 106]
            }
        );
        assert!(matches!(rows.last(), Some(ChatRow::CrewToggle { hidden: 4, nested: true, .. })));
    }
}

#[test]
fn default_worktree_crews_keep_running_agents_and_respect_project_collapse() {
    let mut sidebar = default_crew_sidebar(
        &[
            ChatStatus::Working { since_ms: 0 },
            ChatStatus::Starting,
            ChatStatus::Waiting,
            ChatStatus::Retrying,
            ChatStatus::Done,
            ChatStatus::Done,
        ],
        None,
    );
    sidebar.collapsed_crews.insert(10);
    let chats = std::mem::take(&mut sidebar.projects[0].chats);
    let header = ProjectHeader {
        name: "crew".into(),
        path: "/w/crew".into(),
        ..sidebar.projects[0].header.clone()
    };
    sidebar = sidebar.worktree(0, header, |_, _, _| {}, |_, _, _| {});
    sidebar.projects[0].worktrees[0].chats = chats;
    let rows = sidebar.rows();
    let ids: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            ChatRow::Chat { chat, .. } => Some(chat.id),
            _ => None,
        })
        .collect();
    assert_eq!(ids, [10, 100, 101, 102, 103]);
    assert!(matches!(rows.last(), Some(ChatRow::CrewToggle { hidden: 2, nested: true, .. })));

    let mut sidebar = default_crew_sidebar(&[ChatStatus::Done; 7], Some(0));
    sidebar.projects[0].header.collapsed = true;
    let rows = sidebar.rows();
    let ids: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            ChatRow::Chat { chat, .. } => Some(chat.id),
            _ => None,
        })
        .collect();
    assert_eq!(ids, [100]);
    assert!(!rows.iter().any(|r| matches!(r, ChatRow::CrewToggle { .. })));
}

struct CrewPreview {
    state: Option<Entity<SidebarState>>,
    style: SidebarStyle,
}

impl gpui_kit::Render for CrewPreview {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        self.state =
            Some(window.use_keyed_state("chat-sidebar", cx, |window, _| SidebarState::new(window)));
        let sidebar = if self.style == SidebarStyle::Default {
            default_crew_sidebar(&[ChatStatus::Done; 7], None)
        } else {
            crew_sidebar(&[ChatStatus::Done; 7], None)
        };
        sidebar.render(window, cx)
    }
}

#[gpui_kit::test]
fn crew_toggle_supports_click_and_keyboard_and_retains_state(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    for style in [SidebarStyle::Minimal, SidebarStyle::Default] {
        let window = cx.add_window(|_, _| CrewPreview { state: None, style });
        let preview = window.root(cx).unwrap();
        let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let toggle = visual.debug_bounds("crew-toggle-10").expect("crew expansion row");
        visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
        visual.update(|window, cx| {
            let _ = window.draw(cx);
            let state = preview.read(cx).state.as_ref().unwrap().clone();
            assert!(state.read(cx).collapsed_crews.contains(&10));
            let focus = state.read(cx).focus[&SharedString::from("crew-toggle:10")].clone();
            focus.focus(window, cx);
        });
        visual.simulate_keystrokes("left");
        visual.update(|window, cx| {
            let _ = window.draw(cx);
            let state = preview.read(cx).state.as_ref().unwrap().clone();
            assert!(state.read(cx).collapsed_crews.contains(&10));
        });
        visual.simulate_keystrokes("right");
        visual.update(|window, cx| {
            let _ = window.draw(cx);
            let state = preview.read(cx).state.as_ref().unwrap().clone();
            assert!(!state.read(cx).collapsed_crews.contains(&10));
        });
    }
}

fn outside(id: &str) -> OutsideLine {
    OutsideLine {
        id: id.into(),
        title: "Fix tests".into(),
        provider: Provider::Claude,
        detail: "aether · Terminal · 05:12".into(),
        state: OutsideState::Waiting,
        label: "Needs approval".into(),
        tip: "Needs approval · /code/aether".into(),
    }
}

fn keys(sidebar: &mut ChatSidebar) -> Vec<String> {
    sidebar.rows().iter().map(|row| row.key().to_string()).collect()
}

#[test]
fn outside_view_lists_agents_or_says_none_run() {
    let mut sidebar = ChatSidebar::new("Outside aether")
        .count(2)
        .empty("No agents running outside aether")
        .outside_agent(outside("42"), |_, _, _| {}, |_, _, _| {})
        .outside_agent(outside("ses_x"), |_, _, _| {}, |_, _, _| {});
    assert_eq!(sidebar.count, Some(2));
    let rows = sidebar.rows();
    assert!(rows.iter().all(ChatRow::focusable));
    assert_eq!(
        rows.iter().map(|r| r.key().to_string()).collect::<Vec<_>>(),
        ["outside:42", "outside:ses_x"]
    );

    let mut sidebar =
        ChatSidebar::new("Outside aether").count(0).empty("No agents running outside aether");
    assert_eq!(sidebar.count, None);
    let rows = sidebar.rows();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].focusable());
    match &rows[0] {
        ChatRow::Header { title, .. } => {
            assert_eq!(title.as_ref(), "No agents running outside aether")
        }
        _ => panic!("Expected the empty text"),
    }
}

#[test]
fn outside_aria_counts_agents() {
    assert_eq!(outside_aria(1), "Outside aether, 1 agent");
    assert_eq!(outside_aria(3), "Outside aether, 3 agents");
}

#[test]
fn project_headers_only_show_when_agents_count_is_positive() {
    for count in [0, 1] {
        let mut sidebar = ChatSidebar::new("Aether").project(
            ProjectHeader {
                name: "empty".into(),
                path: "/w/empty".into(),
                count,
                collapsed: false,
                status: ChatStatus::Idle,
                missing: false,
            },
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
            |_, _, _| {},
        );
        let expected =
            if count == 0 { vec![] } else { vec!["caption:Projects", "project:/w/empty"] };
        assert_eq!(keys(&mut sidebar), expected);
    }
}

#[test]
fn projects_header_keys_the_same_with_the_add_button() {
    let mut sidebar = ChatSidebar::new("Aether").on_add_project(|_, _, _| {});
    assert_eq!(keys(&mut sidebar), ["caption:Projects"]);
    let mut sidebar = ChatSidebar::new("Aether");
    assert!(keys(&mut sidebar).is_empty());
}

#[test]
fn rows_carry_the_sidebar_clock() {
    let mut sidebar = ChatSidebar::new("Aether").now(7_000).chat(
        0,
        1,
        "t",
        false,
        ChatStatus::Idle,
        false,
        None,
        None,
        None,
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
        |_, _, _| {},
        None,
        None,
    );
    match &sidebar.rows()[1] {
        ChatRow::Chat { chat, .. } => assert_eq!(chat.now, 7_000),
        _ => panic!("expected chat row"),
    }
}

#[test]
fn agent_stop_helpers() {
    assert_eq!(agent_stop_label("Fix bug"), "Stop Fix bug");
    assert_eq!(agent_stop_tooltip(), "Stop execution (Esc)");
}

#[test]
fn projects_section_collapse_hides_groups_and_preserves_add_button() {
    for collapsed in [false, true] {
        let mut sidebar = ChatSidebar::new("Aether")
            .projects_collapsible(collapsed, |_, _, _| {})
            .on_add_project(|_, _, _| {})
            .project(
                ProjectHeader {
                    name: "aether".into(),
                    path: "/w/aether".into(),
                    count: 1,
                    collapsed: false,
                    status: ChatStatus::Idle,
                    missing: false,
                },
                |_, _, _| {},
                |_, _, _| {},
                |_, _, _| {},
                |_, _, _| {},
                |_, _, _| {},
            )
            .hidden_projects("Show 1 hidden project", |_, _, _| {});
        let rows = sidebar.rows();
        let expected = if collapsed {
            vec!["caption:Projects"]
        } else {
            vec!["caption:Projects", "project:/w/aether", "link:Show 1 hidden project"]
        };
        assert_eq!(rows.iter().map(|row| row.key().to_string()).collect::<Vec<_>>(), expected);
        assert!(rows[0].focusable());
        match &rows[0] {
            ChatRow::Header { on_add, collapse: Some((folded, _)), .. } => {
                assert!(on_add.is_some());
                assert_eq!(*folded, collapsed);
            }
            _ => panic!("expected collapsible Projects heading"),
        }
    }
}

#[test]
fn agents_and_pinned_collapse_independently() {
    for (agents_collapsed, pinned_collapsed) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let mut sidebar = ChatSidebar::new("Aether")
            .agents_collapsible(agents_collapsed, |_, _, _| {})
            .pinned_collapsible(pinned_collapsed, |_, _, _| {})
            .agent(
                3,
                "Mine",
                Provider::Codex,
                "Codex".into(),
                Some("aether".into()),
                AgentState::Working(0),
                |_, _, _| {},
                None::<fn(&ClickEvent, &mut Window, &mut App)>,
            )
            .chat(
                0,
                7,
                "Pinned chat",
                false,
                ChatStatus::Idle,
                true,
                None,
                None,
                None,
                |_, _, _| {},
                |_, _, _| {},
                |_, _, _| {},
                |_, _, _| {},
                |_, _, _| {},
                |_, _, _| {},
                None,
                None,
            );
        let rows = sidebar.rows();
        let actual: Vec<_> = rows.iter().map(|row| row.key().to_string()).collect();
        let mut expected = vec!["caption:Agents"];
        if !agents_collapsed {
            expected.push("agent:3");
        }
        expected.push("caption:Pinned");
        if !pinned_collapsed {
            expected.push("chat:7");
        }
        assert_eq!(actual, expected);
        for (title, collapsed) in [("Agents", agents_collapsed), ("Pinned", pinned_collapsed)] {
            let heading =
                rows.iter().find(|row| row.key().as_ref() == format!("caption:{title}")).unwrap();
            assert!(heading.focusable());
            assert!(
                matches!(heading, ChatRow::Header { collapse: Some((folded, _)), .. } if *folded == collapsed)
            );
        }
    }
}
