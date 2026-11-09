use super::super::{ChatStatus, ClickEvent, RowPlace};
use super::*;
use std::cell::Cell;
use std::rc::Rc;

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
        .child_agents(vec![(100, 1)])
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
            assert!(chat.child());
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
        .child_agents((0..statuses.len()).map(|i| (100 + i as i64, 1)).collect())
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

fn chat_ids(rows: &[ChatRow]) -> Vec<i64> {
    rows.iter()
        .filter_map(|r| match r {
            ChatRow::Chat { chat, .. } => Some(chat.id),
            _ => None,
        })
        .collect()
}

#[test]
fn fifteen_child_crew_starts_as_one_row() {
    let mut sidebar = crew_sidebar(&[ChatStatus::Done; 15], None);
    assert_eq!(keys(&mut sidebar), ["caption:Today", "chat:10"]);
}

#[test]
fn six_children_collapse_but_five_expand() {
    let mut six = crew_sidebar(&[ChatStatus::Done; 6], None);
    assert_eq!(keys(&mut six), ["caption:Today", "chat:10"]);
    let mut five = crew_sidebar(&[ChatStatus::Done; 5], None);
    assert_eq!(
        keys(&mut five),
        ["caption:Today", "chat:10", "chat:104", "chat:103", "chat:102", "chat:101", "chat:100"]
    );
}

fn mixed_crew() -> Vec<ChatStatus> {
    let mut statuses = vec![
        ChatStatus::Waiting,
        ChatStatus::Failed,
        ChatStatus::Working { since_ms: 0 },
        ChatStatus::Working { since_ms: 0 },
    ];
    statuses.extend([ChatStatus::Done; 11]);
    statuses
}

#[test]
fn expanded_crew_lists_attention_then_five_then_more_line() {
    for style in [SidebarStyle::Minimal, SidebarStyle::Default] {
        let mut sidebar = if style == SidebarStyle::Default {
            default_crew_sidebar(&mixed_crew(), None)
        } else {
            crew_sidebar(&mixed_crew(), None)
        };
        sidebar.crew_open.insert(10, true);
        let rows = sidebar.rows();
        assert_eq!(chat_ids(&rows), [10, 100, 101, 103, 102, 114, 113, 112]);
        assert_eq!(rows.last().unwrap().key().as_ref(), "crew-more:10");
        let ChatRow::CrewMore { hidden, all, total, depth, .. } = rows.last().unwrap() else {
            panic!("expected crew-more row");
        };
        assert_eq!((hidden.done, hidden.total(), *all, *total, *depth), (8, 8, false, 15, 1));
        assert!(!rows.last().unwrap().focusable());
    }
}

#[test]
fn show_all_lists_every_child() {
    let mut sidebar = crew_sidebar(&mixed_crew(), None);
    sidebar.crew_open.insert(10, true);
    sidebar.crew_all.insert(10);
    let rows = sidebar.rows();
    assert_eq!(chat_ids(&rows).len(), 16);
    assert!(matches!(rows.last(), Some(ChatRow::CrewMore { all: true, total: 15, .. })));
}

#[test]
fn small_crew_has_no_show_fewer_line() {
    let mut sidebar = crew_sidebar(&[ChatStatus::Done; 5], None);
    sidebar.crew_all.insert(10);
    assert!(!keys(&mut sidebar).iter().any(|k| k.starts_with("crew-more")));
}

#[test]
fn show_all_menu_label_names_the_crew_size() {
    assert_eq!(show_all_label(false, 15).as_ref(), "Show all 15 agents");
    assert_eq!(show_all_label(true, 15).as_ref(), "Show fewer agents");
}

#[test]
fn tier_change_keeps_row_keys() {
    let mut narrow = crew_sidebar(&mixed_crew(), Some(12)).width(200.);
    let mut wide = crew_sidebar(&mixed_crew(), Some(12)).width(480.);
    narrow.crew_open.insert(10, true);
    wide.crew_open.insert(10, true);
    assert_eq!(narrow.tier, Tier::Narrow);
    assert_eq!(wide.tier, Tier::Wide);
    assert_eq!(keys(&mut narrow), keys(&mut wide));
}

#[test]
fn collapsing_hides_busy_children() {
    let mut sidebar = crew_sidebar(
        &[ChatStatus::Working { since_ms: 0 }, ChatStatus::Waiting, ChatStatus::Failed],
        None,
    );
    sidebar.crew_open.insert(10, false);
    assert_eq!(keys(&mut sidebar), ["caption:Today", "chat:10"]);
}

#[test]
fn selected_child_stays_visible_when_collapsed() {
    let mut sidebar = crew_sidebar(&[ChatStatus::Done; 15], Some(3));
    sidebar.rename_ix = Some(6);
    assert_eq!(keys(&mut sidebar), ["caption:Today", "chat:10", "chat:105", "chat:103"]);
}

#[test]
fn lead_row_carries_transitive_summary() {
    let mut sidebar = ChatSidebar::new("Aether")
        .style(SidebarStyle::Minimal)
        .lead_agents(vec![10, 100])
        .child_agents(vec![(100, 1), (200, 2)]);
    for (ix, (id, status)) in [
        (10, ChatStatus::Idle),
        (100, ChatStatus::Idle),
        (200, ChatStatus::Working { since_ms: 0 }),
    ]
    .into_iter()
    .enumerate()
    {
        sidebar = sidebar.chat_in_section(
            "Today",
            ix,
            id,
            "t",
            false,
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
    let rows = sidebar.rows();
    assert_eq!(chat_ids(&rows), [10, 100, 200]);
    let crew = |id: i64| {
        rows.iter()
            .find_map(|r| match r {
                ChatRow::Chat { chat, crew, open, .. } if chat.id == id => Some((*crew, *open)),
                _ => None,
            })
            .unwrap()
    };
    let (lead, open) = crew(10);
    assert_eq!(open, Some(true));
    assert_eq!(lead.unwrap().working, 1);
    assert_eq!(lead.unwrap().total(), 2);
    assert_eq!(crew(100).0.unwrap().working, 1);
    assert_eq!(crew(200), (None, None));
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

struct CrewPreview {
    state: Option<Entity<SidebarState>>,
    style: SidebarStyle,
    opened: Rc<Cell<bool>>,
}

impl gpui_kit::Render for CrewPreview {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        self.state =
            Some(window.use_keyed_state("chat-sidebar", cx, |window, _| SidebarState::new(window)));
        let mut sidebar = if self.style == SidebarStyle::Default {
            default_crew_sidebar(&[ChatStatus::Done; 7], None)
        } else {
            crew_sidebar(&[ChatStatus::Done; 7], None)
        };
        let opened = self.opened.clone();
        let on_click: RowHandler = Rc::new(move |_, _, _| opened.set(true));
        let sections = sidebar.custom_sections.iter_mut().flat_map(|(_, chats)| chats);
        for chat in sections.chain(sidebar.projects.iter_mut().flat_map(|p| &mut p.chats)) {
            if chat.id == 10 {
                chat.on_click = on_click.clone();
            }
        }
        sidebar.render(window, cx)
    }
}

#[gpui_kit::test]
fn crew_toggle_supports_click_and_keyboard_and_retains_state(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    for style in [SidebarStyle::Minimal, SidebarStyle::Default] {
        let opened = Rc::new(Cell::new(false));
        let window =
            cx.add_window(|_, _| CrewPreview { state: None, style, opened: opened.clone() });
        let preview = window.root(cx).unwrap();
        let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
        let state = |visual: &mut gpui_kit::VisualTestContext| {
            visual.update(|window, cx| {
                let _ = window.draw(cx);
                preview.read(cx).state.as_ref().unwrap().clone()
            })
        };
        let open = |visual: &mut gpui_kit::VisualTestContext| {
            let state = state(visual);
            visual.update(|_, cx| state.read(cx).crew_open.get(&10).copied())
        };
        state(&mut visual);
        assert!(visual.debug_bounds("crew-more-10").is_none());
        let chevron = visual.debug_bounds("crew-chevron-10").expect("lead chevron");
        visual.simulate_click(chevron.center(), gpui_kit::Modifiers::default());
        assert_eq!(open(&mut visual), Some(true));
        assert!(!opened.get(), "chevron click must not open the lead chat");
        let more = visual.debug_bounds("crew-more-10").expect("show all target");
        visual.simulate_click(more.center(), gpui_kit::Modifiers::default());
        let s = state(&mut visual);
        visual.update(|window, cx| {
            assert!(s.read(cx).crew_all.contains(&10));
            s.read(cx).focus[&SharedString::from("chat:10")].clone().focus(window, cx);
        });
        visual.simulate_keystrokes("left");
        assert_eq!(open(&mut visual), Some(false));
        visual.simulate_keystrokes("right");
        assert_eq!(open(&mut visual), Some(true));
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
            expected.push("pin:7");
        }
        expected.extend(["caption:Chats", "chat:7"]);
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

fn line(branch: &str, added: u32) -> ChatLine {
    ChatLine {
        provider: Provider::Claude,
        project: None,
        model: "Sonnet".into(),
        place: Some(RowPlace::Branch { name: branch.into(), worktree: false, detached: false }),
        time: "1m".into(),
        path: "/w".into(),
        removed: false,
        changed: usize::from(added > 0),
        added,
        deleted: 0,
        ahead_behind: None,
        background: 0,
    }
}

fn git_flags(sidebar: &mut ChatSidebar) -> Vec<(i64, bool)> {
    sidebar
        .rows()
        .iter()
        .filter_map(|r| match r {
            ChatRow::Chat { chat, git, .. } if chat.child() => Some((chat.id, *git)),
            _ => None,
        })
        .collect()
}

#[test]
fn show_all_groups_consecutive_children_by_card() {
    let mut sidebar = default_crew_sidebar(&[ChatStatus::Done; 4], None);
    sidebar.crew_open.insert(10, true);
    sidebar.crew_all.insert(10);
    sidebar.cards =
        [(100, (7, "Auth".into())), (101, (7, "Auth".into())), (102, (9, "Docs".into()))].into();
    let rows = sidebar.rows();
    let keys: Vec<_> = rows.iter().map(|r| r.key().to_string()).collect();
    assert_eq!(
        keys[3..],
        ["card:10:9", "chat:102", "card:10:7", "chat:101", "chat:100", "chat:103"]
    );
    assert!(matches!(&rows[5], ChatRow::CardHeader { count: 2, card: 7, .. }));
    assert!(!rows[5].focusable());
    let carded: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            ChatRow::Chat { chat, carded, .. } => Some((chat.id, *carded)),
            _ => None,
        })
        .collect();
    assert_eq!(carded[1..], [(102, true), (101, true), (100, true), (103, false)]);
}

#[test]
fn card_headers_need_show_all() {
    let mut sidebar = default_crew_sidebar(&[ChatStatus::Done; 2], None);
    sidebar.cards = [(100, (7, "Auth".into()))].into();
    assert!(!keys(&mut sidebar).iter().any(|k| k.starts_with("card:")));
}

fn with_lines(sidebar: &mut ChatSidebar, lines: &[(&str, u32)]) {
    for (chat, (branch, added)) in sidebar.projects[0].chats.iter_mut().zip(lines) {
        chat.line = Some(line(branch, *added));
    }
}

#[test]
fn child_with_same_branch_hides_git_line() {
    let mut sidebar = default_crew_sidebar(&[ChatStatus::Done; 2], None);
    with_lines(&mut sidebar, &[("main", 3), ("main", 3), ("main", 3)]);
    assert_eq!(git_flags(&mut sidebar), [(101, false), (100, false)]);
}

#[test]
fn child_in_own_worktree_shows_git_line() {
    let mut sidebar = default_crew_sidebar(&[ChatStatus::Done; 3], None);
    with_lines(&mut sidebar, &[("main", 3), ("main", 3), ("wt/x", 3), ("main", 5)]);
    assert_eq!(git_flags(&mut sidebar), [(102, true), (101, true), (100, false)]);
}

#[test]
fn show_all_groups_same_card_once_across_statuses() {
    let statuses = [
        ChatStatus::Working { since_ms: 0 },
        ChatStatus::Done,
        ChatStatus::Working { since_ms: 0 },
        ChatStatus::Done,
    ];
    let mut sidebar = default_crew_sidebar(&statuses, None);
    sidebar.crew_open.insert(10, true);
    sidebar.crew_all.insert(10);
    sidebar.cards = [
        (100, (7, "Auth".into())),
        (101, (9, "Docs".into())),
        (102, (7, "Auth".into())),
        (103, (7, "Auth".into())),
    ]
    .into();
    let rows = sidebar.rows();
    let headers: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            ChatRow::CardHeader { card, count, .. } => Some((r.key().to_string(), *card, *count)),
            _ => None,
        })
        .collect();
    assert_eq!(headers.len(), 2);
    assert!(headers.contains(&("card:10:7".into(), 7, 3)));
    assert!(headers.contains(&("card:10:9".into(), 9, 1)));
    let keys: Vec<_> = rows.iter().map(|r| r.key().to_string()).collect();
    let mut unique = keys.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), keys.len());
}

fn pinned_in_project(chats: &[(i64, ChatStatus, bool, bool)]) -> ChatSidebar {
    let mut sidebar = ChatSidebar::new("Aether")
        .style(SidebarStyle::Default)
        .lead_agents(vec![10])
        .child_agents(vec![(100, 1), (101, 1)])
        .project(
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
    for (ix, &(id, status, pinned, active)) in chats.iter().enumerate() {
        sidebar = sidebar.chat(
            ix,
            id,
            "t",
            active,
            status,
            pinned,
            Some(0),
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
    }
    sidebar
}

#[test]
fn pinned_chat_appears_in_pinned_and_project() {
    let mut sidebar = pinned_in_project(&[(7, ChatStatus::Idle, true, false)]);
    assert_eq!(
        keys(&mut sidebar),
        ["caption:Pinned", "pin:7", "caption:Projects", "project:/w/aether", "chat:7"]
    );
}

#[test]
fn pinned_and_project_copies_have_distinct_keys() {
    let mut sidebar = pinned_in_project(&[
        (10, ChatStatus::Idle, true, true),
        (100, ChatStatus::Waiting, true, false),
        (101, ChatStatus::Idle, false, false),
    ]);
    let rows = sidebar.rows();
    let mut keys: Vec<_> = rows.iter().map(|r| r.key()).collect();
    let all = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), all);
    let active: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            ChatRow::Chat { chat, .. } if chat.active => Some(r.key().to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(active, ["pin:10", "chat:10"]);
}

#[test]
fn pinned_lead_shows_rollup_without_crew_in_pinned() {
    let mut sidebar = pinned_in_project(&[
        (10, ChatStatus::Idle, true, false),
        (100, ChatStatus::Waiting, false, false),
        (101, ChatStatus::Working { since_ms: 0 }, false, false),
    ]);
    let rows = sidebar.rows();
    let find = |key: &str| {
        rows.iter()
            .find_map(|r| match r {
                ChatRow::Chat { chat, crew, open, .. } if r.key() == key => {
                    Some((chat.tally, *crew, *open))
                }
                _ => None,
            })
            .unwrap()
    };
    let (tally, crew, open) = find("pin:10");
    let tally = tally.unwrap();
    assert_eq!((tally.needs_you, tally.working), (1, 1));
    assert_eq!((crew, open), (None, None));
    assert_eq!(keys_of(&rows).iter().filter(|k| k.starts_with("pin:")).count(), 1);
    let (tally, crew, open) = find("chat:10");
    assert_eq!((tally, open), (None, Some(true)));
    assert_eq!(crew.unwrap().total(), 2);
}

fn keys_of(rows: &[ChatRow]) -> Vec<String> {
    rows.iter().map(|r| r.key().to_string()).collect()
}

#[test]
fn pinned_chat_without_project_also_lists_in_chats() {
    let mut sidebar = ChatSidebar::new("Aether").chat(
        0,
        7,
        "t",
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
    let keys = keys(&mut sidebar);
    assert!(keys.contains(&"pin:7".to_string()) && keys.contains(&"chat:7".to_string()));
}
