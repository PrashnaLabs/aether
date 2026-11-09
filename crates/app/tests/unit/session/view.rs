use super::*;
use gpui_kit::TestAppContext;

#[gpui_kit::test]
fn workspace_popover_opens_in_the_tabbed_chat(cx: &mut TestAppContext) {
    use gpui_kit::VisualTestContext;
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
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
            app.settings.workspace_root = std::env::temp_dir().to_string_lossy().into_owned();
            app.listing_worktrees = true;
            app.chats[app.active].provider = aether_core::Provider::Codex;
            app.workspace_pop.machine = Some("Test machine".into());
            assert!(app.side_full && app.side_open());
            cx.notify();
        });
        let _ = window.draw(cx);
    })
    .unwrap();
    let mut visual = VisualTestContext::from_window(main, cx);
    for _ in 0..2 {
        let bounds = visual.read(|cx| app.read(cx).workspace_pop.trigger.get());
        assert!(bounds.size.width > gpui_kit::px(0.), "Workspace button must be rendered");
        visual.simulate_mouse_move(bounds.center(), None, gpui_kit::Modifiers::default());
        visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        visual.read(|cx| {
            assert!(
                app.read(cx).workspace_pop.open,
                "clicking the Workspace button must open the Workspace popover"
            )
        });
        visual.simulate_keystrokes("escape");
        visual.read(|cx| assert!(!app.read(cx).workspace_pop.open));
    }
}

#[gpui_kit::test]
fn sidebar_tab_shortcuts_cycle_while_composing_and_searching(cx: &mut TestAppContext) {
    use crate::app::ChatViewMode;
    use crate::side_panel::SideTab;
    use gpui_kit::VisualTestContext;
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
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
            app.settings.workspace_root = std::env::temp_dir().to_string_lossy().into_owned();
            app.listing_worktrees = true;
            let chat = &mut app.chats[app.active];
            chat.provider = aether_core::Provider::Codex;
            chat.view = ChatViewMode::Explorer;
            app.git.open.insert(chat.id);
            app.side_full = true;
            app.side_tab = SideTab::Agent;
            app.browser_open = true;
            app.diff_open = false;
            app.composer.update(cx, |input, cx| {
                input.set_value("Keep my draft", window, cx);
                input.focus(window, cx);
            });
            cx.notify();
        });
        let _ = window.draw(cx);
    })
    .unwrap();
    let mut visual = VisualTestContext::from_window(main, cx);
    let next = if cfg!(target_os = "macos") { "cmd-shift-l" } else { "ctrl-shift-l" };
    let previous = if cfg!(target_os = "macos") { "cmd-shift-h" } else { "ctrl-shift-h" };
    for expected in [SideTab::Terminal, SideTab::Explorer, SideTab::Browser, SideTab::Agent] {
        visual.simulate_keystrokes(next);
        visual.read(|cx| assert_eq!(app.read(cx).side_tab, expected));
    }
    visual.simulate_keystrokes(previous);
    visual.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.side_tab, SideTab::Browser);
        assert_eq!(app.composer.read(cx).value(), "Keep my draft");
    });
    visual.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.git.open.clear();
            cx.notify();
        })
    });
    visual.simulate_keystrokes(next);
    visual.read(|cx| {
        assert!(app.read(cx).side_open());
        assert_eq!(app.read(cx).side_tab, SideTab::Agent);
    });
}

#[gpui_kit::test]
fn slash_focuses_composer_and_remains_text_when_editing(cx: &mut TestAppContext) {
    use gpui_kit::{Focusable, VisualTestContext};
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
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
            window.focus(&app.focus, cx);
            app.chats[app.active].messages = vec![aether_core::Message::user("Earlier message")];
            app.folded = true;
            cx.notify();
        });
        let _ = window.draw(cx);
    })
    .unwrap();
    let mut visual = VisualTestContext::from_window(main, cx);
    visual.read(|cx| assert!(app.read(cx).folded, "composer starts collapsed"));
    visual.simulate_keystrokes("/");
    visual
        .update_window(main, |_, window, cx| {
            let app = app.read(cx);
            assert!(!app.folded, "the slash shortcut must expand a collapsed composer");
            assert!(app.composer.focus_handle(cx).is_focused(window));
            assert_eq!(app.composer.read(cx).value(), "");
        })
        .unwrap();
    visual.simulate_keystrokes("/");
    visual.read(|cx| assert_eq!(app.read(cx).composer.read(cx).value(), "/"));
    visual
        .update_window(main, |_, window, cx| {
            app.update(cx, |app, cx| app.toggle_palette(window, cx));
        })
        .unwrap();
    visual.simulate_keystrokes("/");
    visual
        .update_window(main, |_, window, cx| {
            let app = app.read(cx);
            assert!(app.palette_query.focus_handle(cx).is_focused(window));
            assert_eq!(app.palette_query.read(cx).value(), "/");
            assert_eq!(app.composer.read(cx).value(), "/");
        })
        .unwrap();
}

#[gpui_kit::test]
fn independent_composers_keep_drafts_when_focus_moves(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    window
        .update(cx, |app, window, cx| {
            let a = app.chats[app.active].id;
            app.composer.update(cx, |s, cx| s.set_value("alpha", window, cx));
            app.workspace.splitting = true;
            app.new_chat(window, cx);
            app.workspace.splitting = false;
            let b = app.chats[app.active].id;
            assert_ne!(a, b);
            app.composer.update(cx, |s, cx| s.set_value("beta", window, cx));
            app.in_session(a, cx, |app, cx| {
                assert_eq!(app.composer.read(cx).value(), "alpha");
                app.composer.update(cx, |s, cx| s.set_value("alpha edited", window, cx));
            });
            assert_eq!(app.composer.read(cx).value(), "beta");
            let snapshots = app.session_snapshots(cx);
            assert_eq!(snapshots[&a].text, "alpha edited");
            assert_eq!(snapshots[&b].text, "beta");
        })
        .unwrap();
}

#[gpui_kit::test]
fn transfer_rebuild_keeps_draft_selection_and_edit_stash(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    window
        .update(cx, |app, window, cx| {
            let id = app.chats[app.active].id;
            app.composer.update(cx, |s, cx| {
                s.set_value("alpha beta", window, cx);
                s.set_selected_range(2..5, cx);
            });
            app.chats[app.active].edit_stash = Some(("unsent draft".into(), vec![]));
            app.rebind_session_ui(id, window, cx);
            assert_eq!(app.composer.read(cx).value(), "alpha beta");
            assert_eq!(app.composer.read(cx).selected_range(), 2..5);
            assert_eq!(app.chats[app.active].edit_stash.as_ref().unwrap().0, "unsent draft");
        })
        .unwrap();
}

#[gpui_kit::test]
fn handle_ignores_deleted_or_rewound_chat(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    let mut handle = None;
    window
        .update(cx, |app, _, cx| {
            handle = Some(app.session_handle(cx));
            app.chats[app.active].epoch += 1;
        })
        .unwrap();
    let mut invoked = false;
    cx.update(|cx| {
        handle.as_ref().unwrap().update(cx, |_, _| {
            invoked = true;
        })
    });
    assert!(!invoked, "a stale callback must not reach a rewound session");
}

#[gpui_kit::test]
fn two_panes_render_with_distinct_composers(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (window, app) = cx.update(|cx| {
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
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let first = app.workspace.model.windows[&0].focused.unwrap();
            app.workspace.splitting = true;
            app.new_chat(window, cx);
            app.workspace.splitting = false;
            let second = app.chats[app.active].id;
            app.workspace
                .model
                .split(
                    first,
                    second,
                    crate::session_workspace::SplitAxis::Horizontal,
                    [1200., 700.],
                )
                .unwrap();
        })
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        let _ = window.draw(cx);
        let runtime = app.read(cx);
        assert_eq!(runtime.workspace.bounds.len(), 2);
        assert!(runtime.workspace.bounds.values().all(|size| size[0] > 0. && size[1] > 0.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn two_streams_and_scoped_stop_survive_pane_moves(cx: &mut TestAppContext) {
    use crate::session_workspace::{Rect, SplitAxis};
    use aether_core::{Role, Update};
    use futures::channel::mpsc;
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    let (a_tx, a_rx) = mpsc::unbounded();
    let (b_tx, b_rx) = mpsc::unbounded();
    let (a, b) = window
        .update(cx, |app, window, cx| {
            let a = app.chats[app.active].id;
            app.take_unprompted(
                app.active,
                aether_agent::Reply { updates: a_rx, cancel: aether_agent::Cancel::inert() },
                window,
                cx,
            );
            app.workspace.splitting = true;
            app.new_chat(window, cx);
            app.workspace.splitting = false;
            let b = app.chats[app.active].id;
            app.take_unprompted(
                app.active,
                aether_agent::Reply { updates: b_rx, cancel: aether_agent::Cancel::inert() },
                window,
                cx,
            );
            let pane = app.workspace.model.pane_for(a).unwrap();
            let other =
                app.workspace.model.split(pane, b, SplitAxis::Horizontal, [1200., 700.]).unwrap();
            app.workspace.model.float(other, Rect::new(20., 20., 400., 300.)).unwrap();
            (a, b)
        })
        .unwrap();
    a_tx.unbounded_send(Update::Delta("reply a".into())).unwrap();
    b_tx.unbounded_send(Update::Delta("reply b".into())).unwrap();
    cx.run_until_parked();
    window
        .update(cx, |app, window, cx| {
            assert!(app.chats.iter().all(|c| c.busy()));
            assert_eq!(app.chats.iter().find(|c| c.id == a).unwrap().messages[0].text, "reply a");
            assert_eq!(app.chats.iter().find(|c| c.id == b).unwrap().messages[0].text, "reply b");
            let pane = app.workspace.model.pane_for(a).unwrap();
            app.workspace.model.close(pane).unwrap();
            let b_ix = app.chats.iter().position(|c| c.id == b).unwrap();
            app.stop_chat(b_ix, cx);
            app.in_session(a, cx, |app, cx| {
                app.composer.update(cx, |s, cx| s.set_value("queued", window, cx));
                app.send(window, cx);
            });
            let chat = app.chats.iter().find(|c| c.id == a).unwrap();
            assert_eq!(chat.queue[0].text, "queued");
            assert!(chat.busy());
            assert!(!chat.stopped);
            assert!(app.chats.iter().find(|c| c.id == b).unwrap().stopped);
        })
        .unwrap();
    a_tx.unbounded_send(Update::Final(String::new())).unwrap();
    drop(a_tx);
    cx.run_until_parked();
    window
        .update(cx, |app, _, _| {
            let a = app.chats.iter().find(|c| c.id == a).unwrap();
            assert!(a.queue.is_empty());
            assert!(a.messages.iter().any(|m| m.role == Role::User && m.text == "queued"));
        })
        .unwrap();
    drop(b_tx);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn detach_return_and_reuse_preserve_draft_across_windows(cx: &mut TestAppContext) {
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
    let (id, pane) = cx
        .update_window(main, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.composer.update(cx, |s, cx| {
                    s.set_value("moving draft", window, cx);
                    s.set_selected_range(2..6, cx);
                });
                let id = app.chats[app.active].id;
                let pane = app.workspace.model.pane_for(id).unwrap();
                let mut other = crate::app::chat_with("other", &[]);
                other.id = id + 1;
                app.chats.push(other);
                app.workspace.model.open(id + 1, 0);
                app.detach_session(pane, window, cx);
                (id, pane)
            })
        })
        .unwrap();
    cx.run_until_parked();
    let host = cx.read(|cx| {
        let app = app.read(cx);
        let host = app.workspace.model.window_of(pane).unwrap();
        assert_ne!(host, 0);
        assert_eq!(app.session_snapshots(cx)[&id].text, "moving draft");
        assert_eq!(app.session_snapshots(cx)[&id].selection, 2..6);
        host
    });
    let detached = cx.read(|cx| app.read(cx).workspace.windows[&host]);
    cx.update_window(detached, |_, window, _| window.activate_window()).unwrap();
    cx.run_until_parked();
    cx.update_window(main, |_, window, cx| {
        app.update(cx, |app, cx| {
            assert!(
                app.session_is_watched(id, window, cx),
                "watching follows the destination window"
            );
            app.chats.iter_mut().find(|c| c.id == id).unwrap().unseen = aether_ui::ChatStatus::Done;
            let ix = app.chats.iter().position(|c| c.id == id).unwrap();
            app.select(ix, window, cx);
            assert_eq!(app.chats[ix].unseen, aether_ui::ChatStatus::Idle);
        });
    })
    .unwrap();
    cx.update_window(main, |_, window, cx| {
        app.update(cx, |app, cx| app.return_session(pane, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).workspace.model.window_of(pane), Some(0)));
    cx.update_window(main, |_, window, cx| {
        app.update(cx, |app, cx| app.detach_session(pane, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.workspace.model.window_of(pane), Some(host));
        assert_eq!(app.workspace.windows.len(), 2);
        assert_eq!(app.session_snapshots(cx)[&id].text, "moving draft");
    });
    cx.update_window(main, |_, _, cx| {
        app.update(cx, |app, cx| app.return_session_window(host, cx))
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).workspace.model.window_of(pane), Some(0)));
}

#[gpui_kit::test]
fn dragging_a_float_moves_without_resizing_or_jumping(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton, VisualTestContext, point, px};
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
    let pane = cx
        .update_window(main, |_, _, cx| {
            app.update(cx, |app, cx| {
                let pane = app.workspace.model.windows[&0].focused.unwrap();
                app.workspace
                    .model
                    .float(pane, crate::session_workspace::Rect::new(40., 40., 400., 400.))
                    .unwrap();
                cx.notify();
                pane
            })
        })
        .unwrap();
    cx.update_window(main, |_, window, cx| {
        let _ = window.draw(cx);
    })
    .unwrap();
    let bounds = cx.read(|cx| app.read(cx).workspace.pane_rects[&pane]);
    let start = point(px(bounds.x + 45.), px(bounds.y + 15.));
    let mut visual = VisualTestContext::from_window(main, cx);
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(
        start + point(px(10.), px(10.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    visual.simulate_mouse_move(
        start + point(px(60.), px(45.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    visual.simulate_mouse_up(
        start + point(px(60.), px(45.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    cx.read(|cx| {
        let moved = app.read(cx).workspace.model.windows[&0].floats[0].bounds;
        assert!(moved.x > 70. && moved.x < 130., "floating pane jumped: {moved:?}");
        assert!(moved.y > 50. && moved.y < 120., "floating pane jumped: {moved:?}");
        assert_eq!([moved.w, moved.h], [400., 400.]);
    });
    cx.update_window(main, |_, window, cx| {
        let _ = window.draw(cx);
    })
    .unwrap();
    let rect = cx.read(|cx| app.read(cx).workspace.pane_rects[&pane]);
    let grip = point(px(rect.x + rect.w - 6.), px(rect.y + rect.h - 6.));
    visual.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(
        grip + point(px(10.), px(10.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    visual.simulate_mouse_move(
        grip + point(px(60.), px(45.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    visual.simulate_mouse_up(
        grip + point(px(60.), px(45.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    cx.read(|cx| {
        let resized = app.read(cx).workspace.model.windows[&0].floats[0].bounds;
        assert!(resized.w > 430. && resized.w < 480., "bad resize: {resized:?}");
        assert!(resized.h > 420. && resized.h < 465., "bad resize: {resized:?}");
        assert!(resized.x > 70. && resized.x < 130.);
    });
}

#[gpui_kit::test]
fn maximizing_a_pane_uses_window_width_in_a_large_layout(cx: &mut TestAppContext) {
    use crate::session_workspace::{PaneAction, SplitAxis};
    use gpui_kit::{px, size};
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
    cx.simulate_window_resize(main, size(px(800.), px(700.)));
    let pane = cx
        .update_window(main, |_, window, cx| {
            app.update(cx, |app, cx| {
                let a = app.workspace.model.windows[&0].focused.unwrap();
                app.workspace.splitting = true;
                app.new_chat(window, cx);
                let b = app.chats[app.active].id;
                let bp =
                    app.workspace.model.split(a, b, SplitAxis::Horizontal, [1400., 700.]).unwrap();
                app.new_chat(window, cx);
                let c = app.chats[app.active].id;
                app.workspace.splitting = false;
                app.workspace.model.split(bp, c, SplitAxis::Horizontal, [800., 700.]).unwrap();
                app.run_session_pane_action(a, PaneAction::Maximize, window, cx);
                a
            })
        })
        .unwrap();
    cx.update_window(main, |_, window, cx| {
        let _ = window.draw(cx);
    })
    .unwrap();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.workspace.bounds[&pane][0] <= app.workspace.surface_sizes[&0][0] + 2.);
    });
}

#[gpui_kit::test]
fn reopening_hidden_session_rebinds_to_destination_window(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let main = cx.add_window(|window, cx| {
        ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    let (app, id, original) = main
        .update(cx, |app, window, cx| {
            app.composer.update(cx, |s, cx| s.set_value("hidden draft", window, cx));
            (cx.entity(), app.chats[app.active].id, app.composer.entity_id())
        })
        .unwrap();
    let other = cx.add_window(|_, _| gpui_kit::Empty);
    other
        .update(cx, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.ensure_session_ui(id, window, cx);
                assert_eq!(app.session_windows[&id], window.window_handle().window_id());
                app.in_session(id, cx, |app, cx| {
                    assert_ne!(app.composer.entity_id(), original);
                    assert_eq!(app.composer.read(cx).value(), "hidden draft");
                });
            });
        })
        .unwrap();
}

#[gpui_kit::test]
fn retained_stall_callback_resolves_chat_after_earlier_deletion(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    let (actions, app, id) = window
        .update(cx, |app, window, cx| {
            app.workspace.splitting = true;
            app.new_chat(window, cx);
            app.workspace.splitting = false;
            let id = app.chats[app.active].id;
            app.chats[app.active].turn_at = 123;
            let actions = app.stall_actions(&app.session_handle(cx));
            app.chats.remove(0);
            app.active = 0;
            (actions, cx.entity(), id)
        })
        .unwrap();
    cx.update_window(*window, |_, window, cx| {
        (actions.keep)(window, cx);
    })
    .unwrap();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.chats[0].id, id);
        assert_eq!(app.chats[0].stall_kept_ms, 123);
    });
}

#[gpui_kit::test]
fn mouse_suggestion_pick_stays_with_its_session(cx: &mut TestAppContext) {
    use crate::suggest::{Pick, Suggestions};
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            Agent::new().unwrap(),
            Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    let (pick, app, a, b) = window
        .update(cx, |app, window, cx| {
            let a = app.chats[app.active].id;
            app.workspace.splitting = true;
            app.new_chat(window, cx);
            app.workspace.splitting = false;
            let b = app.chats[app.active].id;
            app.composer.update(cx, |s, cx| {
                s.set_value("/", window, cx);
                s.set_selected_range(1..1, cx);
            });
            app.suggestions = Some(Suggestions {
                at: 0,
                trigger: '/',
                title: "Commands".into(),
                selected: 0,
                items: vec![("pick".into(), "".into(), Pick::Insert("chosen".into()))],
            });
            let pick = app.suggestion_pick_listener(cx);
            app.select(0, window, cx);
            (pick, cx.entity(), a, b)
        })
        .unwrap();
    cx.update_window(*window, |_, window, cx| pick(&0, window, cx)).unwrap();
    cx.read(|cx| {
        let snapshots = app.read(cx).session_snapshots(cx);
        assert_eq!(snapshots[&a].text, "");
        assert_eq!(snapshots[&b].text, "chosen");
    });
}

#[gpui_kit::test]
fn session_header_context_menu_pins_the_clicked_session(cx: &mut TestAppContext) {
    assert_session_menu(cx, SessionMenuTarget::Header);
}

#[gpui_kit::test]
fn session_body_context_menu_pins_the_clicked_session(cx: &mut TestAppContext) {
    assert_session_menu(cx, SessionMenuTarget::Body);
}

#[gpui_kit::test]
fn populated_chat_background_opens_the_session_menu(cx: &mut TestAppContext) {
    assert_session_menu(cx, SessionMenuTarget::TranscriptGap);
}

#[gpui_kit::test]
fn session_context_menu_yields_to_message_items(cx: &mut TestAppContext) {
    assert_session_menu(cx, SessionMenuTarget::Message);
}

#[gpui_kit::test]
fn session_context_menu_yields_to_the_composer(cx: &mut TestAppContext) {
    assert_session_menu(cx, SessionMenuTarget::Composer);
}

#[gpui_kit::test]
fn session_context_menu_ignores_header_buttons(cx: &mut TestAppContext) {
    assert_session_menu(cx, SessionMenuTarget::Control);
}

#[gpui_kit::test]
fn session_more_menu_keeps_layout_actions(cx: &mut TestAppContext) {
    assert_session_menu(cx, SessionMenuTarget::LayoutMenu);
}

#[derive(Clone, Copy)]
enum SessionMenuTarget {
    Header,
    Body,
    TranscriptGap,
    Message,
    Composer,
    Control,
    LayoutMenu,
}

fn assert_session_menu(cx: &mut TestAppContext, target: SessionMenuTarget) {
    use gpui_kit::{Modifiers, MouseButton, VisualTestContext, point, px};
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
    cx.simulate_window_resize(main, gpui_kit::size(px(1800.), px(1000.)));
    let (pane, a, b) = cx
        .update_window(main, |_, window, cx| {
            app.update(cx, |app, cx| {
                let a = app.chats[app.active].id;
                if matches!(
                    target,
                    SessionMenuTarget::TranscriptGap
                        | SessionMenuTarget::Message
                        | SessionMenuTarget::Composer
                ) {
                    let mut reply = if matches!(target, SessionMenuTarget::Message) {
                        aether_core::Message::user("A reply.")
                    } else {
                        aether_core::Message::assistant()
                    };
                    reply.text = "A reply.".into();
                    app.chats[app.active].messages = vec![reply];
                    app.sync_rows(None, cx);
                }
                let pane = app.workspace.model.pane_for(a).unwrap();
                app.workspace.splitting = true;
                app.new_chat(window, cx);
                app.workspace.splitting = false;
                let b = app.chats[app.active].id;
                app.workspace
                    .model
                    .split(pane, b, crate::session_workspace::SplitAxis::Horizontal, [1200., 700.])
                    .unwrap();
                // Exercise native-view visibility without creating a platform webview.
                app.browser.update(cx, |browser, _| browser.set_shown(true));
                (pane, a, b)
            })
        })
        .unwrap();
    cx.update_window(main, |_, window, cx| {
        let _ = window.draw(cx);
    })
    .unwrap();
    let rect = cx.read(|cx| app.read(cx).workspace.pane_rects[&pane]);
    cx.read(|cx| assert!(app.read(cx).browser.read(cx).native_visible(cx)));
    let mut visual = VisualTestContext::from_window(main, cx);
    let position = match target {
        SessionMenuTarget::Header => point(px(rect.x + 45.), px(rect.y + 15.)),
        SessionMenuTarget::Body => point(px(rect.x + 45.), px(rect.y + 100.)),
        SessionMenuTarget::TranscriptGap => point(px(rect.x + 45.), px(rect.y + 68.)),
        SessionMenuTarget::Message => point(px(rect.x + 45.), px(rect.y + 50.)),
        SessionMenuTarget::Composer => point(px(rect.x + 45.), px(rect.y + rect.h - 50.)),
        SessionMenuTarget::Control => point(px(rect.x + rect.w - 24.), px(rect.y + 15.)),
        SessionMenuTarget::LayoutMenu => point(px(rect.x + rect.w - 48.), px(rect.y + 15.)),
    };
    let button = if matches!(target, SessionMenuTarget::LayoutMenu) {
        MouseButton::Left
    } else {
        MouseButton::Right
    };
    visual.simulate_mouse_down(position, button, Modifiers::default());
    visual.simulate_mouse_up(position, button, Modifiers::default());
    cx.read(|cx| {
        let browser = app.read(cx).browser.read(cx);
        assert_eq!(
            browser.native_visible(cx),
            matches!(
                target,
                SessionMenuTarget::Control
                    | SessionMenuTarget::Message
                    | SessionMenuTarget::Composer
            ),
            "the native webview must yield to session menus"
        );
    });
    visual.simulate_keystrokes("escape");
    cx.read(|cx| {
        assert!(app.read(cx).browser.read(cx).native_visible(cx), "Escape must restore the page");
    });
    visual.simulate_mouse_down(position, button, Modifiers::default());
    visual.simulate_mouse_up(position, button, Modifiers::default());
    visual.simulate_keystrokes("down enter");
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.browser.read(cx).native_visible(cx), "a menu action must restore the page");
        let pins = matches!(
            target,
            SessionMenuTarget::Header | SessionMenuTarget::Body | SessionMenuTarget::TranscriptGap
        );
        assert_eq!(app.chats.iter().find(|c| c.id == a).unwrap().pinned, pins);
        if matches!(target, SessionMenuTarget::LayoutMenu) {
            assert_eq!(
                app.workspace.model.panes.len(),
                3,
                "the ellipsis menu should split the session"
            );
        }
        assert!(!app.chats.iter().find(|c| c.id == b).unwrap().pinned);
        if matches!(target, SessionMenuTarget::Message) {
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
                Some("A reply."),
                "the message's Copy menu must still work"
            );
        }
    });
}
