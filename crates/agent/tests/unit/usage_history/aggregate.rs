use super::*;
use crate::usage_history::{FLAT_RATE, rate_table};

#[test]
fn aggregator_combines_all_six_providers_and_computes_savings() {
    let rates = rate_table(&[("claude-sonnet-4-5", FLAT_RATE), ("gemini-2.5-pro", FLAT_RATE)]);
    let today = Local::now().date_naive();
    let roots = [PathBuf::from("/Users/me/dev/aether")];
    let mut agg = Aggregator::new(today - chrono::Days::new(7), today, &roots);
    let now = Local::now().timestamp_millis();

    for (provider, model, session) in [
        (UsageProvider::Claude, "claude-sonnet-4-5", "s-claude"),
        (UsageProvider::Codex, "claude-sonnet-4-5", "s-codex"),
        (UsageProvider::Pi, "claude-sonnet-4-5", "s-pi"),
        (UsageProvider::OpenCode, "claude-sonnet-4-5", "s-oc"),
        (UsageProvider::Antigravity, "gemini-2.5-pro", "s-agy"),
        (UsageProvider::OpenRouter, "claude-sonnet-4-5", "s-or"),
    ] {
        agg.add(
            &UsageRecord {
                provider,
                timestamp_ms: now,
                model: model.to_owned(),
                session_id: session.to_owned(),
                project: "/Users/me/dev/aether/crates/app".to_owned(),
                totals: TokenTotals {
                    uncached_input: 1_000,
                    cached_input: 2_000,
                    cache_creation: 0,
                    output: 500,
                    reasoning: 0,
                },
                reported_cost_usd: None,
                dedupe_key: Some(format!("k:{session}")),
            },
            &rates,
        );
    }

    let history = derive_history(
        agg,
        UsageWindow::TrailingDays(7),
        today - chrono::Days::new(6),
        today,
        PricingStatus::Fresh,
        6,
        0,
        Vec::new(),
        Duration::ZERO,
    );
    assert_eq!(history.records, 6);
    assert_eq!(history.sessions, 6);
    assert_eq!(history.providers.len(), 6);
    assert_eq!(history.projects.len(), 1);
    assert_eq!(history.projects[0].path, "/Users/me/dev/aether");
    assert_eq!(history.projects[0].sessions, 6);
}
