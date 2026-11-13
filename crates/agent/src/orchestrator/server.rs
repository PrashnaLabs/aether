use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aether_core::MutexExt;

use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

use super::{OrchCall, OrchPending, Stream, connect, try_queue};

const MAX_LINE: u64 = 64 * 1024;
const MAX_CONNS: usize = 32;

pub(crate) type Leads = Arc<Mutex<HashMap<String, String>>>;

pub(crate) fn listen(
    path: &Path,
    leads: Leads,
    calls: UnboundedSender<OrchCall>,
    pending: OrchPending,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let dir = path.parent().ok_or_else(|| std::io::Error::other("no socket directory"))?;
        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let metadata = std::fs::symlink_metadata(dir)?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o777 != 0o700 {
            return Err(std::io::Error::other("socket directory must be private (0700)"));
        }
    }
    if connect(path).is_ok() {
        return Err(std::io::Error::other("another Aether owns the socket"));
    }
    let _ = std::fs::remove_file(path);
    #[cfg(unix)]
    let listener = {
        use std::os::unix::fs::PermissionsExt;
        let listener = std::os::unix::net::UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        listener
    };
    #[cfg(windows)]
    let listener = listen_tcp(path)?;
    let conns = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if conns.fetch_add(1, Ordering::SeqCst) >= MAX_CONNS {
                conns.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let (leads, calls, pending, conns) =
                (leads.clone(), calls.clone(), pending.clone(), conns.clone());
            std::thread::spawn(move || {
                serve(stream, &leads, &calls, &pending);
                conns.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(())
}

#[cfg(any(windows, test))]
fn listen_tcp(path: &Path) -> std::io::Result<std::net::TcpListener> {
    // The orchestrator starts before the store creates the config directory.
    let dir = path.parent().ok_or_else(|| std::io::Error::other("no socket directory"))?;
    aether_core::create_private_dir(dir)?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    std::fs::write(path, listener.local_addr()?.to_string())?;
    Ok(listener)
}

fn serve(
    mut stream: Stream,
    leads: &Leads,
    calls: &UnboundedSender<OrchCall>,
    pending: &OrchPending,
) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Ok(line) = read_request(&stream, Instant::now() + Duration::from_secs(5)) else {
        return;
    };
    let reply = match answer(&line, leads, calls, pending) {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": error}),
    };
    let _ = writeln!(stream, "{reply}");
}

fn read_request(stream: &Stream, deadline: Instant) -> std::io::Result<String> {
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::TimedOut))?;
        stream.set_read_timeout(Some(remaining))?;
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        let n = buf.iter().position(|b| *b == b'\n').map_or(buf.len(), |i| i + 1);
        if line.len() + n > MAX_LINE as usize {
            return Err(std::io::Error::other("request too large"));
        }
        let complete = buf[n - 1] == b'\n';
        line.extend_from_slice(&buf[..n]);
        reader.consume(n);
        if complete {
            break;
        }
    }
    String::from_utf8(line).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn answer(
    line: &str,
    leads: &Leads,
    calls: &UnboundedSender<OrchCall>,
    pending: &OrchPending,
) -> Result<Value, String> {
    let silent = || "Aether didn't answer".to_string();
    let req: Value = serde_json::from_str(line).map_err(|_| silent())?;
    let lead = req["token"]
        .as_str()
        .and_then(|t| leads.guard().get(t).cloned())
        .ok_or("not an Aether lead")?;
    let (reply, rx) = std::sync::mpsc::channel();
    let deadline = Instant::now() + Duration::from_secs(30);
    let call = OrchCall {
        lead,
        tool: req["tool"].as_str().unwrap_or_default().to_string(),
        args: req["args"].clone(),
        reply,
        deadline,
    };
    try_queue(calls, pending, call)?;
    let out = rx.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    out.map_err(|_| silent())?
}

#[cfg(test)]
#[path = "../../tests/unit/orchestrator/server.rs"]
mod tests;
