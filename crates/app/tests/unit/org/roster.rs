use super::*;

#[test]
fn roster_keys_clamp_at_boundaries_and_only_activate_on_enter_or_space() {
    assert_eq!(row_key("up", 0, 3), Some(RowKey::Focus(0)));
    assert_eq!(row_key("down", 2, 3), Some(RowKey::Focus(2)));
    assert_eq!(row_key("down", 0, 3), Some(RowKey::Focus(1)));
    assert_eq!(row_key("up", 2, 3), Some(RowKey::Focus(1)));
    assert_eq!(row_key("down", 0, 1), Some(RowKey::Focus(0)));
    for key in ["enter", "space"] {
        assert_eq!(row_key(key, 1, 3), Some(RowKey::Select));
    }
    for key in ["tab", "escape", "left", "right", "a"] {
        assert_eq!(row_key(key, 1, 3), None);
    }
}

#[test]
fn roster_cells_export_stable_ids_and_column_semantics() {
    use gpui_kit::Element;
    for column in 0..8 {
        let cell = semantic_cell(column);
        assert_eq!(Element::id(&cell), Some(("cell", column).into()));
        assert_eq!(cell.a11y_role(), Some(Role::Cell));
        let mut node = gpui_kit::accesskit::Node::new(Role::Cell);
        cell.write_a11y_info(&mut node);
        assert_eq!(node.column_index(), Some(column));
    }
}

fn assert_cell_content(cells: &[Stateful<Div>; 8], expected: &[(&str, &str); 8]) {
    use gpui_kit::Element;
    for (column, (cell, (label, value))) in cells.iter().zip(expected).enumerate() {
        assert_eq!(Element::id(cell), Some(("cell", column).into()));
        assert_eq!(cell.a11y_role(), Some(Role::Cell));
        let mut node = gpui_kit::accesskit::Node::new(Role::Cell);
        cell.write_a11y_info(&mut node);
        assert_eq!(node.label(), Some(*label), "column {column} name");
        assert_eq!(node.value(), Some(*value), "column {column} value");
        assert_eq!(node.column_index(), Some(column));
    }
}

#[test]
fn roster_content_handles_empty_cards_and_unknown_model_without_empty_names() {
    assert_eq!(cards_label(&[]), "none");
    assert_eq!(cards_label(&[107, 108]), "#107, #108");
    assert_eq!(
        identity_label("Custom worker", "worker", "OpenRouter", ""),
        "Custom worker; role worker; provider OpenRouter; model not set"
    );
}

fn assert_activity_excerpts_from_production_builders(app: &mut ChatApp, cx: &mut Context<ChatApp>) {
    use gpui_kit::Element;
    let head = app.org_current().unwrap().head("ENG").unwrap().clone();
    let worker_ix = app.chat_ix(17).unwrap();
    let original_worker_messages = std::mem::take(&mut app.chats[worker_ix].messages);
    let mut head_chat = crate::app::chat_with("Activity head", &[]);
    head_chat.id = head.chat.unwrap();
    let head_ix = app.chats.len();
    app.chats.push(head_chat);
    let short = "Reviewing 漢字 🦀".to_string();
    let boundary = "界".repeat(182);
    let long = "解析🦀é".repeat(80);
    let truncated = format!("{}解…", "解析🦀é".repeat(45));
    for (activity, expected) in [(&short, &short), (&boundary, &boundary), (&long, &truncated)] {
        // Preserve the existing first non-empty line behavior and full visible activity.
        let reply = format!("\n  \n  {activity}  \nSecond line stays excluded");
        app.chats[head_ix].messages =
            crate::app::chat_with("Activity head", &["request", &reply]).messages;
        app.chats[worker_ix].messages =
            crate::app::chat_with("Activity worker", &["request", &reply]).messages;
        assert_eq!(app.org_head_doing(&head), *activity);
        assert_eq!(app.org_chat_doing(&app.chats[worker_ix]), *activity);
        let head_cells = app.roster_head_cells(&head, app.org_head_status(&head), cx);
        let worker = &app.chats[worker_ix];
        let worker_cells = app.roster_worker_cells(worker, worker.status(now_ms()), cx);
        for cells in [&head_cells, &worker_cells] {
            let mut node = gpui_kit::accesskit::Node::new(Role::Cell);
            cells[2].write_a11y_info(&mut node);
            assert_eq!(node.label(), Some(format!("Current activity: {expected}").as_str()));
            assert_eq!(node.value(), Some(expected.as_str()));
            assert!(node.label().unwrap().chars().count() <= 200);
            assert!(node.value().unwrap().chars().count() <= 200);
            assert_eq!(Element::id(&cells[2]), Some(("cell", 2usize).into()));
        }
    }
    app.chats.pop();
    app.chats[worker_ix].messages = original_worker_messages;
}

fn press(visual: &mut gpui_kit::VisualTestContext, keys: &str) {
    for key in keys.split_whitespace() {
        let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
        visual.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        visual.simulate_event(gpui_kit::KeyUpEvent { keystroke });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }
}

#[gpui_kit::test]
fn roster_keyboard_moves_between_head_and_worker_without_activating_nested_controls(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::app::Page;
    use gpui_kit::{Modifiers, VisualTestContext, point};

    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
    cx.background_executor.allow_parking();
    let (window, app) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| {
                let mut app = ChatApp::with_state(
                    aether_agent::Agent::new().unwrap(),
                    aether_core::Settings {
                        org_enabled: true,
                        provider: Some(aether_core::Provider::OpenRouter),
                        workspace_root: std::env::temp_dir().to_string_lossy().into_owned(),
                        ..Default::default()
                    },
                    None,
                    None,
                    false,
                    window,
                    cx,
                );
                let worker = &mut app.chats[app.active];
                worker.id = 17;
                worker.parent = Some(99);
                worker.title = "Parser worker".into();
                worker.provider = aether_core::Provider::OpenRouter;
                worker.provider_model = "fixture-worker-model".into();
                worker.messages.clear();
                let project = "/org-roster-test".to_string();
                let mut head = Head::new("ENG", 2);
                head.chat = Some(99);
                head.title = "Engineering lead".into();
                head.provider = aether_core::Provider::Claude;
                head.model = "sonnet".into();
                app.cards = vec![crate::board::model::BoardCard {
                    id: 321,
                    title: "Parser".into(),
                    body: String::new(),
                    col: crate::board::model::Col::Todo,
                    position: 0.,
                    chats: vec![99, 17],
                    project: project.clone(),
                    updated_at: 0,
                    by_agent: false,
                    labels: vec![],
                    blocked_by: vec![],
                    notes: vec![],
                    author: String::new(),
                }];
                app.org.orgs = vec![Org {
                    project: project.clone(),
                    heads: vec![head, Head::new("QA", 2)],
                    ..Default::default()
                }];
                app.org.project = Some(project);
                app.open_page(Page::Org, cx);
                app
            })
        })
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window, cx);
    visual.simulate_resize(gpui_kit::size(px(1900.), px(1000.)));
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    // Inspect production row/cell builders, including the content missing from the real AX tree.
    visual.update(|_, cx| {
        use gpui_kit::Element;
        app.update(cx, |app, cx| {
            assert_activity_excerpts_from_production_builders(app, cx);
            let head = app.org_current().unwrap().head("ENG").unwrap().clone();
            let rows = [
                RosterRow { selection: OrgSel::Head("ENG".into()), focus: cx.focus_handle(), scroll: ScrollHandle::new() },
                RosterRow { selection: OrgSel::Worker(17), focus: cx.focus_handle(), scroll: ScrollHandle::new() },
            ];
            app.org.selected = Some(OrgSel::Head("ENG".into()));
            let row = app.roster_head_row(&head, 0, &rows, cx);
            assert_eq!(row.a11y_role(), Some(Role::Row));
            let mut node = gpui_kit::accesskit::Node::new(Role::Row);
            row.write_a11y_info(&mut node);
            assert_eq!(node.author_id(), Some("org-head-ENG"));
            assert_eq!(node.label(), Some("Engineering lead; role ENG; provider Claude Code; model sonnet; status Idle"));
            assert_eq!(node.is_selected(), Some(true));
            assert_eq!(node.row_index(), Some(1));
            let cells = app.roster_head_cells(&head, app.org_head_status(&head), cx);
            assert_cell_content(&cells, &[
                ("Status: Idle", "Idle"),
                ("Who: Engineering lead; role ENG; provider Claude Code; model sonnet", "Engineering lead; role ENG; provider Claude Code; model sonnet"),
                ("Current activity: No chat yet", "No chat yet"),
                ("Cards: #321", "#321"),
                ("Team: 1 of 2 workers", "1 of 2 workers"),
                ("Spend today: $0.00", "$0.00"),
                ("Context usage: 0 percent", "0 percent"),
                ("Actions: Edit Engineering lead head", "Edit Engineering lead head"),
            ]);
            let mut context = gpui_kit::accesskit::Node::new(Role::Cell);
            cells[6].write_a11y_info(&mut context);
            assert_eq!(context.numeric_value(), Some(0.));
            assert_eq!(context.min_numeric_value(), Some(0.));
            assert_eq!(context.max_numeric_value(), Some(100.));
            let chat = app.chats.iter().find(|chat| chat.id == 17).unwrap();
            let row = app.roster_worker_row(chat, 1, &rows, cx);
            let mut node = gpui_kit::accesskit::Node::new(Role::Row);
            row.write_a11y_info(&mut node);
            assert_eq!(node.author_id(), Some("org-worker-17"));
            assert_eq!(node.label(), Some("Parser worker; role worker; provider OpenRouter; model fixture-worker-model; status Idle"));
            assert_eq!(node.is_selected(), Some(false));
            assert_eq!(node.row_index(), Some(2));
            assert_cell_content(&app.roster_worker_cells(chat, chat.status(now_ms()), cx), &[
                ("Status: Idle", "Idle"),
                ("Who: Parser worker; role worker; provider OpenRouter; model fixture-worker-model", "Parser worker; role worker; provider OpenRouter; model fixture-worker-model"),
                ("Current activity: Idle", "Idle"),
                ("Cards: #321", "#321"),
                ("Team: worker", "worker"),
                ("Spend today: $0.00", "$0.00"),
                ("Context usage: unavailable", "unavailable"),
                ("Actions: Stop turn unavailable", "Stop turn unavailable"),
            ]);
            let status = status_dot(aether_ui::ChatStatus::Waiting, cx).into_element();
            assert_eq!(status.a11y_role(), Some(Role::Image));
            let mut node = gpui_kit::accesskit::Node::new(Role::Image);
            status.write_a11y_info(&mut node);
            assert_eq!(node.label(), Some("Needs you"));
            app.org.selected = None;
        });
    });
    let head = visual.debug_bounds("org-head-ENG").unwrap();
    let worker = visual.debug_bounds("org-worker-17").unwrap();
    let qa = visual.debug_bounds("org-head-QA").unwrap();
    let assert_stable_geometry = |visual: &mut VisualTestContext| {
        assert_eq!(
            visual.debug_bounds("org-head-ENG"),
            Some(head),
            "head focus must not change geometry"
        );
        assert_eq!(
            visual.debug_bounds("org-worker-17"),
            Some(worker),
            "worker focus must not change geometry"
        );
        assert_eq!(
            visual.debug_bounds("org-head-QA"),
            Some(qa),
            "focus must not move neighboring rows"
        );
    };
    visual.simulate_click(point(head.left() + px(4.), head.center().y), Modifiers::default());
    press(&mut visual, "down enter");
    assert_stable_geometry(&mut visual);
    visual.read(|cx| assert_eq!(app.read(cx).org.selected, Some(OrgSel::Worker(17))));
    press(&mut visual, "down space");
    assert_stable_geometry(&mut visual);
    visual.read(|cx| assert_eq!(app.read(cx).org.selected, Some(OrgSel::Head("QA".into()))));
    press(&mut visual, "up up space");
    assert_stable_geometry(&mut visual);
    visual.read(|cx| assert_eq!(app.read(cx).org.selected, Some(OrgSel::Head("ENG".into()))));
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let stop = visual.debug_bounds("org-worker-stop-17").unwrap();
    assert!(stop.size.width >= px(24.) && stop.size.height >= px(24.));
    let menu = visual.debug_bounds("org-head-menu-ENG").unwrap();
    assert!(menu.size.width >= px(24.) && menu.size.height >= px(24.));
    // Exercise production Root traversal into the nested button and back to the row.
    let before = visual.update(|window, cx| window.focused(cx));
    press(&mut visual, "tab");
    let after_tab = visual.update(|window, cx| window.focused(cx));
    assert_ne!(before, after_tab, "Tab must move off the row");
    press(&mut visual, "shift-tab");
    let after_back = visual.update(|window, cx| window.focused(cx));
    assert_eq!(before, after_back, "Shift-Tab must return to the row");
    press(&mut visual, "down enter");
    assert_stable_geometry(&mut visual);
    visual.read(|cx| assert_eq!(app.read(cx).org.selected, Some(OrgSel::Worker(17))));
    press(&mut visual, "up space tab down enter");
    visual.read(|cx| {
        assert_eq!(app.read(cx).org.selected, Some(OrgSel::Head("ENG".into())));
        assert_eq!(app.read(cx).org.overlay, OrgOverlay::HeadEditor("ENG".into()));
    });
    for layout in [
        aether_core::OrgLayout::Roster,
        aether_core::OrgLayout::Chart,
        aether_core::OrgLayout::Table,
    ] {
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.org.overlay = OrgOverlay::None;
                app.settings.org_layout = layout;
                cx.notify();
            });
            let _ = window.draw(cx);
        });
        // Round Table uses the shared avatar, but renders its own seat status rings.
        if layout != aether_core::OrgLayout::Table {
            let status = visual.debug_bounds("org-status").expect("shared status icon is rendered");
            assert_eq!(status.size, gpui_kit::size(px(12.), px(12.)));
        }
    }
}
