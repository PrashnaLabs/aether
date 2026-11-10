use aether_driver::ProviderProbe;

use super::*;

#[test]
fn provider_status_explains_each_probe_state() {
    assert_eq!(provider_status(None, ""), "Checking…");
    let found = ProviderProbe {
        installed: true,
        path: Some("/bin/pi".into()),
        models: vec![aether_driver::CliModel {
            id: "a/b".into(),
            name: "b".into(),
            provider: "A".into(),
            ..Default::default()
        }],
        ..ProviderProbe::default()
    };
    assert_eq!(provider_status(Some(&found), ""), "Found at /bin/pi · 1 model");
    let hub = ProviderProbe { installed: true, ..ProviderProbe::default() };
    assert_eq!(provider_status(Some(&hub), ""), "Running as a background service");
    let missing = ProviderProbe::default();
    assert_eq!(provider_status(Some(&missing), "~/bin/pi"), "Not found at ~/bin/pi");
    assert_eq!(provider_status(Some(&missing), ""), "Not found on PATH");
}

#[test]
fn settings_lists_all_terminal_themes() {
    assert_eq!(TerminalTheme::ALL.len(), 5);
    assert_eq!(TerminalTheme::ALL[0], TerminalTheme::AutoCatppuccin);
}

#[test]
fn settings_lists_all_terminal_font_sizes() {
    assert_eq!(Settings::TERMINAL_FONT_SIZES.len(), 6);
    assert_eq!(Settings::TERMINAL_FONT_SIZES[2], 13.0);
}

#[test]
fn worktree_rows_remove_or_prune_unless_main_locked_or_busy() {
    let user = Listed { path: "/w/review".into(), repo: "/code/app".into(), ..Listed::default() };
    assert_eq!(row_action(&user, false), ("Remove worktree…", None));
    assert_eq!(
        row_action(&Listed { managed: true, ..user.clone() }, false),
        ("Remove worktree…", None)
    );
    let gone = Listed { missing: true, ..user.clone() };
    assert_eq!(row_action(&gone, false), ("Prune worktree…", None));
    let (label, why) = row_action(&user, true);
    assert_eq!(label, "Remove worktree…");
    assert_eq!(why.as_deref(), Some("An agent is working here. Stop it to remove the worktree"));
    let main = Listed { main: true, ..user.clone() };
    assert_eq!(row_action(&main, false).1.as_deref(), Some("The main worktree can't be removed"));
    let locked = Listed { locked: Some("usb".into()), missing: true, ..user };
    assert_eq!(
        row_action(&locked, false),
        ("Remove worktree…", Some("Locked: usb. Unlock it in a terminal to remove it".into()))
    );
}

#[test]
fn settings_tabs_cycle_with_wrap_around() {
    use SettingsTab::*;
    assert_eq!(next_settings_tab(Appearance, false), Terminal);
    assert_eq!(next_settings_tab(Appearance, true), BottomBar);
    assert_eq!(next_settings_tab(Terminal, true), Appearance);
    assert_eq!(next_settings_tab(Worktrees, false), Providers);
}

fn settings_test_window(cx: &mut gpui_kit::TestAppContext) -> gpui_kit::WindowHandle<ChatApp> {
    settings_window_with(Settings::default(), cx)
}

fn settings_window_with(
    settings: Settings,
    cx: &mut gpui_kit::TestAppContext,
) -> gpui_kit::WindowHandle<ChatApp> {
    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
    cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            settings,
            None,
            None,
            false,
            window,
            cx,
        );
        app.open_page(crate::app::Page::Settings, cx);
        app
    })
}

#[gpui_kit::test]
fn settings_view_switches_between_category_and_search_results(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    cx.update_window(*window, |_, window, cx| {
        let _ = window.draw(cx);
    })
    .unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    assert!(visual.debug_bounds("theme").is_some());
    assert!(visual.debug_bounds("theme-card-nord").is_some());
    assert!(visual.debug_bounds("theme-card-catppuccin-frappe").is_some());
    assert!(visual.debug_bounds("icon-pack").is_some());
    assert!(visual.debug_bounds("corners").is_some());
    assert!(visual.debug_bounds("font").is_some());
    assert!(visual.debug_bounds("window-opacity").is_some());
    assert_eq!(visual.debug_bounds("window-blur").is_some(), cfg!(target_os = "macos"));
    assert!(visual.debug_bounds("notifications").is_some());
    let sidebar = visual.debug_bounds("settings-sidebar").unwrap();
    assert_eq!(sidebar.size.width, px(240.));
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.settings_search_input
                .update(cx, |input, cx| input.set_value("keychain", window, cx));
        });
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("api-key").is_some());
    assert!(visual.debug_bounds("theme").is_none());
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.settings_search_input
                .update(cx, |input, cx| input.set_value("nonexistentxyz123", window, cx));
        });
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("settings-empty").is_some());
    let clear = visual.debug_bounds("settings-empty-clear").unwrap();
    visual.simulate_click(clear.center(), gpui_kit::Modifiers::default());
    visual.read(|cx| {
        assert!(app.read(cx).settings_search_input.read(cx).value().is_empty());
    });
}

fn settle(visual: &mut gpui_kit::VisualTestContext) {
    for _ in 0..3 {
        visual.update(|window, cx| {
            window.simulate_next_frame(cx);
        });
    }
}

#[gpui_kit::test]
fn theme_card_click_applies_the_variant_it_shows(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    let card = visual.debug_bounds("theme-card-dark").unwrap();
    visual.simulate_click(card.center(), gpui_kit::Modifiers::default());
    visual.read(|cx| assert_eq!(app.read(cx).settings.appearance, Appearance::Dark));
    settle(&mut visual);
    let label = visual.debug_bounds("theme-card-light-label").unwrap();
    visual.simulate_click(label.center(), gpui_kit::Modifiers::default());
    visual.read(|cx| assert_eq!(app.read(cx).settings.appearance, Appearance::Light));
}

#[gpui_kit::test]
fn search_finds_the_new_appearance_rows(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    let mut rows = vec![
        ("mocha", "theme"),
        ("iconoir", "icon-pack"),
        ("radius", "corners"),
        ("typeface", "font"),
        ("translucent", "window-opacity"),
    ];
    if cfg!(target_os = "macos") {
        rows.push(("frosted", "window-blur"));
    }
    for (query, id) in rows {
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings_search_input
                    .update(cx, |input, cx| input.set_value(query, window, cx));
            });
            let _ = window.draw(cx);
        });
        assert!(visual.debug_bounds(id).is_some(), "{query} finds {id}");
        assert!(visual.debug_bounds("notifications").is_none(), "{query} filters");
    }
}

#[gpui_kit::test]
fn font_row_matches_the_font_size_row(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    let (font, size) =
        (visual.debug_bounds("font").unwrap(), visual.debug_bounds("font-size").unwrap());
    assert_eq!(font.size.height, size.size.height);
    visual.update(|window, cx| {
        let fill = Some(aether_ui::field_fill(cx));
        let scale = window.scale_factor();
        let trigger_in = |row: gpui_kit::Bounds<gpui_kit::Pixels>| {
            let row = row.scale(scale);
            window.painted_quads().iter().any(|q| {
                q.background.as_solid() == fill
                    && q.bounds.size.height.0 == 40. * scale
                    && q.bounds.top() >= row.top()
                    && q.bounds.bottom() <= row.bottom()
            })
        };
        assert!(trigger_in(size), "font size trigger");
        assert!(trigger_in(font), "font trigger");
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn terminal_settings_live_in_their_own_developer_tab(cx: &mut gpui_kit::TestAppContext) {
    assert_eq!(SettingsTab::ALL[9], SettingsTab::Terminal);
    let window = settings_test_window(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    assert!(visual.debug_bounds("terminal-theme").is_none());
    let app = window.root(&mut visual).unwrap();
    app.update(&mut visual, |app, cx| app.select_settings_tab(SettingsTab::Terminal, cx));
    settle(&mut visual);
    for id in ["terminal-theme", "terminal-font-size", "terminal-cursor", "terminal-meta"] {
        assert!(visual.debug_bounds(id).is_some(), "{id}");
    }
}

#[gpui_kit::test]
fn composer_settings_hold_the_voice_section(cx: &mut gpui_kit::TestAppContext) {
    assert_eq!(SettingsTab::ALL[3], SettingsTab::Composer);
    let window = settings_test_window(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    assert!(visual.debug_bounds("voice-enabled").is_none());
    let app = window.root(&mut visual).unwrap();
    app.update(&mut visual, |app, cx| app.select_settings_tab(SettingsTab::Composer, cx));
    settle(&mut visual);
    for id in ["voice-enabled", "voice-model"] {
        assert!(visual.debug_bounds(id).is_some(), "{id}");
    }
}

#[gpui_kit::test]
fn turning_notes_and_news_off_closes_every_way_in(cx: &mut gpui_kit::TestAppContext) {
    // reopening news starts a real story fetch whose tokio thread wakes the task
    cx.background_executor.allow_parking();
    let off = Settings { notes_enabled: false, hackernews_enabled: false, ..Settings::default() };
    let window = settings_window_with(off, cx);
    cx.update(crate::actions::bind_keys);
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    let page = visual.read(|cx| app.read(cx).page);
    visual.dispatch_action(crate::actions::OpenNotes);
    visual.dispatch_action(crate::actions::SearchNotes);
    visual.dispatch_action(crate::actions::ToggleHackerNews);
    visual.simulate_keystrokes("alt-n");
    visual.simulate_keystrokes("secondary-h");
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.notes_form(crate::notes::Form::Capture, "x".into(), window, cx)
        })
    });
    visual.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.page, page);
        assert!(!app.notes.search_ui.open);
    });

    app.update(&mut visual, |app, cx| app.select_settings_tab(SettingsTab::Tools, cx));
    settle(&mut visual);
    for id in ["notes-enabled", "hackernews-enabled"] {
        assert!(visual.debug_bounds(id).is_some(), "{id}: the way back on stays");
    }
    assert!(visual.debug_bounds("notes-directory").is_none());

    app.update(&mut visual, |app, cx| {
        app.update_settings(cx, |s| (s.notes_enabled, s.hackernews_enabled) = (true, true))
    });
    settle(&mut visual);
    assert!(visual.debug_bounds("notes-directory").is_none());
    let toggle = visual.debug_bounds("notes-advanced-toggle").unwrap();
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    settle(&mut visual);
    assert!(visual.debug_bounds("notes-directory").is_some());
    visual.dispatch_action(crate::actions::ToggleHackerNews);
    visual.read(|cx| assert_eq!(app.read(cx).page, crate::app::Page::HackerNews));
}

#[gpui_kit::test]
fn the_watch_button_position_only_shows_while_watching_is_on(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    assert!(visual.debug_bounds("watch-enabled").is_some());
    assert!(visual.debug_bounds("watch-corner").is_some());
    let app = window.root(&mut visual).unwrap();
    app.update(&mut visual, |app, cx| app.set_watch_enabled(false, cx));
    settle(&mut visual);
    assert!(visual.debug_bounds("watch-enabled").is_some(), "the way back on stays");
    assert!(visual.debug_bounds("watch-corner").is_none());
}

#[gpui_kit::test]
fn card_previews_round_their_fill(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    let cards = ["icon-pack-card-lucide", "corners-card-rounded"]
        .map(|id| visual.debug_bounds(id).unwrap());
    visual.update(|window, cx| {
        let fill = Some(cx.theme().accent);
        let scale = window.scale_factor();
        let quads = window.painted_quads();
        for card in cards {
            let card = card.scale(scale);
            let preview = quads
                .iter()
                .find(|q| q.background.as_solid() == fill && card.contains(&q.bounds.center()))
                .expect("preview fill");
            assert!(preview.corner_radii.top_left.0 > 0., "square fill pokes past the card");
        }
    });
}

#[gpui_kit::test]
fn card_sections_sit_in_a_bordered_box(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    let sections = ["theme", "icon-pack", "corners"].map(|id| visual.debug_bounds(id).unwrap());
    visual.update(|window, cx| {
        let border = cx.theme().border;
        let scale = window.scale_factor();
        let quads = window.painted_quads();
        for section in sections {
            let section = section.scale(scale);
            assert!(quads.iter().any(|q| {
                q.border_color == border
                    && q.border_widths.top.0 > 0.
                    && q.bounds.size.width == section.size.width
                    && section.contains(&q.bounds.center())
            }));
        }
    });
}

#[gpui_kit::test]
fn restart_offered_when_the_saved_pack_is_not_running(cx: &mut gpui_kit::TestAppContext) {
    let settings = Settings { icon_pack: IconPack::Solar, ..Default::default() };
    let window = settings_window_with(settings, cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    assert!(visual.debug_bounds("icon-pack-restart").is_some());
}

#[gpui_kit::test]
fn no_restart_offered_when_the_saved_pack_is_running(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    assert!(visual.debug_bounds("icon-pack-restart").is_none());
}

fn rooted_window(
    settings: Settings,
    cx: &mut gpui_kit::TestAppContext,
) -> (gpui_kit::AnyWindowHandle, gpui_kit::Entity<ChatApp>) {
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
    cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                ChatApp::with_state(
                    aether_agent::Agent::new().unwrap(),
                    settings,
                    None,
                    None,
                    false,
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    })
}

fn agent_settings(opacity: u8) -> Settings {
    Settings {
        window_opacity: opacity,
        provider: Some(aether_core::Provider::Codex),
        workspace_root: std::env::temp_dir().to_string_lossy().into_owned(),
        ..Settings::default()
    }
}

fn coverage(window: &Window, at: gpui_kit::Point<gpui_kit::Pixels>) -> f32 {
    let at = at.scale(window.scale_factor());
    let clear = window
        .painted_quads()
        .iter()
        .filter(|q| q.bounds.contains(&at) && q.content_mask.bounds.contains(&at))
        .filter_map(|q| q.background.as_solid())
        .fold(1., |clear, c| clear * (1. - c.a));
    1. - clear
}

fn paints_opaque_page(window: &Window, cx: &gpui_kit::App) -> bool {
    let opaque = Some(cx.theme().background.alpha(1.));
    window.painted_quads().iter().any(|q| q.background.as_solid() == opaque)
}

const CONTENT_PAGES: [crate::app::Page; 7] = {
    use crate::app::Page::*;
    [Chat, Settings, Skills, Agents, Usage, HackerNews, Notes]
};

fn right_margin(window: &Window) -> gpui_kit::Point<gpui_kit::Pixels> {
    let size = window.viewport_size();
    gpui_kit::point(size.width - px(3.), size.height * 0.5)
}

#[gpui_kit::test]
fn content_coverage_matches_the_opacity_label(cx: &mut gpui_kit::TestAppContext) {
    let (window, app) = rooted_window(agent_settings(70), cx);
    cx.update_window(window, |_, window, cx| {
        for page in CONTENT_PAGES {
            app.update(cx, |app, _| app.page = page);
            let _ = window.draw(cx);
            let seen = coverage(window, right_margin(window));
            assert!((seen - 0.7).abs() < 0.001, "{page:?}: {seen}");
        }
        use crate::side_panel::SideTab;
        app.update(cx, |app, _| app.page = crate::app::Page::Chat);
        for (tab, full) in
            [(SideTab::Agent, false), (SideTab::Agent, true), (SideTab::Explorer, true)]
        {
            app.update(cx, |app, _| {
                app.side_tab = tab;
                app.side_full = full;
            });
            let _ = window.draw(cx);
            let seen = coverage(window, right_margin(window));
            assert!((seen - 0.7).abs() < 0.001, "{tab:?} full={full}: {seen}");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn content_stays_solid_at_full_opacity(cx: &mut gpui_kit::TestAppContext) {
    let (window, app) = rooted_window(agent_settings(100), cx);
    cx.update_window(window, |_, window, cx| {
        for page in CONTENT_PAGES {
            app.update(cx, |app, _| app.page = page);
            let _ = window.draw(cx);
            assert_eq!(coverage(window, right_margin(window)), 1., "{page:?}");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn draft_editor_stays_opaque_below_full_opacity(cx: &mut gpui_kit::TestAppContext) {
    let (window, app) = rooted_window(agent_settings(70), cx);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_draft_editor(window, cx));
        let _ = window.draw(cx);
    })
    .unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(window, cx);
    let cover = visual.debug_bounds("draft-editor-dialog").unwrap();
    visual.update(|window, _| {
        let header = cover.origin + gpui_kit::point(px(8.), px(8.));
        assert_eq!(coverage(window, header), 1.);
    });
}

#[gpui_kit::test]
fn watch_peek_stays_opaque_below_full_opacity(cx: &mut gpui_kit::TestAppContext) {
    let (window, app) = rooted_window(agent_settings(70), cx);
    cx.update_window(window, |_, window, cx| {
        let _ = window.draw(cx);
        assert!(!paints_opaque_page(window, cx));
        app.update(cx, |app, _| {
            let mut other = crate::app::chat_with("b", &[]);
            other.id = 2;
            app.chats.push(other);
            app.settings.watchlist = vec![aether_core::Watch::Chat(2)];
            app.watch_peek = Some(aether_core::Watch::Chat(2));
        });
        let _ = window.draw(cx);
        assert!(paints_opaque_page(window, cx));
    })
    .unwrap();
}

#[gpui_kit::test]
fn only_floating_panes_stay_opaque_below_full_opacity(cx: &mut gpui_kit::TestAppContext) {
    let (window, app) = rooted_window(agent_settings(70), cx);
    cx.update_window(window, |_, window, cx| {
        let _ = window.draw(cx);
        assert!(!paints_opaque_page(window, cx), "docked pane");
        app.update(cx, |app, _| {
            let pane = app.workspace.model.windows[&0].focused.unwrap();
            let bounds = crate::session_workspace::Rect::new(20., 30., 400., 300.);
            app.workspace.model.float(pane, bounds).unwrap();
        });
        let _ = window.draw(cx);
        assert!(paints_opaque_page(window, cx), "floating pane");
    })
    .unwrap();
}

#[gpui_kit::test]
fn pane_error_banner_stays_opaque_below_full_opacity(cx: &mut gpui_kit::TestAppContext) {
    let (window, app) = rooted_window(agent_settings(70), cx);
    cx.update_window(window, |_, window, cx| {
        let _ = window.draw(cx);
        assert!(!paints_opaque_page(window, cx));
        app.update(cx, |app, _| app.workspace.error = Some("boom".into()));
        let _ = window.draw(cx);
        assert!(paints_opaque_page(window, cx));
    })
    .unwrap();
}

#[gpui_kit::test]
fn image_viewer_stays_opaque_below_full_opacity(cx: &mut gpui_kit::TestAppContext) {
    let dir = std::env::temp_dir().join(format!("aether-viewer-opacity-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("opaque.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255])).save(&path).unwrap();
    let (window, app) = rooted_window(agent_settings(70), cx);
    cx.update_window(window, |_, window, cx| {
        let _ = window.draw(cx);
        assert!(!paints_opaque_page(window, cx));
        assert!(app.update(cx, |app, cx| app.preview_image(&path, window, cx)));
        let _ = window.draw(cx);
        assert!(paints_opaque_page(window, cx));
    })
    .unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui_kit::test]
fn opacity_slider_moves_within_a_step_do_nothing(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    let app = window.root(cx).unwrap();
    cx.update_window(*window, |_, window, cx| {
        app.update(cx, |app, cx| app.set_window_opacity(80, window, cx));
        assert!((cx.theme().background.a - 0.8).abs() < 0.001);
        // apply_look would put this back
        gpui_kit::component::Theme::update(cx, |t| t.colors.background.a = 0.5);
        app.update(cx, |app, cx| app.set_window_opacity(80, window, cx));
        assert_eq!(cx.theme().background.a, 0.5);
        app.update(cx, |app, cx| app.set_window_opacity(81, window, cx));
        assert!((cx.theme().background.a - 0.81).abs() < 0.001);
        assert_eq!(app.read(cx).settings.window_opacity, 81);
    })
    .unwrap();
}

#[gpui_kit::test]
fn font_list_fills_after_launch_and_keeps_the_saved_font(cx: &mut gpui_kit::TestAppContext) {
    let saved = Settings { ui_font_family: "Zapfino".into(), ..Settings::default() };
    let window = settings_window_with(saved, cx);
    let app = window.root(cx).unwrap();
    let select = cx.read(|cx| app.read(cx).font_select.clone());
    let value = move |cx: &gpui_kit::App| select.read(cx).selected_value().map(ToString::to_string);
    let chosen = value.clone();
    cx.read(|cx| assert_eq!(chosen(cx).as_deref(), Some("Zapfino")));
    cx.update_window(*window, |_, window, cx| {
        let select = app.read(cx).font_select.clone();
        let installed = ["Arial", "Menlo", "Zapfino"].map(String::from);
        let fill = crate::font_picker::font_choices(&installed);
        crate::font_picker::fill_font_select(&select, fill.clone(), "Zapfino", window, cx);
        assert_eq!(select.read(cx).selected_index(cx).map(|ix| ix.row), Some(3));
        assert_eq!(value(cx).as_deref(), Some("Zapfino"));
        crate::font_picker::fill_font_select(&select, fill, "Gone", window, cx);
        assert_eq!(select.read(cx).selected_index(cx).map(|ix| ix.row), Some(0));
        assert_eq!(value(cx).as_deref(), Some(""));
    })
    .unwrap();
}

#[gpui_kit::test]
fn font_list_fill_waits_for_the_background_task(cx: &mut gpui_kit::TestAppContext) {
    let saved = Settings { ui_font_family: "Zapfino".into(), ..Settings::default() };
    let window = settings_window_with(saved, cx);
    let app = window.root(cx).unwrap();
    let selected = |cx: &gpui_kit::App| {
        let select = app.read(cx).font_select.read(cx);
        select.selected_value().map(ToString::to_string)
    };
    cx.read(|cx| assert_eq!(selected(cx).as_deref(), Some("Zapfino")));
    cx.run_until_parked();
    // test platform lists no fonts, so the fill leaves only Default
    cx.read(|cx| assert_eq!(selected(cx).as_deref(), Some("")));
}

#[gpui_kit::test]
fn theme_arrow_scrolls_the_row_without_selecting_the_card_under_it(
    cx: &mut gpui_kit::TestAppContext,
) {
    let window = settings_test_window(cx);
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    assert!(visual.debug_bounds("theme-carousel-prev").is_none());
    let next = visual.debug_bounds("theme-carousel-next").expect("row overflows on first open");
    let before = visual.debug_bounds("theme-card-dracula").unwrap();
    visual.simulate_click(next.center(), gpui_kit::Modifiers::default());
    settle(&mut visual);
    visual.read(|cx| assert_eq!(app.read(cx).settings.appearance, Appearance::System));
    assert!(visual.debug_bounds("theme-card-dracula").unwrap().left() < before.left());
    assert!(visual.debug_bounds("theme-carousel-prev").is_some());
}

#[gpui_kit::test]
fn theme_row_reveals_a_selected_card_scrolled_out_on_open(cx: &mut gpui_kit::TestAppContext) {
    let settings = Settings { appearance: Appearance::Dracula, ..Settings::default() };
    let window = settings_window_with(settings, cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    let row = visual.debug_bounds("theme").unwrap();
    let card = visual.debug_bounds("theme-card-dracula").unwrap();
    assert!(card.left() >= row.left() && card.right() <= row.right());
    assert!(visual.debug_bounds("theme-carousel-prev").is_some());
}

#[gpui_kit::test]
fn theme_row_scrolls_on_horizontal_wheel_and_lets_vertical_through(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::{ScrollDelta, ScrollWheelEvent, point};
    let window = settings_test_window(cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    let card = visual.debug_bounds("theme-card-dark").unwrap();
    let wheel = |delta| ScrollWheelEvent {
        position: card.center(),
        delta: ScrollDelta::Pixels(delta),
        ..Default::default()
    };
    visual.simulate_event(wheel(point(px(0.), px(-40.))));
    settle(&mut visual);
    assert_eq!(visual.debug_bounds("theme-card-dark").unwrap().left(), card.left());
    visual.simulate_event(wheel(point(px(-40.), px(0.))));
    settle(&mut visual);
    assert!(visual.debug_bounds("theme-card-dark").unwrap().left() < card.left());
}

#[gpui_kit::test]
fn theme_variant_menu_applies_a_variant_without_selecting_the_card(
    cx: &mut gpui_kit::TestAppContext,
) {
    let window = settings_test_window(cx);
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    settle(&mut visual);
    let picker = visual.debug_bounds("theme-variant-catppuccin-frappe").unwrap();
    assert!(picker.size.width <= px(aether_ui::CAROUSEL_CARD_WIDTH));
    visual.simulate_click(picker.center(), gpui_kit::Modifiers::default());
    settle(&mut visual);
    visual.read(|cx| assert_eq!(app.read(cx).settings.appearance, Appearance::System));
    visual.simulate_keystrokes("down enter");
    settle(&mut visual);
    visual.read(|cx| {
        let picked = app.read(cx).settings.appearance;
        assert_eq!(aether_core::family_index(picked), 3, "{picked:?}");
    });
}

#[gpui_kit::test]
fn settings_escape_handling_preserves_search_focus_then_returns_to_chat(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::Focusable;
    let window = settings_test_window(cx);
    cx.run_until_parked();
    window
        .update(cx, |app, window, cx| {
            app.settings_search_input.update(cx, |input, cx| {
                input.set_value("theme", window, cx);
                input.focus(window, cx);
            });
        })
        .unwrap();
    cx.update_window(*window, |_, window, cx| {
        let _ = window.draw(cx);
    })
    .unwrap();
    cx.simulate_keystrokes(*window, "escape");
    window
        .update(cx, |app, window, cx| {
            assert!(app.settings_search_input.read(cx).value().is_empty());
            assert_eq!(app.page, crate::app::Page::Settings);
            assert!(app.settings_search_input.focus_handle(cx).is_focused(window));
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "escape");
    window
        .update(cx, |app, window, cx| {
            assert_eq!(app.page, crate::app::Page::Chat);
            assert!(app.composer.focus_handle(cx).is_focused(window));
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "secondary-,");
    window
        .update(cx, |app, window, _| {
            assert_eq!(app.page, crate::app::Page::Settings);
            assert!(app.settings_sidebar_focus.is_focused(window));
        })
        .unwrap();
}

#[gpui_kit::test]
fn settings_sidebar_keys_wrap_without_stealing_input_arrows(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::Focusable;
    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
    // Use the same Root as the application: it handles native Tab traversal.
    let (window, app) = cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                ChatApp::with_state(
                    aether_agent::Agent::new().unwrap(),
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
        app.update(cx, |app, cx| app.open_page(crate::app::Page::Settings, cx));
        let _ = window.draw(cx);
    })
    .unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(window, cx);
    visual.simulate_keystrokes("up");
    visual.read(|cx| assert_eq!(app.read(cx).settings_tab, SettingsTab::Terminal));
    visual.simulate_keystrokes("down");
    visual.read(|cx| assert_eq!(app.read(cx).settings_tab, SettingsTab::Appearance));
    visual.simulate_keystrokes("secondary-k");
    visual.update(|window, cx| {
        let app = app.read(cx);
        assert!(!app.palette_open);
        assert!(app.settings_search_input.focus_handle(cx).is_focused(window));
    });
    visual.simulate_keystrokes("down");
    visual.read(|cx| assert_eq!(app.read(cx).settings_tab, SettingsTab::Appearance));
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| assert!(app.read(cx).settings_sidebar_focus.is_focused(window)));
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| assert!(!app.read(cx).settings_sidebar_focus.is_focused(window)));
    visual.simulate_keystrokes("shift-tab");
    visual.update(|window, cx| assert!(app.read(cx).settings_sidebar_focus.is_focused(window)));
}

#[gpui_kit::test]
fn settings_category_entry_clears_prior_search(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    cx.run_until_parked();
    window
        .update(cx, |app, window, cx| {
            app.settings_search_input
                .update(cx, |input, cx| input.set_value("keychain", window, cx));
            app.open_page(crate::app::Page::Chat, cx);
            app.settings_tab = SettingsTab::Providers;
            app.open_page(crate::app::Page::Settings, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |app, _, cx| {
            assert!(app.settings_search_input.read(cx).value().is_empty());
            assert_eq!(app.settings_tab, SettingsTab::Providers);
        })
        .unwrap();
}

#[test]
fn settings_sidebar_icons_are_bundled() {
    use gpui_kit::AssetSource;
    let listed = crate::assets::AppAssets.list("icons").unwrap();
    for tab in SettingsTab::ALL {
        let path = tab.icon().path();
        let bytes = crate::assets::AppAssets
            .load(&path)
            .unwrap_or_else(|error| panic!("{} icon is missing: {error}", tab.label()))
            .expect("settings icon must be bundled");
        assert!(bytes.starts_with(b"<svg"), "{path}");
        assert!(listed.contains(&path), "{path}");
    }
}

#[gpui_kit::test]
fn appearance_reuses_installed_font_snapshot(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    let app = window.root(cx).unwrap();
    cx.update_window(*window, |_, window, cx| {
        let first = aether_terminal::view::installed_font_names_cached(cx);
        app.update(cx, |app, _| {
            app.settings.terminal_font_family = "Missing test font".into();
        });
        for _ in 0..10 {
            app.update(cx, |_, cx| cx.notify());
            let _ = window.draw(cx);
            let next = aether_terminal::view::installed_font_names_cached(cx);
            assert!(std::ptr::eq(first, next));
        }
    })
    .unwrap();
}

#[gpui::test]
fn missing_ui_font_falls_back_to_the_system_font(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    cx.update_window(*window, |_, window, cx| {
        let settings =
            aether_core::Settings { ui_font_family: "No Such Font".into(), ..Default::default() };
        crate::app::apply_look(&settings, window, cx);
        assert_eq!(cx.theme().font_family.as_ref(), ".SystemUIFont");
        crate::app::apply_look(&aether_core::Settings::default(), window, cx);
        assert_eq!(cx.theme().font_family.as_ref(), ".SystemUIFont");
    })
    .unwrap();
}

#[gpui::test]
fn scheme_change_keeps_font_and_radius(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    cx.update_window(*window, |_, window, cx| {
        let family = aether_terminal::view::installed_font_names_cached(cx)
            .first()
            .cloned()
            .unwrap_or_default();
        let settings = aether_core::Settings {
            ui_font_family: family.clone(),
            appearance: aether_core::Appearance::Nord,
            ..Default::default()
        };
        crate::app::apply_look(&settings, window, cx);
        if !family.is_empty() {
            assert_eq!(cx.theme().font_family.as_ref(), family);
        }
        assert_eq!(cx.theme().radius, px(6.));
    })
    .unwrap();
}

#[gpui::test]
fn square_corners_keep_pills_round(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    cx.update_window(*window, |_, window, cx| {
        let settings = aether_core::Settings {
            corner_style: aether_core::CornerStyle::Square,
            ..Default::default()
        };
        crate::app::apply_look(&settings, window, cx);
        assert!(cx.theme().radius_full() > px(0.));
        assert!(cx.theme().radius < px(1.));
    })
    .unwrap();
}

#[gpui::test]
fn scheme_change_keeps_translucency(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    cx.update_window(*window, |_, window, cx| {
        let settings = aether_core::Settings {
            window_opacity: 70,
            appearance: aether_core::Appearance::Nord,
            ..Default::default()
        };
        crate::app::apply_look(&settings, window, cx);
        assert!((cx.theme().background.a - 0.7).abs() < 0.001);
        assert!((cx.theme().tokens.background.color.a - 0.7).abs() < 0.001);
    })
    .unwrap();
}

#[gpui_kit::test]
fn model_reasoning_cards_change_effort_and_preserve_search(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.settings_tab = SettingsTab::Model;
            cx.notify();
        });
        let _ = window.draw(cx);
    });
    settle(&mut visual);
    assert!(visual.debug_bounds("default-model").is_some());
    assert!(visual.debug_bounds("web-search").is_some());
    for (id, effort) in [
        ("reasoning-high", Reasoning::High),
        ("reasoning-off", Reasoning::Off),
        ("reasoning-medium", Reasoning::Medium),
        ("reasoning-low", Reasoning::Low),
    ] {
        let card = visual.debug_bounds(id).unwrap();
        visual.simulate_click(card.center(), gpui_kit::Modifiers::default());
        visual.read(|cx| assert_eq!(app.read(cx).settings.reasoning, effort));
        settle(&mut visual);
    }
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.settings_search_input.update(cx, |input, cx| {
                input.set_value("custom instructions", window, cx);
            });
        });
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("instructions").is_some());
    assert!(visual.debug_bounds("reasoning").is_none());
}
