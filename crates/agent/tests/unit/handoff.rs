use super::*;

fn message(i: usize, role: Role, text: &str) -> HistoricalMessage {
    HistoricalMessage {
        role,
        kind: if role == Role::User { "user_message" } else { "assistant_message" },
        text: text.into(),
        chat: 1,
        message: i,
        item: i.to_string(),
        model: None,
        status: "completed".into(),
    }
}

#[test]
fn selects_intact_items_and_accounts_for_escaping() {
    let items = vec![
        message(0, Role::User, "first"),
        message(1, Role::Assistant, &"界🧪".repeat(20_000)),
        message(2, Role::Assistant, &"x".repeat(4_000)),
        message(3, Role::Assistant, "last"),
    ];
    let selected = select_history(&items, "coverage", 0, 3_000);
    assert_eq!(selected.messages, vec![items[0].clone(), items[3].clone()]);
    assert_eq!(selected.omitted_item_ids, vec!["1", "2"]);
    assert!(history_cost(&selected.messages, &selected.context) <= 3_000);
    let items: Vec<_> = (0..500)
        .map(|i| {
            message(
                i,
                if i % 2 == 0 { Role::User } else { Role::Assistant },
                &"\0\\\"🧪界".repeat(15),
            )
        })
        .collect();
    for cap in [1_024, 4_000, 16_000] {
        let s = select_history(&items, "coverage", 0, cap);
        assert!(history_cost(&s.messages, &s.context) <= cap);
        assert!(s.omitted_items > 0);
    }
}

#[test]
fn digit_boundaries_fit() {
    let items: Vec<_> = (0..20).map(|i| message(i, Role::User, "Short request")).collect();
    for cap in 4_000..=9_000 {
        let s = select_history(&items, "Recover history", 90, cap);
        assert!(history_cost(&s.messages, &s.context) <= cap);
    }
}

#[test]
fn budget_subtracts_input_and_attachments() {
    let base = budget(16_000, "Continue", 0, 0, Some(32_000), None, 0);
    assert!(base < 16_000);
    assert!(budget(16_000, "Continue", 1, 0, Some(32_000), None, 0) < base);
    assert_eq!(budget(16_000, &"界".repeat(30_000), 0, 0, Some(32_000), None, 0), 0);
    assert_eq!(budget(16_000, "Continue", 0, 0, Some(32_000), Some(23_000), 0), 0);
}

#[test]
fn renders_attribution_and_native_items() {
    let messages = vec![message(0, Role::User, "Hello"), message(1, Role::Assistant, "Done")];
    assert_eq!(
        render(&messages[0]),
        "[Historical user; user_message; chat=1; message=0; item=0; model=none; status=completed]\nHello"
    );
    let items = history_items(&messages, "context");
    assert_eq!(items[0]["content"][0]["text"], "context");
    assert_eq!(items[1]["content"][0]["type"], "input_text");
    assert_eq!(items[2]["content"][0]["type"], "output_text");
    assert_eq!(with_inline_context("", "new"), "new");
    assert_eq!(with_inline_context("context", "new"), "context\n\nUser message:\nnew");
    let selection = select_history(&messages, "coverage", 0, 0);
    assert!(selection.messages.is_empty());
    assert!(history_cost(&selection.messages, &selection.context) > 0);
}

#[test]
fn delta_selects_message_range_and_labels_coverage() {
    let items = vec![
        message(1, Role::User, "old"),
        message(2, Role::Assistant, "new"),
        message(3, Role::User, "later"),
    ];
    let delta = delta_history(&items, 2);
    assert_eq!(delta, items[1..]);
    assert!(
        strategy_coverage(&delta, "delta_since_target_last_seen", false)
            .starts_with("Context handoff (delta_since_target_last_seen):\n")
    );
}

#[test]
fn side_text_compacts_and_truncates_without_splitting_characters() {
    assert_eq!(compact_text(" \n hello\t  world \r\n"), "hello world");
    assert_eq!(compact_text(&"x".repeat(240)), "x".repeat(240));
    assert_eq!(compact_text(&"x".repeat(241)), format!("{}...", "x".repeat(237)));
    assert_eq!(compact_text(&"界🧪".repeat(130)), format!("{}界...", "界🧪".repeat(118)));
}

#[test]
fn side_digest_summarizes_only_supported_activity() {
    let mut item = message(0, Role::Assistant, "Command: cargo test\nExit code: 0\nsecret output");
    item.kind = "command_execution";
    assert_eq!(summarize_delta_item(&item).as_deref(), Some("- Command: cargo test"));
    item.kind = "file_change";
    item.text = "File change: src/main.rs".into();
    assert_eq!(summarize_delta_item(&item).as_deref(), Some("- File change: src/main.rs"));
    item.kind = "error";
    assert_eq!(summarize_delta_item(&item), None);
}

#[test]
fn side_context_keeps_recent_items_in_oldest_first_order() {
    let items: Vec<_> =
        (0..100).map(|i| message(i, Role::User, &format!("{i:03} {}", "界".repeat(230)))).collect();
    let context = make_side_chat_context(42, &items);
    assert!(context.chars().count() <= SIDE_CHAT_CONTEXT_MAX_CHARS);
    assert!(context.contains("- Earlier items truncated."));
    assert!(!context.contains("- User: 000"));
    assert!(context.find("- User: 098").unwrap() < context.find("- User: 099").unwrap());
    assert!(context.ends_with("Use read_history on chat 42 to see the main chat's full history."));
    let short = make_side_chat_context(42, &items[..2]);
    assert!(!short.contains("truncated"));
    assert!(short.find("- User: 000").unwrap() < short.find("- User: 001").unwrap());
}

#[test]
fn side_context_reserves_the_whole_block_at_the_exact_cap() {
    let framing = "You are in a side chat beside the main chat. The main chat may still be running in the same folder. Answer the user's questions; do not continue or redo the main chat's task.";
    let reserved = [
        framing,
        "",
        "Main chat activity so far:",
        "- Earlier items truncated.",
        "",
        "Use read_history on chat 1 to see the main chat's full history.",
    ]
    .join("\n")
    .chars()
    .count();
    let available = SIDE_CHAT_CONTEXT_MAX_CHARS - reserved;
    let mut items = vec![message(0, Role::User, &"x".repeat(240))];
    for i in 0..available / 249 {
        items.push(message(i + 1, Role::User, &"x".repeat(240)));
    }
    let rest = available % 249;
    assert!(rest >= 9);
    items.push(message(items.len(), Role::User, &"x".repeat(rest - 9)));
    let context = make_side_chat_context(1, &items);
    assert_eq!(context.chars().count(), SIDE_CHAT_CONTEXT_MAX_CHARS);
    assert!(context.contains("- Earlier items truncated."));
}
