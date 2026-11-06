use super::*;
use crate::project_page::model::CardStatus;
use aether_core::Provider;
use aether_driver::ProviderProbe;
use std::path::PathBuf;

fn card(id: i64, activity_ms: i64) -> Card {
    Card {
        ix: 0,
        id,
        title: String::new(),
        provider: Provider::Claude,
        model: String::new(),
        status: CardStatus::Idle,
        since_ms: 0,
        place: String::new(),
        added: 0,
        removed: 0,
        activity_ms,
        text: String::new(),
        cwd: PathBuf::new(),
    }
}

#[test]
fn headline_names_the_project_for_agent_chats() {
    assert_eq!(
        start_headline(true, Some("aether")),
        ("What should we build in ".to_string(), Some("aether".to_string()), "?".to_string())
    );
    assert_eq!(
        start_headline(true, None),
        ("What should we build?".to_string(), None, String::new())
    );
    assert_eq!(
        start_headline(false, Some("aether")),
        ("What can I help with?".to_string(), None, String::new())
    );
}

#[test]
fn recents_skip_the_draft_and_keep_the_newest_four() {
    let cards: Vec<Card> = (1..=6).map(|i| card(i, i * 10)).collect();
    let ids: Vec<i64> = recent_chats(cards, 6, 4).iter().map(|c| c.id).collect();
    assert_eq!(ids, [5, 4, 3, 2]);
}

#[test]
fn health_reads_the_probe() {
    let ready = ProviderProbe {
        installed: true,
        version: Some("2.0.32".into()),
        status: "Service running on :4096".into(),
        ..ProviderProbe::default()
    };
    assert_eq!(health(None), Health::Checking);
    assert_eq!(health(Some(&ProviderProbe::default())), Health::Missing);
    assert_eq!(health(Some(&ready)), Health::Ready("Service running on :4096".into()));
    let degraded = ProviderProbe {
        degraded: Some("Auto mode unavailable on this plan; runs in Default".into()),
        ..ready.clone()
    };
    assert_eq!(
        health(Some(&degraded)),
        Health::Degraded("Auto mode unavailable on this plan; runs in Default".into())
    );
    assert_eq!(version_text(Provider::OpenCode, Some(&ready)), "2.0.32");
    assert_eq!(version_text(Provider::OpenRouter, None), "built-in");
    assert_eq!(version_chip("OpenCode", "2.0.32"), "OpenCode 2.0.32");
    assert_eq!(version_chip("Claude Code", ""), "Claude Code");
}

#[test]
fn service_only_opencode_is_ready_not_missing() {
    let service = ProviderProbe {
        installed: true,
        status: "Service running".into(),
        ..ProviderProbe::default()
    };
    assert_eq!(health(Some(&service)), Health::Ready("Service running".into()));
}

#[test]
fn openrouter_health_counts_models() {
    assert_eq!(openrouter_health(true, 42), Health::Ready("Key set · 42 models".into()));
    assert_eq!(openrouter_health(false, 0), Health::Degraded("Add an API key in Settings".into()));
}

#[test]
fn starters_follow_the_chats_folder() {
    assert_eq!(starters(false, false), GENERAL_STARTERS);
    assert_eq!(starters(false, true), GENERAL_STARTERS);
    assert_eq!(starters(true, true), FOLDER_STARTERS);
    assert_eq!(starters(true, false), CODE_STARTERS);
}
