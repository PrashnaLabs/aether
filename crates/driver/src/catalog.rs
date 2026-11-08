use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use reqwest::Method;
use serde_json::{Value, json};

use aether_core::{Provider, Settings, find_binary, home_dir};

use super::DriverStartOptions;
use super::opencode::{Api, service_api};
use super::spawn::command;
use super::version::{self, Support};

/// Whether a CLI is usable, which models it offers and its slash commands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProviderProbe {
    pub installed: bool,
    pub path: Option<PathBuf>,
    pub models: Vec<CliModel>,
    pub commands: Vec<CliCommand>,
    pub version: Option<String>,
    pub status: String,
    pub chips: Vec<String>,
    pub degraded: Option<String>,
}

/// A slash command the CLI itself handles, without the leading `/`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CliCommand {
    pub name: String,
    pub description: String,
}

/// One model a CLI offers; `id` is what `--model` takes, `default` the CLI's own pick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CliModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub default: bool,
    pub description: String,
    /// Context window in tokens, when the CLI reports one.
    pub context: Option<u64>,
    /// Effort levels this model takes; `None` when the CLI doesn't say.
    pub efforts: Option<Vec<String>>,
}

const LIST_TIMEOUT: Duration = Duration::from_secs(20);
const CLAUDE_TIMEOUT: Duration = Duration::from_secs(25);
const PI_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_LINE: usize = 4 * 1024 * 1024;

/// The status label and the reason behind a limited or blocked one.
pub fn probe_status(
    installed: bool,
    service: bool,
    support: Option<&Support>,
) -> (String, Option<String>) {
    match support {
        _ if !installed => (if service { "Service running" } else { "Not installed" }.into(), None),
        Some(Support::Unsupported(m)) => ("Update needed".into(), Some(m.clone())),
        Some(Support::Degraded(m)) => ("Limited".into(), Some(m.clone())),
        _ => ("Ready".into(), None),
    }
}

/// Blocking: searches PATH and may run the CLI. Keep off the UI thread.
pub fn probe(provider: Provider, settings: &Settings) -> ProviderProbe {
    let name = match provider {
        Provider::Claude => "claude",
        Provider::Codex => "codex",
        Provider::Pi => "pi",
        Provider::OpenCode => "opencode",
        Provider::Antigravity => "agy",
        Provider::OpenRouter => return ProviderProbe::default(),
    };
    let opts = DriverStartOptions::for_provider(settings, provider, home_dir());
    let path = find_binary(name, &opts.binary);
    let service = match provider {
        Provider::OpenCode => service_api(),
        _ => None,
    };
    let (models, commands) = match provider {
        Provider::Claude => claude_catalog(&opts),
        Provider::Pi => pi_catalog(&opts),
        Provider::Codex => (codex_models(&opts), codex_commands()),
        _ => service.as_ref().map(opencode_catalog).unwrap_or_default(),
    };
    let models =
        if models.is_empty() && path.is_some() { fallback_models(provider, &opts) } else { models };
    let version = match (&service, &path) {
        (Some(api), _) if provider == Provider::OpenCode => api.version(),
        (_, Some(path)) => version::cli_version(path),
        _ => None,
    };
    let support =
        version.as_deref().and_then(version::parse).map(|v| version::support(provider, v));
    let installed = path.is_some() || service.is_some();
    let (status, degraded) = probe_status(path.is_some(), service.is_some(), support.as_ref());
    let chips = if installed {
        version::capabilities(provider).iter().map(|c| c.to_string()).collect()
    } else {
        Vec::new()
    };
    ProviderProbe { installed, path, models, commands, version, status, chips, degraded }
}

/// Claude's `/model` catalog and slash commands via the SDK initialize: no model turn, no saved session.
fn claude_catalog(opts: &DriverStartOptions) -> (Vec<CliModel>, Vec<CliCommand>) {
    let Some(mut cmd) = command(opts, "claude") else {
        return Default::default();
    };
    cmd.args([
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "stream-json",
        "--no-session-persistence",
        "--mcp-config",
        r#"{"mcpServers":{}}"#,
        "--strict-mcp-config",
        "--setting-sources",
        "user",
    ])
    .env("ENABLE_CLAUDEAI_MCP_SERVERS", "false");
    let request = |id: &str, request: Value| json!({"type": "control_request", "request_id": id, "request": request});
    let requests = [
        ("aether-initialize", request("aether-initialize", json!({"subtype": "initialize"}))),
        (
            "aether-context",
            request("aether-context", json!({"subtype": "get_context_usage", "detail": "summary"})),
        ),
    ];
    let replies = rpc(cmd, &requests, CLAUDE_TIMEOUT);
    let Some(init) = replies.first() else { return Default::default() };
    let mut models = parse_claude_models(init);
    let window = replies.get(1).and_then(|r| r["response"]["response"]["maxTokens"].as_u64());
    if let Some(m) = models.iter_mut().find(|m| m.default) {
        m.context = window;
    }
    let commands = init["response"]["response"]["commands"]
        .as_array()
        .map(|list| commands(list, "name", "description"))
        .unwrap_or_default();
    (models, commands)
}

fn codex_commands() -> Vec<CliCommand> {
    [
        ("compact", "Summarize the conversation to free context"),
        ("review", "Review uncommitted changes"),
    ]
    .map(|(name, description)| CliCommand { name: name.into(), description: description.into() })
    .to_vec()
}

/// Named entries of a command list; the first description line only.
fn commands(list: &[Value], name: &str, description: &str) -> Vec<CliCommand> {
    list.iter()
        .filter_map(|c| {
            Some(CliCommand {
                name: text(&c[name])?.trim_start_matches('/').into(),
                description: text(&c[description])
                    .and_then(|d| d.lines().next())
                    .unwrap_or_default()
                    .into(),
            })
        })
        .collect()
}

fn pi_catalog(opts: &DriverStartOptions) -> (Vec<CliModel>, Vec<CliCommand>) {
    let Some(mut cmd) = command(opts, "pi") else {
        return Default::default();
    };
    cmd.args([
        "--mode",
        "rpc",
        "--no-session",
        "--no-skills",
        "--no-prompt-templates",
        "--no-context-files",
    ])
    .env("PI_SKIP_VERSION_CHECK", "1");
    let requests = [
        ("aether-models", json!({"id": "aether-models", "type": "get_available_models"})),
        ("aether-state", json!({"id": "aether-state", "type": "get_state"})),
        ("aether-commands", json!({"id": "aether-commands", "type": "get_commands"})),
    ];
    let replies = rpc(cmd, &requests, PI_TIMEOUT);
    let models = replies
        .first()
        .map(|models| parse_pi_models(models, replies.get(1).unwrap_or(&Value::Null)))
        .unwrap_or_default();
    let commands = replies
        .get(2)
        .and_then(|r| r["data"]["commands"].as_array())
        .map(|list| commands(list, "name", "description"))
        .unwrap_or_default();
    (models, commands)
}

fn opencode_catalog(api: &Api) -> (Vec<CliModel>, Vec<CliCommand>) {
    let commands = match api.send(Method::GET, "/api/command", None, Duration::from_secs(2)) {
        Ok((200, text)) => serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["data"].as_array().map(|list| commands(list, "name", "description")))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    (opencode_models(api), commands)
}

fn opencode_models(api: &Api) -> Vec<CliModel> {
    let active_providers: Option<HashSet<String>> = api
        .send(Method::GET, "/api/provider", None, Duration::from_secs(2))
        .ok()
        .and_then(|(status, text)| {
            if (200..300).contains(&status) {
                let v: Value = serde_json::from_str(&text).ok()?;
                let providers = v["data"]
                    .as_array()?
                    .iter()
                    .filter_map(|p| p["id"].as_str().or(p["integrationID"].as_str()))
                    .map(String::from)
                    .collect();
                Some(providers)
            } else {
                None
            }
        });
    let default_id: Option<String> = api
        .send(Method::GET, "/api/model/default", None, Duration::from_secs(2))
        .ok()
        .and_then(|(status, text)| {
            if (200..300).contains(&status) {
                let v: Value = serde_json::from_str(&text).ok()?;
                let m = &v["data"];
                let p = m["providerID"].as_str()?;
                let id = m["id"].as_str().or(m["modelID"].as_str())?;
                Some(format!("{p}/{id}"))
            } else {
                None
            }
        });
    match api.send(Method::GET, "/api/model", None, LIST_TIMEOUT) {
        Ok((200, text)) => serde_json::from_str(&text)
            .map(|v| parse_opencode_models(&v, default_id.as_deref(), active_providers.as_ref()))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The list commands, for when the richer catalog is unavailable.
fn fallback_models(provider: Provider, opts: &DriverStartOptions) -> Vec<CliModel> {
    let (name, args): (&str, &[&str]) = match provider {
        Provider::Claude => return claude_aliases(),
        Provider::Codex => return codex_models(opts),
        Provider::Antigravity => ("agy", &["models"]),
        Provider::Pi => ("pi", &["--list-models"]),
        Provider::OpenCode => ("opencode", &["models"]),
        Provider::OpenRouter => return Vec::new(),
    };
    let models = list_output(opts, name, args, LIST_TIMEOUT)
        .map(|out| match provider {
            Provider::Pi => parse_pi_list(&out),
            Provider::Antigravity => parse_antigravity_list(&out),
            _ => parse_opencode_list(&out),
        })
        .unwrap_or_default();
    if models.is_empty() && provider == Provider::Antigravity {
        antigravity_models()
    } else {
        models
    }
}

pub fn parse_antigravity_list(out: &str) -> Vec<CliModel> {
    out.lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("Available")
                && !line.starts_with("ERROR")
                && !line.starts_with("Usage")
                && !line.starts_with("Failed")
        })
        .map(|line| {
            let (id, label) = line.split_once('\t').unwrap_or((line, ""));
            let id = id.split_whitespace().next().unwrap_or(id);
            let name = if label.trim().is_empty() { readable(id) } else { label.trim().into() };
            // the id already names its effort, and agy rejects a second one via --effort
            let suffixed =
                ["-low", "-medium", "-high", "-xhigh", "-max"].iter().any(|e| id.ends_with(e));
            let efforts = suffixed.then(Vec::new);
            CliModel {
                id: id.into(),
                name,
                provider: antigravity_vendor(id),
                efforts,
                ..Default::default()
            }
        })
        .collect()
}

/// `agy models` serves several vendors' models under one login.
fn antigravity_vendor(id: &str) -> String {
    match id.split('-').next().unwrap_or_default() {
        "claude" => "Anthropic",
        "gpt" | "o3" | "o4" => "OpenAI",
        _ => "Google",
    }
    .into()
}

fn antigravity_models() -> Vec<CliModel> {
    [("gemini-2.5-pro", "Gemini 2.5 Pro"), ("gemini-2.5-flash", "Gemini 2.5 Flash")]
        .map(|(id, name)| CliModel {
            id: id.into(),
            name: name.into(),
            provider: "Google".into(),
            ..Default::default()
        })
        .to_vec()
}

pub fn codex_home_dir() -> PathBuf {
    std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".codex"))
}

pub fn parse_codex_models_json(val: &Value) -> Vec<CliModel> {
    let array = if let Some(arr) = val.as_array() {
        arr
    } else if let Some(arr) = val["models"].as_array() {
        arr
    } else {
        return Vec::new();
    };

    let mut items: Vec<(i64, CliModel)> = array
        .iter()
        .filter(|m| m["visibility"].as_str() != Some("hide"))
        .filter_map(|m| {
            let id = m["slug"].as_str().or_else(|| m["id"].as_str())?.trim();
            if id.is_empty() {
                return None;
            }
            let name = m["display_name"]
                .as_str()
                .or_else(|| m["name"].as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .unwrap_or_else(|| readable(id));
            let priority = m["priority"].as_i64().unwrap_or(999);
            let percent = m["effective_context_window_percent"].as_u64().unwrap_or(100);
            let context = m["context_window"].as_u64().map(|w| w * percent / 100);
            let efforts = m["supported_reasoning_levels"].as_array().map(|levels| {
                levels.iter().filter_map(|l| text(&l["effort"]).map(String::from)).collect()
            });
            Some((
                priority,
                CliModel {
                    id: id.to_string(),
                    name,
                    provider: "OpenAI".into(),
                    description: text(&m["description"]).unwrap_or_default().into(),
                    context,
                    efforts,
                    ..Default::default()
                },
            ))
        })
        .collect();

    items.sort_by_key(|(priority, _)| *priority);
    items.into_iter().map(|(_, m)| m).collect()
}

fn static_codex_models() -> Vec<CliModel> {
    [
        ("gpt-6.1-sol", "GPT-6.1-Sol", "OpenAI"),
        ("gpt-6-astra", "GPT-6-Astra", "OpenAI"),
        ("gpt-6-sol", "GPT-6-Sol", "OpenAI"),
        ("gpt-6-luna", "GPT-6-Luna", "OpenAI"),
        ("gpt-5.6-sol", "GPT-5.6-Sol", "OpenAI"),
        ("gpt-5.6-terra", "GPT-5.6-Terra", "OpenAI"),
        ("gpt-5.6-luna", "GPT-5.6-Luna", "OpenAI"),
    ]
    .into_iter()
    .map(|(id, name, provider)| CliModel {
        id: id.into(),
        name: name.into(),
        provider: provider.into(),
        ..Default::default()
    })
    .collect()
}

pub fn codex_models(opts: &DriverStartOptions) -> Vec<CliModel> {
    let codex_dir = codex_home_dir();

    // 1. Read ~/.codex/models_cache.json directly (instant, accurate, offline)
    let cache_path = codex_dir.join("models_cache.json");
    if let Ok(content) = std::fs::read_to_string(&cache_path)
        && let Ok(val) = serde_json::from_str::<Value>(&content)
    {
        let parsed = parse_codex_models_json(&val);
        if !parsed.is_empty() {
            return parsed;
        }
    }

    // 2. Try CLI debug command if cache file was missing or unreadable
    if let Some(out) = list_output(opts, "codex", &["debug", "models"], Duration::from_secs(5))
        && let Ok(val) = serde_json::from_str::<Value>(&out)
    {
        let parsed = parse_codex_models_json(&val);
        if !parsed.is_empty() {
            return parsed;
        }
    }

    // 3. Fallback to realistic static Codex models
    static_codex_models()
}

fn claude_aliases() -> Vec<CliModel> {
    [("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")]
        .map(|(id, name)| CliModel {
            id: id.into(),
            name: name.into(),
            provider: "Anthropic".into(),
            ..Default::default()
        })
        .to_vec()
}

/// Replies to JSON-line requests, in order; stops at the first unanswered within `timeout`.
fn rpc(mut cmd: Command, requests: &[(&str, Value)], timeout: Duration) -> Vec<Value> {
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let Ok(mut child) = cmd.spawn() else {
        return Vec::new();
    };
    let Some((mut stdin, stdout)) = child.stdin.take().zip(child.stdout.take()) else {
        kill_group(&mut child);
        return Vec::new();
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = (&mut reader).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut line);
            // oversized line: stop reading, so the pending request counts as unanswered
            if !matches!(read, Ok(n) if n > 0 && n <= MAX_LINE) {
                break;
            }
            if let Ok(v) = serde_json::from_str::<Value>(&String::from_utf8_lossy(&line))
                && tx.send(v).is_err()
            {
                break;
            }
        }
    });
    let mut replies = Vec::new();
    for (id, request) in requests {
        if writeln!(stdin, "{request}").and_then(|_| stdin.flush()).is_err() {
            break;
        }
        let deadline = Instant::now() + timeout;
        let reply = std::iter::from_fn(|| {
            rx.recv_timeout(deadline.checked_duration_since(Instant::now())?).ok()
        })
        .find(|v| v["id"] == *id || v["response"]["request_id"] == *id);
        match reply {
            Some(v) => replies.push(v),
            None => break,
        }
    }
    drop(stdin);
    kill_group(&mut child);
    replies
}

/// Kills the process group `rpc` made `child` lead, then reaps `child`.
fn kill_group(child: &mut Child) {
    let pid = child.id();
    if pid > 1 {
        aether_core::signal_process(pid, "KILL", true);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Stdout of a successful run, or `None` on failure or after `timeout`.
fn list_output(
    opts: &DriverStartOptions,
    name: &str,
    args: &[&str],
    timeout: Duration,
) -> Option<String> {
    let mut cmd = command(opts, name)?;
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    if name == "pi" {
        cmd.env("PI_SKIP_VERSION_CHECK", "1");
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = tx.send(stdout.read_to_string(&mut out).map(|_| out));
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .ok()?
                    .ok();
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn text(v: &Value) -> Option<&str> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn parse_claude_models(reply: &Value) -> Vec<CliModel> {
    let models = reply["response"]["response"]["models"].as_array();
    models
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = text(&m["value"])?;
            let name = text(&m["displayName"])
                .map(String::from)
                .unwrap_or_else(|| readable(text(&m["resolvedModel"]).unwrap_or(id)));
            Some(CliModel {
                id: id.into(),
                name,
                provider: "Anthropic".into(),
                default: id == "default" || m["isDefault"] == true,
                description: text(&m["description"]).unwrap_or_default().into(),
                context: None,
                efforts: Some(
                    m["supportedEffortLevels"]
                        .as_array()
                        .filter(|_| m["supportsEffort"] == true)
                        .map(|l| l.iter().filter_map(|e| text(e).map(String::from)).collect())
                        .unwrap_or_default(),
                ),
            })
        })
        .collect()
}

fn parse_pi_models(models: &Value, state: &Value) -> Vec<CliModel> {
    let current = &state["data"]["model"];
    models["data"]["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let (provider, id) = (text(&m["provider"])?, text(&m["id"])?);
            let name = text(&m["name"]).map(|n| match provider {
                "openrouter" => strip_vendor(n),
                _ => n,
            });
            Some(CliModel {
                id: format!("{provider}/{id}"),
                name: name.map_or_else(|| readable(id), String::from),
                provider: provider_label(provider),
                default: current["provider"] == provider && current["id"] == id,
                description: String::new(),
                context: m["contextWindow"].as_u64(),
                efforts: Some(pi_efforts(m)),
            })
        })
        .collect()
}

/// Pi's `getSupportedThinkingLevels`: none without reasoning; a level mapped to null is
/// dropped, and `xhigh`/`max` need an explicit mapping.
fn pi_efforts(m: &Value) -> Vec<String> {
    if m["reasoning"] != true {
        return Vec::new();
    }
    let map = &m["thinkingLevelMap"];
    ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
        .into_iter()
        .filter(|level| match map.get(level) {
            Some(Value::Null) => false,
            Some(_) => true,
            None => !matches!(*level, "xhigh" | "max"),
        })
        .map(String::from)
        .collect()
}

fn parse_opencode_models(
    v: &Value,
    default_id: Option<&str>,
    active_providers: Option<&HashSet<String>>,
) -> Vec<CliModel> {
    let mut models: Vec<CliModel> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["enabled"] == true)
        .filter_map(|m| {
            let (provider, id) = (text(&m["providerID"])?, text(&m["id"])?);
            if let Some(active) = active_providers
                && !active.contains(provider)
            {
                return None;
            }
            let slug = format!("{provider}/{id}");
            let is_default = default_id.is_some_and(|d| d == slug);
            Some(CliModel {
                id: slug,
                name: text(&m["name"]).unwrap_or(id).into(),
                provider: provider_label(provider),
                default: is_default,
                description: String::new(),
                context: m["limit"]["context"].as_u64(),
                efforts: m["variants"]
                    .as_array()
                    .map(|v| v.iter().filter_map(|v| text(&v["id"]).map(String::from)).collect()),
            })
        })
        .collect();

    if !models.is_empty() && !models.iter().any(|m| m.default) {
        if let Some(pos) = models.iter().position(|m| {
            m.id.starts_with("google/gemini-3.8-flash")
                || m.id.starts_with("google/gemini-3.6-flash")
                || m.id.starts_with("opencode/")
        }) {
            models[pos].default = true;
        } else if let Some(first) = models.first_mut() {
            first.default = true;
        }
    }

    models
}

fn from_slug(slug: String) -> CliModel {
    let (provider, name) = slug.split_once('/').unwrap_or(("", &slug));
    CliModel {
        name: name.into(),
        provider: provider_label(provider),
        id: slug,
        ..Default::default()
    }
}

/// `pi --list-models` table rows as `provider/model`; lines above the
/// header are noise.
pub fn parse_pi_list(out: &str) -> Vec<CliModel> {
    out.lines()
        .skip_while(|line| !line.trim_start().starts_with("provider"))
        .skip(1)
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            Some(from_slug(format!("{}/{}", cols.next()?, cols.next()?)))
        })
        .collect()
}

/// `opencode models` prints one `provider/model` per line.
pub fn parse_opencode_list(out: &str) -> Vec<CliModel> {
    out.lines()
        .map(str::trim)
        .filter(|line| line.contains('/') && !line.contains(char::is_whitespace))
        .map(|line| from_slug(line.into()))
        .collect()
}

/// Display name for a provider key such as `openrouter` or `x-ai`.
pub fn provider_label(key: &str) -> String {
    match key {
        "openrouter" => "OpenRouter".into(),
        "opencode" => "OpenCode".into(),
        "openai" => "OpenAI".into(),
        "x-ai" | "xai" => "xAI".into(),
        "deepseek" => "DeepSeek".into(),
        "github-copilot" => "GitHub Copilot".into(),
        "antigravity" | "agy" => "Antigravity".into(),
        "codex" => "Codex".into(),
        _ => readable(key),
    }
}

/// `claude-opus-4-6` as `Claude Opus 4 6`.
fn readable(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `Anthropic: Claude X` as `Claude X`: the subtitle already names the vendor.
pub fn strip_vendor(name: &str) -> &str {
    let name = name.trim();
    match name.split_once(": ") {
        Some((vendor, rest))
            if vendor.chars().count() <= 24 && !rest.contains(": ") && !rest.trim().is_empty() =>
        {
            rest.trim()
        }
        _ => name,
    }
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
