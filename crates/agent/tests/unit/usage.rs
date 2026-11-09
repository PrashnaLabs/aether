use super::*;

#[test]
fn codex_auth_path_prefers_the_account_dir() {
    let dir = std::path::Path::new("/w/codex");
    assert_eq!(codex_auth_path(Some(dir)), dir.join("auth.json"));
}

#[test]
fn claude_account_without_a_credentials_file_has_no_token() {
    assert_eq!(read_claude_credentials(Some(std::path::Path::new("/nonexistent/aether"))), None);
}

#[test]
fn parses_anthropic_usage_json() {
    let payload = r#"{
            "five_hour": {
                "utilization": 24.5,
                "resets_at": "2026-10-17T15:00:00Z"
            },
            "seven_day": {
                "utilization": 68.0,
                "resets_at": "2026-10-24T00:00:00Z"
            }
        }"#;

    let usage =
        PlanUsage::parse_anthropic(payload, Some("max")).expect("should parse valid usage payload");
    assert_eq!(usage.plan_name, "Claude Max Subscription");
    assert_eq!(usage.limits.len(), 2);
    assert_eq!(usage.limits[0].label, "Session");
    assert!((usage.limits[0].percent_left - 75.5).abs() < 1e-3);
    assert_eq!(usage.limits[1].label, "Weekly");
    assert!((usage.limits[1].percent_left - 32.0).abs() < 1e-3);
}

#[test]
fn anthropic_model_scoped_weekly_limits_are_listed() {
    let payload = r#"{
            "five_hour": {"utilization": 20.0, "resets_at": "2026-10-17T15:00:00Z"},
            "seven_day": {"utilization": 86.0, "resets_at": "2026-10-24T00:00:00Z"},
            "limits": [
                {"kind": "session", "percent": 20, "resets_at": "2026-10-17T15:00:00Z", "scope": null},
                {"kind": "weekly_all", "percent": 86, "resets_at": "2026-10-24T00:00:00Z", "scope": null},
                {"kind": "weekly_scoped", "percent": 30, "resets_at": "2026-10-24T00:00:00Z",
                 "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null}}
            ]
        }"#;

    let usage = PlanUsage::parse_anthropic(payload, Some("max")).expect("parses");
    let labels: Vec<&str> = usage.limits.iter().map(|l| l.label.as_str()).collect();
    assert_eq!(labels, ["Session", "Weekly", "Weekly (Fable)"]);
    assert!((usage.limits[2].percent_left - 70.0).abs() < 1e-3);
    assert!(usage.limits[2].reset_label.starts_with("resets"));
}

#[test]
fn parses_codex_usage_json() {
    let payload = r#"{
            "plan_type": "plus",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 3,
                    "reset_after_seconds": 12106
                },
                "secondary_window": {
                    "used_percent": 50,
                    "reset_after_seconds": 432464
                }
            },
            "rate_limit_reset_credits": {
                "available_count": 1
            }
        }"#;

    let usage = PlanUsage::parse_codex(payload).expect("should parse codex payload");
    assert_eq!(usage.plan_name, "ChatGPT Plus Subscription");
    assert_eq!(usage.limits.len(), 2);
    assert_eq!(usage.limits[0].label, "Session");
    assert_eq!(usage.limits[0].percent_left, 97.0);
    assert_eq!(usage.limits[1].label, "Weekly");
    assert_eq!(usage.limits[1].percent_left, 50.0);
    assert_eq!(usage.footer_action.as_deref(), Some("Use reset"));
    assert!(usage.footer_text.as_deref().unwrap().contains("1 reset credit banked"));
}

#[test]
fn parses_antigravity_quota_text() {
    let text = r#"Quota:
Gemini Models          Weekly Limit Remaining       80%   2026-10-24 06:21 +0545
Gemini Models          Five Hour Limit Remaining    93%   2026-10-17 16:21 +0545
Claude and GPT models  Weekly Limit Remaining       9%    2026-10-22 15:13 +0545
"#;
    let usage = PlanUsage::parse_antigravity(text).expect("should parse antigravity quota text");
    assert_eq!(usage.plan_name, "Google Antigravity Plan");
    assert_eq!(usage.limits.len(), 3);
    assert_eq!(usage.limits[0].label, "Weekly (Gemini)");
    assert_eq!(usage.limits[0].percent_left, 80.0);
    assert_eq!(usage.limits[1].label, "Session (Gemini)");
    assert_eq!(usage.limits[1].percent_left, 93.0);
    assert_eq!(usage.limits[2].label, "Weekly (Claude/GPT)");
    assert_eq!(usage.limits[2].percent_left, 9.0);
}

#[test]
fn parses_piped_antigravity_usage_lines() {
    let text = "Gemini Models\tWeekly Limit Remaining\t70%\t2999-10-24T00:36:01Z\n\
                Claude and GPT models\tFive Hour Limit Remaining\t100%\t2999-10-19T04:39:34Z\n\
                Gemini 4 Argon\tGemini 4 Argon Weekly Limit\t1%\tnot-a-date\n";
    let usage = PlanUsage::parse_antigravity(text).expect("should parse piped usage");
    let labels: Vec<_> = usage.limits.iter().map(|l| l.label.as_str()).collect();
    assert_eq!(labels, ["Weekly (Gemini)", "Session (Claude/GPT)", "Weekly (Gemini 4 Argon)"]);
    assert_eq!(usage.limits[0].percent_left, 70.0);
    assert!(usage.limits[0].reset_label.starts_with("resets in"));
    assert_eq!(usage.limits[2].reset_label, "resets soon");
}

#[test]
fn parses_opencode_usage_limits() {
    let go = r#"{"data":{"workspace":{"id":"wrk_1","usageURL":"u"},
        "go":{"rolling":{"percent":30,"limited":false,"resetsAt":32503680000000},
              "weekly":{"percent":10,"limited":false,"resetsAt":32503680000000},
              "monthly":{"percent":5,"limited":false,"resetsAt":0}},
        "budgets":[{"id":"b","limit":50,"spent":12.5,"resetsAt":32503680000000}]}}"#;
    let usage = PlanUsage::parse_opencode(go).expect("should parse opencode limits");
    assert_eq!(usage.plan_name, "OpenCode Go");
    let labels: Vec<_> = usage.limits.iter().map(|l| l.label.as_str()).collect();
    assert_eq!(labels, ["Session", "Weekly", "Monthly", "Budget · $12.50 of $50.00"]);
    assert_eq!(usage.limits[0].percent_left, 70.0);
    assert_eq!(usage.limits[3].percent_left, 75.0);
    assert!(usage.limits[0].reset_label.starts_with("resets in"));
    assert_eq!(usage.limits[2].reset_label, "resets now");

    let zen = r#"{"data":{"workspace":{"id":"wrk_1","usageURL":"u"},"budgets":[]}}"#;
    let usage = PlanUsage::parse_opencode(zen).expect("signed in, no limits");
    assert_eq!(usage.plan_name, "OpenCode Zen");
    assert!(usage.limits.is_empty());

    assert_eq!(PlanUsage::parse_opencode(r#"{"data":null}"#), None);
}

#[test]
fn extracts_claude_token_and_sub_from_nested() {
    let json = r#"{
            "claudeAiOauth": {
                "accessToken": "sk-ant-oauth-test",
                "subscriptionType": "max"
            }
        }"#;
    let (token, sub) = extract_claude_token_and_sub(json);
    assert_eq!(token, Some("sk-ant-oauth-test".to_string()));
    assert_eq!(sub, Some("max".to_string()));
}

#[test]
fn formats_seconds_countdown_hours_and_days() {
    assert_eq!(format_seconds_countdown(12106), "in 3h 21m");
    assert_eq!(format_seconds_countdown(432464), "in 5d 0h");
}
