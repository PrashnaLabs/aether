use crate::app::chat_with;
use aether_core::Update;

use super::*;

#[test]
fn chat_markdown_lists_each_turn_and_skips_empty_drafts() {
    let mut chat = chat_with("Greet", &["hello", "Hi there"]);
    chat.messages.insert(1, Message::assistant());
    assert_eq!(chat_markdown(&chat), "# Greet\n\n## You\n\nhello\n\n## Aether\n\nHi there\n");
}

#[test]
fn export_filename_sanitizes_and_falls_back() {
    assert_eq!(export_filename("What is Rust?"), "What is Rust.md");
    assert_eq!(export_filename("..."), "chat.md");
    assert_eq!(export_filename("  "), "chat.md");
}

#[test]
fn reply_cost_uses_its_own_model_prices() {
    let mut reply = Message::assistant();
    reply.model = Some("m".into());
    reply.apply(Update::Count { prompt: 2_000_000, completion: 500_000, cached: 0 });
    assert_eq!(reply.prompt_tokens, 2_000_000);
    assert_eq!(reply.completion_tokens, 500_000);
    let prices = HashMap::from([("m".to_string(), (1e-6, 3e-6))]);
    let cost = message_cost(&reply, &prices).unwrap();
    assert!((cost - 3.5).abs() < 1e-9, "got {cost}");
    assert_eq!(message_cost(&reply, &HashMap::new()), None);
    reply.model = None;
    assert_eq!(message_cost(&reply, &prices), None);
}

#[test]
fn chat_spend_prefers_reported_cost_and_falls_back_to_prices() {
    let mut chat = chat_with("c", &["q", "a", "q2", "b"]);
    let prices = HashMap::from([("m".to_string(), (1e-6, 0.))]);
    for ix in [1, 3] {
        chat.messages[ix].model = Some("m".into());
        chat.messages[ix].apply(Update::Count { prompt: 1_000_000, completion: 0, cached: 0 });
    }
    chat.messages[1].cost_micros = 1_234_567;
    assert_eq!(reply_spend(&chat.messages[1], &prices), (1_234_567, false));
    assert_eq!(reply_spend(&chat.messages[3], &prices), (1_000_000, true));
    assert_eq!(chat_spend(&chat, &prices), 2.23);
    assert_eq!(chat_spend(&chat, &HashMap::new()), 1.23);
}

#[test]
fn chat_spend_uses_reported_cost_even_without_model_prices() {
    let mut chat = chat_with("reported", &["q", "a"]);
    chat.messages[1].model = Some("m".into());
    chat.messages[1].prompt_tokens = 1_000_000;
    chat.messages[1].cost_micros = 120_000;
    let prices = HashMap::from([("m".to_string(), (1e-6, 0.))]);
    assert_eq!(message_cost(&chat.messages[1], &prices), Some(1.));
    assert_eq!(chat_spend(&chat, &prices), 0.12);
    assert_eq!(chat_spend(&chat, &HashMap::new()), 0.12);
}

#[test]
fn costs_read_in_cents() {
    assert_eq!(cost_text(0, false), None);
    assert_eq!(cost_text(4_000, false).as_deref(), Some("<$0.01"));
    assert_eq!(cost_text(80_000, false).as_deref(), Some("$0.08"));
    assert_eq!(cost_text(80_000, true).as_deref(), Some("≈$0.08"));
}

#[test]
fn full_counts_and_times_format() {
    assert_eq!(format_full(2_604), "2,604");
    assert_eq!(format_full(103), "103");
    assert_eq!(format_full(1_000_000), "1,000,000");
    assert_eq!(format_cost(0.), "$0");
    assert_eq!(format_cost(0.0234), "$0.0234");
    assert_eq!(format_cost(1.5), "$1.5");
    let clock = format_time(1_700_000_000_000).unwrap();
    assert!(clock.contains(':'), "got {clock}");
    assert_eq!(format_time(0), None, "unknown time");
}

#[test]
fn display_path_shortens_home() {
    let home = std::path::Path::new("/Users/me");
    assert_eq!(display_path("/Users/me/w/x", home), "~/w/x");
    assert_eq!(display_path("/Users/meow/x", home), "/Users/meow/x");
    assert_eq!(display_path("/opt/x", home), "/opt/x");
}
