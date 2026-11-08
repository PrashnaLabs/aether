use super::*;

#[test]
fn service_file_parses_url_and_password() {
    let s = parse_service(r#"{"id":"ac9f","version":"2.0.28","url":"http://127.0.0.1:49374","pid":6414,"password":"pw"}"#).unwrap();
    assert_eq!((s.url.as_str(), s.password.as_str()), ("http://127.0.0.1:49374", "pw"));
    let s2 = parse_service(r#"{"password":"pw"}"#).unwrap();
    assert_eq!((s2.url.as_str(), s2.password.as_str()), ("http://127.0.0.1:4096", "pw"));
    assert!(read_service_from(Path::new("/definitely/not/here-xyz/service.json")).is_none());
}

#[test]
fn service_log_lives_in_the_config_dir() {
    assert!(service_log().unwrap().ends_with("logs/opencode-service.log"));
}

#[test]
fn log_tail_keeps_the_last_lines() {
    let dir = std::env::temp_dir().join(format!("aether-oc-log-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("l.log");
    std::fs::write(&log, "a\nb\nc\n").unwrap();
    std::fs::write(&log, "a\n\n\x1b[31mb\x1b[0m\nc\n").unwrap();
    assert_eq!(tail_of(&log, 2), "b\nc");
    assert_eq!(tail_of(&dir.join("none.log"), 2), "");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_adopted_service_has_no_log() {
    STARTED.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(service_log_tail(20), "");
}

fn lead() -> crate::LeadTools {
    crate::LeadTools {
        exe: "/Apps/Aether".into(),
        sock: "/tmp/orch.sock".into(),
        token: "tok-secret".into(),
        specs: serde_json::json!([]),
    }
}

#[test]
fn lead_config_content_has_aether_entry_without_codemode() {
    let v: Value = serde_json::from_str(&lead_config(&lead())).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"mcp": {"aether": {
            "type": "local",
            "command": ["/Apps/Aether", "mcp"],
            "environment": {"AETHER_ORCH_SOCK": "/tmp/orch.sock", "AETHER_ORCH_TOKEN": "tok-secret"},
            "codemode": false,
        }}})
    );
}

#[test]
fn parses_listening_port_line() {
    assert_eq!(
        listening_url("server listening on http://127.0.0.1:4797\n").as_deref(),
        Some("http://127.0.0.1:4797")
    );
    assert_eq!(listening_url("server password abc"), None);
}

#[test]
fn mcp_ready_waits_for_connected() {
    let status =
        |s: &str| serde_json::json!({"data": [{"name": "aether", "status": {"status": s}}]});
    let mut seq =
        vec![serde_json::json!({"data": []}), status("pending"), status("connected")].into_iter();
    assert!(mcp_ready(|| seq.next(), Duration::from_secs(5)));
    assert!(!mcp_ready(|| Some(status("pending")), Duration::from_millis(300)));
}

fn spec(cmd: &std::process::Command) -> (Vec<String>, Vec<(String, String)>) {
    let s = |o: &std::ffi::OsStr| o.to_string_lossy().into_owned();
    let args = cmd.get_args().map(s).collect();
    let envs = cmd.get_envs().filter_map(|(k, v)| Some((s(k), s(v?)))).collect();
    (args, envs)
}

#[test]
fn non_lead_uses_shared_service() {
    let mut cmd = std::process::Command::new("opencode");
    serve(&mut cmd, None);
    let (args, envs) = spec(&cmd);
    assert_eq!(args, ["serve", "--service"]);
    assert!(envs.iter().all(|(k, _)| !k.starts_with("OPENCODE_")), "{envs:?}");
}

#[test]
fn lead_server_gets_token_through_env_only() {
    let mut cmd = std::process::Command::new("opencode");
    serve(&mut cmd, Some((&lead(), "pw")));
    let (args, envs) = spec(&cmd);
    assert_eq!(args, ["serve", "--port", "0", "--hostname", "127.0.0.1"]);
    assert!(envs.contains(&("OPENCODE_SERVER_PASSWORD".into(), "pw".into())));
    assert!(envs.contains(&("OPENCODE_CONFIG_CONTENT".into(), lead_config(&lead()))));
}

#[test]
fn stdout_scan_survives_non_utf8_and_drains() {
    let text = b"\xff\xfe junk\nserver listening on http://127.0.0.1:5\nmore \xff\nlast".to_vec();
    let mut out = std::io::Cursor::new(text);
    let (found, url) = std::sync::mpsc::channel();
    scan_stdout(&mut out, &found);
    assert_eq!(url.try_recv().as_deref(), Ok("http://127.0.0.1:5"));
    assert_eq!(out.position() as usize, out.get_ref().len());
}

fn sleeper() -> LeadServer {
    LeadServer::new(std::process::Command::new("sleep").arg("60").spawn().unwrap())
}

#[test]
fn lead_pids_track_spawn_and_drop() {
    let server = sleeper();
    let pid = server.0.id();
    assert!(LEAD_PIDS.lock().unwrap().contains(&pid));
    drop(server);
    assert!(!LEAD_PIDS.lock().unwrap().contains(&pid));
}

#[cfg(unix)]
#[test]
fn kill_lead_servers_kills_registered() {
    use std::os::unix::process::ExitStatusExt;
    let mut server = sleeper();
    kill_lead_servers();
    let status = server.0.wait().unwrap();
    assert_eq!(status.signal(), Some(9));
}
