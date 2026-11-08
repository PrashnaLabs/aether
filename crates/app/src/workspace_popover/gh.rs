use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aether_core::{find_binary, home_dir, repaired_path};

use super::model::{GhState, first_stderr_line, gh_state};

pub(super) const QUICK: Duration = Duration::from_secs(15);
pub(super) const SLOW: Duration = Duration::from_secs(120);

pub(super) fn run(cwd: &Path, args: &[String], timeout: Duration) -> Result<(), String> {
    let bin = find_binary("gh", "").ok_or("GitHub CLI not found")?;
    let mut child = aether_core::command(bin)
        .args(args)
        .current_dir(cwd)
        .env("PATH", repaired_path())
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        // empty, not unset: stops git falling back to SSH_ASKPASS
        .env("GIT_ASKPASS", "")
        .env_remove("SSH_ASKPASS")
        .envs(aether_git::batch_ssh(cwd))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stderr = child.stderr.take();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_end(&mut out);
        }
        let _ = tx.send(out);
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(_) => {
                let err = rx.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
                return Err(first_stderr_line(&String::from_utf8_lossy(&err)));
            }
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            None => {
                let _ = child.kill();
                std::thread::spawn(move || child.wait());
                return Err(format!("gh took longer than {} s and was stopped", timeout.as_secs()));
            }
        }
    }
}

pub(super) fn detect() -> GhState {
    let found = find_binary("gh", "").is_some();
    let auth = || run(&home_dir(), &["auth".into(), "status".into()], QUICK).is_ok();
    gh_state(found, found && auth())
}
