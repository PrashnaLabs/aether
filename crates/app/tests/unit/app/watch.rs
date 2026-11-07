use super::view::{clamp_peek, drag_peek, fab_edges};
use super::*;
use crate::app::chat_with;
use crate::app::composer::voice::VoiceState;
use crate::side_panel::SideTab;
use aether_agent::Agent;
use aether_core::{Provider, Settings, TileLayout, WatchCorner};
use aether_terminal::view::TerminalEvent;
use gpui_kit::AppContext;
use gpui_kit::{TestAppContext, px};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[test]
fn only_live_chats_are_watchable() {
    let mut chat = chat_with("a", &[]);
    assert!(watchable(Watch::Chat(chat.id), Some(&chat)));
    assert!(!watchable(Watch::Chat(2), None));
    chat.archived = true;
    assert!(!watchable(Watch::Chat(chat.id), Some(&chat)));
}

#[test]
fn only_agent_chats_have_a_watchable_terminal() {
    let mut chat = chat_with("a", &[]);
    chat.provider = Provider::OpenRouter;
    assert!(!watchable(Watch::Terminal(chat.id), Some(&chat)));
    chat.provider = Provider::Claude;
    assert!(watchable(Watch::Terminal(chat.id), Some(&chat)));
}

#[gpui_kit::test]
fn archiving_a_watched_chat_drops_it_and_closes_its_peek(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let (main, app) = cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
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
    cx.update_window(main, |_, window, cx| {
        app.update(cx, |app, cx| {
            let mut other = chat_with("b", &[]);
            other.id = 2;
            app.chats.push(other);
            app.settings.watchlist = vec![Watch::Chat(2), Watch::Terminal(2)];
            app.watch_peek = Some(Watch::Chat(2));
            app.archive_chat(1, window, cx);
            assert!(app.settings.watchlist.is_empty());
            assert_eq!(app.watch_peek, None);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn web_views_uncover_when_the_watch_overlay_closes(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let (main, app) = cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
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
    cx.update_window(main, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.watch_open = true;
            app.render_watch(window, cx);
            assert!(app.browser.read(cx).covered);
            assert!(app.hn_browser.read(cx).covered);
            app.watch_open = false;
            app.render_watch(window, cx);
            assert!(!app.browser.read(cx).covered);
            assert!(!app.hn_browser.read(cx).covered);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn removing_a_row_does_not_peek_it(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        app.settings.watchlist = vec![Watch::Chat(app.chats[app.active].id)];
        app.watch_open = true;
        app
    });
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let remove = visual.debug_bounds("watch-remove").unwrap();
    visual.simulate_click(remove.center(), gpui_kit::Modifiers::default());
    visual.read(|cx| {
        let app = app.read(cx);
        assert!(app.settings.watchlist.is_empty());
        assert_eq!(app.watch_peek, None);
    });
}

#[gpui_kit::test]
fn fab_lifts_only_where_a_composer_can_reach_the_corner(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        app.page = Page::Chat;
        app.chats[app.active].provider = Provider::OpenRouter;
        app
    });
    window
        .update(cx, |app, _, _| {
            assert!(app.fab_lifted(px(900.)));
            assert!(!app.fab_lifted(px(3000.)));
            app.page = Page::Settings;
            assert!(!app.fab_lifted(px(900.)));
        })
        .unwrap();
}

fn app_with_two_chats(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<ChatApp> {
    cx.update(gpui_kit::init);
    cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        let mut other = chat_with("b", &["hi"]);
        other.id = 2;
        app.chats.push(other);
        app
    })
}

#[gpui_kit::test]
fn peek_gives_way_when_its_chat_is_on_screen(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.watch_peek = Some(Watch::Chat(2));
            app.settle_peek();
            assert_eq!(app.watch_peek, Some(Watch::Chat(2)));
            app.select(1, window, cx);
            app.settle_peek();
            assert_eq!(app.watch_peek, None);
        })
        .unwrap();
}

#[gpui_kit::test]
fn stop_in_peek_targets_the_peeked_chat(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    cx.update(crate::actions::bind_keys);
    window
        .update(cx, |app, _, _| {
            for chat in &mut app.chats {
                chat.cancel = Some(aether_agent::Cancel::inert());
            }
            app.watch_peek = Some(Watch::Chat(2));
        })
        .unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    focus_peek(&mut visual, &app);
    // Esc now asks to confirm, which visual tests can't render; stop_chat is
    // the confirmed path.
    visual.update(|_, cx| app.update(cx, |app, cx| app.stop_chat(1, cx)));
    visual.read(|cx| {
        let app = app.read(cx);
        assert!(app.chats[1].stopped);
        assert!(!app.chats[0].stopped);
        assert_eq!(app.page, Page::Chat);
        assert_eq!(app.ui_owner, Some(app.chats[app.active].id));
    });
    visual.update(|window, cx| {
        app.update(cx, |app, cx| app.composer.update(cx, |s, cx| s.focus(window, cx)));
        let _ = window.draw(cx);
    });
    visual.update(|_, cx| app.update(cx, |app, cx| app.stop_chat(0, cx)));
    visual.read(|cx| assert!(app.read(cx).chats[0].stopped));
    visual.update(|_, cx| {
        app.update(cx, |app, _| {
            app.chats[1].cancel = None;
            app.page = Page::Settings;
        })
    });
    focus_peek(&mut visual, &app);
    visual.simulate_keystrokes("escape");
    visual.read(|cx| assert_eq!(app.read(cx).page, Page::Settings));
}

fn focus_peek(visual: &mut gpui_kit::VisualTestContext, app: &gpui_kit::Entity<ChatApp>) {
    visual.update(|window, cx| {
        let _ = window.draw(cx);
        app.update(cx, |app, cx| {
            app.in_session(2, cx, |app, cx| app.composer.update(cx, |s, cx| s.focus(window, cx)));
        });
        let _ = window.draw(cx);
    });
}

#[gpui_kit::test]
fn peek_scopes_session_actions_to_its_chat(cx: &mut TestAppContext) {
    use gpui_kit::Focusable;
    let window = app_with_two_chats(cx);
    cx.update(crate::actions::bind_keys);
    window
        .update(cx, |app, _, _| {
            app.watch_peek = Some(Watch::Chat(2));
        })
        .unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    focus_peek(&mut visual, &app);
    visual.dispatch_action(crate::actions::FocusComposer);
    visual.update(|window, cx| {
        let peek_focused = app.update(cx, |app, cx| {
            app.in_session(2, cx, |app, cx| app.composer.focus_handle(cx).is_focused(window))
        });
        let app = app.read(cx);
        assert_eq!(app.page, Page::Chat);
        assert_eq!(peek_focused, Some(true));
    });
    visual.update(|_, cx| {
        app.update(cx, |app, _| {
            app.settings.workspace_root = std::env::temp_dir().to_string_lossy().into();
            app.chats[0].provider = Provider::Claude;
        })
    });
    focus_peek(&mut visual, &app);
    let tab = visual.read(|cx| app.read(cx).side_tab);
    visual.simulate_keystrokes("secondary-shift-l");
    visual.read(|cx| assert_eq!(app.read(cx).side_tab, tab));
}

#[cfg(unix)]
#[gpui_kit::test]
fn peeked_terminal_is_skipped_by_the_drawer(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.settings.workspace_root = std::env::temp_dir().to_string_lossy().into();
            app.chats[app.active].provider = Provider::Claude;
            (app.terminal_tab, app.side_full) = (false, false);
            let id = app.chats[app.active].id;
            let view = app.drawer_for(app.active, window, cx).unwrap();
            view.update(cx, |t, cx| {
                t.open = true;
                t.set_shown(true, cx);
            });
            assert!(app.terminal_drawer(window, cx).is_some());
            app.watch_peek = Some(Watch::Terminal(id));
            assert!(app.terminal_drawer(window, cx).is_none());
            assert!(view.read(cx).shown);
        })
        .unwrap();
}

fn as_agents(app: &mut ChatApp) {
    app.settings.workspace_root = std::env::temp_dir().to_string_lossy().into();
    for chat in &mut app.chats {
        chat.provider = Provider::Claude;
    }
    app.page = Page::Chat;
}

#[gpui_kit::test]
fn fab_lifts_over_an_agent_chat_composer(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, _| {
            as_agents(app);
            assert!(app.side_open() && app.side_full);
            assert!(app.fab_lifted(px(900.)));
            app.settings.sidebar_width = 5000.;
            assert!(!app.fab_lifted(px(3000.)));
            app.side_tab = SideTab::Terminal;
            assert!(!app.fab_lifted(px(900.)));
        })
        .unwrap();
}

#[cfg(unix)]
#[gpui_kit::test]
fn fullscreen_toggle_fills_the_window_with_the_tab_terminal(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            as_agents(app);
            app.select_tab(SideTab::Terminal, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let toggle = visual.debug_bounds("terminal-toggle-maximize").expect("fullscreen button");
    let tabbed = visual.debug_bounds("terminal-workspace").unwrap();
    assert!(tabbed.origin.x > px(0.) && tabbed.origin.y > px(0.));
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::none());
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let full = visual.debug_bounds("terminal-workspace").unwrap();
    assert_eq!(full.origin, gpui_kit::point(px(0.), px(0.)), "sidebar and tabs give way");
    let viewport = visual.update(|window, _| window.viewport_size());
    let bar = px(crate::app::status_bar::STATUS_BAR);
    assert_eq!(full.size, gpui_kit::size(viewport.width, viewport.height - bar), "nav bar stays");
    window
        .update(cx, |app, window, cx| {
            let id = app.chats[app.active].id;
            assert!(app.fullscreen_terminal(window, cx).is_some());
            app.hide_terminal(id, window, cx);
            app.select_tab(SideTab::Terminal, window, cx);
            assert!(app.fullscreen_terminal(window, cx).is_none(), "hiding left fullscreen");
        })
        .unwrap();
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_bang_message_runs_in_the_terminal_instead_of_the_chat(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            as_agents(app);
            let sent = app.chats[app.active].messages.len();
            app.composer.update(cx, |c, cx| c.set_value("!exit 0", window, cx));
            app.send(window, cx);
            assert_eq!(app.chats[app.active].messages.len(), sent, "nothing went to the agent");
            assert!(app.composer.read(cx).value().is_empty());
            let id = app.chats[app.active].id;
            let terminal = app.terminals[&id].view.read(cx);
            assert!(terminal.open && terminal.contains_focused(window, cx));
        })
        .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn a_transcript_lands_in_the_chat_it_started_in(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.composer.update(cx, |c, cx| c.set_value("typed", window, cx));
            app.chats[1].draft = "draft".into();
            app.deliver_transcript(2, "hello", window, cx);
            assert_eq!(app.composer.read(cx).value(), "typed");
            assert_eq!(app.chats[1].draft, "draft hello");
            app.chats[1].draft.clear();
            app.deliver_transcript(2, "hello", window, cx);
            assert_eq!(app.chats[1].draft, "hello", "no leading space on an empty draft");
            app.deliver_transcript(2, "", window, cx);
            assert_eq!(app.chats[1].draft, "hello", "empty text is a no-op");
            app.deliver_transcript(99, "hello", window, cx);
            assert_eq!(app.composer.read(cx).value(), "typed", "a deleted chat drops the text");
        })
        .unwrap();
}

#[gpui_kit::test]
fn a_transcript_reaches_the_composer_of_a_background_chat(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.ensure_session_ui(2, window, cx);
            app.composer.update(cx, |c, cx| c.set_value("typed", window, cx));
            let active = app.active;
            app.deliver_transcript(2, "hello", window, cx);
            assert_eq!(app.active, active);
            assert_eq!(app.composer.read(cx).value(), "typed");
            let other = app.in_session(2, cx, |app, cx| app.composer.read(cx).value()).unwrap();
            assert_eq!(other, "hello");
        })
        .unwrap();
    window.update(cx, |app, _, _| assert_eq!(app.chats[1].draft, "hello")).unwrap();
}

#[gpui_kit::test]
fn a_transcript_joins_the_draft_at_the_cursor(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let draft = "🙂 fix";
            app.composer.update(cx, |c, cx| {
                c.set_value(draft, window, cx);
                c.set_selected_range(draft.len()..draft.len(), cx);
            });
            let id = app.chats[app.active].id;
            app.deliver_transcript(id, "hello", window, cx);
            assert_eq!(app.composer.read(cx).value(), "🙂 fix hello");
        })
        .unwrap();
    window.update(cx, |app, _, _| assert_eq!(app.chats[app.active].draft, "🙂 fix hello")).unwrap();
}

#[gpui_kit::test]
fn a_transcript_replaces_the_selection_without_doubling_spaces(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.composer.update(cx, |c, cx| {
                c.set_value("a bad word", window, cx);
                c.set_selected_range(2..5, cx);
            });
            let id = app.chats[app.active].id;
            app.deliver_transcript(id, "good", window, cx);
            assert_eq!(app.composer.read(cx).value(), "a good word");
        })
        .unwrap();
}

#[gpui_kit::test]
fn a_transcript_survives_the_open_draft_editor(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.composer.update(cx, |c, cx| c.set_value("fix", window, cx));
            app.open_draft_editor(window, cx);
            assert!(app.draft_editor.is_some());
            let id = app.chats[app.active].id;
            app.deliver_transcript(id, "the bug", window, cx);
            app.close_draft_editor(true, window, cx);
            assert_eq!(app.composer.read(cx).value(), "fix the bug");
        })
        .unwrap();
}

#[gpui_kit::test]
fn only_the_receiving_chats_draft_editor_takes_the_transcript(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.ensure_session_ui(2, window, cx);
            app.open_draft_editor(window, cx);
            app.deliver_transcript(2, "hello", window, cx);
            let other = app.in_session(2, cx, |app, cx| {
                app.open_draft_editor(window, cx);
                app.composer.read(cx).value()
            });
            assert_eq!(other.unwrap(), "hello", "no editor there: its composer takes it");
            app.deliver_transcript(2, "again", window, cx);
            assert_eq!(app.draft_editor.as_ref().unwrap().read(cx).value(), "");
            let other = app.in_session(2, cx, |app, cx| {
                app.close_draft_editor(true, window, cx);
                app.composer.read(cx).value()
            });
            assert_eq!(other.unwrap(), "hello again");
        })
        .unwrap();
}

#[gpui_kit::test]
fn a_transcript_reaches_a_disabled_composer(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.ensure_session_ui(2, window, cx);
            app.composer.update(cx, |c, cx| {
                c.set_value("fix", window, cx);
                c.set_selected_range(3..3, cx);
                c.set_disabled(true, cx);
            });
            let id = app.chats[app.active].id;
            app.deliver_transcript(id, "the bug", window, cx);
            assert_eq!(app.composer.read(cx).value(), "fix the bug");
            assert!(!app.composer.read(cx).is_editable(), "the restriction comes back");

            app.in_session(2, cx, |app, cx| {
                app.composer.update(cx, |c, cx| c.set_disabled(true, cx));
            });
            app.deliver_transcript(2, "hello", window, cx);
            let other = app.in_session(2, cx, |app, cx| app.composer.read(cx).value()).unwrap();
            assert_eq!(other, "hello");
        })
        .unwrap();
    window
        .update(cx, |app, _, _| {
            assert_eq!(app.chats[app.active].draft, "fix the bug");
            assert_eq!(app.chats[1].draft, "hello");
        })
        .unwrap();
}

fn doomed_chat(app: &mut ChatApp) -> usize {
    let mut chat = chat_with("doomed", &[]);
    // delete_chat wipes this id's folder in the real attachments dir
    chat.id = -7;
    app.chats.push(chat);
    app.chats.len() - 1
}

#[gpui_kit::test]
fn deleting_a_chat_cancels_the_dictation_it_owns(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let ix = doomed_chat(app);
            app.voice = VoiceState::NeedsModel { chat: -7 };
            app.delete_chat(ix, window, cx);
            assert!(matches!(app.voice, VoiceState::Idle));

            let ix = doomed_chat(app);
            let cancel = Arc::new(AtomicBool::new(false));
            app.voice = VoiceState::Downloading { chat: -7, done: 0, cancel: cancel.clone() };
            app.delete_chat(ix, window, cx);
            assert!(matches!(app.voice, VoiceState::Idle));
            assert!(cancel.load(Ordering::Relaxed));

            let ix = doomed_chat(app);
            app.voice = VoiceState::Opening { chat: -7, started: Instant::now() };
            app.delete_chat(ix, window, cx);
            assert!(matches!(app.voice, VoiceState::Idle));

            let ix = doomed_chat(app);
            app.voice = VoiceState::Transcribing { chat: -7 };
            app.delete_chat(ix, window, cx);
            assert!(matches!(app.voice, VoiceState::Transcribing { .. }), "it still settles");

            let ix = doomed_chat(app);
            app.voice = VoiceState::NeedsModel { chat: 2 };
            app.delete_chat(ix, window, cx);
            assert!(matches!(app.voice, VoiceState::NeedsModel { .. }), "another chat's stays");
        })
        .unwrap();
}

#[gpui_kit::test]
fn archiving_a_chat_cancels_the_dictation_it_owns(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.voice = VoiceState::NeedsModel { chat: 2 };
            app.archive_chat(1, window, cx);
            assert!(matches!(app.voice, VoiceState::Idle));

            app.chats[1].archived = false;
            let cancel = Arc::new(AtomicBool::new(false));
            app.voice = VoiceState::Downloading { chat: 2, done: 0, cancel: cancel.clone() };
            app.archive_chat(1, window, cx);
            assert!(matches!(app.voice, VoiceState::Idle));
            assert!(cancel.load(Ordering::Relaxed));

            app.chats[1].archived = false;
            app.voice = VoiceState::Opening { chat: 2, started: Instant::now() };
            app.archive_chat(1, window, cx);
            assert!(matches!(app.voice, VoiceState::Idle));

            app.chats[1].archived = false;
            app.voice = VoiceState::Transcribing { chat: 2 };
            app.archive_chat(1, window, cx);
            assert!(matches!(app.voice, VoiceState::Transcribing { .. }), "it still delivers");
        })
        .unwrap();
}

#[gpui_kit::test]
fn sending_waits_for_dictation(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let chat = app.chats[app.active].id;
            let sent = app.chats[app.active].messages.len();
            app.composer.update(cx, |c, cx| c.set_value("hi", window, cx));
            app.voice = VoiceState::Transcribing { chat };
            assert!(app.voice_active());
            app.send(window, cx);
            assert_eq!(app.chats[app.active].messages.len(), sent);
            assert_eq!(app.composer.read(cx).value(), "hi");

            let cancel = Arc::new(AtomicBool::new(false));
            app.voice = VoiceState::Downloading { chat, done: 0, cancel };
            assert!(!app.voice_active(), "a download must not block sending");
            app.voice = VoiceState::NeedsModel { chat };
            assert!(!app.voice_active());
            app.voice = VoiceState::Transcribing { chat: 2 };
            assert!(!app.voice_active(), "dictation elsewhere must not block sending here");
            app.voice = VoiceState::Idle;
            assert!(!app.voice_active());
        })
        .unwrap();
}

#[gpui_kit::test]
fn a_note_clears_when_dictation_starts_over(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let chat = app.chats[app.active].id;
            app.voice_note = Some("old".into());
            app.voice = VoiceState::NeedsModel { chat };
            app.toggle_voice(window, cx);
            assert!(app.voice_note.is_none());
            assert!(matches!(app.voice, VoiceState::Idle));
        })
        .unwrap();
}

#[gpui_kit::test]
fn toggling_leaves_a_download_or_transcription_alone(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let chat = app.chats[app.active].id;
            app.voice = VoiceState::Transcribing { chat };
            app.toggle_voice(window, cx);
            assert!(matches!(app.voice, VoiceState::Transcribing { .. }));
            let cancel = Arc::new(AtomicBool::new(false));
            app.voice = VoiceState::Downloading { chat, done: 0, cancel };
            app.toggle_voice(window, cx);
            assert!(matches!(app.voice, VoiceState::Downloading { .. }));
        })
        .unwrap();
}

#[gpui_kit::test]
fn cancelling_ignores_the_download_it_abandoned(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, cx| {
            let chat = app.chats[app.active].id;
            let abandoned = Arc::new(AtomicBool::new(false));
            app.voice = VoiceState::Downloading { chat, done: 0, cancel: abandoned.clone() };
            app.cancel_voice(cx);
            assert!(abandoned.load(Ordering::Relaxed));
            assert!(matches!(app.voice, VoiceState::Idle));
            let cancelled = || Err(aether_voice::VoiceError::Download("download cancelled".into()));
            app.finish_download(&abandoned, cancelled(), cx);
            assert!(app.voice_note.is_none(), "a cancelled run shows no error");

            let current = Arc::new(AtomicBool::new(false));
            app.voice = VoiceState::Downloading { chat, done: 0, cancel: current.clone() };
            app.finish_download(&abandoned, cancelled(), cx);
            assert!(matches!(app.voice, VoiceState::Downloading { .. }));
            assert!(app.voice_note.is_none());

            app.finish_download(&current, Ok(Default::default()), cx);
            assert!(matches!(app.voice, VoiceState::Idle));
            assert_eq!(app.voice_note.clone().unwrap().as_str(), "Speech model ready");

            app.voice_note = None;
            app.voice = VoiceState::Downloading { chat, done: 0, cancel: current.clone() };
            let failed = aether_voice::VoiceError::Download("offline".into());
            let text = failed.to_string();
            app.finish_download(&current, Err(failed), cx);
            assert!(matches!(app.voice, VoiceState::Idle));
            assert_eq!(app.voice_note.clone().unwrap().as_str(), text);
        })
        .unwrap();
}

#[gpui_kit::test]
fn transcribing_cannot_be_cancelled(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, cx| {
            app.voice = VoiceState::Transcribing { chat: 2 };
            app.cancel_voice(cx);
            assert!(matches!(app.voice, VoiceState::Transcribing { .. }));
            app.voice = VoiceState::NeedsModel { chat: 2 };
            app.cancel_voice(cx);
            assert!(matches!(app.voice, VoiceState::Idle));
        })
        .unwrap();
}

#[gpui_kit::test]
fn the_recording_tick_ends_with_its_recording(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let started = std::time::Instant::now();
            assert!(!app.tick_recording(started, window, cx));
            app.voice = VoiceState::Transcribing { chat: 2 };
            assert!(!app.tick_recording(started, window, cx));
            assert!(matches!(app.voice, VoiceState::Transcribing { .. }));
        })
        .unwrap();
}

#[gpui_kit::test]
fn a_finished_dictation_delivers_or_notes_and_goes_idle(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    let chat = window.update(cx, |app, _, _| app.chats[app.active].id).unwrap();
    let settle = |outcome: Result<String, String>, cx: &mut TestAppContext| {
        window
            .update(cx, |app, window, cx| {
                app.voice = VoiceState::Transcribing { chat };
                app.settle_transcript(chat, gpui_kit::Task::ready(outcome), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
    };

    settle(Ok("hello".into()), cx);
    window
        .update(cx, |app, _, cx| {
            assert!(matches!(app.voice, VoiceState::Idle));
            assert_eq!(app.composer.read(cx).value(), "hello");
            assert_eq!(app.chats[app.active].draft, "hello");
            assert!(app.voice_note.is_none());
        })
        .unwrap();

    settle(Err("no good".into()), cx);
    window
        .update(cx, |app, _, cx| {
            assert!(matches!(app.voice, VoiceState::Idle));
            assert_eq!(app.composer.read(cx).value(), "hello");
            assert_eq!(app.voice_note.clone().unwrap().as_str(), "no good");
        })
        .unwrap();

    settle(Err("later".into()), cx);
    window
        .update(cx, |app, _, _| {
            assert_eq!(app.voice_note.clone().unwrap().as_str(), "no good", "device note stays");
        })
        .unwrap();
}

#[gpui_kit::test]
fn a_closed_window_cannot_strand_a_transcript(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    let app = window.root(cx).unwrap();
    let (finished, done) = futures::channel::oneshot::channel::<Result<String, String>>();
    window
        .update(cx, |app, window, cx| {
            app.voice = VoiceState::Transcribing { chat: 2 };
            let work = cx.background_spawn(async move { done.await.unwrap() });
            app.settle_transcript(2, work, window, cx);
            window.remove_window();
        })
        .unwrap();
    cx.run_until_parked();
    finished.send(Ok("hello".into())).unwrap();
    cx.run_until_parked();
    app.read_with(cx, |app, _| {
        assert!(matches!(app.voice, VoiceState::Idle));
        assert_eq!(app.chats[1].draft, "hello");
    });
}

#[gpui_kit::test]
fn opening_the_mic_blocks_sending_and_only_cancel_ends_it(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let chat = app.chats[app.active].id;
            let sent = app.chats[app.active].messages.len();
            app.composer.update(cx, |c, cx| c.set_value("hi", window, cx));
            app.voice = VoiceState::Opening { chat, started: Instant::now() };
            assert!(app.voice_active());
            assert!(!app.recording_voice(), "the red border waits for a live mic");
            app.send(window, cx);
            assert_eq!(app.chats[app.active].messages.len(), sent);
            assert_eq!(app.composer.read(cx).value(), "hi");

            app.toggle_voice(window, cx);
            assert!(matches!(app.voice, VoiceState::Opening { .. }));
            app.cancel_voice(cx);
            assert!(matches!(app.voice, VoiceState::Idle));

            app.voice = VoiceState::Opening { chat: 2, started: Instant::now() };
            assert!(!app.voice_active(), "another chat's mic must not block sending here");
            app.cancel_voice_of(chat, cx);
            assert!(matches!(app.voice, VoiceState::Opening { .. }));
            app.cancel_voice_of(2, cx);
            assert!(matches!(app.voice, VoiceState::Idle));
        })
        .unwrap();
}

#[gpui_kit::test]
fn a_mic_result_only_counts_while_its_opening_is_current(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let chat = app.chats[app.active].id;
            let started = Instant::now();
            let failed = || Err(aether_voice::VoiceError::NoMicrophone);

            app.finish_opening(started, failed(), window, cx);
            assert!(matches!(app.voice, VoiceState::Idle));
            assert!(app.voice_note.is_none(), "cancelled while opening: nothing to report");

            let newer = started + Duration::from_millis(1);
            app.voice = VoiceState::Opening { chat, started: newer };
            app.finish_opening(started, failed(), window, cx);
            assert!(matches!(app.voice, VoiceState::Opening { started, .. } if started == newer));
            assert!(app.voice_note.is_none());

            app.voice = VoiceState::Transcribing { chat };
            app.finish_opening(started, failed(), window, cx);
            assert!(matches!(app.voice, VoiceState::Transcribing { .. }));
            assert!(app.voice_note.is_none());

            app.voice = VoiceState::Opening { chat, started };
            app.finish_opening(started, failed(), window, cx);
            assert!(matches!(app.voice, VoiceState::Idle));
            assert_eq!(app.voice_note.clone().unwrap().as_str(), "No microphone found");
        })
        .unwrap();
}

#[gpui_kit::test]
fn a_closed_window_cannot_strand_an_opening_mic(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    let app = window.root(cx).unwrap();
    let (finished, done) = futures::channel::oneshot::channel();
    window
        .update(cx, |app, window, cx| {
            let started = Instant::now();
            app.voice = VoiceState::Opening { chat: 2, started };
            let opening = cx.background_spawn(async move { done.await.unwrap() });
            app.settle_opening(started, opening, window, cx);
            window.remove_window();
        })
        .unwrap();
    cx.run_until_parked();
    finished.send(Err(aether_voice::VoiceError::NoMicrophone)).ok();
    cx.run_until_parked();
    app.read_with(cx, |app, _| assert!(matches!(app.voice, VoiceState::Idle)));
}

#[gpui_kit::test]
fn leaving_the_chat_page_ends_a_mic_it_would_hide(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    let opening = |chat| VoiceState::Opening { chat, started: Instant::now() };
    window
        .update(cx, |app, _, cx| {
            app.voice = opening(2);
            app.open_page(Page::Settings, cx);
        })
        .unwrap();
    window.update(cx, |app, _, _| assert!(matches!(app.voice, VoiceState::Idle))).unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    window
        .update(cx, |app, _, cx| {
            app.voice = VoiceState::Downloading { chat: 2, done: 0, cancel: cancel.clone() };
            app.open_page(Page::Settings, cx);
        })
        .unwrap();
    window
        .update(cx, |app, _, cx| {
            assert!(matches!(app.voice, VoiceState::Downloading { .. }));
            assert!(!cancel.load(Ordering::Relaxed));
            app.voice = VoiceState::NeedsModel { chat: 2 };
            app.open_page(Page::Settings, cx);
        })
        .unwrap();
    window
        .update(cx, |app, _, cx| {
            assert!(matches!(app.voice, VoiceState::NeedsModel { .. }));
            app.voice = opening(2);
            app.open_page(Page::Chat, cx);
        })
        .unwrap();
    window
        .update(cx, |app, _, _| {
            assert!(matches!(app.voice, VoiceState::Opening { .. }), "chat shows the banner");
        })
        .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn opening_the_draft_editor_ends_a_mic_it_would_hide(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let cancel = Arc::new(AtomicBool::new(false));
            app.voice = VoiceState::Downloading { chat: 2, done: 0, cancel };
            app.open_draft_editor(window, cx);
            assert!(app.draft_editor.is_some());
            assert!(matches!(app.voice, VoiceState::Downloading { .. }));
            app.close_draft_editor(false, window, cx);

            app.voice = VoiceState::Opening { chat: 2, started: Instant::now() };
            app.open_draft_editor(window, cx);
            assert!(app.draft_editor.is_some());
            assert!(matches!(app.voice, VoiceState::Idle));
        })
        .unwrap();
}

#[gpui_kit::test]
fn opening_a_note_ends_a_mic_it_would_hide(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let dir = std::env::temp_dir().join(format!("aether-voice-note-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let note = dir.join("hidden.md");
    std::fs::write(&note, "# Hidden").unwrap();
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.settings.notes.directory = dir.display().to_string();
            app.notes.last_note_file = Some(dir.join("last-note"));
            app.voice = VoiceState::Opening { chat: 2, started: Instant::now() };
            app.open_note(note.clone(), Some(aether_core::NotesEditor::Aether), window, cx);
            assert!(matches!(app.voice, VoiceState::Idle));
        })
        .unwrap();
    cx.run_until_parked();
    std::fs::remove_dir_all(dir).unwrap();
}

#[gpui_kit::test]
fn the_banner_shows_in_every_chat_and_names_the_target(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, _| {
            let here = app.chats[app.active].id;
            let line = |app: &ChatApp| app.voice_line().map(|line| line.to_string());
            assert_eq!(line(app), None);

            app.voice = VoiceState::Opening { chat: 2, started: Instant::now() };
            assert_eq!(line(app).unwrap(), "Starting microphone… · goes to \"b\"");
            app.voice = VoiceState::Transcribing { chat: 2 };
            assert_eq!(line(app).unwrap(), "Transcribing… · goes to \"b\"");
            app.voice = VoiceState::Transcribing { chat: here };
            assert_eq!(line(app).unwrap(), "Transcribing…");
            app.voice = VoiceState::Transcribing { chat: 99 };
            assert_eq!(line(app).unwrap(), "Transcribing…", "its chat is gone: no one to name");

            app.voice = VoiceState::NeedsModel { chat: 2 };
            assert!(line(app).unwrap().ends_with("It runs on this device."));
            let cancel = Arc::new(AtomicBool::new(false));
            app.voice = VoiceState::Downloading { chat: 2, done: 0, cancel };
            assert_eq!(line(app).unwrap(), "Downloading speech model, 0%");

            app.voice = VoiceState::Idle;
            app.voice_note = Some("heads up".into());
            assert_eq!(line(app).unwrap(), "heads up");
            app.active = 1;
            assert_eq!(line(app).unwrap(), "heads up");
        })
        .unwrap();
}

#[gpui_kit::test]
fn the_banner_buttons_work_from_any_chat(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    let click = |button: &'static str, visual: &mut gpui_kit::VisualTestContext| {
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let bounds = visual.debug_bounds(button).unwrap_or_else(|| panic!("no {button}"));
        visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    };

    window
        .update(&mut visual, |app, _, _| {
            app.voice = VoiceState::Opening { chat: 2, started: Instant::now() };
        })
        .unwrap();
    click("voice-cancel", &mut visual);
    window
        .update(&mut visual, |app, _, _| {
            assert!(matches!(app.voice, VoiceState::Idle));
            app.voice_note = Some("heads up".into());
        })
        .unwrap();
    click("voice-dismiss", &mut visual);
    window.update(&mut visual, |app, _, _| assert!(app.voice_note.is_none())).unwrap();
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_bang_draft_is_command_mode_only_where_it_can_run(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            as_agents(app);
            let mut mode = |app: &mut ChatApp, draft: &str| {
                let draft = gpui_kit::SharedString::from(draft.to_string());
                app.composer.update(cx, |c, cx| c.set_value(draft, window, cx));
                app.shell_mode(cx)
            };
            assert!(mode(app, "!ls"));
            assert!(mode(app, "  !ls"));
            assert!(mode(app, "!"));
            assert!(!mode(app, "ls"));
            assert!(!mode(app, "fix this!"));
            app.editing = Some((app.chats[app.active].id, 0));
            assert!(!mode(app, "!ls"), "editing rewrites a message");
            app.editing = None;
            app.chats[app.active].provider = Provider::OpenRouter;
            assert!(!mode(app, "!ls"), "no terminal to run it in");
        })
        .unwrap();
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_bare_bang_waits_for_a_command_and_offers_no_suggestions(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            as_agents(app);
            let sent = app.chats[app.active].messages.len();
            app.composer.update(cx, |c, cx| c.set_value("!", window, cx));
            app.send(window, cx);
            assert_eq!(app.chats[app.active].messages.len(), sent);
            assert_eq!(app.composer.read(cx).value().as_str(), "!");
            app.composer.update(cx, |c, cx| c.set_value("!cat @", window, cx));
            app.update_suggestions(cx);
            assert!(app.suggestions.is_none());
        })
        .unwrap();
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_tool_tab_opens_in_the_agent_chats_terminal_only(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            app.open_tool_tab("cat", window, cx);
            assert!(app.terminals.is_empty(), "a plain chat has no terminal");
            as_agents(app);
            app.open_tool_tab("cat", window, cx);
            let id = app.chats[app.active].id;
            let terminal = app.terminals[&id].view.read(cx);
            assert!(terminal.command_running("cat", cx));
            assert!(terminal.open && terminal.contains_focused(window, cx));
        })
        .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn turning_voice_off_clears_its_prompt_and_keeps_the_mic_closed(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let chat = app.chats[app.active].id;
            app.voice = VoiceState::NeedsModel { chat };
            app.set_voice_enabled(false, cx);
            assert!(!app.settings.voice_enabled);
            assert!(matches!(app.voice, VoiceState::Idle));
            app.toggle_voice(window, cx);
            assert!(matches!(app.voice, VoiceState::Idle));
        })
        .unwrap();
}

#[gpui_kit::test]
fn hiding_a_peeked_terminal_leaves_the_page_alone(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    let view = window
        .update(cx, |app, window, cx| {
            as_agents(app);
            app.side_tab = SideTab::Explorer;
            app.watch_peek = Some(Watch::Terminal(2));
            app.drawer_for(1, window, cx).unwrap()
        })
        .unwrap();
    view.update(cx, |_, cx| cx.emit(TerminalEvent::Hide));
    window
        .update(cx, |app, _, _| {
            assert_eq!(app.watch_peek, None);
            assert_eq!(app.page, Page::Chat);
            assert_eq!(app.side_tab, SideTab::Explorer);
        })
        .unwrap();
}

#[gpui_kit::test]
fn composer_focus_in_a_chat_peek_keeps_the_side_tab(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            as_agents(app);
            app.side_tab = SideTab::Explorer;
            app.watch_peek = Some(Watch::Chat(2));
            app.ensure_session_ui(2, window, cx);
            app.in_session(2, cx, |app, cx| app.focus_composer(window, cx));
            assert_eq!(app.side_tab, SideTab::Explorer);
        })
        .unwrap();
}

#[gpui_kit::test]
fn background_monocle_tabs_are_off_screen(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, _| {
            app.page = Page::Chat;
            app.settings.tile_layout = TileLayout::Monocle;
            let first = app.chats[0].id;
            let shown = app.workspace.model.open(first, 0);
            app.workspace.model.open(2, 0);
            app.workspace.model.windows.get_mut(&0).unwrap().shown = Some(shown);
            assert!(app.session_visible(first));
            assert!(!app.session_visible(2));
        })
        .unwrap();
}

#[gpui_kit::test]
fn deleting_a_watched_chat_drops_it_and_closes_its_peek(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let first = app.chats[0].id;
            app.settings.watchlist = vec![Watch::Chat(first), Watch::Chat(2), Watch::Terminal(2)];
            app.watch_peek = Some(Watch::Terminal(2));
            app.delete_chat(1, window, cx);
            assert_eq!(app.settings.watchlist, vec![Watch::Chat(first)]);
            assert_eq!(app.watch_peek, None);
        })
        .unwrap();
}

#[gpui_kit::test]
fn prune_keeps_live_entries_in_order(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, cx| {
            let first = app.chats[0].id;
            let mut agent = chat_with("c", &[]);
            agent.id = 3;
            agent.provider = Provider::Claude;
            app.chats.push(agent);
            app.chats[1].archived = true;
            app.settings.watchlist = vec![
                Watch::Chat(3),
                Watch::Chat(2),
                Watch::Terminal(9),
                Watch::Chat(first),
                Watch::Terminal(2),
                Watch::Terminal(3),
            ];
            app.prune_watchlist(cx);
            assert_eq!(
                app.settings.watchlist,
                vec![Watch::Chat(3), Watch::Chat(first), Watch::Terminal(3)]
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn restore_reports_whether_the_chat_store_loaded(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, _| {
            assert!(!app.restore_chats());
            let dir = std::env::temp_dir().join(format!("aether-watch-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            app.store = Some(app.agent.open_store(&dir.join("aether.db")).unwrap());
            assert!(app.restore_chats());
            let _ = std::fs::remove_dir_all(&dir);
        })
        .unwrap();
}

#[gpui_kit::test]
fn open_on_a_detached_chat_leaves_the_main_page(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            let first = app.chats[0].id;
            app.workspace.model.open(2, 7);
            app.page = Page::Settings;
            app.jump_to_watched(Watch::Chat(2), window, cx);
            assert_eq!(app.page, Page::Settings);
            assert_eq!(app.chats[app.active].id, first);
        })
        .unwrap();
}

#[cfg(unix)]
#[gpui_kit::test]
fn open_on_a_watched_terminal_shows_it(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, window, cx| {
            as_agents(app);
            app.page = Page::Settings;
            app.jump_to_watched(Watch::Terminal(2), window, cx);
            assert_eq!(app.chats[app.active].id, 2);
            assert_eq!(app.page, Page::Chat);
            assert_eq!(app.side_tab, SideTab::Terminal);
            assert!(app.terminals[&2].view.read(cx).open);
        })
        .unwrap();
}

#[gpui_kit::test]
fn pop_out_gives_the_chat_its_own_window_and_leaves_main_alone(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    let main = window
        .update(cx, |app, window, cx| {
            let mut third = chat_with("c", &[]);
            third.id = 3;
            app.chats.push(third);
            app.workspace.model.open(3, 0);
            app.side_tab = SideTab::Explorer;
            assert_eq!(app.workspace.model.pane_for(2), None);
            let main = app.workspace.model.windows[&0].clone();
            app.watch_peek = Some(Watch::Chat(2));
            app.pop_out_watched(Watch::Chat(2), window, cx);
            assert_eq!(app.watch_peek, None);
            (main.panes(), main.focused)
        })
        .unwrap();
    cx.run_until_parked();
    let (pane, host) = window
        .update(cx, |app, window, cx| {
            let pane = app.workspace.model.pane_for(2).unwrap();
            let host = app.workspace.model.window_of(pane).unwrap();
            assert_ne!(host, 0);
            let layout = &app.workspace.model.windows[&0];
            assert_eq!((layout.panes(), layout.focused), main, "main layout and focus untouched");
            app.watch_peek = Some(Watch::Terminal(2));
            app.pop_out_watched(Watch::Terminal(2), window, cx);
            (pane, host)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |app, _, _| {
            assert_eq!(app.watch_peek, None);
            assert_eq!(app.workspace.model.window_of(pane), Some(host));
            assert_eq!(app.workspace.windows.len(), 2, "a detached chat reuses its window");
            assert_eq!(app.side_tab, SideTab::Explorer);
        })
        .unwrap();
}

#[test]
fn each_corner_maps_to_its_edges() {
    assert_eq!(fab_edges(WatchCorner::BottomRight), (false, false));
    assert_eq!(fab_edges(WatchCorner::BottomLeft), (true, false));
    assert_eq!(fab_edges(WatchCorner::TopRight), (false, true));
    assert_eq!(fab_edges(WatchCorner::TopLeft), (true, true));
}

#[gpui_kit::test]
fn fab_never_covers_sidebar_or_top_chrome(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, cx| {
            as_agents(app);
            assert!(app.side_open() && app.side_full);
            app.settings.sidebar_collapsed = false;
            app.settings.sidebar_width = 300.;
            let margin = px(12.);
            let bar = px(crate::app::status_bar::STATUS_BAR);
            let wide = px(3000.);
            app.settings.watch_corner = WatchCorner::BottomRight;
            assert_eq!(app.fab_inset(wide, cx), (margin, bar + margin));
            app.settings.watch_corner = WatchCorner::BottomLeft;
            assert_eq!(app.fab_inset(wide, cx), (px(300.) + margin, bar + margin));
            app.settings.sidebar_collapsed = true;
            assert_eq!(app.fab_inset(wide, cx).0, margin);
            app.settings.sidebar_collapsed = false;
            app.settings.watch_corner = WatchCorner::TopLeft;
            assert_eq!(app.fab_inset(wide, cx), (px(300.) + margin, px(38.) + margin));
            app.settings.watch_corner = WatchCorner::TopRight;
            assert_eq!(app.fab_inset(px(900.), cx), (margin, px(38.) + margin));
            for corner in [WatchCorner::BottomLeft, WatchCorner::BottomRight] {
                app.settings.watch_corner = corner;
                assert!(app.fab_inset(px(900.), cx).1 > bar + margin);
            }
        })
        .unwrap();
}

#[test]
fn clamp_peek_keeps_the_card_inside_and_above_min_size() {
    let (area, min) = ([0., 0., 1000., 700.], (320., 240.));
    assert_eq!(clamp_peek([100., 100., 400., 300.], area, min), [100., 100., 400., 300.]);
    assert_eq!(clamp_peek([-50., -50., 3000., 2000.], area, min), [0., 0., 1000., 700.]);
    assert_eq!(clamp_peek([900., 650., 400., 300.], area, min), [600., 400., 400., 300.]);
    assert_eq!(clamp_peek([-500., -500., 400., 300.], area, min), [0., 0., 400., 300.]);
    assert_eq!(clamp_peek([10., 10., 50., 20.], area, min), [10., 10., 320., 240.]);
    assert_eq!(clamp_peek([f32::NAN, 10., f32::NAN, 300.], area, min), [0., 10., 320., 300.]);
    let below_chrome = [0., 40., 1000., 660.];
    assert_eq!(clamp_peek([100., -20., 400., 300.], below_chrome, min), [100., 40., 400., 300.]);
    assert_eq!(clamp_peek([0., 0., 3000., 2000.], below_chrome, min), [0., 40., 1000., 660.]);
}

#[test]
fn edge_resize_holds_the_far_edge_and_the_min_size() {
    let (area, start) = ([0., 0., 1000., 700.], [100., 100., 400., 300.]);
    let left = [true, false, false, false];
    assert_eq!(drag_peek(start, left, (300., 0.), area), [180., 100., 320., 300.]);
    assert_eq!(drag_peek(start, left, (-300., 0.), area), [0., 100., 500., 300.]);
    let top = [false, true, false, false];
    assert_eq!(drag_peek(start, top, (0., 200.), area), [100., 160., 400., 240.]);
    let bottom_right = [false, false, true, true];
    assert_eq!(drag_peek(start, bottom_right, (900., 900.), area), [100., 100., 900., 600.]);
    assert_eq!(drag_peek(start, [true; 4], (2000., -50.), area), [600., 50., 400., 300.]);
    let below_chrome = [0., 40., 1000., 660.];
    assert_eq!(drag_peek(start, top, (0., -500.), below_chrome), [100., 40., 400., 360.]);
    assert_eq!(drag_peek(start, [true; 4], (0., -500.), below_chrome), [100., 40., 400., 300.]);
}

#[gpui_kit::test]
fn peek_drag_and_resize_save_bounds_on_release(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton, Pixels, Point, point};
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, _| {
            assert!(app.side_full && app.terminal_tab);
            app.watch_peek = Some(Watch::Chat(2));
        })
        .unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    let none = Modifiers::default();
    // the first move past gpui's drag threshold only picks the card up
    let drag = |visual: &mut gpui_kit::VisualTestContext, from: Point<Pixels>, by| {
        let start = from + point(px(3.), px(0.));
        visual.simulate_mouse_move(from, None, none);
        visual.simulate_mouse_down(from, MouseButton::Left, none);
        visual.simulate_mouse_move(start, Some(MouseButton::Left), none);
        visual.simulate_mouse_move(start + by, Some(MouseButton::Left), none);
        start + by
    };
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let card = visual.debug_bounds("watch-peek").unwrap();
    let header = visual.debug_bounds("watch-peek-header").unwrap();
    let grab = point(header.left() + px(40.), header.center().y);
    let to = drag(&mut visual, grab, point(px(-100.), px(-50.)));
    visual.read(|cx| {
        let app = app.read(cx);
        assert!(app.watch_peek_live.is_some());
        assert_eq!(app.settings.watch_peek_bounds, None, "nothing saved mid-drag");
    });
    visual.simulate_mouse_up(to, MouseButton::Left, none);
    let [x, y, w, h] = [card.left(), card.top(), card.size.width, card.size.height].map(f32::from);
    visual.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.watch_peek_live, None);
        assert_eq!(app.settings.watch_peek_bounds, Some([x - 100., y - 50., w, h]));
    });

    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let card = visual.debug_bounds("watch-peek").unwrap();
    let edge = point(card.left() + px(2.), card.center().y);
    let to = drag(&mut visual, edge, point(px(-60.), px(0.)));
    visual.simulate_mouse_up(to, MouseButton::Left, none);
    visual.read(|cx| {
        let saved = app.read(cx).settings.watch_peek_bounds;
        assert_eq!(saved, Some([x - 160., y - 50., w + 60., h]));
    });

    visual.update(|window, cx| {
        app.update(cx, |app, _| app.settings.watch_peek_bounds = Some([0., 9000., 400., 300.]));
        let _ = window.draw(cx);
    });
    let card = visual.debug_bounds("watch-peek").unwrap();
    let height = visual.update(|window, _| window.viewport_size().height);
    let bar = px(crate::app::status_bar::STATUS_BAR);
    assert_eq!(card.bottom(), height - bar, "card sits on the status bar, not over it");

    visual.update(|window, cx| {
        app.update(cx, |app, _| app.settings.watch_peek_bounds = Some([0., -500., 400., 300.]));
        let _ = window.draw(cx);
    });
    let card = visual.debug_bounds("watch-peek").unwrap();
    assert_eq!(card.top(), px(40.), "card stays below the top chrome");

    let close = visual.debug_bounds("watch-peek-close").unwrap();
    visual.simulate_click(close.center(), none);
    visual.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.watch_peek, None);
        assert_eq!(app.settings.watch_peek_bounds, Some([0., -500., 400., 300.]));
    });
}

#[gpui_kit::test]
fn top_chrome_counts_the_bars_actually_shown(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, cx| {
            as_agents(app);
            assert!(app.side_open() && app.side_full && app.terminal_tab);
            app.settings.tile_layout = TileLayout::Monocle;
            app.settings.watch_corner = WatchCorner::TopRight;
            assert_eq!(app.top_chrome(cx), px(38.));
            let first = app.workspace.model.open(app.chats[0].id, 0);
            app.workspace.model.open(2, 0);
            app.workspace.model.windows.get_mut(&0).unwrap().shown = Some(first);
            assert_eq!(app.top_chrome(cx), px(70.));
            assert_eq!(app.fab_inset(px(3000.), cx).1, px(70. + 12.));
            app.settings.tile_layout = TileLayout::Manual;
            assert_eq!(app.top_chrome(cx), px(38.));
            app.page = Page::Notes;
            assert_eq!(app.top_chrome(cx), px(40.));
        })
        .unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        app.update(cx, |app, _| {
            app.page = Page::Chat;
            app.settings.tile_layout = TileLayout::Monocle;
            let first = app.workspace.model.pane_for(app.chats[0].id).unwrap();
            app.workspace.model.focus(first);
            app.watch_peek = Some(Watch::Chat(2));
            app.settings.watch_peek_bounds = Some([0., -500., 400., 300.]);
        });
        let _ = window.draw(cx);
    });
    let card = visual.debug_bounds("watch-peek").unwrap();
    assert_eq!(card.top(), px(70.), "peek area clears the hoisted tabs and tab bar");
}

#[gpui_kit::test]
fn peek_paints_above_the_fab_it_overlaps(cx: &mut TestAppContext) {
    use gpui_kit::Modifiers;
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, _| {
            assert!(app.side_full && app.terminal_tab);
            app.watch_peek = Some(Watch::Chat(2));
        })
        .unwrap();
    window.update(cx, |app, _, _| app.settings.watch_corner = WatchCorner::TopLeft).unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let fab = visual.debug_bounds("watch-fab").unwrap();
    let card = visual.debug_bounds("watch-peek").unwrap();
    let open = visual.debug_bounds("watch-peek-open").unwrap();
    visual.update(|window, cx| {
        let at = card.origin + fab.center() - open.center();
        app.update(cx, |app, _| {
            app.settings.watch_peek_bounds = Some([f32::from(at.x), f32::from(at.y), 400., 300.]);
        });
        let _ = window.draw(cx);
    });
    let open = visual.debug_bounds("watch-peek-open").unwrap();
    let fab = visual.debug_bounds("watch-fab").unwrap();
    let overlap = open.intersect(&fab);
    assert!(overlap.size.width > px(0.) && overlap.size.height > px(0.), "open sits on the fab");
    visual.simulate_click(overlap.center(), Modifiers::default());
    visual.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.watch_peek, None);
        assert_eq!(app.chats[app.active].id, 2, "the peek's button took the click");
    });
}

#[gpui_kit::test]
fn closing_a_peek_drops_stale_drag_bounds(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, cx| {
            app.watch_peek = Some(Watch::Chat(2));
            app.watch_peek_live = Some([1., 2., 3., 4.]);
            app.close_peek(cx);
            assert_eq!(app.watch_peek_live, None);
            app.watch_peek_live = Some([1., 2., 3., 4.]);
            app.watch_peek = Some(Watch::Chat(99));
            app.settle_peek();
            assert_eq!(app.watch_peek_live, None);
        })
        .unwrap();
}

#[gpui_kit::test]
fn changing_the_corner_forgets_saved_peek_bounds(cx: &mut TestAppContext) {
    let window = app_with_two_chats(cx);
    window
        .update(cx, |app, _, cx| {
            app.settings.watch_peek_bounds = Some([1., 2., 3., 4.]);
            app.set_watch_corner(WatchCorner::TopLeft, cx);
            assert_eq!(app.settings.watch_corner, WatchCorner::TopLeft);
            assert_eq!(app.settings.watch_peek_bounds, None);
        })
        .unwrap();
}

fn pop_out_chat_two(
    cx: &mut TestAppContext,
    window: &gpui_kit::WindowHandle<ChatApp>,
    item: Watch,
) -> u64 {
    window
        .update(cx, |app, window, cx| {
            as_agents(app);
            app.pop_out_watched(item, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |app, _, _| {
            let pane = app.workspace.model.pane_for(2).unwrap();
            app.workspace.model.window_of(pane).unwrap()
        })
        .unwrap()
}

#[gpui_kit::test]
fn side_tabs_are_per_window(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    let host = pop_out_chat_two(cx, &window, Watch::Chat(2));
    let app = window.root(cx).unwrap();
    let detached = app.read_with(cx, |app, _| app.workspace.windows[&host]);
    detached
        .update(cx, |_, window, cx| {
            app.update(cx, |app, cx| app.select_tab(SideTab::Explorer, window, cx))
        })
        .unwrap();
    app.read_with(cx, |app, _| {
        assert_eq!(app.side_tab_for(host), SideTab::Explorer);
        assert_eq!(app.side_tab, SideTab::Agent);
    });
    window.update(cx, |app, window, cx| app.select_tab(SideTab::Terminal, window, cx)).unwrap();
    app.read_with(cx, |app, _| {
        assert_eq!(app.side_tab, SideTab::Terminal);
        assert_eq!(app.side_tab_for(host), SideTab::Explorer);
    });
}

#[gpui_kit::test]
fn popping_out_a_terminal_opens_its_window_on_terminal(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    window.update(cx, |app, _, _| app.side_tab = SideTab::Explorer).unwrap();
    let host = pop_out_chat_two(cx, &window, Watch::Terminal(2));
    window
        .update(cx, |app, _, _| {
            assert!(app.side_full && app.terminal_tab);
            assert_eq!(app.side_tab_for(host), SideTab::Terminal);
            assert_eq!(app.side_tab, SideTab::Explorer);
        })
        .unwrap();
}

#[gpui_kit::test]
fn changes_view_and_draft_editor_answer_per_window(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    let host = pop_out_chat_two(cx, &window, Watch::Chat(2));
    window
        .update(cx, |app, _, cx| {
            app.explorer_view = crate::explorer::ExplorerView::Changes;
            app.side_tab = SideTab::Explorer;
            assert!(app.changes_view_shown(0) && !app.changes_view_shown(host));
            assert!(!app.draft_editor_visible(0, cx) && app.draft_editor_visible(host, cx));
            app.side_tab = SideTab::Agent;
            app.set_side_tab(host, SideTab::Explorer);
            assert!(!app.changes_view_shown(0) && app.changes_view_shown(host));
            assert!(app.draft_editor_visible(0, cx) && !app.draft_editor_visible(host, cx));
        })
        .unwrap();
}

#[gpui_kit::test]
fn a_window_off_browser_does_not_hide_the_browser_another_shows(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    let host = pop_out_chat_two(cx, &window, Watch::Chat(2));
    let app = window.root(cx).unwrap();
    let detached = app.read_with(cx, |app, _| app.workspace.windows[&host]);
    window
        .update(cx, |app, window, cx| {
            app.browser_open = true;
            app.side_tab = SideTab::Browser;
            app.side_panel(None, window, cx);
            assert!(app.browser.read(cx).native_visible(cx));
        })
        .unwrap();
    detached
        .update(cx, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.side_panel(None, window, cx);
                assert!(
                    app.browser.read(cx).native_visible(cx),
                    "detached render hid main's browser"
                );
            })
        })
        .unwrap();
}

#[gpui_kit::test]
fn closing_a_detached_browser_window_lets_main_hide_the_browser(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    let host = pop_out_chat_two(cx, &window, Watch::Chat(2));
    window
        .update(cx, |app, window, cx| {
            app.browser_open = true;
            app.set_side_tab(host, SideTab::Browser);
            app.side_tab = SideTab::Browser;
            app.side_panel(None, window, cx);
            assert!(app.browser.read(cx).native_visible(cx));
            app.side_tab = SideTab::Agent;
            app.return_session_window(host, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |app, window, cx| {
            assert!(!app.side_tabs.contains_key(&host));
            app.side_panel(None, window, cx);
            assert!(!app.browser.read(cx).native_visible(cx), "stale host kept the browser shown");
        })
        .unwrap();
}

fn watch_window(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<ChatApp> {
    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
    cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        app.settings.watchlist = vec![Watch::Chat(app.chats[app.active].id)];
        app
    })
}

#[gpui_kit::test]
fn cmd_shift_w_toggles_the_watch_list(cx: &mut TestAppContext) {
    let window = watch_window(cx);
    let focus = window.update(cx, |app, _, _| app.focus.clone()).unwrap();
    cx.update_window(*window, |_, window, cx| {
        focus.focus(window, cx);
        let _ = window.draw(cx);
    })
    .unwrap();
    cx.simulate_keystrokes(*window, "secondary-shift-w");
    window.update(cx, |app, _, _| assert!(app.watch_open)).unwrap();
    cx.simulate_keystrokes(*window, "secondary-shift-w");
    window.update(cx, |app, _, _| assert!(!app.watch_open)).unwrap();
}

#[gpui_kit::test]
fn clicking_outside_the_list_closes_it_but_the_fab_still_toggles(cx: &mut TestAppContext) {
    let window = watch_window(cx);
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    let open = |visual: &mut gpui_kit::VisualTestContext| {
        visual.update(|window, cx| {
            app.update(cx, |app, _| app.watch_open = true);
            let _ = window.draw(cx);
        })
    };
    let is_open =
        |visual: &mut gpui_kit::VisualTestContext| visual.read(|cx| app.read(cx).watch_open);

    open(&mut visual);
    visual.simulate_click(gpui_kit::point(px(200.), px(200.)), gpui_kit::Modifiers::default());
    assert!(!is_open(&mut visual), "a click outside closes the list");

    open(&mut visual);
    let fab = visual.debug_bounds("watch-fab").unwrap();
    visual.simulate_click(fab.center(), gpui_kit::Modifiers::default());
    assert!(!is_open(&mut visual), "the FAB closes an open list");
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    visual.simulate_click(fab.center(), gpui_kit::Modifiers::default());
    assert!(is_open(&mut visual), "the FAB opens a closed list");
}

#[gpui_kit::test]
fn turning_watch_off_removes_it_all_and_keeps_the_list(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let window = app_with_two_chats(cx);
    cx.update(crate::actions::bind_keys);
    let drawer = window
        .update(cx, |app, window, cx| {
            app.settings.workspace_root = std::env::temp_dir().to_string_lossy().into();
            app.chats[app.active].provider = Provider::Claude;
            app.settings.watchlist = vec![Watch::Chat(2)];
            app.watch_open = true;
            app.watch_peek = Some(Watch::Chat(2));
            app.drawer_for(app.active, window, cx).unwrap()
        })
        .unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    let fab = |visual: &mut gpui_kit::VisualTestContext| {
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        visual.debug_bounds("watch-fab").is_some()
    };
    assert!(fab(&mut visual));
    visual.read(|cx| assert_eq!(drawer.read(cx).watched, Some(false)));

    visual.update(|_, cx| app.update(cx, |app, cx| app.set_watch_enabled(false, cx)));
    assert!(!fab(&mut visual));
    visual.dispatch_action(crate::actions::ToggleWatchList);
    visual.read(|cx| {
        let app = app.read(cx);
        assert!(!app.watch_open, "neither closing nor the shortcut leaves the list open");
        assert_eq!(app.watch_peek, None);
        assert_eq!(app.settings.watchlist, [Watch::Chat(2)], "the list waits for turning back on");
        assert!(!app.browser.read(cx).covered);
        assert_eq!(drawer.read(cx).watched, None, "no Watch in the terminal menu");
    });

    visual.update(|_, cx| app.update(cx, |app, cx| app.set_watch_enabled(true, cx)));
    assert!(fab(&mut visual));
    visual.read(|cx| assert_eq!(drawer.read(cx).watched, Some(false)));
    visual.dispatch_action(crate::actions::ToggleWatchList);
    visual.read(|cx| assert!(app.read(cx).watch_open, "the shortcut works again"));
}
