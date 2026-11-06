//! External note opening. Call `prepare` on a worker, then send its command to
//! the global notes terminal. Live direct servers use a remote UI client.
//! Process probes must not block the UI thread.
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub fn prepare(
    settings: &aether_core::NotesSettings,
    root: &Path,
    path: &Path,
    line: Option<usize>,
) -> Result<String, String> {
    let root = root.canonicalize().map_err(|e| format!("Open notes directory: {e}"))?;
    let path = path.canonicalize().map_err(|e| format!("Open note: {e}"))?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err("The note must be a file inside the notes directory".into());
    }
    let nvim = executable(&settings.nvim, "nvim").ok_or_else(|| {
        "Neovim was not found. Install nvim or set its executable in Notes settings; native Aether editing is available.".to_string()
    })?;
    let socket = if settings.socket.trim().is_empty() {
        std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("notes-nvim.sock")
    } else {
        expand_home(&settings.socket)
    };
    let args = nvim_args(&socket, &path, line);
    let session = if settings.session.trim().is_empty() { "notes" } else { &settings.session };
    if session.starts_with('-') || session.contains([':', '.', '\n', '\r']) {
        return Err(
            "The notes tmux session name cannot start with '-' or contain ':', '.', or newlines"
                .into(),
        );
    }
    let live = run(
        &nvim,
        &["--server".into(), socket.as_os_str().into(), "--remote-expr".into(), "1".into()],
    )
    .is_ok_and(|out| out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "1");
    if live {
        let mut remote = vec!["--server".into(), socket.as_os_str().into(), "--remote".into()];
        if let Some(line) = line {
            remote.push(format!("+{}", line.max(1)).into());
        }
        remote.push(path.as_os_str().into());
        checked(&nvim, &remote, "Send note to Neovim")?;
    }
    if !live {
        remove_stale_socket(&socket).map_err(|error| format!("Check Neovim socket: {error}"))?;
    }
    let Some(tmux) = executable(&settings.tmux, "tmux") else {
        return if live { remote_ui_command(&nvim, &socket) } else { shell_command(&nvim, &args) };
    };
    if live {
        // Locate the server's actual pane instead of attaching an unrelated or
        // newly created empty session after the user changes session settings.
        let pid = run(
            &nvim,
            &[
                "--server".into(),
                socket.as_os_str().into(),
                "--remote-expr".into(),
                "getpid()".into(),
            ],
        )
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());
        if let Some(pid) =
            pid.filter(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
        {
            let panes = run(
                &tmux,
                &[
                    "list-panes".into(),
                    "-a".into(),
                    "-F".into(),
                    "#{pane_pid}\t#{window_id}\t#{session_name}".into(),
                ],
            )
            .ok()
            .filter(|out| out.status.success());
            if let Some(panes) = panes {
                for pane in String::from_utf8_lossy(&panes.stdout).lines() {
                    let fields: Vec<_> = pane.split('\t').collect();
                    if fields.len() == 3
                        && fields[0] == pid
                        && fields[1].strip_prefix('@').is_some_and(|id| {
                            !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
                        })
                    {
                        checked(
                            &tmux,
                            &["select-window".into(), "-t".into(), fields[1].into()],
                            "Select Neovim notes window",
                        )?;
                        return attach_command(
                            &tmux,
                            &[
                                "attach-session".into(),
                                "-t".into(),
                                format!("={}", fields[2]).into(),
                            ],
                        );
                    }
                }
            }
        }
        // A responding server can run outside tmux, or behind an intermediate
        // process. The note is open; leave unrelated sessions untouched.
        return remote_ui_command(&nvim, &socket);
    }
    let exists = run(&tmux, &["has-session".into(), "-t".into(), format!("={session}").into()])
        .is_ok_and(|out| out.status.success());
    if !exists {
        let mut create = vec![
            "new-session".into(),
            "-d".into(),
            "-s".into(),
            session.into(),
            "-n".into(),
            "notes".into(),
            "-c".into(),
            root.as_os_str().into(),
        ];
        create.push(shell_command(&nvim, &args)?.into());
        checked(&tmux, &create, "Create notes tmux session")?;
    } else {
        // A window named notes may belong to someone else. Never respawn or kill it.
        checked(
            &tmux,
            &[
                "new-window".into(),
                "-t".into(),
                format!("{session}:").into(),
                "-n".into(),
                "notes".into(),
                "-c".into(),
                root.as_os_str().into(),
                shell_command(&nvim, &args)?.into(),
            ],
            "Create notes tmux window",
        )?;
    }
    attach_command(&tmux, &["attach-session".into(), "-t".into(), format!("={session}").into()])
}

fn remote_ui_command(nvim: &Path, socket: &Path) -> Result<String, String> {
    shell_command(nvim, &["--server".into(), socket.as_os_str().into(), "--remote-ui".into()])
}

// Aether can itself be launched inside tmux; its own PTY is a separate client.
fn attach_command(program: &Path, args: &[std::ffi::OsString]) -> Result<String, String> {
    shell_command(program, args)
        .map(|command| format!("/usr/bin/env -u TMUX -u TMUX_PANE {command}"))
}

fn nvim_args(socket: &Path, path: &Path, line: Option<usize>) -> Vec<std::ffi::OsString> {
    let mut args = vec!["--listen".into(), socket.as_os_str().into()];
    if let Some(line) = line {
        args.push(format!("+{}", line.max(1)).into());
    }
    args.push("--".into());
    args.push(path.as_os_str().into());
    args
}

fn expand_home(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    PathBuf::from(value)
}

fn executable(value: &str, default: &str) -> Option<PathBuf> {
    let path = expand_home(if value.trim().is_empty() { default } else { value });
    if path.components().count() > 1 || path.is_absolute() {
        return is_executable(&path).then_some(path);
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&path))
        .find(|p| is_executable(p))
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Only remove a disconnected Unix socket; regular files and links are never removed.
fn remove_stale_socket(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::{
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
        };
        let before = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if !before.file_type().is_socket() {
            return Ok(());
        }
        if let Err(error) = UnixStream::connect(path)
            && error.kind() == std::io::ErrorKind::ConnectionRefused {
                let after = std::fs::symlink_metadata(path)?;
                if after.file_type().is_socket()
                    && before.dev() == after.dev()
                    && before.ino() == after.ino()
                {
                    std::fs::remove_file(path)?;
                }
            }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

// Files avoid pipe-buffer deadlocks and inherited pipe handles from a custom
// executable's descendants. Each call owns private output files and removes them.
struct ProcessOutput {
    file: std::fs::File,
    path: PathBuf,
}
impl ProcessOutput {
    fn new() -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "aether-notes-process-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = std::fs::OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => return Ok(Self { file, path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }
    fn bytes(&mut self) -> std::io::Result<Vec<u8>> {
        self.file.rewind()?;
        let mut bytes = Vec::new();
        (&mut self.file).take(1024 * 1024).read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}
impl Drop for ProcessOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
fn run(program: &Path, args: &[std::ffi::OsString]) -> std::io::Result<Output> {
    run_with_timeout(program, args, Duration::from_secs(5))
}
fn run_with_timeout(
    program: &Path,
    args: &[std::ffi::OsString],
    timeout: Duration,
) -> std::io::Result<Output> {
    let mut stdout = ProcessOutput::new()?;
    let mut stderr = ProcessOutput::new()?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.file.try_clone()?))
        .stderr(Stdio::from(stderr.file.try_clone()?))
        .spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(Output { status, stdout: stdout.bytes()?, stderr: stderr.bytes()? });
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(error) => error,
                    _ => {
                        std::io::Error::new(std::io::ErrorKind::TimedOut, "Notes process timed out")
                    }
                });
            }
        }
    }
}
fn checked(program: &Path, args: &[std::ffi::OsString], action: &str) -> Result<(), String> {
    let out = run(program, args).map_err(|e| format!("{action}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{action}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}
fn quote(value: &std::ffi::OsStr) -> Result<String, String> {
    let text = value.to_str().ok_or("The terminal requires UTF-8 note and executable paths")?;
    if text.chars().any(char::is_control) {
        return Err("Terminal arguments cannot contain control characters".into());
    }
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}
fn shell_command(program: &Path, args: &[std::ffi::OsString]) -> Result<String, String> {
    std::iter::once(program.as_os_str())
        .chain(args.iter().map(|a| a.as_os_str()))
        .map(quote)
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join(" "))
}

#[cfg(test)]
#[path = "../../tests/unit/notes/external.rs"]
mod tests;
