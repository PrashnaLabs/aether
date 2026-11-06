use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aether_core::MutexExt;

use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

use super::{OrchCall, OrchPending, done_pending, try_queue};

const MAX_LINE: u64 = 64 * 1024;
const MAX_CONNS: usize = 32;

pub(crate) type Leads = Arc<Mutex<HashMap<String, String>>>;

pub(crate) fn listen(
    path: &Path,
    leads: Leads,
    calls: UnboundedSender<OrchCall>,
    pending: OrchPending,
) -> std::io::Result<()> {
    if UnixStream::connect(path).is_ok() {
        return Err(std::io::Error::other("another Aether owns the socket"));
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
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

fn serve(
    mut stream: UnixStream,
    leads: &Leads,
    calls: &UnboundedSender<OrchCall>,
    pending: &OrchPending,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    if BufReader::new(&stream).take(MAX_LINE + 1).read_line(&mut line).is_err()
        || line.len() as u64 > MAX_LINE
    {
        return;
    }
    let reply = match answer(&line, leads, calls, pending) {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": error}),
    };
    let _ = writeln!(stream, "{reply}");
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
    let call = OrchCall {
        lead,
        tool: req["tool"].as_str().unwrap_or_default().to_string(),
        args: req["args"].clone(),
        reply,
    };
    try_queue(calls, pending, call)?;
    let out = rx.recv_timeout(Duration::from_secs(30));
    done_pending(pending);
    out.map_err(|_| silent())?
}

#[cfg(test)]
#[path = "../../tests/unit/orchestrator/server.rs"]
mod tests;
