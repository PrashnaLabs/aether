use super::*;
use crate::usage_history::{FLAT_RATE, rate_table};

#[test]
fn model_names_normalize_and_rate_table_round_trips() {
    assert_eq!(normalize_model_name("anthropic/Claude-Sonnet-4-5"), "claude-sonnet-4-5");
    let rates = rate_table(&[("claude-sonnet-4-5", FLAT_RATE)]);
    assert!(lookup_rate(&rates, "anthropic/claude-sonnet-4-5").is_some());
    assert!(lookup_rate(&rates, "claude-sonnet-4-5-20250929").is_some());
    assert!(lookup_rate(&rates, "<synthetic>").is_none());

    let doc: Value = serde_json::from_str(
        r#"{
                "claude-sonnet-4-5": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6,
                    "cache_read_input_token_cost": 1e-7, "cache_creation_input_token_cost": 1.25e-6},
                "incomplete": {"input_cost_per_token": 1e-6}
            }"#,
    )
    .unwrap();
    let parsed = parse_rate_table(&doc);
    assert_eq!(parsed.len(), 1);
    let dir = std::env::temp_dir().join(format!("aether-usage-rates-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(RATES_CACHE_FILE);
    write_rates_cache(&path, 99999, &parsed);
    let (ts, restored) = read_rates_cache(&path).expect("round-trip");
    assert_eq!(ts, 99999);
    assert_eq!(restored["claude-sonnet-4-5"], parsed["claude-sonnet-4-5"]);
    let _ = std::fs::remove_dir_all(&dir);
}
