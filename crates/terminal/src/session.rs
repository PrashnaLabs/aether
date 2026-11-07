//! One shell in a PTY. Alacritty's event loop owns the IO thread; the listener
//! only flags new output and queues notices, and the UI polls both.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use aether_core::{MutexExt, TerminalCursor};
use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, State};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Config;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::tty::{self, Options, Pty, Shell};
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle};

use super::grid::Palette;

/// Grace a shell gets to exit on SIGHUP before it is killed.
const KILL_AFTER: Duration = Duration::from_millis(250);
const STOP_GRACE: Duration = Duration::from_secs(3);
const SCROLLBACK: usize = 10_000;

pub(crate) enum Notice {
    Exited(Option<ExitStatus>),
    Copy(String),
    /// Window title a program set (OSC 0/2); `None` when it reset it.
    Title(Option<String>),
}

/// Lock order: the PTY thread takes the terminal lock first and these mutexes
/// inside it, so the UI never takes the terminal lock while holding one of them.
struct Shared {
    dirty: AtomicBool,
    exited: AtomicBool,
    notices: Sender<Notice>,
    pty: OnceLock<EventLoopSender>,
    size: Mutex<WindowSize>,
    palette: Mutex<Palette>,
}

impl Shared {
    fn write(&self, bytes: Vec<u8>) {
        // alacritty's loop hangs on an empty write
        if let (false, Some(pty)) = (bytes.is_empty(), self.pty.get()) {
            let _ = pty.send(Msg::Input(Cow::Owned(bytes)));
        }
    }
}

/// Runs on the PTY thread inside the terminal lock, so it answers queries from
/// shared copies and never touches the UI or the terminal.
#[derive(Clone)]
pub(crate) struct Listener(Arc<Shared>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let s = &self.0;
        match event {
            Event::Wakeup => {}
            Event::PtyWrite(text) => s.write(text.into_bytes()),
            Event::ColorRequest(index, format) => {
                let rgb = s.palette.guard().index(index);
                s.write(format(rgb).into_bytes());
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *s.size.guard();
                s.write(format(size).into_bytes());
            }
            Event::ClipboardStore(_, text) => {
                let _ = s.notices.send(Notice::Copy(text));
            }
            Event::Title(title) => {
                let _ = s.notices.send(Notice::Title(Some(title)));
            }
            Event::ResetTitle => {
                let _ = s.notices.send(Notice::Title(None));
            }
            Event::ChildExit(status) => {
                s.exited.store(true, Ordering::Release);
                let _ = s.notices.send(Notice::Exited(Some(status)));
            }
            Event::Exit => {
                s.exited.store(true, Ordering::Release);
                let _ = s.notices.send(Notice::Exited(None));
            }
            _ => return,
        }
        s.dirty.store(true, Ordering::Release);
    }
}

pub(crate) struct Session {
    pub term: Arc<FairMutex<Term<Listener>>>,
    /// Shell program path.
    pub shell: String,
    shared: Arc<Shared>,
    notices: Receiver<Notice>,
    pid: u32,
    /// PTY controller fd, alive as long as `io` holds the event loop.
    #[cfg_attr(not(unix), allow(dead_code))]
    master: i32,
    io: Option<JoinHandle<(EventLoop<Pty, Listener>, State)>>,
}

fn term_config(cursor: TerminalCursor) -> Config {
    let shape = match cursor {
        TerminalCursor::Block => CursorShape::Block,
        TerminalCursor::Bar => CursorShape::Beam,
        TerminalCursor::Underline => CursorShape::Underline,
    };
    Config {
        scrolling_history: SCROLLBACK,
        default_cursor_style: CursorStyle { shape, blinking: false },
        ..Config::default()
    }
}

impl Session {
    /// The user's login shell in `folder`. Blocking: resolves PATH through a
    /// login shell and forks. Run off the UI thread.
    pub fn spawn(folder: &Path, size: WindowSize, palette: Palette) -> Result<Self, String> {
        let shell = user_shell();
        let args = login_args(&shell);
        Self::start(folder, shell, args, size, palette)
    }

    pub fn spawn_command(
        folder: &Path,
        command: &str,
        size: WindowSize,
        palette: Palette,
    ) -> Result<Self, String> {
        let shell = user_shell();
        let args = command_args(&shell, command);
        Self::start(folder, shell, args, size, palette)
    }

    fn start(
        folder: &Path,
        shell: String,
        args: Vec<String>,
        size: WindowSize,
        palette: Palette,
    ) -> Result<Self, String> {
        if !folder.is_dir() {
            return Err(format!("{} doesn't exist anymore.", folder.display()));
        }
        let unset = foreign_terminal_vars(std::env::vars().map(|(k, _)| k));
        let (program, args) = launch(&shell, args, unset);
        // windows Options has extra fields (escape_args)
        #[allow(clippy::needless_update)]
        let options = Options {
            shell: Some(Shell::new(program, args)),
            working_directory: Some(folder.into()),
            drain_on_exit: true,
            env: shell_env(aether_core::repaired_path(), |k| std::env::var_os(k).is_some()),
            ..Options::default()
        };
        let pty = tty::new(&options, size, 0).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let (pid, master) = {
            use std::os::fd::AsRawFd;
            (pty.child().id(), pty.file().as_raw_fd())
        };
        #[cfg(windows)]
        let (pid, master) = (pty.child_watcher().pid().map_or(0, |pid| pid.get()), -1);
        let (tx, notices) = mpsc::channel();
        let shared = Arc::new(Shared {
            dirty: AtomicBool::new(true),
            exited: AtomicBool::new(false),
            notices: tx,
            pty: OnceLock::new(),
            size: Mutex::new(size),
            palette: Mutex::new(palette),
        });
        let config = term_config(TerminalCursor::default());
        let dims = TermSize::new(size.num_cols as usize, size.num_lines as usize);
        let term = Arc::new(FairMutex::new(Term::new(config, &dims, Listener(shared.clone()))));
        let event_loop = EventLoop::new(term.clone(), Listener(shared.clone()), pty, true, false)
            .map_err(|e| e.to_string())?;
        let _ = shared.pty.set(event_loop.channel());
        let io = Some(event_loop.spawn());
        Ok(Self { term, shell, shared, notices, pid, master, io })
    }

    pub fn write(&self, bytes: Vec<u8>) {
        self.shared.write(bytes);
    }

    /// True once since the last call when the screen changed.
    pub fn take_dirty(&self) -> bool {
        self.shared.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn notices(&self) -> impl Iterator<Item = Notice> + '_ {
        self.notices.try_iter()
    }

    pub fn same_size(&self, size: WindowSize) -> bool {
        let current = *self.shared.size.guard();
        (current.num_cols, current.num_lines, current.cell_width, current.cell_height)
            == (size.num_cols, size.num_lines, size.cell_width, size.cell_height)
    }

    pub fn resize(&self, size: WindowSize) {
        *self.shared.size.guard() = size;
        self.term.lock().resize(TermSize::new(size.num_cols as usize, size.num_lines as usize));
        if let Some(pty) = self.shared.pty.get() {
            let _ = pty.send(Msg::Resize(size));
        }
    }

    /// The cursor for programs that don't set their own (DECSCUSR).
    pub fn set_cursor(&self, cursor: TerminalCursor) {
        self.term.lock().set_options(term_config(cursor));
        self.shared.dirty.store(true, Ordering::Release);
    }

    /// Colors the PTY thread reports when a program asks (OSC 4/10/11).
    pub fn set_palette(&self, palette: Palette) {
        *self.shared.palette.guard() = palette;
        self.shared.dirty.store(true, Ordering::Release);
    }

    /// Dynamic palette update when theme changes.
    pub fn update_theme(&self, palette: Palette) {
        self.set_palette(palette);
    }

    /// The program in the foreground instead of the shell: its name, or an
    /// empty string when the name can't be read. `None` while the shell is idle.
    #[cfg(unix)]
    #[allow(unsafe_code)]
    pub fn running_job(&self) -> Option<String> {
        unsafe extern "C" {
            fn tcgetpgrp(fd: i32) -> i32;
        }
        if self.shared.exited.load(Ordering::Acquire) {
            return None;
        }
        // SAFETY: an ioctl on our own controller fd, which the event loop keeps open.
        let group = unsafe { tcgetpgrp(self.master) };
        (group > 0 && group as u32 != self.pid).then(|| process_name(group).unwrap_or_default())
    }

    #[cfg(not(unix))]
    pub fn running_job(&self) -> Option<String> {
        None
    }

    pub fn terminate(&self) -> bool {
        if self.io.is_none() || self.shared.exited.load(Ordering::Acquire) {
            return false;
        }
        signal_group(self.pid, SIGTERM);
        let (shared, pid) = (Arc::downgrade(&self.shared), self.pid);
        std::thread::spawn(move || {
            std::thread::sleep(STOP_GRACE);
            // dead once the session is dropped and the shell reaped; its pid may be reused
            if shared.upgrade().is_some_and(|s| !s.exited.load(Ordering::Acquire)) {
                signal_group(pid, SIGKILL);
            }
        });
        true
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(pty) = self.shared.pty.get() {
            let _ = pty.send(Msg::Shutdown);
        }
        let Some(io) = self.io.take() else {
            return;
        };
        let pid = self.pid;
        // once exited, the loop has reaped the shell and its pid may be reused
        if !self.shared.exited.load(Ordering::Acquire) {
            signal(pid, SIGHUP);
        }
        // Dropping alacritty's PTY sends SIGHUP and waits with no timeout, which
        // hangs on a shell that ignores it; do that here, with a kill as backstop.
        std::thread::spawn(move || {
            let Ok(event_loop) = io.join() else {
                return;
            };
            let reaped = Arc::new(AtomicBool::new(false));
            let watchdog = reaped.clone();
            std::thread::spawn(move || {
                std::thread::sleep(KILL_AFTER);
                if !watchdog.load(Ordering::Acquire) {
                    signal(pid, SIGKILL);
                }
            });
            drop(event_loop);
            reaped.store(true, Ordering::Release);
        });
    }
}

const SIGHUP: i32 = 1;
const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;

#[cfg(unix)]
#[allow(unsafe_code)]
fn signal(pid: u32, sig: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // SAFETY: kill(2) takes plain integers and touches no memory of ours.
    unsafe {
        kill(pid as i32, sig);
    }
}

// no signals here: SIGKILL ends the tree, anything else is left to the PTY closing
#[cfg(windows)]
fn signal(pid: u32, sig: i32) {
    if sig == SIGKILL && pid > 1 {
        aether_core::signal_process(pid, "KILL", true);
    }
}

/// The shell leads its PTY session, so its pid is also its process group.
#[cfg(unix)]
#[allow(unsafe_code)]
fn signal_group(pid: u32, sig: i32) {
    unsafe extern "C" {
        fn killpg(pgrp: i32, sig: i32) -> i32;
    }
    // SAFETY: killpg(2) takes plain integers and touches no memory of ours.
    unsafe {
        killpg(pid as i32, sig);
    }
}

#[cfg(windows)]
fn signal_group(pid: u32, _: i32) {
    if pid > 1 {
        std::thread::spawn(move || aether_core::signal_process(pid, "KILL", true));
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn process_name(pid: i32) -> Option<String> {
    unsafe extern "C" {
        fn proc_name(pid: i32, buffer: *mut u8, size: u32) -> i32;
    }
    let mut buf = [0u8; 256];
    // SAFETY: proc_name writes at most `size` bytes into `buf`.
    let len = unsafe { proc_name(pid, buf.as_mut_ptr(), buf.len() as u32) };
    (len > 0).then(|| String::from_utf8_lossy(&buf[..len as usize]).into_owned())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn process_name(pid: i32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|s| s.trim().to_string())
}

/// The account's login shell from the user database.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(unsafe_code)]
fn account_shell() -> Option<String> {
    use std::ffi::{CStr, c_char};
    #[cfg(target_os = "macos")]
    #[repr(C)]
    struct Passwd {
        name: *mut c_char,
        passwd: *mut c_char,
        uid: u32,
        gid: u32,
        change: i64,
        class: *mut c_char,
        gecos: *mut c_char,
        dir: *mut c_char,
        shell: *mut c_char,
        expire: i64,
    }
    #[cfg(target_os = "linux")]
    #[repr(C)]
    struct Passwd {
        name: *mut c_char,
        passwd: *mut c_char,
        uid: u32,
        gid: u32,
        gecos: *mut c_char,
        dir: *mut c_char,
        shell: *mut c_char,
    }
    unsafe extern "C" {
        fn getuid() -> u32;
        fn getpwuid_r(
            uid: u32,
            pwd: *mut Passwd,
            buf: *mut c_char,
            len: usize,
            result: *mut *mut Passwd,
        ) -> i32;
    }
    let mut pwd = std::mem::MaybeUninit::<Passwd>::uninit();
    let mut buf = [0 as c_char; 4096];
    let mut result = std::ptr::null_mut();
    // SAFETY: getpwuid_r fills `pwd` with pointers into `buf`; both outlive the reads below.
    unsafe {
        if getpwuid_r(getuid(), pwd.as_mut_ptr(), buf.as_mut_ptr(), buf.len(), &mut result) != 0
            || result.is_null()
        {
            return None;
        }
        let shell = (*result).shell;
        (!shell.is_null()).then(|| CStr::from_ptr(shell).to_string_lossy().into_owned())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
#[cfg_attr(windows, allow(dead_code))]
fn account_shell() -> Option<String> {
    None
}

/// PowerShell 7, else Windows PowerShell, else `%ComSpec%` (cmd.exe).
#[cfg(windows)]
fn user_shell() -> String {
    ["pwsh", "powershell"]
        .iter()
        .find_map(|name| aether_core::find_binary(name, ""))
        .map(|path| path.display().to_string())
        .or_else(|| std::env::var("ComSpec").ok())
        .unwrap_or_else(|| "cmd.exe".into())
}

#[cfg(not(windows))]
fn user_shell() -> String {
    let env_shell = std::env::var("SHELL").ok();
    shell_program(account_shell().as_deref(), env_shell.as_deref(), |p| Path::new(p).is_file())
}

/// The account's shell, else `$SHELL`, as long as it is an absolute path to an
/// existing program; else `/bin/sh`.
#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn shell_program(
    account: Option<&str>,
    env: Option<&str>,
    exists: impl Fn(&str) -> bool,
) -> String {
    [account, env]
        .into_iter()
        .flatten()
        .find(|s| s.starts_with('/') && exists(s))
        .unwrap_or("/bin/sh")
        .to_string()
}

fn is_powershell(shell: &str) -> bool {
    let name = shell.rsplit(['/', '\\']).next().unwrap_or(shell).to_ascii_lowercase();
    matches!(name.trim_end_matches(".exe"), "pwsh" | "powershell")
}

/// Arguments for an interactive shell.
fn login_args(shell: &str) -> Vec<String> {
    match (cfg!(windows), is_powershell(shell)) {
        (false, _) => vec!["-l".into()],
        (true, true) => vec!["-NoLogo".into()],
        (true, false) => Vec::new(),
    }
}

/// Arguments that make `shell` run `command` and exit; PowerShell and cmd take the
/// rest of the line as the command, so it is passed unquoted.
pub(crate) fn command_args(shell: &str, command: &str) -> Vec<String> {
    match (cfg!(windows), is_powershell(shell)) {
        (false, _) => vec!["-lc".into(), command.to_string()],
        (true, true) => vec!["-NoLogo".into(), "-Command".into(), command.to_string()],
        (true, false) => vec!["/C".into(), command.to_string()],
    }
}

/// Variables the shell gets on top of the app's own environment.
pub(crate) fn shell_env(path: String, is_set: impl Fn(&str) -> bool) -> HashMap<String, String> {
    let mut env = HashMap::from([
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("TERM_PROGRAM".to_string(), "aether".to_string()),
        ("PATH".to_string(), path),
    ]);
    // apps opened from Finder get no locale, and shells then mangle UTF-8
    if !["LC_ALL", "LC_CTYPE", "LANG"].into_iter().any(is_set) {
        env.insert("LANG".into(), "en_US.UTF-8".into());
    }
    env
}

/// Inherited variables that would make the shell and its tools think they run in
/// the terminal Aether was launched from.
pub(crate) fn foreign_terminal_vars(names: impl Iterator<Item = String>) -> Vec<String> {
    const EXACT: [&str; 2] = ["TERM_PROGRAM_VERSION", "TERM_SESSION_ID"];
    const PREFIXES: [&str; 6] =
        ["ITERM_", "LC_TERMINAL", "KITTY_", "ALACRITTY_", "WEZTERM_", "VSCODE_"];
    let mut found: Vec<String> = names
        .filter(|n| EXACT.contains(&n.as_str()) || PREFIXES.iter().any(|p| n.starts_with(p)))
        .collect();
    found.sort();
    found
}

/// Program and arguments that start `shell` without the `unset` variables;
/// alacritty can add variables but not remove them, so `env -u` does.
pub(crate) fn launch(shell: &str, args: Vec<String>, unset: Vec<String>) -> (String, Vec<String>) {
    // alacritty puts the program on the command line unquoted
    if cfg!(windows) {
        return (format!("\"{shell}\""), args);
    }
    if unset.is_empty() {
        return (shell.to_string(), args);
    }
    let mut all: Vec<String> =
        unset.into_iter().flat_map(|name| ["-u".to_string(), name]).collect();
    all.push(shell.to_string());
    all.extend(args);
    ("/usr/bin/env".into(), all)
}

#[cfg(test)]
#[path = "../tests/unit/session.rs"]
mod tests;
