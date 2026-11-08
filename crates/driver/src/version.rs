use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;

use aether_core::{Notice, NoticeKind, Provider, Update, find_binary, repaired_path};

use crate::DriverStartOptions;

pub type Version = (u32, u32, u32);

#[derive(Clone, Debug, PartialEq)]
pub enum Support {
    Ok,
    Degraded(String),
    Unsupported(String),
}

const TIMEOUT: Duration = Duration::from_secs(5);

/// The first `x.y.z` in `text`, with an optional `v` in front.
pub fn parse(text: &str) -> Option<Version> {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '.').find_map(|token| {
        let mut parts = token.trim_start_matches('v').split('.');
        let mut next = |last: bool| {
            let part = parts.next()?;
            let end = part.find(|c: char| !c.is_ascii_digit()).unwrap_or(part.len());
            (end > 0 && (last || end == part.len())).then(|| part[..end].parse().ok())?
        };
        Some((next(false)?, next(false)?, next(true)?))
    })
}

/// Runs `<binary> --version`, at most 5 s; the first `x.y.z` as text.
pub fn cli_version(binary: &Path) -> Option<String> {
    run_version(binary, TIMEOUT)
}

fn run_version(binary: &Path, timeout: Duration) -> Option<String> {
    let mut child = aether_core::command(binary)
        .arg("--version")
        .env("PATH", repaired_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        let _ = tx.send(out);
    });
    let out = rx.recv_timeout(timeout);
    let _ = child.kill();
    let _ = child.wait();
    let (a, b, c) = parse(&out.ok()?)?;
    Some(format!("{a}.{b}.{c}"))
}

fn name(provider: Provider) -> Option<&'static str> {
    Some(match provider {
        Provider::Claude => "Claude Code",
        Provider::Codex => "Codex",
        Provider::OpenCode => "OpenCode",
        Provider::Pi => "Pi",
        Provider::Antigravity => "agy",
        Provider::OpenRouter => return None,
    })
}

fn older(name: &str, v: &str, tested: &str) -> String {
    format!(
        "{name} {v} is older than {tested}, the version Aether is tested with. Some events may not show."
    )
}

pub fn support(provider: Provider, v: Version) -> Support {
    let Some(name) = name(provider) else { return Support::Ok };
    let found = format!("{}.{}.{}", v.0, v.1, v.2);
    let tested = |t: &str| Support::Degraded(older(name, &found, t));
    let rules: Vec<(Version, Support)> = match provider {
        Provider::Claude => vec![((2, 1, 304), tested("2.1.304"))],
        Provider::Codex => vec![((0, 163, 0), tested("0.163.0"))],
        Provider::OpenCode => vec![
            (
                (2, 0, 0),
                Support::Unsupported(format!(
                    "OpenCode {found} isn't supported. Update to 2.0 or later."
                )),
            ),
            ((2, 0, 32), tested("2.0.32")),
        ],
        Provider::Pi => vec![
            (
                (0, 80, 4),
                Support::Unsupported(format!(
                    "Pi {found} can't tell Aether when a run ends. Update to 0.80.4 or later."
                )),
            ),
            (
                (0, 99, 0),
                Support::Degraded(format!(
                    "Pi {found} is older than 0.99.0: runs that extensions handle may end early, and nested tool calls show flat."
                )),
            ),
            ((1, 3, 0), tested("1.3.0")),
        ],
        Provider::Antigravity => vec![
            (
                (1, 1, 15),
                Support::Unsupported(format!(
                    "agy {found} can't take Aether's input. Update to 1.1.15 or later."
                )),
            ),
            (
                (1, 1, 27),
                Support::Degraded(format!(
                    "agy {found} doesn't report blocked actions, so their approvals can't show. Update to 1.1.27 or later."
                )),
            ),
            ((1, 4, 2), tested("1.4.2")),
        ],
        Provider::OpenRouter => vec![],
    };
    rules.into_iter().find(|(below, _)| v < *below).map_or(Support::Ok, |(_, s)| s)
}

/// Tells the user which version was found; `Err` carries the reason it can't run.
pub(crate) fn gate(
    provider: Provider,
    found: Option<&str>,
    tx: &UnboundedSender<Update>,
) -> Result<(), String> {
    let Some(name) = name(provider) else { return Ok(()) };
    let version = found.and_then(parse);
    let text = match version {
        Some((a, b, c)) => format!("Found {name} {a}.{b}.{c}"),
        None => format!("Found {name} (version unknown)"),
    };
    let _ = tx.unbounded_send(Update::Notice(Notice::new(
        "startup-version",
        NoticeKind::Startup,
        text,
    )));
    match version.map(|v| support(provider, v)) {
        Some(Support::Unsupported(m)) => Err(m),
        Some(Support::Degraded(m)) => {
            let _ = tx.unbounded_send(Update::Warning(m));
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `gate` for a CLI that answers `--version`; skipped when the binary is missing, which spawn reports.
pub(crate) fn gate_binary(
    provider: Provider,
    name: &str,
    opts: &DriverStartOptions,
    tx: &UnboundedSender<Update>,
) -> Result<(), String> {
    let Some(path) = find_binary(name, &opts.binary) else { return Ok(()) };
    gate(provider, cli_version(&path).as_deref(), tx)
}

/// What the provider's chips show.
pub fn capabilities(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => {
            &["Subagents", "Background tasks", "Approvals", "Questions", "Forms", "Plans", "Cost"]
        }
        Provider::Codex => &[
            "Subagents",
            "Background terminals",
            "Approvals",
            "Questions",
            "Forms",
            "Review",
            "Plans",
        ],
        Provider::OpenCode => &[
            "Subagents",
            "Background",
            "Approvals",
            "Forms",
            "Cost",
            "Keeps running after Aether quits",
        ],
        Provider::Pi => &["Extensions", "Dialogs", "Cost"],
        Provider::Antigravity => &["Subagents", "Background", "Rerun with full access"],
        Provider::OpenRouter => &[],
    }
}

#[cfg(test)]
#[path = "../tests/unit/version.rs"]
mod tests;
