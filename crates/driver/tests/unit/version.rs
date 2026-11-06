use super::*;

#[test]
fn parse_finds_the_first_version() {
    assert_eq!(parse("2.1.304 (Claude Code)"), Some((2, 1, 304)));
    assert_eq!(parse("codex_cli_rs/0.163.0 (Mac OS 27.0.0; arm64) iTerm"), Some((0, 163, 0)));
    assert_eq!(parse("v2.0.32"), Some((2, 0, 32)));
    assert_eq!(parse("agy 1.4.2"), Some((1, 4, 2)));
    assert_eq!(parse("unknown"), None);
    assert_eq!(parse("1.3.0-beta.2"), Some((1, 3, 0)));
    assert_eq!(parse("v2.0.32-rc1 (build)"), Some((2, 0, 32)));
}

#[test]
fn support_applies_floors_then_the_tested_version() {
    use aether_core::Provider::*;
    assert!(matches!(support(Pi, (0, 80, 3)), Support::Unsupported(m) if m.contains("0.80.4")));
    assert!(matches!(support(Pi, (0, 98, 0)), Support::Degraded(m) if m.contains("0.99.0")));
    assert!(matches!(support(Pi, (1, 2, 0)), Support::Degraded(m) if m.contains("1.3.0")));
    assert_eq!(support(Pi, (1, 3, 0)), Support::Ok);
    assert_eq!(support(Pi, (2, 0, 0)), Support::Ok);
    assert!(matches!(support(Antigravity, (1, 1, 10)), Support::Unsupported(_)));
    assert!(
        matches!(support(Antigravity, (1, 1, 20)), Support::Degraded(m) if m.contains("1.1.27"))
    );
    assert!(matches!(support(OpenCode, (1, 9, 0)), Support::Unsupported(m) if m.contains("2.0")));
    assert!(matches!(support(Claude, (2, 1, 200)), Support::Degraded(_)));
    assert_eq!(support(Codex, (0, 170, 0)), Support::Ok);
}

#[test]
fn gate_announces_then_warns_or_refuses() {
    use aether_core::{Provider, Update};
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    assert_eq!(gate(Provider::Pi, Some("1.3.0"), &tx), Ok(()));
    assert!(matches!(rx.try_recv(), Ok(Update::Notice(n)) if n.text == "Found Pi 1.3.0"));
    assert!(rx.try_recv().is_err());
    assert_eq!(gate(Provider::Pi, Some("0.98.0"), &tx), Ok(()));
    rx.try_recv().unwrap();
    assert!(matches!(rx.try_recv(), Ok(Update::Warning(_))));
    assert!(gate(Provider::Pi, Some("0.80.0"), &tx).is_err());
    rx.try_recv().unwrap();
    assert_eq!(gate(Provider::Claude, None, &tx), Ok(()));
    assert!(
        matches!(rx.try_recv(), Ok(Update::Notice(n)) if n.text == "Found Claude Code (version unknown)")
    );
    assert!(rx.try_recv().is_err());
}

fn script(body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("aether-version-{}-{n}", std::process::id()));
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn cli_version_runs_with_the_repaired_path() {
    let path = aether_core::repaired_path();
    let bin = script(&format!(r#"[ "$PATH" = "{path}" ] && echo 'tool 1.2.3'"#));
    assert_eq!(cli_version(&bin), Some("1.2.3".into()));
}

#[test]
fn cli_version_gives_up_on_a_silent_cli() {
    let bin = script("exec sleep 30");
    let start = std::time::Instant::now();
    assert_eq!(run_version(&bin, Duration::from_millis(300)), None);
    assert!(start.elapsed() < Duration::from_secs(5));
}
