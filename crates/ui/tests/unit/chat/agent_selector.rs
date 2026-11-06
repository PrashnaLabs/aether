use super::*;

fn item(id: &str, name: &str) -> ModelItem {
    ModelItem {
        id: id.into(),
        name: name.into(),
        detail: "Anthropic · Test".into(),
        default: false,
        description: "".into(),
    }
}

fn providers() -> Vec<ProviderModels> {
    let models =
        |provider, items: Vec<ModelItem>, hidden| ProviderModels { provider, items, hidden };
    vec![
        models(
            Provider::OpenRouter,
            vec![item("anthropic/claude-opus", "Claude Opus"), item("google/gemma:free", "Gemma")],
            false,
        ),
        models(Provider::Claude, vec![item("opus", "Opus"), item("sonnet", "Sonnet")], false),
        models(Provider::Pi, vec![item("opus-pi", "Opus Pi")], true),
        models(Provider::OpenCode, vec![item("oc/opus", "Opus OC")], false),
    ]
}

fn ids(rows: &[(Provider, ModelItem)]) -> Vec<(Provider, &str)> {
    rows.iter().map(|(p, m)| (*p, m.id.as_ref())).collect()
}

#[test]
fn favorite_keys_round_trip_and_keep_colons_in_the_model() {
    assert_eq!(
        favorite_key(Provider::OpenRouter, "google/gemma:free"),
        "openrouter:google/gemma:free"
    );
    assert_eq!(
        parse_favorite("openrouter:google/gemma:free"),
        Some((Provider::OpenRouter, "google/gemma:free"))
    );
    assert_eq!(
        parse_favorite(&favorite_key(Provider::Claude, "opus")),
        Some((Provider::Claude, "opus"))
    );
    assert_eq!(
        parse_favorite(&favorite_key(Provider::Codex, "gpt-5.3-codex")),
        Some((Provider::Codex, "gpt-5.3-codex"))
    );
    assert_eq!(
        parse_favorite(&favorite_key(Provider::Antigravity, "gemini-2.5-pro")),
        Some((Provider::Antigravity, "gemini-2.5-pro"))
    );
    assert_eq!(parse_favorite("nope:opus"), None);
    assert_eq!(parse_favorite("claude"), None);
}

#[test]
fn rail_skips_hidden_and_locked_out_providers() {
    let p = providers();
    let tab = SelectorTab::Provider;
    assert_eq!(
        rail_tabs(&p, None),
        [
            SelectorTab::Favorites,
            tab(Provider::OpenRouter),
            tab(Provider::Claude),
            tab(Provider::OpenCode)
        ]
    );
    assert_eq!(
        rail_tabs(&p, Some(Provider::Claude)),
        [SelectorTab::Favorites, tab(Provider::Claude)]
    );
}

#[test]
fn rail_includes_codex_and_antigravity() {
    let p: Vec<ProviderModels> = Provider::ALL
        .map(|provider| ProviderModels {
            provider,
            items: vec![item("default", "Default")],
            hidden: false,
        })
        .to_vec();
    let tabs = rail_tabs(&p, None);
    assert_eq!(tabs.len(), 7);
    assert_eq!(tabs[0], SelectorTab::Favorites);
    assert_eq!(tabs[1], SelectorTab::Provider(Provider::OpenRouter));
    assert_eq!(tabs[2], SelectorTab::Provider(Provider::Claude));
    assert_eq!(tabs[3], SelectorTab::Provider(Provider::Codex));
    assert_eq!(tabs[4], SelectorTab::Provider(Provider::Pi));
    assert_eq!(tabs[5], SelectorTab::Provider(Provider::OpenCode));
    assert_eq!(tabs[6], SelectorTab::Provider(Provider::Antigravity));
}

#[test]
fn empty_query_lists_the_tab_and_search_spans_usable_providers() {
    let p = providers();
    let tab = SelectorTab::Provider(Provider::Claude);
    assert_eq!(
        ids(&visible_rows(tab, "", None, &[], &p)),
        [(Provider::Claude, "opus"), (Provider::Claude, "sonnet")]
    );
    assert_eq!(visible_rows(tab, "  ", None, &[], &p).len(), 2);
    assert_eq!(
        ids(&visible_rows(tab, "OPUS", None, &[], &p)),
        [
            (Provider::OpenRouter, "anthropic/claude-opus"),
            (Provider::Claude, "opus"),
            (Provider::OpenCode, "oc/opus"),
        ]
    );
    assert_eq!(
        ids(&visible_rows(SelectorTab::Favorites, "opus", Some(Provider::Claude), &[], &p)),
        [(Provider::Claude, "opus")]
    );
    assert!(visible_rows(tab, "opus sonnet", None, &[], &p).is_empty());
}

#[test]
fn favorites_keep_their_order_and_skip_unusable_or_missing() {
    let p = providers();
    let favorites = [
        "opencode:oc/opus",
        "pi:opus-pi",
        "claude:gone",
        "openrouter:google/gemma:free",
        "claude:sonnet",
    ]
    .map(String::from);
    let fav = SelectorTab::Favorites;
    assert_eq!(
        ids(&visible_rows(fav, "", None, &favorites, &p)),
        [
            (Provider::OpenCode, "oc/opus"),
            (Provider::OpenRouter, "google/gemma:free"),
            (Provider::Claude, "sonnet"),
        ]
    );
    assert_eq!(
        ids(&visible_rows(fav, "", Some(Provider::Claude), &favorites, &p)),
        [(Provider::Claude, "sonnet")]
    );
}

#[test]
fn highlight_wraps_both_ways() {
    assert_eq!(next_highlight(None, 0, 1), None);
    assert_eq!(next_highlight(None, 3, 1), Some(0));
    assert_eq!(next_highlight(None, 3, -1), Some(2));
    assert_eq!(next_highlight(Some(2), 3, 1), Some(0));
    assert_eq!(next_highlight(Some(0), 3, -1), Some(2));
    assert_eq!(next_highlight(Some(1), 3, 1), Some(2));
    assert_eq!(next_highlight(Some(9), 3, 1), Some(0));
}

#[test]
fn tab_leaves_rail_only_past_the_ends() {
    let fav = SelectorTab::Favorites;
    let tabs =
        [fav, SelectorTab::Provider(Provider::OpenRouter), SelectorTab::Provider(Provider::Claude)];
    assert!(leaves_rail(tabs[2], &tabs, 1));
    assert!(leaves_rail(fav, &tabs, -1));
    assert!(!leaves_rail(fav, &tabs, 1));
    assert!(!leaves_rail(tabs[2], &tabs, -1));
    assert!(!leaves_rail(SelectorTab::Provider(Provider::Pi), &tabs, 1));
}

#[test]
fn tabs_cycle_and_reset_when_unusable() {
    let fav = SelectorTab::Favorites;
    let tab = SelectorTab::Provider;
    let tabs = [fav, tab(Provider::OpenRouter), tab(Provider::Claude)];
    assert_eq!(cycle_tab(fav, &tabs, 1), tabs[1]);
    assert_eq!(cycle_tab(tabs[1], &tabs, 1), tabs[2]);
    assert_eq!(cycle_tab(tab(Provider::Pi), &tabs, 1), fav);
    assert_eq!(cycle_tab(fav, &[fav], 1), fav);
}

#[test]
fn locked_rail_keeps_other_providers_visible_but_disabled() {
    let models = |provider, hidden| ProviderModels { provider, items: vec![], hidden };
    let providers = vec![
        models(Provider::Claude, false),
        models(Provider::Codex, false),
        models(Provider::Pi, true),
    ];
    let rail: Vec<_> = rail_entries(&providers, Some(Provider::Claude))
        .iter()
        .map(|e| (e.tab, e.enabled))
        .collect();
    assert_eq!(
        rail,
        vec![
            (SelectorTab::Favorites, true),
            (SelectorTab::Provider(Provider::Claude), true),
            (SelectorTab::Provider(Provider::Codex), false),
        ]
    );
}

#[test]
fn row_subtitle_prefers_the_description_inside_a_provider_tab() {
    let mut item = ModelItem {
        id: "gpt-6.1-sol".into(),
        name: "GPT-6.1-Sol".into(),
        detail: "OpenAI · Codex".into(),
        default: false,
        description: "Latest workhorse model".into(),
    };
    assert_eq!(row_subtitle(&item, "Codex", false), "Latest workhorse model");
    assert_eq!(row_subtitle(&item, "Codex", true), "Codex");
    item.description = "".into();
    assert_eq!(row_subtitle(&item, "Codex", false), "OpenAI · Codex");
}

#[test]
fn picker_notes_name_the_provider() {
    assert_eq!(
        lock_note("Claude Code"),
        "This chat stays on Claude Code. Start a new chat to switch agents."
    );
    assert_eq!(no_reasoning_note("OpenCode"), "OpenCode sets its own reasoning");
}

#[test]
fn trigger_shows_effort_only_when_the_model_has_efforts_and_one_is_set() {
    let efforts = vec!["low".to_string(), "high".to_string()];
    assert_eq!(effort_shown(&efforts, "high"), Some(effort_label("high")));
    assert_eq!(effort_shown(&efforts, ""), None);
    assert_eq!(effort_shown(&[], "high"), None);
}
