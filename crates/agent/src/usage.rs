use aether_core::home_dir;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanLimit {
    pub label: String,
    pub percent_left: f32,
    pub reset_label: String,
    pub threshold: Option<f32>,
    pub trend_down: bool,
}

impl PlanLimit {
    pub fn new(
        label: impl Into<String>,
        percent_left: f32,
        reset_label: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            percent_left: percent_left.clamp(0.0, 100.0),
            reset_label: reset_label.into(),
            threshold: None,
            trend_down: true,
        }
    }

    pub fn threshold(mut self, threshold: Option<f32>) -> Self {
        self.threshold = threshold;
        self
    }

    pub fn trend_down(mut self, trend_down: bool) -> Self {
        self.trend_down = trend_down;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanWindow {
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanUsage {
    pub plan_name: String,
    pub limits: Vec<PlanLimit>,
    pub footer_text: Option<String>,
    pub footer_action: Option<String>,
    pub primary: Option<PlanWindow>,
    pub secondary: Option<PlanWindow>,
}

impl PlanUsage {
    pub fn parse_anthropic(json: &str, subscription_hint: Option<&str>) -> Option<Self> {
        let val: serde_json::Value = serde_json::from_str(json).ok()?;
        let now = Utc::now();

        let parse_window = |obj: &serde_json::Value| -> Option<PlanWindow> {
            let used = obj
                .get("utilization")
                .or_else(|| obj.get("used_percent"))
                .and_then(|v| v.as_f64())?;
            let used_percent = if used > 0.0 && used <= 1.0 { used * 100.0 } else { used };
            let resets_at = obj
                .get("resets_at")
                .and_then(|v| v.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc));
            Some(PlanWindow { used_percent, resets_at })
        };

        let primary = val.get("five_hour").and_then(parse_window);
        let secondary = val.get("seven_day").and_then(parse_window);

        if primary.is_none() && secondary.is_none() {
            return None;
        }

        let plan_name = match subscription_hint.unwrap_or("").to_lowercase().as_str() {
            "max" => "Claude Max Subscription".to_string(),
            "pro" => "Claude Pro Subscription".to_string(),
            "team" => "Claude Team Subscription".to_string(),
            "enterprise" => "Claude Enterprise Subscription".to_string(),
            _ => "Claude Max Subscription".to_string(),
        };

        let mut limits = Vec::new();
        if let Some(ref p) = primary {
            let left = (100.0 - p.used_percent as f32).clamp(0.0, 100.0);
            let reset = match p.resets_at {
                Some(at) => format!("resets {}", format_reset_countdown(at, now)),
                None if (p.used_percent as f32) <= 0.001 => "resets on idle".to_string(),
                None => "resets in 5h".to_string(),
            };
            limits.push(
                PlanLimit::new("Session", left, reset).threshold(Some(75.0)).trend_down(true),
            );
        }

        let mut weekly_depleted = false;
        let mut weekly_reset_str = String::new();
        if let Some(ref s) = secondary {
            let left = (100.0 - s.used_percent as f32).clamp(0.0, 100.0);
            if left <= 0.001 {
                weekly_depleted = true;
            }
            let reset = match s.resets_at {
                Some(at) => {
                    let text = format!("resets {}", format_reset_countdown(at, now));
                    weekly_reset_str = format_reset_countdown(at, now);
                    text
                }
                None => "resets in 7d".to_string(),
            };
            limits.push(
                PlanLimit::new("Weekly", left, reset).threshold(Some(50.0)).trend_down(false),
            );
        }

        let footer_text = if weekly_depleted && !weekly_reset_str.is_empty() {
            Some(format!("Weekly limit reached · Resets {weekly_reset_str}"))
        } else {
            Some("All limits active · Reset timer running".to_string())
        };

        Some(Self {
            plan_name,
            limits,
            footer_text,
            footer_action: Some("Dashboard".to_string()),
            primary,
            secondary,
        })
    }

    pub fn parse_codex(json: &str) -> Option<Self> {
        let val: serde_json::Value = serde_json::from_str(json).ok()?;

        let plan_type = val.get("plan_type").and_then(|v| v.as_str()).unwrap_or("plus");
        let plan_name = match plan_type.to_lowercase().as_str() {
            "plus" => "ChatGPT Plus Subscription".to_string(),
            "pro" => "ChatGPT Pro Subscription".to_string(),
            "team" => "ChatGPT Team Subscription".to_string(),
            "enterprise" => "ChatGPT Enterprise Subscription".to_string(),
            _ => "ChatGPT Subscription".to_string(),
        };

        let mut limits = Vec::new();
        let rate_limit = val.get("rate_limit");

        let primary_window = rate_limit.and_then(|rl| rl.get("primary_window"));
        let mut primary_win = None;
        if let Some(pw) = primary_window {
            let used = pw.get("used_percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let left = (100.0 - used as f32).clamp(0.0, 100.0);
            let secs = pw.get("reset_after_seconds").and_then(|v| v.as_u64()).unwrap_or(14400);
            let reset = format!("resets {}", format_seconds_countdown(secs));
            limits.push(
                PlanLimit::new("Session", left, reset).threshold(Some(75.0)).trend_down(true),
            );
            primary_win = Some(PlanWindow { used_percent: used, resets_at: None });
        }

        let secondary_window = rate_limit.and_then(|rl| rl.get("secondary_window"));
        let mut secondary_win = None;
        if let Some(sw) = secondary_window {
            let used = sw.get("used_percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let left = (100.0 - used as f32).clamp(0.0, 100.0);
            let secs = sw.get("reset_after_seconds").and_then(|v| v.as_u64()).unwrap_or(432000);
            let reset = format!("resets {}", format_seconds_countdown(secs));
            limits.push(
                PlanLimit::new("Weekly", left, reset).threshold(Some(50.0)).trend_down(false),
            );
            secondary_win = Some(PlanWindow { used_percent: used, resets_at: None });
        }

        let banked_credits = val
            .pointer("/rate_limit_reset_credits/available_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let (footer_text, footer_action) = if banked_credits > 0 {
            (
                Some(format!(
                    "{banked_credits} reset credit{} banked · next expires in 20d 17h",
                    if banked_credits == 1 { "" } else { "s" }
                )),
                Some("Use reset".to_string()),
            )
        } else {
            (
                Some("All limits active · Reset timer running".to_string()),
                Some("Dashboard".to_string()),
            )
        };

        Some(Self {
            plan_name,
            limits,
            footer_text,
            footer_action,
            primary: primary_win,
            secondary: secondary_win,
        })
    }

    pub fn parse_antigravity(text: &str) -> Option<Self> {
        if !text.contains("Quota:") && !text.contains("Limit") {
            return None;
        }

        let mut limits = Vec::new();
        let now = Utc::now();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("Quota:") {
                continue;
            }

            // piped `/usage`: group, label, percent and an RFC 3339 reset, tab-separated
            if let [group, label, pct, reset, ..] =
                trimmed.split('\t').map(str::trim).collect::<Vec<_>>()[..]
                && let Ok(pct) = pct.trim_end_matches('%').parse::<f32>()
            {
                let who = match group {
                    "Gemini Models" => "Gemini",
                    g if g.starts_with("Claude") => "Claude/GPT",
                    g => g,
                };
                let (name, threshold, trend_down) = if label.contains("Five Hour") {
                    (format!("Session ({who})"), 75.0, true)
                } else {
                    (format!("Weekly ({who})"), 50.0, false)
                };
                let countdown = DateTime::parse_from_rfc3339(reset).map_or_else(
                    |_| "resets soon".to_string(),
                    |dt| format!("resets {}", format_reset_countdown(dt.with_timezone(&Utc), now)),
                );
                limits.push(
                    PlanLimit::new(name, pct, countdown)
                        .threshold(Some(threshold))
                        .trend_down(trend_down),
                );
                continue;
            }

            // Example line: "Gemini Models Five Hour Limit Remaining 93% 2026-10-17 16:21 +0545"
            // Example line: "Gemini Models Weekly Limit Remaining 80% 2026-10-24 06:21 +0545"
            // Example line: "Claude and GPT models Weekly Limit Remaining 9% 2026-10-22 15:13 +0545"
            if let Some(pct_idx) = trimmed.find('%') {
                let prefix = &trimmed[..pct_idx];
                let suffix = trimmed[pct_idx + 1..].trim();

                let num_start =
                    prefix.rfind(|c: char| !c.is_ascii_digit()).map(|i| i + 1).unwrap_or(0);
                if let Ok(pct) = prefix[num_start..].parse::<f32>() {
                    let label_part = prefix[..num_start].trim();
                    let (name, threshold, trend_down) = if label_part.contains("Five Hour") {
                        if label_part.contains("Claude") {
                            ("Session (Claude/GPT)", Some(75.0), true)
                        } else {
                            ("Session (Gemini)", Some(75.0), true)
                        }
                    } else if label_part.contains("Weekly") {
                        if label_part.contains("Claude") {
                            ("Weekly (Claude/GPT)", Some(50.0), false)
                        } else {
                            ("Weekly (Gemini)", Some(50.0), false)
                        }
                    } else {
                        (label_part, Some(75.0), true)
                    };

                    let countdown = if let Ok(dt) =
                        DateTime::parse_from_str(suffix, "%Y-%m-%d %H:%M %z")
                    {
                        format!("resets {}", format_reset_countdown(dt.with_timezone(&Utc), now))
                    } else {
                        "resets soon".to_string()
                    };

                    limits.push(
                        PlanLimit::new(name, pct, countdown)
                            .threshold(threshold)
                            .trend_down(trend_down),
                    );
                }
            }
        }

        if limits.is_empty() {
            return None;
        }

        Some(Self {
            plan_name: "Google Antigravity Plan".to_string(),
            limits,
            footer_text: Some("Gemini 2.0 & Frontier models active · Auto-refreshed".to_string()),
            footer_action: Some("Dashboard".to_string()),
            primary: None,
            secondary: None,
        })
    }

    pub fn parse_openrouter(json: &str) -> Option<Self> {
        let val: serde_json::Value = serde_json::from_str(json).ok()?;
        let data = val.get("data")?;
        let total_credits = data.get("total_credits").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let total_usage = data.get("total_usage").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let mut limits = Vec::new();
        limits.push(
            PlanLimit::new("Credits", 100.0, format!("${:.2} balance remaining", total_credits))
                .threshold(Some(50.0))
                .trend_down(false),
        );

        limits.push(
            PlanLimit::new("Usage", 100.0, format!("${:.4} total spent", total_usage))
                .threshold(Some(75.0))
                .trend_down(true),
        );

        Some(Self {
            plan_name: "OpenRouter Pay-as-you-go".to_string(),
            limits,
            footer_text: Some(format!(
                "Pay-as-you-go · ${:.4} spent · ${:.2} credit",
                total_usage, total_credits
            )),
            footer_action: Some("Dashboard".to_string()),
            primary: None,
            secondary: None,
        })
    }

    /// Body of OpenCode v2's `/api/experimental/usage/limits`; `data` is null when not signed in.
    pub fn parse_opencode(json: &str) -> Option<Self> {
        let val: serde_json::Value = serde_json::from_str(json).ok()?;
        let data = val.get("data")?.as_object()?;
        let now = Utc::now();
        let resets = |v: &serde_json::Value| {
            v.get("resetsAt")
                .and_then(|v| v.as_i64())
                .and_then(DateTime::from_timestamp_millis)
                .map_or_else(
                    || "resets soon".to_string(),
                    |at| format!("resets {}", format_reset_countdown(at, now)),
                )
        };
        let go = data.get("go");
        let mut limits: Vec<PlanLimit> = [
            ("rolling", "Session", 75.0, true),
            ("weekly", "Weekly", 50.0, false),
            ("monthly", "Monthly", 50.0, false),
        ]
        .into_iter()
        .filter_map(|(key, label, threshold, trend_down)| {
            let meter = go?.get(key)?;
            let used = meter.get("percent")?.as_f64()? as f32;
            Some(
                PlanLimit::new(label, 100.0 - used, resets(meter))
                    .threshold(Some(threshold))
                    .trend_down(trend_down),
            )
        })
        .collect();
        for b in data.get("budgets").and_then(|v| v.as_array()).into_iter().flatten() {
            let (Some(limit), Some(spent)) =
                (b.get("limit").and_then(|v| v.as_f64()), b.get("spent").and_then(|v| v.as_f64()))
            else {
                continue;
            };
            let used = if limit > 0.0 { spent / limit * 100.0 } else { 100.0 };
            limits.push(
                PlanLimit::new(
                    format!("Budget · ${spent:.2} of ${limit:.2}"),
                    100.0 - used as f32,
                    resets(b),
                )
                .threshold(Some(75.0))
                .trend_down(true),
            );
        }
        Some(Self {
            plan_name: if go.is_some() { "OpenCode Go" } else { "OpenCode Zen" }.to_string(),
            limits,
            footer_action: Some("Dashboard".to_string()),
            ..Self::default()
        })
    }
}

pub fn format_seconds_countdown(secs: u64) -> String {
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let mins = (rem % 3600) / 60;
    if days > 0 {
        format!("in {}d {}h", days, hours)
    } else if hours > 0 {
        format!("in {}h {}m", hours, mins)
    } else {
        format!("in {}m", mins.max(1))
    }
}

pub fn format_reset_countdown(resets_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let diff = resets_at.signed_duration_since(now);
    if diff.num_seconds() <= 0 {
        return "now".to_string();
    }
    let total_minutes = diff.num_minutes();
    let hours = total_minutes / 60;
    let mins = total_minutes % 60;
    let days = hours / 24;
    let rem_hours = hours % 24;
    if days > 0 {
        format!("in {}d {}h", days, rem_hours)
    } else if hours > 0 {
        format!("in {}h {}m", hours, mins)
    } else {
        format!("in {}m", mins.max(1))
    }
}

fn extract_claude_token_and_sub(secret: &str) -> (Option<String>, Option<String>) {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(secret) {
        let sub = v
            .pointer("/claudeAiOauth/subscriptionType")
            .and_then(|s| s.as_str())
            .or_else(|| v.get("subscriptionType").and_then(|s| s.as_str()))
            .map(|s| s.to_string());

        if let Some(token) = v.pointer("/claudeAiOauth/accessToken").and_then(|t| t.as_str()) {
            return (Some(token.to_string()), sub);
        }
        if let Some(token) = v.get("accessToken").and_then(|t| t.as_str()) {
            return (Some(token.to_string()), sub);
        }
        if let Some(token) = v.pointer("/oauth/accessToken").and_then(|t| t.as_str()) {
            return (Some(token.to_string()), sub);
        }
        if let Some(token) = v.get("token").and_then(|t| t.as_str()) {
            return (Some(token.to_string()), sub);
        }
    }
    let trimmed = secret.trim();
    if !trimmed.is_empty() && !trimmed.starts_with('{') {
        return (Some(trimmed.to_string()), None);
    }
    (None, None)
}

/// Reads Claude credentials and optional subscription type from environment, system keychain, or local credentials files.
pub fn read_claude_credentials() -> Option<(String, Option<String>)> {
    if let Ok(tok) = std::env::var("ANTHROPIC_AUTH_TOKEN")
        && !tok.trim().is_empty()
    {
        return Some((tok.trim().to_string(), None));
    }
    if let Ok(tok) = std::env::var("CLAUDE_CODE_TOKEN")
        && !tok.trim().is_empty()
    {
        return Some((tok.trim().to_string(), None));
    }

    let user = std::env::var("USER").unwrap_or_default();
    let candidates = [user.as_str(), "claude", ""];
    for acct in candidates {
        if let Ok(entry) = keyring::Entry::new("Claude Code-credentials", acct)
            && let Ok(pwd) = entry.get_password()
        {
            let (token, sub) = extract_claude_token_and_sub(&pwd);
            if let Some(tok) = token {
                return Some((tok, sub));
            }
        }
    }

    let home = home_dir();
    let cred_paths = [
        home.join(".claude").join(".credentials.json"),
        home.join(".claude.json"),
        home.join(".config").join("claude").join("credentials.json"),
    ];

    for path in &cred_paths {
        if let Ok(raw) = fs::read_to_string(path) {
            let (token, sub) = extract_claude_token_and_sub(&raw);
            if let Some(tok) = token {
                return Some((tok, sub));
            }
        }
    }

    None
}

fn run_in_tokio<F, T>(f: F) -> Option<T>
where
    F: std::future::Future<Output = Option<T>>,
{
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
    rt.block_on(f)
}

/// Fetches Claude plan rate limits and quota usage via Anthropic OAuth endpoint.
pub fn fetch_claude_plan_usage(token: &str, subscription_hint: Option<&str>) -> Option<PlanUsage> {
    let token = token.to_string();
    let sub = subscription_hint.map(|s| s.to_string());
    run_in_tokio(async move {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(4)).build().ok()?;
        let res = client
            .get("https://api.anthropic.com/api/oauth/usage")
            .header("Authorization", format!("Bearer {token}"))
            .header("anthropic-version", "2023-06-01")
            .header("User-Agent", "Claude-Code/0.1.0")
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        let text = res.text().await.ok()?;
        PlanUsage::parse_anthropic(&text, sub.as_deref())
    })
}

/// Reads Codex auth token from ~/.codex/auth.json.
pub fn read_codex_token() -> Option<String> {
    let home = home_dir();
    let auth_path = match std::env::var_os("CODEX_HOME") {
        Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir).join("auth.json"),
        _ => home.join(".codex").join("auth.json"),
    };
    if let Ok(raw) = fs::read_to_string(auth_path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw)
        && let Some(tok) = val.pointer("/tokens/access_token").and_then(|t| t.as_str())
    {
        return Some(tok.to_string());
    }
    None
}

/// Fetches Codex rate limits and reset credits via ChatGPT backend API.
pub fn fetch_codex_plan_usage(token: &str) -> Option<PlanUsage> {
    let token = token.to_string();
    run_in_tokio(async move {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(4)).build().ok()?;
        let res = client
            .get("https://chatgpt.com/backend-api/wham/usage")
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", "codex/0.163.0")
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        let text = res.text().await.ok()?;
        PlanUsage::parse_codex(&text)
    })
}

/// Reads OpenRouter API key from keychain or environment.
pub fn read_openrouter_key() -> Option<String> {
    if let Ok(k) = std::env::var("OPENROUTER_API_KEY")
        && !k.trim().is_empty()
    {
        return Some(k.trim().to_string());
    }
    if let Ok(entry) = keyring::Entry::new("aether", "openrouter-api-key")
        && let Ok(pwd) = entry.get_password()
        && !pwd.trim().is_empty()
    {
        return Some(pwd.trim().to_string());
    }
    None
}

/// Fetches OpenRouter credit balance and usage.
pub fn fetch_openrouter_plan_usage(api_key: &str) -> Option<PlanUsage> {
    let key = api_key.to_string();
    run_in_tokio(async move {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(4)).build().ok()?;
        let res = client
            .get("https://openrouter.ai/api/v1/credits")
            .header("Authorization", format!("Bearer {key}"))
            .header("User-Agent", "aether/0.1.0")
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        let text = res.text().await.ok()?;
        PlanUsage::parse_openrouter(&text)
    })
}

/// Fetches OpenCode Go and Console budget limits from the running OpenCode service.
pub fn fetch_opencode_plan_usage() -> Option<PlanUsage> {
    let api = aether_driver::opencode::service_api()?;
    let Ok((200, body)) = api.send(
        reqwest::Method::GET,
        "/api/experimental/usage/limits",
        None,
        Duration::from_secs(16),
    ) else {
        return None;
    };
    PlanUsage::parse_opencode(&body)
}

/// Fetches Antigravity quota by invoking `agy -p /usage`.
pub fn fetch_antigravity_plan_usage() -> Option<PlanUsage> {
    let home = home_dir();
    let candidates = [
        home.join(".local/bin/agy"),
        std::path::PathBuf::from("/usr/local/bin/agy"),
        std::path::PathBuf::from("agy"),
    ];

    for bin in &candidates {
        if let Ok(out) = Command::new(bin).arg("-p").arg("/usage").output()
            && out.status.success()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(usage) = PlanUsage::parse_antigravity(&text) {
                return Some(usage);
            }
        }
    }
    None
}

/// Placeholder until a provider's real limits load; never invented numbers.
pub fn default_plan_usage(provider: &str) -> PlanUsage {
    PlanUsage { plan_name: provider.to_string(), ..PlanUsage::default() }
}

#[cfg(test)]
#[path = "../tests/unit/usage.rs"]
mod tests;
