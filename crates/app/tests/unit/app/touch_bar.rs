use super::*;
use crate::app::{ChatApp, Page, chat_with};
use aether_agent::Agent;
use aether_core::Settings;
use gpui_kit::{AppContext, TestAppContext};

#[gpui_kit::test]
fn touch_bar_uses_each_windows_focused_session(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let (window, app) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| {
                ChatApp::with_state(
                    Agent::new().unwrap(),
                    Settings::default(),
                    None,
                    None,
                    false,
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    });
    cx.update_window(window, |_, _, cx| {
        app.update(cx, |app, _| {
            app.chats = vec![chat_with("Main", &[]), chat_with("Detached", &[])];
            app.chats[0].id = 101;
            app.chats[1].id = 202;
            app.active = 0;
            app.page = Page::Chat;
            app.workspace.model = Default::default();
            app.workspace.model.open(101, 0);
            let detached = app.workspace.model.open(202, 7);
            assert_eq!(app.touch_bar_target(0), Some(101));
            assert_eq!(app.touch_bar_target(7), Some(202));
            app.workspace.model.focus(detached);
            assert_eq!(app.touch_bar_target(0), Some(101));
            assert_eq!(app.touch_bar_target(7), Some(202));
            app.workspace.model.transfer(detached, 0).unwrap();
            assert_eq!(app.touch_bar_target(7), None);
            for action in ButtonAction::ALL {
                assert!(!app.touch_bar_enabled(7, action), "empty retained window: {action:?}");
            }
            app.page = Page::Settings;
            assert!(!app.touch_bar_enabled(0, ButtonAction::Terminal));
            assert!(app.touch_bar_enabled(0, ButtonAction::NewChat));
            assert_eq!(app.touch_bar_elapsed(0), None);
            assert_eq!(app.touch_bar_elapsed(7), None);

            app.page = Page::Chat;
            let now = crate::app::now_ms();
            let target = app.touch_bar_target(0).unwrap();
            let target_ix = app.chats.iter().position(|c| c.id == target).unwrap();
            app.chats[target_ix].cancel = Some(aether_agent::Cancel::inert());
            app.chats[target_ix].turn_at = now - 5_000;
            assert_eq!(app.touch_bar_elapsed(0), Some("5s".into()));
            app.chats[target_ix].cancel = None;
            app.chats[target_ix].turn_at = 0;
            assert_eq!(app.touch_bar_elapsed(0), None);

            app.page = Page::Settings;
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn touch_bar_files_targets_detached_session_while_main_shows_settings(cx: &mut TestAppContext) {
    use crate::app::ChatViewMode;
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let (window, app) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| {
                ChatApp::with_state(
                    Agent::new().unwrap(),
                    Settings::default(),
                    None,
                    None,
                    false,
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    });
    let (main_id, detached_id, count) = cx
        .update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                let main = app.chats[app.active].id;
                app.chats[app.active].provider = aether_core::Provider::Codex;
                app.new_chat(window, cx);
                let detached = app.chats[app.active].id;
                app.chats[app.active].provider = aether_core::Provider::Codex;
                app.chats[app.active].cwd = std::env::temp_dir().to_string_lossy().into_owned();
                let pane = app.workspace.model.pane_for(detached).unwrap();
                app.workspace.model.transfer(pane, 7).unwrap();
                app.workspace.model.open(main, 0);
                app.active = app.chats.iter().position(|c| c.id == main).unwrap();
                app.activate_session_ui(main, window, cx);
                app.page = Page::Settings;
                assert!(app.touch_bar_enabled(7, ButtonAction::Files));
                assert!(!app.touch_bar_enabled(7, ButtonAction::Changes), "unknown repo");
                app.run_touch_bar_action(7, ButtonAction::Files, window, cx);
                assert_eq!(app.page, Page::Settings, "detached actions preserve the main page");
                let detached = app.chats.iter().find(|c| c.id == detached).unwrap();
                assert_eq!(detached.view, ChatViewMode::Explorer);
                assert_eq!(app.chats[app.active].id, main);
                assert_eq!(app.chats[app.active].view, ChatViewMode::Chat);
                let detached = detached.id;
                app.run_touch_bar_action(7, ButtonAction::Commands, window, cx);
                assert!(!app.palette_open, "main palette remains closed");
                app.in_session(detached, cx, |app, _| {
                    assert!(app.palette_open, "Commands opens the detached session's palette");
                });
                let count = app.chats.len();
                app.side_tab = crate::side_panel::SideTab::Terminal;
                app.run_touch_bar_action(7, ButtonAction::NewChat, window, cx);
                (main, detached, count)
            })
        })
        .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.chats.len(), count + 1);
        assert_eq!(app.side_tab, crate::side_panel::SideTab::Agent);
        assert_ne!(app.chats[app.active].id, main_id);
        assert_ne!(app.chats[app.active].id, detached_id);
        assert_eq!(app.touch_bar_target(0), Some(app.chats[app.active].id));
        assert_eq!(app.touch_bar_target(7), Some(detached_id));
    });
    for host in [0, 7] {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.page = Page::Chat;
                assert!(app.touch_bar_enabled(host, ButtonAction::Settings));
                app.run_touch_bar_action(host, ButtonAction::Settings, window, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| assert_eq!(app.read(cx).page, Page::Settings));
    }
}

#[gpui_kit::test]
fn detached_sessions_keep_their_tabs_when_main_leaves_chat(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (window, app) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| {
                ChatApp::with_state(
                    Agent::new().unwrap(),
                    Settings::default(),
                    None,
                    None,
                    false,
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let main = app.chats[app.active].id;
            app.new_chat(window, cx);
            let detached = app.chats[app.active].id;
            app.chats[app.active].provider = aether_core::Provider::Codex;
            let pane = app.workspace.model.pane_for(detached).unwrap();
            app.workspace.model.transfer(pane, 7).unwrap();
            app.workspace.model.open(main, 0);
            app.active = app.chats.iter().position(|c| c.id == main).unwrap();
            app.page = Page::Project;
            let open = app.in_session(detached, cx, |app, _| app.side_open());
            assert_eq!(open, Some(true), "a detached agent session keeps its tabs");
            assert_eq!(app.page, Page::Project, "the main window stays on its page");
            app.in_session(detached, cx, |app, _| app.page = Page::Settings);
            assert_eq!(app.page, Page::Settings, "pages opened from a detached window stick");
        });
    })
    .unwrap();
}
