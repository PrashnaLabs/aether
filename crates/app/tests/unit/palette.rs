use aether_core::Update;

use super::*;

#[test]
fn history_search_finds_message_text_case_insensitively() {
    let mut reply = Message::assistant();
    reply.apply(Update::Final("The QUICK brown fox".into()));
    let messages = vec![Message::user("hello"), reply];
    let (ix, text) = first_match(&messages, "quick").unwrap();
    assert_eq!(ix, 1);
    assert!(text.contains("QUICK"));
    assert!(first_match(&messages, "missing").is_none());
}

#[test]
fn rows_match_label_or_keyword_ignoring_case_and_padding() {
    let terms: Vec<SharedString> = vec!["Theme: Dark".into(), "dark mode".into()];
    assert!(matches("", &terms));
    assert!(matches("  THEME ", &terms));
    assert!(matches("mode", &terms));
    assert!(!matches("light", &terms));
    assert_eq!(item("New chat").keywords(["fresh"]).terms, ["New chat", "fresh"]);
}

#[test]
fn long_lines_truncate_to_one_row() {
    assert!(snippet(&"x".repeat(200)).chars().count() <= 90);
    assert_eq!(snippet("  hi  "), "hi");
}

#[test]
fn theme_group_lists_every_appearance() {
    let rows = theme_rows(Appearance::System);
    assert_eq!(rows.len(), 16);
    assert!(rows.iter().any(|(i, _)| i.terms[0] == "Theme: Rosé Pine Moon"));
}

#[test]
fn theme_rows_match_scheme_names_without_accents() {
    let rows = theme_rows(Appearance::System);
    let label = |query: &str| {
        rows.iter().find(|(i, _)| matches(query, &i.terms)).map(|(i, _)| i.terms[0].to_string())
    };
    assert_eq!(label("rose").as_deref(), Some("Theme: Rosé Pine Main"));
    assert_eq!(label("rose pine moon").as_deref(), Some("Theme: Rosé Pine Moon"));
    assert_eq!(label("rosé pine dawn").as_deref(), Some("Theme: Rosé Pine Dawn"));
    assert_eq!(label("frappe").as_deref(), Some("Theme: Catppuccin Frappé"));
}

#[test]
fn lead_capable_covers_every_harness() {
    use crate::app::orchestrate::lead_capable;
    use aether_core::Provider;
    for p in crate::app::CLIS.into_iter().chain([Provider::OpenRouter]) {
        assert!(lead_capable(p), "{p:?}");
    }
}

#[test]
fn window_move_rows_depend_on_host() {
    use crate::session_workspace::PaneAction;
    let main: Vec<_> = session_pane_actions(0).into_iter().collect();
    assert!(
        main.iter().any(|(l, a)| *l == "Open in new window" && matches!(a, PaneAction::Detach))
    );
    assert!(!main.iter().any(|(_, a)| matches!(a, PaneAction::Return)));
    let side: Vec<_> = session_pane_actions(3).into_iter().collect();
    assert!(
        side.iter()
            .any(|(l, a)| *l == "Move back to main window" && matches!(a, PaneAction::Return))
    );
    assert!(!side.iter().any(|(_, a)| matches!(a, PaneAction::Detach)));
}
