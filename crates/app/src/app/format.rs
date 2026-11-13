use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use aether_core::{Message, Role};

use super::Chat;

/// `path` with the `home` folder shortened to `~`.
pub(crate) fn display_path(path: &str, home: &std::path::Path) -> String {
    match std::path::Path::new(path).strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.into(),
    }
}

/// Dollars for one reply from its token counts and model, when priced.
pub(super) fn message_cost(message: &Message, prices: &HashMap<String, (f64, f64)>) -> Option<f64> {
    let (prompt_price, completion_price) = prices.get(message.model.as_deref()?)?;
    Some(
        message.prompt_tokens as f64 * prompt_price
            + message.completion_tokens as f64 * completion_price,
    )
}

/// Micros spent on one reply: reported, else estimated from token prices (flagged true).
pub(super) fn reply_spend(message: &Message, prices: &HashMap<String, (f64, f64)>) -> (u64, bool) {
    match message.cost_micros {
        0 => (message_cost(message, prices).map_or(0, |c| (c * 1e6) as u64), true),
        micros => (micros, false),
    }
}

/// Dollars spent across a chat, rounded to cents.
pub(crate) fn chat_spend(chat: &Chat, prices: &HashMap<String, (f64, f64)>) -> f64 {
    let micros: u64 = chat.messages.iter().map(|m| reply_spend(m, prices).0).sum();
    (micros as f64 / 1e4).round() / 100.
}

/// Formats dollars compactly: $0.0234, $1.5.
pub(super) fn format_cost(cost: f64) -> String {
    if cost <= 0. {
        return "$0".into();
    }
    let text = format!("{cost:.4}");
    format!("${}", text.trim_end_matches('0').trim_end_matches('.'))
}

/// Full token counts with thousands separators: 2,604.
pub(crate) fn format_full(n: u64) -> String {
    let digits: Vec<char> = n.to_string().chars().rev().collect();
    let mut out = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(*c);
    }
    out.chars().rev().collect()
}

/// Local clock time like 2:11 PM.
pub(crate) fn format_time(ms: i64) -> Option<String> {
    let text = chrono::DateTime::from_timestamp_millis(ms)
        .filter(|_| ms > 0)?
        .with_timezone(&chrono::Local)
        .format("%I:%M %p")
        .to_string();
    Some(text.strip_prefix('0').unwrap_or(&text).to_string())
}

/// A reply's cost in cents; `estimated` marks a price-table guess.
pub(crate) fn cost_text(micros: u64, estimated: bool) -> Option<String> {
    let text = match micros {
        0 => return None,
        1..10_000 => "<$0.01".to_string(),
        _ => format!("${:.2}", micros as f64 / 1e6),
    };
    Some(if estimated { format!("≈{text}") } else { text })
}

/// The chat as Markdown: title plus each turn's text. Empty drafts are skipped.
pub(crate) fn chat_markdown(chat: &Chat) -> String {
    let mut md = format!("# {}\n", chat.title);
    for m in &chat.messages {
        if m.text.trim().is_empty() {
            continue;
        }
        md.push_str(match m.role {
            Role::User => "\n## You\n\n",
            Role::Assistant => "\n## Aether\n\n",
        });
        md.push_str(m.text.trim());
        md.push('\n');
    }
    md
}

/// A save-dialog name derived from the title; falls back to `chat.md`.
pub(super) fn export_filename(title: &str) -> String {
    let stem: String = title
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .take(40)
        .collect();
    let stem = stem.trim();
    if stem.is_empty() { "chat.md".into() } else { format!("{stem}.md") }
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
#[path = "../../tests/unit/app/format.rs"]
mod tests;
