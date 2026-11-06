use std::path::Path;

use aether_agent::usage_history::UsageWindow;
use chrono::NaiveDate;

fn group_thousands(number: &str) -> String {
    let (integer, fraction) = match number.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (number, None),
    };
    let grouped = integer
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(",");
    match fraction {
        Some(fraction) => format!("{grouped}.{fraction}"),
        None => grouped,
    }
}

pub(crate) fn format_usd(value: f64) -> String {
    format!("${}", group_thousands(&format!("{:.2}", value.max(0.0))))
}

pub(super) fn format_count(value: u64) -> String {
    group_thousands(&value.to_string())
}

/// Compacts a token count to three significant figures with a unit suffix (`19.9B`, `76.7M`, `804K`).
pub(crate) fn format_tokens_compact(value: f64) -> String {
    let abs = value.abs();
    let (scaled, suffix) = if abs >= 1e12 {
        (value / 1e12, "T")
    } else if abs >= 1e9 {
        (value / 1e9, "B")
    } else if abs >= 1e6 {
        (value / 1e6, "M")
    } else if abs >= 1e3 {
        (value / 1e3, "K")
    } else {
        return format_count(value.round().max(0.0) as u64);
    };
    let digits = if scaled.abs() >= 100.0 {
        0
    } else if scaled.abs() >= 10.0 {
        1
    } else {
        2
    };
    let mut text = format!("{scaled:.digits$}");
    if let Some(dot) = text.find('.')
        && text[dot + 1..].bytes().all(|byte| byte == b'0')
    {
        text.truncate(dot);
    }
    format!("{text}{suffix}")
}

pub(super) fn format_percent(share: f64) -> String {
    format!("{:.1}%", share * 100.0)
}

/// Keep the full project path, abbreviating only the user's home directory.
pub(crate) fn usage_project_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

pub(super) fn format_day_short(day: NaiveDate) -> String {
    day.format("%b %-d").to_string()
}

pub(super) fn format_month(first_day: NaiveDate) -> String {
    first_day.format("%B %Y").to_string()
}

pub(super) fn format_month_short(day: NaiveDate) -> String {
    day.format("%b %Y").to_string()
}

pub(crate) fn window_choice_label(window: UsageWindow) -> &'static str {
    match window {
        UsageWindow::TrailingDays(7) => "Last 7 days",
        UsageWindow::TrailingDays(30) => "Last 30 days",
        UsageWindow::TrailingDays(90) => "Last 90 days",
        UsageWindow::ThisMonth => "This month",
        UsageWindow::LastMonth => "Last month",
        UsageWindow::TrailingDays(_) | UsageWindow::Months(_) => "Custom",
    }
}

pub(super) fn count_noun(count: u64, noun: &str) -> String {
    let plural = match noun {
        "project" => "projects",
        "session" => "sessions",
        "model" => "models",
        "active day" => "active days",
        _ => noun,
    };
    format!("{} {}", format_count(count), if count == 1 { noun } else { plural })
}

#[cfg(test)]
#[path = "../../tests/unit/usage_view/format.rs"]
mod tests;
