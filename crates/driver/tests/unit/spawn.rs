use super::*;

#[test]
fn stderr_filter_hides_transport_noise() {
    assert!(!visible_stderr("transport closed"));
    assert!(visible_stderr("error: connection refused"));
}

#[test]
fn stderr_tail_keeps_the_last_lines_after_the_headline() {
    let mut tail = StderrTail::default();
    for i in 0..45 {
        tail.push(&format!("line {i}"));
    }
    tail.push("\x1b[31mthread 'main' panicked at src/x.rs:1:1\x1b[0m");
    tail.push("   ");
    tail.push("note: run with `RUST_BACKTRACE=1`");
    let fatal = tail.fatal("codex exited (exit status: 101)");
    let lines: Vec<&str> = fatal.lines().collect();
    assert_eq!(lines[0], "codex exited (exit status: 101)");
    assert_eq!(lines.len(), 41);
    assert!(lines.contains(&"thread 'main' panicked at src/x.rs:1:1"));
    assert_eq!(lines.last(), Some(&"note: run with `RUST_BACKTRACE=1`"));
    assert_eq!(StderrTail::default().fatal("pi exited"), "pi exited");
}

#[test]
fn unknown_events_are_logged_once() {
    assert!(log_unknown("test", "system/teleport"));
    assert!(!log_unknown("test", "system/teleport"));
    assert!(log_unknown("test", "system/other"));
}

#[test]
fn describe_hides_env() {
    let mut cmd = Command::new("/bin/codex");
    cmd.arg("app-server").env("AETHER_ORCH_TOKEN", "secret");
    let d = describe(&cmd);
    assert!(!d.contains("secret") && !d.contains("AETHER_ORCH_TOKEN"));
    assert!(d.contains("/bin/codex") && d.contains("app-server"));
}

#[test]
fn command_applies_account_env() {
    let opts = DriverStartOptions {
        ephemeral: false,
        env: vec![("CLAUDE_CONFIG_DIR".into(), "/w/claude".into())],
        binary: std::env::current_exe().unwrap().to_string_lossy().into_owned(),
        cwd: "/tmp".into(),
        model: String::new(),
        effort: String::new(),
        access: aether_core::AccessMode::Full,
        resume: String::new(),
        fork: false,
        rollback: String::new(),
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let cmd = command(&opts, "sh").unwrap();
    let want = (std::ffi::OsStr::new("CLAUDE_CONFIG_DIR"), Some(std::ffi::OsStr::new("/w/claude")));
    assert!(cmd.get_envs().any(|e| e == want));
}

#[test]
fn command_strips_lead_env() {
    let opts = DriverStartOptions {
        ephemeral: false,
        env: Vec::new(),
        binary: std::env::current_exe().unwrap().to_string_lossy().into_owned(),
        cwd: "/tmp".into(),
        model: String::new(),
        effort: String::new(),
        access: aether_core::AccessMode::Full,
        resume: String::new(),
        rollback: String::new(),
        fork: false,
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let cmd = command(&opts, "sh").unwrap();
    let envs: Vec<_> = cmd.get_envs().collect();
    for key in
        ["AETHER_ORCH_SOCK", "AETHER_ORCH_TOKEN", "AETHER_ORCH_SPECS", "OPENCODE_CONFIG_CONTENT"]
    {
        assert!(envs.contains(&(std::ffi::OsStr::new(key), None)), "{key}");
    }
}
