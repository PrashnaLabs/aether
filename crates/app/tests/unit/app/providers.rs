use super::*;

fn cli_model(id: &str, default: bool) -> CliModel {
    CliModel {
        id: id.into(),
        name: id.to_uppercase(),
        provider: "Anthropic".into(),
        default,
        ..Default::default()
    }
}

#[test]
fn default_row_means_no_model_flag() {
    let opencode = cli_items(Provider::OpenCode, &[cli_model("a/b", false)], "");
    assert_eq!(picker_value(Provider::OpenCode, "", &opencode), CLI_DEFAULT);
    assert_eq!(stored_model(Provider::OpenCode, CLI_DEFAULT), "");
    let flagged = [cli_model("opus", false), cli_model("default", true)];
    let claude = cli_items(Provider::Claude, &flagged, "");
    assert_eq!(picker_value(Provider::Claude, "", &claude), "default");
    assert_eq!(picker_value(Provider::Claude, "opus", &claude), "opus");
    assert_eq!(picker_value(Provider::OpenRouter, "a/b", &[]), "a/b");
    assert_eq!(picker_value(Provider::OpenRouter, "", &[]), "");
    let mut s = Settings::default();
    s.provider_models.insert("claude".into(), "opus".into());
    set_default_model(&mut s, Provider::Claude, String::new());
    let opts = aether_driver::DriverStartOptions::for_provider(&s, Provider::Claude, "/");
    assert_eq!(opts.model, "");
}

#[test]
fn cli_items_name_their_source_and_add_default_and_custom_rows() {
    let models = [cli_model("opus", false), cli_model("sonnet", false)];
    let items = cli_items(Provider::Claude, &models, "my-model");
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_ref()).collect();
    assert_eq!(ids, [CLI_DEFAULT, "my-model", "opus", "sonnet"]);
    assert_eq!(items[0].detail, "Claude Code config");
    assert_eq!(items[1], ModelItem::custom("my-model"));
    assert_eq!(items[2].name, "OPUS");
    assert_eq!(items[2].detail, "Anthropic · Claude Code");
    let flagged = [cli_model("opus", false), cli_model("default", true)];
    let items = cli_items(Provider::Claude, &flagged, "opus");
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_ref()).collect();
    assert_eq!(ids, ["opus", "default"]);
    assert!(items[1].default);
}

#[test]
fn openrouter_rows_show_names_under_their_upstream_provider() {
    let info = ModelInfo {
        id: "openai/gpt-5".into(),
        name: "OpenAI: GPT-5".into(),
        context_length: None,
        prompt_price: None,
        completion_price: None,
    };
    let item = openrouter_item(&info);
    assert_eq!((item.id.as_ref(), item.name.as_ref()), ("openai/gpt-5", "OpenAI: GPT-5"));
    assert_eq!(item.detail, "OpenAI · OpenRouter");
    assert!(!item.default);
}

#[test]
fn detail_names_a_provider_once_when_upstream_is_the_source() {
    assert_eq!(detail("Anthropic", "Claude Code"), "Anthropic · Claude Code");
    assert_eq!(detail("OpenCode", "OpenCode"), "OpenCode");
    assert_eq!(detail("opencode", "OpenCode"), "opencode");
    let mut model = cli_model("big-pickle", false);
    model.provider = "OpenCode".into();
    assert_eq!(cli_items(Provider::OpenCode, &[model], "")[1].detail, "OpenCode");
    let info = ModelInfo {
        id: "openrouter/auto".into(),
        name: "Auto Router".into(),
        context_length: None,
        prompt_price: None,
        completion_price: None,
    };
    assert_eq!(openrouter_item(&info).detail, "OpenRouter");
}

#[test]
fn selector_hides_missing_clis_but_never_the_chats_own() {
    assert!(rail_hidden(Provider::Pi, Provider::OpenRouter, Some(false)));
    assert!(!rail_hidden(Provider::Pi, Provider::OpenRouter, None));
    assert!(!rail_hidden(Provider::Claude, Provider::OpenRouter, Some(true)));
    assert!(!rail_hidden(Provider::Pi, Provider::Pi, Some(false)));
}

#[test]
fn default_context_limit_covers_all_providers() {
    assert_eq!(default_context_limit(Provider::Claude, ""), 1_000_000);
    assert_eq!(default_context_limit(Provider::Claude, "opus"), 1_000_000);
    assert_eq!(default_context_limit(Provider::Claude, "claude-sonnet-4-6"), 200_000);
    assert_eq!(default_context_limit(Provider::Claude, "claude-opus-4-6[1m]"), 1_000_000);
    assert_eq!(default_context_limit(Provider::Codex, "gpt-5.6-luna"), 258_400);
    assert_eq!(default_context_limit(Provider::Antigravity, "gemini-3.8-flash-medium"), 1_048_576);
    assert_eq!(default_context_limit(Provider::Antigravity, "gpt-oss-120b-medium"), 131_072);
    assert_eq!(default_context_limit(Provider::OpenCode, ""), 200_000);
    assert_eq!(default_context_limit(Provider::Pi, ""), 200_000);
    assert_eq!(default_context_limit(Provider::OpenRouter, "google/gemini-3"), 1_000_000);
    assert_eq!(default_context_limit(Provider::OpenRouter, "custom-model"), 200_000);
}

#[test]
fn catalog_context_matches_the_model_or_the_cli_default() {
    let mut a = cli_model("a", false);
    a.context = Some(10);
    let mut b = cli_model("b", true);
    b.context = Some(20);
    let models = [a, b];
    assert_eq!(catalog_context(&models, "a"), Some(10));
    assert_eq!(catalog_context(&models, ""), Some(20));
    assert_eq!(catalog_context(&models, "zzz"), None);
}

#[test]
fn usage_refetches_only_when_stale() {
    let min = 60_000;
    let now = 100 * min;
    assert!(!usage_due(Some(now - 2 * min), now - 2 * min, true, now), "fresh data");
    assert!(usage_due(Some(now - 11 * min), now - 11 * min, false, now), "old data");
    assert!(!usage_due(Some(now - 11 * min), now - 3 * min, true, now), "just retried");
    assert!(usage_due(None, now - 11 * min, true, now), "none yet, configured");
    assert!(!usage_due(None, now - 11 * min, false, now), "none yet, nothing configured");
    assert!(!usage_due(None, now - min, true, now), "none yet, just tried");
}
