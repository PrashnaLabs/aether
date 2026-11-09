use super::*;

#[test]
fn account_env_follows_the_chosen_account_per_provider() {
    let mut s: Settings = serde_json::from_str(
        r#"{"accounts":[
            {"provider":"claude","name":"work","dir":"/w/claude"},
            {"provider":"codex","name":"alt","dir":"/w/codex"},
            {"provider":"claude","name":"blank","dir":" "}],
          "default_accounts":{"claude":"work","codex":"alt"}}"#,
    )
    .unwrap();
    let pair = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];
    assert_eq!(s.account_env(Provider::Claude), pair("CLAUDE_CONFIG_DIR", "/w/claude"));
    assert_eq!(s.account_env(Provider::Codex), pair("CODEX_HOME", "/w/codex"));
    assert!(s.account_env(Provider::OpenCode).is_empty());
    s.default_accounts.insert("claude".into(), "blank".into());
    assert!(s.account_env(Provider::Claude).is_empty());
    s.default_accounts.insert("claude".into(), "gone".into());
    assert!(s.account_env(Provider::Claude).is_empty());
    assert!(Settings::default().account_env(Provider::Claude).is_empty());
}

#[test]
fn missing_fields_fall_back_to_defaults() {
    let settings: Settings =
        serde_json::from_str(r#"{ "appearance": "dark", "reasoning": "off" }"#).unwrap();
    assert_eq!(
        settings,
        Settings { appearance: Appearance::Dark, reasoning: Reasoning::Off, ..Settings::default() }
    );
    assert_eq!(Reasoning::High.effort(), Some("high"));
}

#[test]
fn provider_defaults_to_openrouter_with_empty_overrides() {
    let s = Settings::default();
    assert_eq!(s.provider, Provider::OpenRouter);
    assert!(s.provider_models.is_empty() && s.claude_binary.is_empty());
    let parsed: Settings = serde_json::from_str(r#"{"model":"x"}"#).unwrap();
    assert_eq!(parsed.provider, Provider::OpenRouter);
}

#[test]
fn open_in_app_defaults_empty() {
    let old: Settings = serde_json::from_str(r#"{ "appearance": "dark" }"#).unwrap();
    assert!(old.open_in_app.is_empty());
}

#[test]
fn project_folders_default_empty_and_round_trip() {
    let old: Settings = serde_json::from_str(r#"{ "appearance": "dark" }"#).unwrap();
    assert!(old.collapsed_projects.is_empty() && old.hidden_projects.is_empty());
    let s = Settings {
        collapsed_projects: vec!["/code/a".into()],
        hidden_projects: vec!["/code/b".into(), "/Users/me".into()],
        ..Settings::default()
    };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.collapsed_projects, ["/code/a"]);
    assert_eq!(back.hidden_projects, ["/code/b", "/Users/me"]);
}

#[test]
fn favorite_models_default_empty_and_round_trip() {
    let old: Settings = serde_json::from_str(r#"{"model":"x","provider":"claude"}"#).unwrap();
    assert!(old.favorite_models.is_empty());
    let s = Settings { favorite_models: vec!["claude:opus".into()], ..Settings::default() };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.favorite_models, ["claude:opus"]);
}

#[test]
fn provider_keys_match_their_saved_names() {
    for p in Provider::ALL {
        assert_eq!(serde_json::to_string(&p).unwrap(), format!("\"{}\"", p.key()));
    }
    assert_eq!(Provider::ALL.len(), 6);
    assert_eq!(Provider::ALL[0], Provider::OpenRouter);
    assert_eq!(Provider::Claude.label(), "Claude Code");
    assert_eq!(Provider::Codex.label(), "Codex");
    assert_eq!(Provider::Antigravity.label(), "Antigravity (AGY)");
    assert_eq!(serde_json::from_str::<Provider>("\"agy\"").unwrap(), Provider::Antigravity);
}

#[test]
fn access_modes_per_provider_and_saved_names() {
    assert_eq!(serde_json::to_string(&AccessMode::AutoEdit).unwrap(), "\"autoedit\"");
    assert_eq!(serde_json::to_string(&AccessMode::ReadOnly).unwrap(), "\"readonly\"");
    assert_eq!(AccessMode::for_provider(Provider::Pi), [AccessMode::ReadOnly, AccessMode::Full]);
    assert_eq!(
        AccessMode::for_provider(Provider::Claude),
        [AccessMode::Ask, AccessMode::AutoEdit, AccessMode::Auto, AccessMode::Full]
    );
    assert_eq!(
        AccessMode::for_provider(Provider::OpenCode),
        [AccessMode::Ask, AccessMode::AutoEdit, AccessMode::Full]
    );
    assert_eq!(
        AccessMode::for_provider(Provider::Codex),
        [
            AccessMode::Ask,
            AccessMode::ReadOnly,
            AccessMode::AutoEdit,
            AccessMode::Auto,
            AccessMode::Full
        ]
    );
    assert_eq!(
        AccessMode::for_provider(Provider::Antigravity),
        [AccessMode::AutoEdit, AccessMode::Auto, AccessMode::Full]
    );
    assert_eq!(serde_json::to_string(&AccessMode::Auto).unwrap(), "\"auto\"");
    assert!(AccessMode::for_provider(Provider::OpenRouter).is_empty());
    assert_eq!(AccessMode::Full.label(), "Full access");
    assert_eq!(effort_levels(Provider::Claude), ["low", "medium", "high", "xhigh", "max"]);
    assert_eq!(effort_levels(Provider::Antigravity), ["low", "medium", "high", "xhigh", "max"]);
    assert!(effort_levels(Provider::OpenCode).is_empty());
    assert_eq!(effort_levels(Provider::Codex), ["low", "medium", "high", "xhigh", "max"]);
}

#[test]
fn access_labels_and_descriptions() {
    assert_eq!(AccessMode::Ask.label(), "Supervised");
    assert_eq!(AccessMode::AutoEdit.label(), "Auto-accept edits");
    assert_eq!(AccessMode::Auto.label(), "Auto");
    assert_eq!(AccessMode::ReadOnly.label(), "Read only");
    assert_eq!(AccessMode::Ask.description(), "Ask before commands and file changes");
    assert_eq!(AccessMode::Full.description(), "Allow commands and edits without prompts");
    assert!(AccessMode::ALL.iter().all(|m| !m.description().is_empty()));
}

#[test]
fn effort_labels() {
    assert_eq!(effort_label(""), "Default");
    assert_eq!(effort_label("high"), "High");
    assert_eq!(effort_label("xhigh"), "Extra High");
    assert_eq!(effort_label("off"), "Off");
    assert_eq!(effort_label("some_new-level"), "Some New Level");
}

#[test]
fn footer_chips_follow_provider() {
    let c = FooterChips::for_provider(Provider::Claude);
    assert!(c.effort && c.access && c.folder && !c.toggles);
    let c = FooterChips::for_provider(Provider::OpenCode);
    assert!(!c.effort && c.access && c.folder && !c.toggles);
    let c = FooterChips::for_provider(Provider::OpenRouter);
    assert!(!c.effort && !c.access && !c.folder && c.toggles);
}

#[test]
fn terminal_cursor_loads_and_falls_back_to_block() {
    let load = |json| serde_json::from_str::<Settings>(json).unwrap().terminal_cursor;
    assert_eq!(load(r#"{"terminal_cursor":"bar"}"#), TerminalCursor::Bar);
    assert_eq!(load(r#"{"terminal_cursor":"underline"}"#), TerminalCursor::Underline);
    assert_eq!(load(r#"{"terminal_cursor":"blink"}"#), TerminalCursor::Block);
    assert_eq!(load("{}"), TerminalCursor::Block);
}

#[test]
fn terminal_theme_serializes_and_resolves_appearance() {
    assert_eq!(TerminalTheme::default(), TerminalTheme::AutoCatppuccin);
    assert!(TerminalTheme::AutoCatppuccin.is_dark(true));
    assert!(!TerminalTheme::AutoCatppuccin.is_dark(false));
    assert!(TerminalTheme::CatppuccinMocha.is_dark(false));
    assert!(TerminalTheme::CatppuccinMacchiato.is_dark(false));
    assert!(TerminalTheme::CatppuccinFrappe.is_dark(false));
    assert!(!TerminalTheme::CatppuccinLatte.is_dark(true));
    assert!(TerminalTheme::Aether.is_dark(true));
    assert!(!TerminalTheme::Aether.is_dark(false));

    assert_eq!(TerminalTheme::ALL.len(), 5);
    assert_eq!(TerminalTheme::AutoCatppuccin.label(), "Auto (Catppuccin)");
    assert_eq!(TerminalTheme::CatppuccinMocha.label(), "Catppuccin Mocha");
    assert_eq!(TerminalTheme::CatppuccinMacchiato.label(), "Catppuccin Macchiato");
    assert_eq!(TerminalTheme::CatppuccinFrappe.label(), "Catppuccin Frappé");
    assert_eq!(TerminalTheme::CatppuccinLatte.label(), "Catppuccin Latte");
    assert_eq!(TerminalTheme::Aether.label(), "Aether Default");

    let json = serde_json::to_string(&TerminalTheme::CatppuccinMocha).unwrap();
    assert_eq!(json, "\"catppuccin_mocha\"");
    let back: TerminalTheme = serde_json::from_str(&json).unwrap();
    assert_eq!(back, TerminalTheme::CatppuccinMocha);

    let parsed: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(parsed.terminal_theme, TerminalTheme::AutoCatppuccin);
}

#[test]
fn terminal_font_size_defaults_and_index() {
    let s = Settings::default();
    assert_eq!(s.terminal_font_size, 13.0);
    assert_eq!(s.terminal_font_size_index(), 2);

    let s11 = Settings { terminal_font_size: 11.0, ..Settings::default() };
    assert_eq!(s11.terminal_font_size_index(), 0);

    let s18 = Settings { terminal_font_size: 18.0, ..Settings::default() };
    assert_eq!(s18.terminal_font_size_index(), 5);

    let parsed: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(parsed.terminal_font_size, 13.0);
}

#[test]
fn sidebar_collapsed_defaults_and_round_trips() {
    let s = Settings::default();
    assert!(!s.sidebar_collapsed);

    let parsed: Settings = serde_json::from_str("{}").unwrap();
    assert!(!parsed.sidebar_collapsed);

    let serialized =
        serde_json::to_string(&Settings { sidebar_collapsed: true, ..Settings::default() })
            .unwrap();
    let restored: Settings = serde_json::from_str(&serialized).unwrap();
    assert!(restored.sidebar_collapsed);
}

#[test]
fn project_maps_default_empty_and_round_trip() {
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert!(old.project_open_in.is_empty() && old.project_scripts.is_empty());
    let s = Settings {
        project_open_in: [("/p".into(), "cursor".into())].into(),
        project_scripts: [(
            "/p".into(),
            vec![ProjectScript {
                name: "dev".into(),
                command: "pnpm dev".into(),
                on_new_worktree: false,
            }],
        )]
        .into(),
        ..Settings::default()
    };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
}

#[test]
fn project_script_on_new_worktree_defaults_false() {
    let s: ProjectScript = serde_json::from_str(r#"{"name":"a","command":"b"}"#).unwrap();
    assert!(!s.on_new_worktree);
}

#[test]
fn short_labels_name_cli_agents_only() {
    assert_eq!(Provider::Claude.short_label(), Some("Claude"));
    assert_eq!(Provider::Antigravity.short_label(), Some("AGY"));
    assert_eq!(Provider::OpenRouter.short_label(), None);
}

#[test]
fn saved_projects_default_empty_and_round_trip() {
    let s: Settings = serde_json::from_str(r#"{"appearance":"dark"}"#).unwrap();
    assert!(s.projects.is_empty());
    let s = Settings { projects: vec!["/w/a".into()], ..Settings::default() };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.projects, vec!["/w/a".to_string()]);
}

#[test]
fn provider_access_defaults_are_supervised_where_supported() {
    let settings: Settings = serde_json::from_str("{}").unwrap();
    for provider in [Provider::Claude, Provider::Codex, Provider::OpenCode] {
        assert_eq!(settings.default_access(provider), AccessMode::Ask);
    }
    assert_eq!(settings.default_access(Provider::Pi), AccessMode::ReadOnly);
    assert_eq!(settings.default_access(Provider::Antigravity), AccessMode::AutoEdit);
}

#[test]
fn provider_access_overrides_persist_and_reject_unsupported_modes() {
    let mut settings = Settings::default();
    settings.provider_access.insert(Provider::Codex.key().into(), AccessMode::Full);
    settings.provider_access.insert(Provider::Pi.key().into(), AccessMode::Ask);
    let restored: Settings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(restored.default_access(Provider::Codex), AccessMode::Full);
    assert_eq!(restored.default_access(Provider::Claude), AccessMode::Ask);
    assert_eq!(restored.default_access(Provider::Pi), AccessMode::ReadOnly);
}

#[test]
fn old_settings_with_status_bar_keys_still_load() {
    let old: Settings = serde_json::from_str(
        r#"{"status_bar_visible": false, "status_bar_position": "sidebar", "status_bar_show_seconds": true, "notifications": true}"#,
    )
    .unwrap();
    assert!(old.status_bar_show_seconds);
    assert!(old.notifications);
    let saved = serde_json::to_string(&old).unwrap();
    assert!(!saved.contains("status_bar_visible"));
    assert!(!saved.contains("status_bar_position"));
}

#[test]
fn notes_defaults_support_old_and_partial_settings() {
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(old.notes, NotesSettings::default());
    let partial: Settings =
        serde_json::from_str(r#"{"notes":{"directory":"/custom/notes","editor":"neovim"}}"#)
            .unwrap();
    assert_eq!(partial.notes.directory, "/custom/notes");
    assert_eq!(partial.notes.editor, NotesEditor::Neovim);
    assert_eq!(partial.notes.nvim, "nvim");
    assert_eq!(partial.notes.tmux, "tmux");
    assert_eq!(partial.notes.session, "notes");
    assert!(!partial.notes.project_scope);
    assert!(partial.notes.socket.is_empty());
}

#[test]
fn notes_settings_round_trip_all_overrides() {
    let mut settings = Settings::default();
    settings.notes = NotesSettings {
        directory: "/custom/notes".into(),
        editor: NotesEditor::Neovim,
        project_scope: true,
        nvim: "/bin/custom-nvim".into(),
        tmux: "/bin/custom-tmux".into(),
        session: "personal".into(),
        socket: "/tmp/personal.sock".into(),
    };
    let parsed: Settings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(parsed.notes, settings.notes);
}

#[test]
fn toggle_watch_adds_once_then_removes() {
    let mut list = vec![Watch::Chat(1)];
    assert!(toggle_watch(&mut list, Watch::Terminal(1)));
    assert_eq!(list, [Watch::Chat(1), Watch::Terminal(1)]);
    assert!(!toggle_watch(&mut list, Watch::Chat(1)));
    assert_eq!(list, [Watch::Terminal(1)]);
    assert_eq!(Watch::Terminal(9).chat(), 9);
}

#[test]
fn old_settings_load_with_an_empty_watchlist() {
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert!(s.watchlist.is_empty());
}

#[test]
fn watchlist_round_trips() {
    let s = Settings { watchlist: vec![Watch::Chat(7), Watch::Terminal(7)], ..Settings::default() };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.watchlist, s.watchlist);
}

#[test]
fn watch_corner_defaults_to_bottom_right_and_round_trips() {
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(old.watch_corner, WatchCorner::BottomRight);
    assert_eq!(WatchCorner::TopLeft.label(), "Top left");
    for corner in WatchCorner::ALL {
        let s = Settings { watch_corner: corner, ..Settings::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.watch_corner, corner);
    }
}

#[test]
fn watch_peek_bounds_default_to_none_and_round_trip() {
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(old.watch_peek_bounds, None);
    let s = Settings { watch_peek_bounds: Some([10., 20., 480., 360.]), ..Settings::default() };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.watch_peek_bounds, s.watch_peek_bounds);
}

#[test]
fn appearance_keeps_old_values_and_round_trips_every_scheme() {
    for (json, a) in [
        ("\"system\"", Appearance::System),
        ("\"light\"", Appearance::Light),
        ("\"dark\"", Appearance::Dark),
    ] {
        assert_eq!(serde_json::from_str::<Appearance>(json).unwrap(), a);
        assert_eq!(serde_json::to_string(&a).unwrap(), json);
    }
    for a in Appearance::ALL {
        assert_eq!(
            serde_json::from_str::<Appearance>(&serde_json::to_string(&a).unwrap()).unwrap(),
            a
        );
    }
    let slugs: std::collections::HashSet<_> = Appearance::ALL.iter().map(|a| a.slug()).collect();
    assert_eq!(slugs.len(), 16);
    assert_eq!(Appearance::System.is_dark(), None);
    assert_eq!(Appearance::RosePineDawn.is_dark(), Some(false));
    assert_eq!(Appearance::Nord.is_dark(), Some(true));
}

#[test]
fn theme_families_cover_every_appearance_once() {
    let listed: Vec<_> = THEME_FAMILIES.iter().flat_map(|f| f.variants.iter().copied()).collect();
    assert_eq!(listed, Appearance::ALL.to_vec());
    assert_eq!(family_index(Appearance::Dracula), 8);
    assert_eq!(family_index(Appearance::CatppuccinMocha), 3);
    assert_eq!(THEME_FAMILIES[3].shown(Appearance::Dark), Appearance::CatppuccinFrappe);
    assert_eq!(THEME_FAMILIES[3].shown(Appearance::CatppuccinMocha), Appearance::CatppuccinMocha);
    assert!(THEME_FAMILIES[3].has_both_modes());
    assert!(!THEME_FAMILIES[7].has_both_modes());
}

#[test]
fn settings_without_new_fields_load_with_defaults() {
    let s: Settings = serde_json::from_str(r#"{"appearance":"dark"}"#).unwrap();
    assert_eq!(s.appearance, Appearance::Dark);
    assert_eq!(s.corner_style, CornerStyle::Rounded);
    assert_eq!(s.icon_pack, IconPack::Lucide);
    assert_eq!(s.window_opacity, 100);
    assert!(!s.window_blur);
    assert!(s.ui_font_family.is_empty());
}

#[test]
fn window_opacity_clamps_hand_edited_values() {
    let at = |window_opacity| {
        Settings { window_opacity, ..Settings::default() }.clamped_window_opacity()
    };
    assert_eq!((at(0), at(80), at(250)), (60, 80, 100));
}

fn with_field(field: &str) -> Settings {
    let json = format!(
        r#"{{"model":"m","instructions":"keep","skills":{{"a":true}},"notes":{{"directory":"/n"}},{field}}}"#
    );
    let s: Settings = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{field}: {e}"));
    assert_eq!(s.model, "m", "{field}");
    assert_eq!(s.instructions, "keep", "{field}");
    assert_eq!(s.skills.get("a"), Some(&true), "{field}");
    assert_eq!(s.notes.directory, "/n", "{field}");
    s
}

#[test]
fn bad_window_opacity_saturates_or_defaults_and_keeps_the_file() {
    for (raw, want) in [
        ("0.8", 60),
        ("300", 100),
        ("-1", 60),
        ("85", 85),
        ("85.4", 85),
        ("1e300", 100),
        ("\"mocha\"", 100),
        ("null", 100),
        ("true", 100),
        ("[80]", 100),
    ] {
        let s = with_field(&format!(r#""window_opacity":{raw}"#));
        assert_eq!(s.window_opacity, want, "{raw}");
    }
}

#[test]
fn bad_look_fields_default_and_keep_the_file() {
    for raw in ["\"mocha\"", "0.8", "300", "-1", "null", "{}"] {
        assert_eq!(with_field(&format!(r#""appearance":{raw}"#)).appearance, Appearance::System);
        assert_eq!(
            with_field(&format!(r#""corner_style":{raw}"#)).corner_style,
            CornerStyle::Rounded
        );
        assert_eq!(with_field(&format!(r#""icon_pack":{raw}"#)).icon_pack, IconPack::Lucide);
        assert!(!with_field(&format!(r#""window_blur":{raw}"#)).window_blur);
    }
}

#[test]
fn good_look_fields_still_load() {
    assert_eq!(with_field(r#""corner_style":"square""#).corner_style, CornerStyle::Square);
    assert_eq!(with_field(r#""icon_pack":"remix""#).icon_pack, IconPack::Remix);
    assert!(with_field(r#""window_blur":true"#).window_blur);
}

#[test]
fn appearance_loads_from_its_slug_and_saves_as_before() {
    for a in Appearance::ALL {
        assert_eq!(with_field(&format!(r#""appearance":"{}""#, a.slug())).appearance, a);
        let saved = serde_json::to_string(&a).unwrap();
        assert_eq!(with_field(&format!(r#""appearance":{saved}"#)).appearance, a);
    }
    assert_eq!(serde_json::to_string(&Appearance::RosePineMoon).unwrap(), "\"rosepinemoon\"");
}

#[test]
fn settings_saved_before_voice_keep_dictation_on() {
    let mut json = serde_json::to_value(Settings::default()).unwrap();
    json.as_object_mut().unwrap().remove("voice_enabled");
    assert!(serde_json::from_value::<Settings>(json).unwrap().voice_enabled);
}

#[test]
fn settings_saved_before_the_feature_toggles_keep_notes_and_news_on() {
    let mut json = serde_json::to_value(Settings::default()).unwrap();
    let fields = json.as_object_mut().unwrap();
    fields.remove("notes_enabled");
    fields.remove("hackernews_enabled");
    let settings = serde_json::from_value::<Settings>(json).unwrap();
    assert!(settings.notes_enabled && settings.hackernews_enabled);
}

#[test]
fn chat_layout_defaults_to_tabbed_and_round_trips() {
    assert_eq!(Settings::default().chat_layout, ChatLayout::Tabbed);
    assert_eq!(Settings::default().right_panel_share, 0.6);
    assert!(!Settings::default().right_panel_collapsed);
    assert_eq!(ChatLayout::ALL.len(), 3);
    assert_eq!(ChatLayout::Tabbed.label(), "Tabbed");
    assert_eq!(ChatLayout::Sidebar.label(), "Sidebar");
    assert_eq!(ChatLayout::Split.label(), "Split");
    assert_eq!(serde_json::from_str::<ChatLayout>("\"sidebar\"").unwrap(), ChatLayout::Sidebar);
    assert_eq!(serde_json::from_str::<ChatLayout>("\"split\"").unwrap(), ChatLayout::Split);
    assert_eq!(serde_json::to_string(&ChatLayout::Sidebar).unwrap(), "\"sidebar\"");
    assert_eq!(serde_json::to_string(&ChatLayout::Split).unwrap(), "\"split\"");
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(old.chat_layout, ChatLayout::Tabbed);
    assert_eq!(old.right_panel_share, 0.6);
    assert!(!old.right_panel_collapsed);
    let s = Settings {
        chat_layout: ChatLayout::Sidebar,
        right_panel_share: 0.7,
        right_panel_collapsed: true,
        ..Settings::default()
    };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.chat_layout, ChatLayout::Sidebar);
    assert_eq!(back.right_panel_share, 0.7);
    assert!(back.right_panel_collapsed);
    for layout in ChatLayout::ALL {
        let json = serde_json::to_string(&layout).unwrap();
        assert_eq!(serde_json::from_str::<ChatLayout>(&json).unwrap(), layout);
    }
}

#[test]
fn settings_saved_before_the_watch_toggle_keep_watching_on() {
    let mut json = serde_json::to_value(Settings::default()).unwrap();
    json.as_object_mut().unwrap().remove("watch_enabled");
    assert!(serde_json::from_value::<Settings>(json).unwrap().watch_enabled);
}
