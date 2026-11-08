use std::time::{Duration, Instant};

use aether_core::{Provider, Settings};

use super::*;
use crate::DriverStartOptions;

fn model(id: &str, name: &str, provider: &str, default: bool) -> CliModel {
    CliModel {
        id: id.into(),
        name: name.into(),
        provider: provider.into(),
        default,
        ..Default::default()
    }
}

fn with(mut m: CliModel, context: Option<u64>, efforts: Option<&[&str]>) -> CliModel {
    m.context = context;
    m.efforts = efforts.map(|e| e.iter().map(|s| s.to_string()).collect());
    m
}

#[cfg(windows)]
fn cmd_opts() -> DriverStartOptions {
    DriverStartOptions { binary: std::env::var("ComSpec").unwrap(), ..sh_opts() }
}

/// A program the probe can run: exits at once with no output.
#[cfg(windows)]
fn fake_binary() -> String {
    let path = std::env::temp_dir().join(format!("aether-fake-cli-{}.cmd", std::process::id()));
    std::fs::write(&path, "@exit /b 0\r\n").unwrap();
    path.display().to_string()
}

#[cfg(not(windows))]
fn fake_binary() -> String {
    "/bin/sh".into()
}

fn sh_opts() -> DriverStartOptions {
    DriverStartOptions {
        ephemeral: false,
        binary: "/bin/sh".into(),
        cwd: "/".into(),
        model: String::new(),
        effort: String::new(),
        access: Default::default(),
        resume: String::new(),
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    }
}

#[test]
fn claude_initialize_lists_models_with_names_and_default() {
    let reply = json!({"type": "control_response", "response": {
    "subtype": "success", "request_id": "aether-initialize", "response": {"models": [
        {"value": "default", "displayName": "Default (recommended)", "description": "Opus 4.6",
         "supportsEffort": true, "supportedEffortLevels": ["low", "max"]},
        {"value": "sonnet", "displayName": "  Sonnet  "},
        {"value": "custom", "resolvedModel": "claude-haiku-4-5", "isDefault": true},
        {"value": "  "},
        {"value": "opus", "displayName": ""}
    ]}}});
    assert_eq!(
        parse_claude_models(&reply),
        [
            CliModel {
                description: "Opus 4.6".into(),
                ..with(
                    model("default", "Default (recommended)", "Anthropic", true),
                    None,
                    Some(&["low", "max"])
                )
            },
            with(model("sonnet", "Sonnet", "Anthropic", false), None, Some(&[])),
            with(model("custom", "Claude Haiku 4 5", "Anthropic", true), None, Some(&[])),
            with(model("opus", "Opus", "Anthropic", false), None, Some(&[])),
        ]
    );
    assert!(parse_claude_models(&json!({"type": "control_response"})).is_empty());
}

#[test]
fn pi_rpc_models_take_names_and_the_state_default() {
    let models = json!({"id": "aether-models", "type": "response", "success": true, "data": {"models": [
        {"provider": "openrouter", "id": "anthropic/claude-x", "name": " Anthropic: Claude X ",
         "contextWindow": 1000000, "reasoning": true, "thinkingLevelMap": {"off": null, "max": "max"}},
        {"provider": "opencode", "id": "a", "name": "Keep: Me"},
        {"provider": "anthropic", "id": "claude-opus-4-6", "name": " "},
        {"provider": "", "id": "orphan"}
    ]}});
    let state = json!({"id": "aether-state", "type": "response", "success": true,
        "data": {"model": {"provider": "anthropic", "id": "claude-opus-4-6"}}});
    assert_eq!(
        parse_pi_models(&models, &state),
        [
            with(
                model("openrouter/anthropic/claude-x", "Claude X", "OpenRouter", false),
                Some(1_000_000),
                Some(&["minimal", "low", "medium", "high", "max"])
            ),
            with(model("opencode/a", "Keep: Me", "OpenCode", false), None, Some(&[])),
            with(
                model("anthropic/claude-opus-4-6", "Claude Opus 4 6", "Anthropic", true),
                None,
                Some(&[])
            ),
        ]
    );
    assert!(parse_pi_models(&models, &Value::Null).iter().all(|m| !m.default));
}

#[test]
fn opencode_api_models_keep_enabled_ones() {
    let v = json!({"location": {}, "data": [
        {"id": "gemini-4-argon", "modelID": "gemini-4-argon", "providerID": "google",
         "name": "Gemini 4 Argon", "enabled": true, "status": "active", "limit": {"context": 1, "output": 1},
         "variants": [{"id": "low"}, {"id": "high"}]},
        {"id": "auto", "modelID": "auto", "providerID": "openrouter", "name": "", "enabled": true},
        {"id": "old", "modelID": "old", "providerID": "opencode", "name": "Old", "enabled": false}
    ]});
    assert_eq!(
        parse_opencode_models(&v, Some("google/gemini-4-argon"), None),
        [
            with(
                model("google/gemini-4-argon", "Gemini 4 Argon", "Google", true),
                Some(1),
                Some(&["low", "high"])
            ),
            model("openrouter/auto", "auto", "OpenRouter", false),
        ]
    );
    let active = HashSet::from(["google".to_string()]);
    assert_eq!(
        parse_opencode_models(&v, Some("google/gemini-4-argon"), Some(&active)),
        [with(
            model("google/gemini-4-argon", "Gemini 4 Argon", "Google", true),
            Some(1),
            Some(&["low", "high"])
        )]
    );
}

#[test]
fn provider_labels_are_readable() {
    assert_eq!(provider_label("openrouter"), "OpenRouter");
    assert_eq!(provider_label("opencode"), "OpenCode");
    assert_eq!(provider_label("anthropic"), "Anthropic");
    assert_eq!(provider_label("openai"), "OpenAI");
    assert_eq!(provider_label("github-copilot"), "GitHub Copilot");
    assert_eq!(provider_label("meta-llama"), "Meta Llama");
}

#[test]
fn pi_list_reads_provider_and_model_columns_after_the_header() {
    let out = "Warning: update available\n\
               provider    model            context  max-out\n\
               opencode    big-pickle       200K     32K\n\
               anthropic   claude-opus-4-6  1M       128K\n\n";
    assert_eq!(
        parse_pi_list(out),
        [
            model("opencode/big-pickle", "big-pickle", "OpenCode", false),
            model("anthropic/claude-opus-4-6", "claude-opus-4-6", "Anthropic", false),
        ]
    );
    assert!(parse_pi_list("no table here").is_empty());
}

#[test]
fn opencode_list_keeps_slash_ids_only() {
    let out = "google/gemini-2.5-pro\n\n  openrouter/openai/gpt-5  \nINFO 2026/10/14 started\n";
    assert_eq!(
        parse_opencode_list(out),
        [
            model("google/gemini-2.5-pro", "gemini-2.5-pro", "Google", false),
            model("openrouter/openai/gpt-5", "openai/gpt-5", "OpenRouter", false),
        ]
    );
}

#[test]
fn claude_falls_back_to_named_aliases_only_when_found() {
    let missing =
        Settings { claude_binary: "/definitely/not/here-xyz".into(), ..Settings::default() };
    assert_eq!(
        probe(Provider::Claude, &missing),
        ProviderProbe { status: "Not installed".into(), ..ProviderProbe::default() }
    );
    let found = Settings { claude_binary: fake_binary(), ..Settings::default() };
    let probe = probe(Provider::Claude, &found);
    assert!(probe.installed);
    assert_eq!(
        probe.models,
        [
            model("opus", "Opus", "Anthropic", false),
            model("sonnet", "Sonnet", "Anthropic", false),
            model("haiku", "Haiku", "Anthropic", false),
        ]
    );
}

#[cfg(unix)]
#[test]
fn rpc_matches_replies_by_id_and_stops_at_a_silent_one() {
    let script =
        r#"read a; echo noise; echo '{"id":"other"}'; echo '{"id":"one","n":1}'; read b; sleep 5"#;
    let mut cmd = command(&sh_opts(), "sh").unwrap();
    cmd.args(["-c", script]);
    let start = Instant::now();
    let replies = rpc(
        cmd,
        &[("one", json!({"id": "one"})), ("two", json!({"id": "two"}))],
        Duration::from_millis(300),
    );
    assert_eq!(replies, [json!({"id": "one", "n": 1})]);
    assert!(start.elapsed() < Duration::from_secs(3));
    let mut cmd = command(&sh_opts(), "sh").unwrap();
    cmd.args(["-c", r#"read a; echo '{"response":{"request_id":"init","ok":true}}'"#]);
    let replies = rpc(cmd, &[("init", json!({"request_id": "init"}))], Duration::from_secs(5));
    assert_eq!(replies, [json!({"response": {"request_id": "init", "ok": true}})]);
}

fn sh_rpc(script: &str, timeout: Duration) -> Vec<Value> {
    let mut cmd = command(&sh_opts(), "sh").unwrap();
    cmd.args(["-c", script]);
    rpc(cmd, &[("one", json!({"id": "one"}))], timeout)
}

#[cfg(unix)]
#[test]
fn rpc_reads_bad_utf8_lossily_and_stops_on_an_oversized_line() {
    let bad = r#"read a; printf '{"id":"one","s":"\377"}\n'"#;
    let replies = sh_rpc(bad, Duration::from_secs(5));
    assert_eq!(replies, [json!({"id": "one", "s": "\u{fffd}"})]);
    let huge = format!(
        r#"read a; head -c {} /dev/zero | tr '\0' a; echo; echo '{{"id":"one"}}'; sleep 5"#,
        MAX_LINE + 1
    );
    let start = Instant::now();
    assert!(sh_rpc(&huge, Duration::from_secs(5)).is_empty());
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[cfg(unix)]
#[test]
fn rpc_kills_the_probe_process_group() {
    let script = r#"read a; sleep 30 & echo "{\"id\":\"one\",\"pid\":$!}"; sleep 30"#;
    let replies = sh_rpc(script, Duration::from_secs(5));
    let pid = replies[0]["pid"].as_u64().unwrap().to_string();
    let alive = || {
        Command::new("/bin/kill")
            .args(["-0", &pid])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(), "grandchild {pid} survived");
}

#[test]
fn hung_list_command_times_out() {
    #[cfg(unix)]
    let (opts, hang, quick, out) = (sh_opts(), ["-c", "sleep 5"], ["-c", "echo a/b"], "a/b\n");
    #[cfg(windows)]
    let (opts, hang, quick, out) =
        (cmd_opts(), ["/C", "ping -n 6 127.0.0.1"], ["/C", "echo a/b"], "a/b\r\n");
    let start = Instant::now();
    assert_eq!(list_output(&opts, "sh", &hang, Duration::from_millis(200)), None);
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(list_output(&opts, "sh", &quick, Duration::from_secs(5)).as_deref(), Some(out));
}

#[test]
fn success_path_is_bounded_when_a_grandchild_holds_stdout() {
    let opts = sh_opts();
    let start = Instant::now();
    assert_eq!(
        list_output(&opts, "sh", &["-c", "(sleep 5 &) ; echo a/b"], Duration::from_millis(300)),
        None
    );
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[test]
#[ignore]
fn live_probe_lists_models() {
    for provider in [Provider::Claude, Provider::Pi, Provider::OpenCode] {
        let probe = probe(provider, &Settings::default());
        println!(
            "== {} installed={} models={}",
            provider.label(),
            probe.installed,
            probe.models.len()
        );
        for m in &probe.models {
            let default = if m.default { " [default]" } else { "" };
            println!("{}/{} ({}){default}", m.provider, m.name, m.id);
        }
    }
}

#[test]
fn parses_and_sorts_codex_models_from_json() {
    let json = json!({
        "models": [
            {
                "slug": "gpt-6.1-sol",
                "display_name": "GPT-6.1-Sol",
                "description": "Latest workhorse model for coding and everyday work.",
                "visibility": "list",
                "priority": 1,
                "context_window": 500000,
                "effective_context_window_percent": 95
            },
            {
                "slug": "gpt-reserve",
                "display_name": "GPT-Reserve",
                "visibility": "hide",
                "priority": 4
            },
            {
                "slug": "gpt-6-astra",
                "display_name": "GPT-6-Astra",
                "visibility": "list",
                "priority": 2
            },
            {
                "slug": "gpt-5.6-luna",
                "display_name": "GPT-5.6-Luna",
                "visibility": "list",
                "priority": 9
            }
        ]
    });
    let models = parse_codex_models_json(&json);
    assert_eq!(models.len(), 3);
    assert_eq!(models[0].id, "gpt-6.1-sol");
    assert_eq!(models[0].name, "GPT-6.1-Sol");
    assert_eq!(models[0].provider, "OpenAI");
    assert_eq!(models[0].description, "Latest workhorse model for coding and everyday work.");
    assert!(!models[0].default);
    assert_eq!(models[0].context, Some(475_000));

    assert_eq!(models[1].id, "gpt-6-astra");
    assert_eq!(models[1].context, None);
    assert_eq!(models[1].name, "GPT-6-Astra");
    assert_eq!(models[1].provider, "OpenAI");

    assert_eq!(models[2].id, "gpt-5.6-luna");
    assert_eq!(models[2].name, "GPT-5.6-Luna");
    assert_eq!(models[2].provider, "OpenAI");
}

#[test]
fn static_codex_models_provides_standard_models() {
    let models = static_codex_models();
    assert!(!models.is_empty());
    assert_eq!(models[0].id, "gpt-6.1-sol");
    assert_eq!(models[0].provider, "OpenAI");
    assert!(models.iter().any(|m| m.id == "gpt-6-astra"));
    assert!(models.iter().all(|m| m.provider == "OpenAI"));
}

#[test]
fn vendor_prefixes_are_stripped_only_when_short_and_single() {
    assert_eq!(strip_vendor(" Anthropic: Claude X "), "Claude X");
    assert_eq!(strip_vendor("MoonshotAI: Kimi K2.6"), "Kimi K2.6");
    assert_eq!(strip_vendor("GPT-4o"), "GPT-4o");
    assert_eq!(strip_vendor("A: B: C"), "A: B: C");
    assert_eq!(strip_vendor("Anthropic: "), "Anthropic:");
    let long = "A vendor name over the cap: X";
    assert_eq!(strip_vendor(long), long);
}

#[test]
fn antigravity_list_reads_tab_separated_ids_and_labels() {
    let out = "gemini-3.8-flash-medium\tGemini 3.8 Flash (Medium)\nclaude-opus-5-5-low\tClaude Opus 5.5 (Low)\nbare-id\n";
    let models = parse_antigravity_list(out);
    let fixed = Some(&[][..]);
    assert_eq!(
        models[0],
        with(
            model("gemini-3.8-flash-medium", "Gemini 3.8 Flash (Medium)", "Google", false),
            None,
            fixed
        )
    );
    assert_eq!(
        models[1],
        with(
            model("claude-opus-5-5-low", "Claude Opus 5.5 (Low)", "Anthropic", false),
            None,
            fixed
        )
    );
    assert_eq!((models[2].name.as_str(), models[2].efforts.clone()), ("Bare Id", None));
}

#[test]
fn command_lists_keep_named_entries_and_first_description_lines() {
    let list = [
        json!({"name": "review", "description": "review changes\nmore"}),
        json!({"name": "/init", "description": ""}),
        json!({"description": "nameless"}),
    ];
    assert_eq!(
        commands(&list, "name", "description"),
        [
            CliCommand { name: "review".into(), description: "review changes".into() },
            CliCommand { name: "init".into(), description: String::new() },
        ]
    );
}

#[test]
fn probe_status_says_why() {
    use crate::version::Support;
    assert_eq!(probe_status(false, false, None), ("Not installed".into(), None));
    assert_eq!(
        probe_status(true, false, Some(&Support::Degraded("old".into()))),
        ("Limited".into(), Some("old".into()))
    );
    assert_eq!(
        probe_status(true, false, Some(&Support::Unsupported("no".into()))),
        ("Update needed".into(), Some("no".into()))
    );
    assert_eq!(probe_status(false, true, None), ("Service running".into(), None));
    assert_eq!(probe_status(false, true, Some(&Support::Ok)), ("Service running".into(), None));
    assert_eq!(
        probe_status(false, true, Some(&Support::Unsupported("no".into()))),
        ("Service running".into(), None)
    );
    assert_eq!(probe_status(true, false, None), ("Ready".into(), None));
}
