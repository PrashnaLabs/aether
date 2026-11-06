use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use aether_core::home_dir;
use reqwest::Method;
use serde_json::Value;

use super::parsers::{
    CodexScanState, parse_antigravity_records, parse_any_timestamp_ms, parse_claude_line,
    parse_codex_line, parse_opencode_message_json, parse_pi_records,
};
use super::{TokenTotals, UsageProvider, UsageRecord};

pub(super) fn provider_roots(provider: UsageProvider) -> Vec<PathBuf> {
    let home = home_dir();
    if home.as_os_str().is_empty() {
        return Vec::new();
    }
    match provider {
        UsageProvider::Claude => match std::env::var_os("CLAUDE_CONFIG_DIR") {
            Some(dir) if !dir.is_empty() => vec![PathBuf::from(dir).join("projects")],
            _ => vec![home.join(".claude/projects")],
        },
        UsageProvider::Codex => match std::env::var_os("CODEX_HOME") {
            Some(dir) if !dir.is_empty() => vec![PathBuf::from(dir).join("sessions")],
            _ => vec![home.join(".codex/sessions")],
        },
        UsageProvider::Pi => match std::env::var_os("PI_CODING_AGENT_DIR") {
            Some(dir) if !dir.is_empty() => vec![PathBuf::from(dir).join("sessions")],
            _ => vec![home.join(".pi/agent/sessions")],
        },
        UsageProvider::Antigravity => {
            vec![home.join(".gemini/antigravity-cli/brain"), home.join(".gemini/antigravity/brain")]
        }
        UsageProvider::OpenCode => {
            let data = std::env::var_os("XDG_DATA_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"));
            vec![data.join("opencode/storage/message")]
        }
        UsageProvider::OpenRouter => Vec::new(),
    }
}

pub(super) fn list_files_with_extension(
    root: &Path,
    ext: &str,
    since_ms: i64,
    found: &mut Vec<(PathBuf, u64, i64)>,
) -> usize {
    let mut skipped = 0;
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            skipped += list_files_with_extension(&path, ext, since_ms, found);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or(0);
        if mtime_ms < since_ms {
            skipped += 1;
            continue;
        }
        found.push((path, metadata.len(), mtime_ms));
    }
    skipped
}

pub(super) fn list_antigravity_transcripts(
    brain_root: &Path,
    since_ms: i64,
    found: &mut Vec<(PathBuf, u64, i64)>,
) -> usize {
    let mut skipped = 0;
    let Ok(entries) = std::fs::read_dir(brain_root) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let transcript = entry.path().join(".system_generated/logs/transcript.jsonl");
        let Ok(metadata) = std::fs::metadata(&transcript) else {
            continue;
        };
        let mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or(0);
        if mtime_ms < since_ms {
            skipped += 1;
            continue;
        }
        found.push((transcript, metadata.len(), mtime_ms));
    }
    skipped
}

pub(super) fn load_opencode_session_dirs() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let home = home_dir();
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let session_root = data.join("opencode/storage/session");
    let mut files = Vec::new();
    list_files_with_extension(&session_root, "json", 0, &mut files);
    for (path, _, _) in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let (Some(id), Some(dir)) = (
            v.get("id").and_then(Value::as_str),
            v.get("directory").or_else(|| v.pointer("/location/directory")).and_then(Value::as_str),
        ) && !id.is_empty()
            && !dir.is_empty()
        {
            map.insert(id.to_owned(), dir.to_owned());
        }
    }
    map
}

pub(super) fn collect_opencode_service_records(
    seen_sessions: &HashSet<String>,
) -> Vec<UsageRecord> {
    let Some(api) = aether_driver::opencode::service_api() else {
        return Vec::new();
    };
    let Ok((200, body)) =
        api.send(Method::GET, "/api/session?limit=200", None, Duration::from_secs(2))
    else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&body) else {
        return Vec::new();
    };
    let Some(sessions) = v.get("data").or(Some(&v)).and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for s in sessions {
        let Some(id) = s.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
            continue;
        };
        if seen_sessions.contains(id) {
            continue;
        }
        let Some(tokens) = s.get("tokens") else {
            continue;
        };
        let count =
            |val: Option<&Value>| val.and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as u64;
        let totals = TokenTotals {
            uncached_input: count(tokens.get("input")),
            cached_input: count(tokens.pointer("/cache/read")),
            cache_creation: count(tokens.pointer("/cache/write")),
            output: count(tokens.get("output")),
            reasoning: count(tokens.get("reasoning")),
        };
        if totals.total() == 0 {
            continue;
        }
        let Some(timestamp_ms) = s
            .pointer("/time/updated")
            .or_else(|| s.pointer("/time/created"))
            .and_then(parse_any_timestamp_ms)
        else {
            continue;
        };
        let model = s
            .pointer("/model/id")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
            .unwrap_or("opencode")
            .to_owned();
        let project =
            s.pointer("/location/directory").and_then(Value::as_str).unwrap_or_default().to_owned();
        let reported_cost_usd =
            s.get("cost").and_then(Value::as_f64).filter(|c| c.is_finite() && *c > 0.0);
        records.push(UsageRecord {
            provider: UsageProvider::OpenCode,
            timestamp_ms,
            model,
            session_id: id.to_owned(),
            project,
            totals,
            reported_cost_usd,
            dedupe_key: Some(format!("oc-session:{id}")),
        });
    }
    records
}

pub(super) fn read_transcript_records(
    path: &Path,
    provider: UsageProvider,
    opencode_dirs: &HashMap<String, String>,
) -> Option<Vec<UsageRecord>> {
    match provider {
        UsageProvider::Claude | UsageProvider::Codex => {
            let file = std::fs::File::open(path).ok()?;
            let mut reader = std::io::BufReader::new(file);
            let mut line = String::new();
            let mut records = Vec::new();
            let mut codex_state = CodexScanState::new();
            let mut seen_in_file = HashSet::new();

            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(_) => return None,
                }
                match provider {
                    UsageProvider::Codex => {
                        if !line.contains("\"last_token_usage\"")
                            && !line.contains("\"turn_context\"")
                            && !line.contains("\"session_meta\"")
                        {
                            continue;
                        }
                        if let Some(record) = parse_codex_line(&line, &mut codex_state) {
                            records.push(record);
                        }
                    }
                    UsageProvider::Claude => {
                        if !line.contains("\"usage\"") {
                            continue;
                        }
                        if let Some(record) = parse_claude_line(&line) {
                            if let Some(key) = &record.dedupe_key
                                && !seen_in_file.insert(key.clone())
                            {
                                continue;
                            }
                            records.push(record);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            Some(records)
        }
        UsageProvider::Pi => {
            let file = std::fs::File::open(path).ok()?;
            let fallback =
                path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            Some(parse_pi_records(std::io::BufReader::new(file), &fallback))
        }
        UsageProvider::Antigravity => {
            let file = std::fs::File::open(path).ok()?;
            // `<brain>/<conv-id>/.system_generated/logs/transcript.jsonl`
            let session_id = path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "antigravity".to_owned());
            Some(parse_antigravity_records(std::io::BufReader::new(file), &session_id))
        }
        UsageProvider::OpenCode => {
            let text = std::fs::read_to_string(path).ok()?;
            Ok::<Vec<UsageRecord>, ()>(
                parse_opencode_message_json(&text, opencode_dirs).into_iter().collect(),
            )
            .ok()
        }
        UsageProvider::OpenRouter => Some(Vec::new()),
    }
}
