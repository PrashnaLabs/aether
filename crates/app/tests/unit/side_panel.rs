use super::*;

#[test]
fn chat_body_only_moves_into_sidebar_when_the_panel_opens() {
    use ChatLayout::*;
    assert!(chat_body_in_sidebar(Sidebar, true));
    assert!(chat_body_in_sidebar(Split, true));
    assert!(!chat_body_in_sidebar(Sidebar, false));
    assert!(!chat_body_in_sidebar(Split, false));
    assert!(!chat_body_in_sidebar(Tabbed, true));
}

#[test]
fn workspace_tabs_keep_changes_and_diff_inside_explorer() {
    use SideTab::*;
    assert_eq!(tab_order(false), [Agent, Terminal, Explorer]);
    assert_eq!(tab_order(true), [Agent, Terminal, Explorer, Browser]);
    let tabs = tab_order(true);
    assert_eq!(adjacent_tab(Agent, &tabs, false), Some(Terminal));
    assert_eq!(adjacent_tab(Terminal, &tabs, false), Some(Explorer));
    assert_eq!(adjacent_tab(Explorer, &tabs, false), Some(Browser));
    assert_eq!(adjacent_tab(Browser, &tabs, false), Some(Agent));
    assert_eq!(adjacent_tab(Agent, &tabs, true), Some(Browser));
    assert_eq!(shown_tab(Browser, Agent, &tab_order(false)), Agent);
}

#[test]
fn surfaces_map_to_tabs_and_explorer_views() {
    use ExplorerView::*;
    assert_eq!(Surface::tabs(false, false, false, false), [Surface::Terminal, Surface::Files]);
    assert_eq!(
        Surface::tabs(false, true, false, false),
        [Surface::Terminal, Surface::Files, Surface::Review]
    );
    assert_eq!(
        Surface::tabs(false, true, true, false),
        [Surface::Terminal, Surface::Files, Surface::Review, Surface::Browser]
    );
    assert!(!Surface::Terminal.closable());
    assert!(!Surface::Files.closable());
    assert!(Surface::Review.closable());
    assert!(Surface::Browser.closable());
    assert_eq!(Surface::Browser.tab(), SideTab::Browser);
    assert_eq!(Surface::Terminal.tab(), SideTab::Terminal);
    assert_eq!(Surface::Files.tab(), SideTab::Explorer);
    assert_eq!(Surface::Review.tab(), SideTab::Explorer);
    assert_eq!(Surface::Files.explorer_view(false), Some(Files));
    assert_eq!(Surface::Review.explorer_view(false), Some(Changes));
    assert_eq!(Surface::Review.explorer_view(true), Some(Diff));
    assert_eq!(Surface::Browser.explorer_view(true), None);
    assert_eq!(surface_for_tab(SideTab::Browser, Files, false), Surface::Browser);
    assert_eq!(surface_for_tab(SideTab::Terminal, Files, false), Surface::Terminal);
    assert_eq!(surface_for_tab(SideTab::Explorer, Files, false), Surface::Files);
    assert_eq!(surface_for_tab(SideTab::Explorer, Changes, false), Surface::Review);
    assert_eq!(surface_for_tab(SideTab::Explorer, Diff, false), Surface::Review);
}

#[test]
fn right_panel_share_clamps_and_collapse_ignores_split() {
    use ChatLayout::*;
    assert_eq!(right_panel_share(0.1), 0.25);
    assert_eq!(right_panel_share(0.6), 0.6);
    assert_eq!(right_panel_share(0.9), 0.8);
    assert_eq!(right_panel_share(1.5), 0.8);
    assert!(right_panel_collapsed(Sidebar, true));
    assert!(!right_panel_collapsed(Split, true));
    assert!(!right_panel_collapsed(Tabbed, true));
}

#[test]
fn terminal_sits_in_the_tab_once_moved_or_while_the_panel_fills_the_window() {
    assert!(tab_hosts_terminal(true, false, false));
    assert!(tab_hosts_terminal(false, true, true));
    assert!(!tab_hosts_terminal(false, true, false));
    assert!(!tab_hosts_terminal(false, false, true));
}

#[test]
fn diff_tab_forgets_a_reply_once_the_chat_is_rewound_or_cleared() {
    let messages = crate::app::chat_with("t", &["q", "a", "q2", "a2"]).messages;
    let target = |reply, epoch| DiffTarget { reply, epoch, scroll: ScrollHandle::new() };
    let shown = shown_reply(&messages, 0, &target(3, 0)).map(|m| m.text.as_str());
    assert_eq!(shown, Some("a2"));
    assert!(shown_reply(&messages, 1, &target(3, 0)).is_none());
    assert!(shown_reply(&messages, 0, &target(4, 0)).is_none());
    assert!(shown_reply(&messages, 0, &target(2, 0)).is_none());
    assert!(shown_reply(&[], 1, &target(1, 1)).is_none());
}

#[gpui_kit::test]
fn fixed_workspace_tabs_preserve_chat_drafts_and_explorer_buffers(
    cx: &mut gpui_kit::TestAppContext,
) {
    // This fixture uses disk-backed editor state and Agent's Tokio I/O.
    cx.background_executor.allow_parking();
    let folder = std::env::temp_dir().join(format!(
        "aether-explorer-panel-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("example.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
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
            app.settings.workspace_root = folder.to_string_lossy().into_owned();
            app.listing_worktrees = true;
            let chat = &mut app.chats[app.active];
            chat.provider = aether_core::Provider::Codex;
            chat.messages = vec![Message::user("Keep this conversation visible")];
            app.composer.update(cx, |input, cx| input.set_value("Unsent draft", window, cx));
            assert!(app.side_full);
            assert!(app.side_open());
            assert_eq!(app.side_tab, SideTab::Agent);
            assert_eq!(app.side_tabs(cx), [SideTab::Agent, SideTab::Terminal, SideTab::Explorer]);
            app.browser_open = true;
            app.close_browser_tab(cx);
            assert!(!app.side_tabs(cx).contains(&SideTab::Browser));
            app.diff_open = true;
            app.side_tab = SideTab::Explorer;
            app.explorer_view = crate::explorer::ExplorerView::Diff;
            app.close_diff_tab(cx);
            assert_eq!(app.explorer_view, crate::explorer::ExplorerView::Files);
            app.focus_composer(window, cx);
            assert_eq!(app.side_tabs(cx).len(), 3);
            app.toggle_explorer_page(window, cx);
            assert_eq!(app.page, Page::Chat);
            assert!(app.side_open());
            assert_eq!(app.side_tab, SideTab::Explorer);
            assert!(app.side_tabs(cx).contains(&SideTab::Explorer));
            assert_eq!(app.chats[app.active].messages[0].text, "Keep this conversation visible");
            assert_eq!(app.composer.read(cx).value(), "Unsent draft");
            app.side_tab = SideTab::Agent;
            app.open_explorer_file(file.clone(), window, cx);
            assert_eq!(
                app.side_tab,
                SideTab::Explorer,
                "Quick Open and file links must reveal the Explorer tab"
            );
            let explorer = app.active_project_explorer(cx);
            explorer.update(cx, |state, _| state.tabs[0].is_dirty = true);
            app.toggle_explorer_page(window, cx);
            assert!(app.side_open());
            assert_eq!(app.side_tab, SideTab::Agent);
            app.toggle_explorer_page(window, cx);
            assert!(app.side_open());
            assert!(
                explorer.read(cx).tabs[0].is_dirty,
                "switching tabs must preserve unsaved buffers"
            );
            app.side_tab = SideTab::Agent;
            app.search_workspace(window, cx);
            assert_eq!(app.side_tab, SideTab::Explorer);
            assert_eq!(explorer.read(cx).sidebar_tab, aether_explorer::ExplorerSidebarTab::Grep);
            app.open_explorer_view(crate::explorer::ExplorerView::Changes, window, cx);
            assert_eq!(app.side_tab, SideTab::Explorer);
            assert_eq!(app.explorer_view, crate::explorer::ExplorerView::Changes);
            app.focus_composer(window, cx);
            app.select_tab(SideTab::Explorer, window, cx);
            assert_eq!(app.explorer_view, crate::explorer::ExplorerView::Changes);
            app.open_reply_diff(0, None, window, cx);
            assert_eq!(app.side_tab, SideTab::Explorer);
            assert_eq!(app.explorer_view, crate::explorer::ExplorerView::Diff);
            assert!(app.diff_open);
            assert_eq!(app.side_tabs(cx).len(), 3);
            app.close_diff_tab(cx);
            assert_eq!(app.side_tab, SideTab::Explorer);
            assert_eq!(app.explorer_view, crate::explorer::ExplorerView::Files);
            assert_eq!(app.composer.read(cx).value(), "Unsent draft");
            app.close_explorer_panel(window, cx);
            assert!(app.side_tabs(cx).contains(&SideTab::Explorer));
            assert_eq!(
                explorer.read(cx).tabs.len(),
                1,
                "closing the panel tab must retain file buffers"
            );
        })
        .unwrap();
    let _ = std::fs::remove_dir_all(folder);
}
