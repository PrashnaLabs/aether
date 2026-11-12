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
