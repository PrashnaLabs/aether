use super::*;
use crate::orchestrator::forward;
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

fn pend() -> crate::orchestrator::OrchPending {
    Arc::new(AtomicUsize::new(0))
}

#[test]
fn tcp_listener_starts_before_config_directory_exists() {
    let dir = std::env::temp_dir().join(format!("ao-tcp-first-run-{}", std::process::id()));
    let path = dir.join("aether/orchestrator.sock");
    let _ = std::fs::remove_dir_all(&dir);
    let listener = listen_tcp(&path).expect("first-run orchestrator must create its directory");
    let addr = std::fs::read_to_string(&path).unwrap();
    assert_eq!(addr, listener.local_addr().unwrap().to_string());
    assert!(listener.local_addr().unwrap().ip().is_loopback());
    let _client = std::net::TcpStream::connect(addr).unwrap();
    listener.accept().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

fn sock(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("ao-{name}-{}", std::process::id()));
    #[cfg(unix)]
    let path = path.join("o.sock");
    path
}

#[test]
fn known_token_reaches_app_with_lead_key_and_reply_returns() {
    let p = sock("ok");
    let leads: Leads = Default::default();
    leads.lock().unwrap().insert("tok".into(), "42".into());
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    listen(&p, leads, tx, pend()).unwrap();
    let app = std::thread::spawn(move || {
        let call = futures::executor::block_on(rx.next()).unwrap();
        assert_eq!((call.lead.as_str(), call.tool.as_str()), ("42", "list_agents"));
        call.reply.send(Ok(serde_json::json!({"agents": []}))).unwrap();
    });
    assert_eq!(
        forward(&p, "tok", "list_agents", serde_json::json!({})).unwrap()["agents"],
        serde_json::json!([])
    );
    app.join().unwrap();
}

#[test]
fn unknown_token_is_refused_without_reaching_app() {
    let p = sock("bad");
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    listen(&p, Default::default(), tx, pend()).unwrap();
    assert_eq!(
        forward(&p, "nope", "list_agents", serde_json::json!({})).unwrap_err(),
        "not an Aether lead"
    );
    assert!(rx.try_recv().is_err(), "nothing queued");
}

#[cfg(unix)]
#[test]
fn stale_socket_file_is_replaced_and_mode_is_0600() {
    use std::os::unix::fs::PermissionsExt;
    let p = sock("stale");
    aether_core::create_private_dir(p.parent().unwrap()).unwrap();
    std::fs::write(&p, b"").unwrap();
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    listen(&p, Default::default(), tx, pend()).unwrap();
    assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn too_long_path_is_an_error_not_a_panic() {
    let p = std::env::temp_dir().join("x".repeat(200)).join("o.sock");
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    assert!(listen(&p, Default::default(), tx, pend()).is_err());
}

#[test]
fn dropped_reply_reports_app_gone() {
    let p = sock("gone");
    let leads: Leads = Default::default();
    leads.lock().unwrap().insert("tok".into(), "1".into());
    let (tx, rx) = futures::channel::mpsc::unbounded();
    listen(&p, leads, tx, pend()).unwrap();
    drop(rx);
    assert_eq!(
        forward(&p, "tok", "list_agents", serde_json::json!({})).unwrap_err(),
        "Aether didn't answer"
    );
}

#[test]
fn live_socket_is_not_stolen() {
    let p = sock("live");
    let leads: Leads = Default::default();
    leads.lock().unwrap().insert("tok".into(), "7".into());
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    listen(&p, leads, tx, pend()).unwrap();
    let (tx2, _rx2) = futures::channel::mpsc::unbounded();
    assert!(listen(&p, Default::default(), tx2, pend()).is_err());
    let app = std::thread::spawn(move || {
        let call = futures::executor::block_on(rx.next()).unwrap();
        assert_eq!(call.lead, "7");
        call.reply.send(Ok(serde_json::json!({}))).unwrap();
    });
    forward(&p, "tok", "list_agents", serde_json::json!({})).unwrap();
    app.join().unwrap();
}

#[cfg(unix)]
#[test]
fn oversize_line_is_dropped_without_reaching_app() {
    use std::io::{Read, Write};
    let p = sock("big");
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    listen(&p, Default::default(), tx, pend()).unwrap();
    let mut s = std::os::unix::net::UnixStream::connect(&p).unwrap();
    s.write_all(&vec![b'x'; 70 * 1024]).unwrap();
    s.shutdown(std::net::Shutdown::Write).unwrap();
    let mut buf = [0u8; 8];
    assert_eq!(s.read(&mut buf).unwrap_or(0), 0);
    assert!(rx.try_recv().is_err(), "nothing queued");
}

#[test]
fn full_pending_queue_is_busy_not_queued() {
    let p = sock("full");
    let leads: Leads = Default::default();
    leads.lock().unwrap().insert("tok".into(), "1".into());
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let pending = pend();
    pending.store(crate::orchestrator::MAX_PENDING, std::sync::atomic::Ordering::SeqCst);
    listen(&p, leads, tx, pending).unwrap();
    assert_eq!(
        forward(&p, "tok", "list_agents", serde_json::json!({})).unwrap_err(),
        crate::orchestrator::BUSY
    );
    assert!(rx.try_recv().is_err(), "nothing queued");
}

#[cfg(unix)]
#[test]
fn socket_directory_is_private_and_shared_parent_is_unchanged() {
    use std::os::unix::fs::PermissionsExt;
    let shared = std::env::temp_dir();
    let mode = std::fs::metadata(&shared).unwrap().permissions().mode();
    let p = sock("private");
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    listen(&p, Default::default(), tx, pend()).unwrap();
    assert_eq!(std::fs::metadata(p.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(shared).unwrap().permissions().mode(), mode);
}

#[cfg(unix)]
#[test]
fn unsafe_existing_socket_directory_is_rejected_without_chmod() {
    use std::os::unix::fs::PermissionsExt;
    let p = sock("public");
    let dir = p.parent().unwrap();
    std::fs::create_dir_all(dir).unwrap();
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    assert!(listen(&p, Default::default(), tx, pend()).is_err());
    assert_eq!(std::fs::metadata(dir).unwrap().permissions().mode() & 0o777, 0o755);
    assert!(!p.exists());
    std::fs::remove_dir(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn trickling_request_cannot_extend_total_read_deadline() {
    let (server, mut client) = Stream::pair().unwrap();
    let start = Instant::now();
    let writer = std::thread::spawn(move || {
        for _ in 0..20 {
            if client.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    });
    assert!(read_request(&server, start + Duration::from_millis(150)).is_err());
    assert!(start.elapsed() < Duration::from_millis(500));
    drop(server);
    writer.join().unwrap();
}

#[test]
fn timed_out_call_keeps_pending_slot_until_ui_consumes_it() {
    let leads: Leads = Default::default();
    leads.lock().unwrap().insert("tok".into(), "1".into());
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let pending = pend();
    pending.store(crate::orchestrator::MAX_PENDING - 1, Ordering::SeqCst);
    let request = r#"{"token":"tok","tool":"list_agents","args":{}}"#;
    assert_eq!(answer(request, &leads, &tx, &pending).unwrap_err(), "Aether didn't answer");
    assert_eq!(pending.load(Ordering::SeqCst), crate::orchestrator::MAX_PENDING);
    assert_eq!(answer(request, &leads, &tx, &pending).unwrap_err(), crate::orchestrator::BUSY);
    let call = rx.try_recv().unwrap();
    assert!(call.deadline <= Instant::now());
    assert!(call.reply.send(Ok(serde_json::json!({}))).is_err());
    crate::orchestrator::done_pending(&pending);
    assert_eq!(pending.load(Ordering::SeqCst), crate::orchestrator::MAX_PENDING - 1);
}
