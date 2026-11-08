use crate::settings_search::*;
use crate::settings_view::SettingsTab;

#[test]
fn terminal_rows_search_into_the_terminal_tab() {
    for (query, id) in [("cursor", "terminal-cursor"), ("nerd", "terminal-font-family")] {
        let hit = search_settings(query).into_iter().find(|m| m.id == id).expect(id);
        assert_eq!(hit.tab, SettingsTab::Terminal);
    }
}

#[test]
fn feature_toggles_search_into_the_tools_tab() {
    for (query, id) in [("notes", "notes-enabled"), ("hn", "hackernews-enabled")] {
        let hit = search_settings(query).into_iter().find(|m| m.id == id).expect(id);
        assert_eq!(hit.tab, SettingsTab::Tools);
    }
}

#[test]
fn watching_searches_to_its_toggle() {
    for query in ["watching", "fab", "peek"] {
        let hit = search_settings(query).into_iter().find(|m| m.id == "watch-enabled");
        assert_eq!(hit.expect(query).tab, SettingsTab::Appearance);
    }
}

#[test]
fn voice_rows_search_into_the_composer_tab() {
    for (query, id) in
        [("dictation", "voice-enabled"), ("mic", "voice-enabled"), ("whisper", "voice-model")]
    {
        let hit = search_settings(query).into_iter().find(|m| m.id == id).expect(id);
        assert_eq!(hit.tab, SettingsTab::Composer);
    }
}

#[test]
fn chat_layout_searches_into_appearance() {
    for query in ["chat layout", "tabbed", "split panel"] {
        let hit = search_settings(query).into_iter().find(|m| m.id == "chat-layout");
        assert_eq!(hit.expect(query).tab, SettingsTab::Appearance);
    }
}

#[test]
fn search_settings_matches_title_description_and_keywords() {
    assert!(!all_setting_items().is_empty());
    assert!(search_settings("THEME").iter().any(|m| m.id == "theme"));
    assert!(search_settings("keychain").iter().any(|m| m.tab == SettingsTab::Model));
    assert!(search_settings("claude").iter().any(|m| m.tab == SettingsTab::Providers));
    assert!(search_settings("nonexistentxyz123").is_empty());
}

#[test]
fn search_settings_requires_all_tokens_and_ignores_whitespace() {
    let matches = search_settings("  TERMINAL   font  ");
    assert_eq!(matches.len(), 2);
    assert!(matches.iter().all(|item| item.id.starts_with("terminal-font")));
    assert!(search_settings("terminal keychain").is_empty());
    assert_eq!(search_settings("  ").len(), all_setting_items().len());
}

#[test]
fn tab_match_count_aggregates_per_category() {
    let matches = search_settings("font");
    assert_eq!(tab_match_count(SettingsTab::Appearance, &matches), 2);
    assert_eq!(tab_match_count(SettingsTab::Terminal, &matches), 2);
    assert_eq!(tab_match_count(SettingsTab::Model, &matches), 0);
}

#[test]
fn search_catalog_has_unique_ids_and_covers_every_category() {
    let mut ids = std::collections::HashSet::new();
    for item in all_setting_items() {
        assert!(ids.insert(item.id), "duplicate {}", item.id);
    }
    for tab in SettingsTab::ALL {
        assert!(all_setting_items().iter().any(|item| item.tab == tab));
    }
}

#[test]
fn theme_search_finds_scheme_names() {
    for query in ["gruvbox", "dracula", "rose pine", "nord"] {
        assert!(search_settings(query).iter().any(|m| m.id == "theme"), "{query}");
    }
    assert!(search_settings("catppuccin").iter().any(|m| m.id == "theme"));
    assert!(search_settings("catppuccin").iter().any(|m| m.id == "terminal-theme"));
}

#[test]
fn theme_search_finds_variant_names_with_or_without_accents() {
    for query in [
        "rosé pine",
        "rose pine",
        "rosé",
        "rose",
        "frappé",
        "frappe",
        "latte",
        "macchiato",
        "mocha",
        "moon",
        "dawn",
    ] {
        assert!(search_settings(query).iter().any(|m| m.id == "theme"), "{query}");
    }
}

#[test]
fn notes_settings_are_searchable_in_tools() {
    let matches = search_settings("notes");
    for id in [
        "notes-directory",
        "notes-editor",
        "notes-scope",
        "notes-nvim",
        "notes-tmux",
        "notes-session",
        "notes-socket",
    ] {
        assert!(
            matches.iter().any(|item| item.id == id && item.tab == SettingsTab::Tools),
            "missing {id}"
        );
    }
    assert!(search_settings("notes socket").iter().any(|item| item.id == "notes-socket"));
    assert!(search_settings("notes project").iter().any(|item| item.id == "notes-scope"));
}

#[test]
fn status_bar_position_and_visibility_are_not_searchable() {
    for query in ["status bar position", "show status bar"] {
        assert!(
            search_settings(query)
                .iter()
                .all(|m| !m.id.starts_with("status-bar-p") && m.id != "status-bar-visible")
        );
    }
}
