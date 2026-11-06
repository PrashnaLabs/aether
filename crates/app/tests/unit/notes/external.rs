use super::*;

#[test]
fn terminal_arguments_are_single_quoted() {
    let command =
        shell_command(Path::new("/some dir/nvim"), &["a'$(touch /tmp/bad);file".into()]).unwrap();
    assert_eq!(command, "'/some dir/nvim' 'a'\\''$(touch /tmp/bad);file'");
}

#[test]
fn terminal_control_characters_are_rejected() {
    assert!(quote(std::ffi::OsStr::new("bad\u{3}name")).is_err());
    assert!(quote(std::ffi::OsStr::new("bad\rname")).is_err());
}

#[test]
fn nvim_places_filename_after_option_separator() {
    let args = nvim_args(Path::new("/tmp/socket"), Path::new("/notes/-evil.md"), Some(0));
    assert_eq!(
        args,
        vec!["--listen", "/tmp/socket", "+1", "--", "/notes/-evil.md"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>()
    );
}

#[cfg(unix)]
struct Fixture {
    root: PathBuf,
    settings: aether_core::NotesSettings,
    log: PathBuf,
}
#[cfg(unix)]
impl Fixture {
    fn new(live: bool) -> Self {
        use std::os::unix::fs::PermissionsExt;
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "aether-notes-external-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let log = root.join("calls");
        let nvim = root.join("fake nvim");
        let tmux = root.join("fake tmux");
        let log_arg = quote(log.as_os_str()).unwrap();
        std::fs::write(&nvim, format!("#!/bin/sh\nprintf '%s\\n' NVIM \"$@\" >> {log_arg}\ncase \"$*\" in *--remote-expr*) echo {}; exit 0;; esac\n", if live { "1" } else { "0" })).unwrap();
        std::fs::write(
            &tmux,
            format!("#!/bin/sh\nprintf '%s\\n' TMUX \"$@\" >> {log_arg}\ncase \"$1\" in list-panes) printf '1\\t@7\\tactual-notes\\n';; esac\nexit 0\n"),
        )
        .unwrap();
        for tool in [&nvim, &tmux] {
            std::fs::set_permissions(tool, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut settings = aether_core::NotesSettings::default();
        settings.nvim = nvim.to_str().unwrap().into();
        settings.tmux = tmux.to_str().unwrap().into();
        settings.socket = root.join("socket").to_str().unwrap().into();
        Self { root, settings, log }
    }
    fn note(&self) -> PathBuf {
        let path = self.root.join("a'$(evil).md");
        std::fs::write(&path, "note").unwrap();
        path
    }
}
#[cfg(unix)]
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
#[cfg(unix)]
fn live_server_is_reused_without_respawning_windows() {
    let fixture = Fixture::new(true);
    let command = prepare(&fixture.settings, &fixture.root, &fixture.note(), Some(12)).unwrap();
    assert!(command.contains("'attach-session'"));
    assert!(command.starts_with("/usr/bin/env -u TMUX -u TMUX_PANE "));
    let calls = std::fs::read_to_string(&fixture.log).unwrap();
    assert!(calls.contains("--remote\n+12\n"));
    assert!(calls.contains("select-window\n-t\n@7\n"));
    assert!(command.contains("actual-notes"));
    assert!(!calls.contains("new-session"));
    assert!(!calls.contains("new-window"));
    assert!(!calls.contains("respawn"));
    assert!(!calls.contains("kill"));
}

#[test]
#[cfg(unix)]
fn unavailable_server_creates_window_without_killing_existing_window() {
    let fixture = Fixture::new(false);
    prepare(&fixture.settings, &fixture.root, &fixture.note(), None).unwrap();
    let calls = std::fs::read_to_string(&fixture.log).unwrap();
    assert!(calls.contains("new-window\n"));
    assert!(calls.contains("'--listen'"));
    assert!(!calls.contains("respawn"));
    assert!(!calls.contains("kill"));
}

#[test]
#[cfg(unix)]
fn absent_tmux_returns_direct_nvim_command() {
    let mut fixture = Fixture::new(false);
    fixture.settings.tmux = fixture.root.join("absent").to_str().unwrap().into();
    let command = prepare(&fixture.settings, &fixture.root, &fixture.note(), None).unwrap();
    assert!(command.contains("fake nvim' '--listen'"));
    assert!(command.contains(" '--' "));
    let calls = std::fs::read_to_string(&fixture.log).unwrap();
    assert!(calls.contains("--remote-expr\n"));
    assert!(!calls.contains("--remote\n"));
}

#[test]
#[cfg(unix)]
fn missing_nvim_is_actionable() {
    let mut fixture = Fixture::new(false);
    fixture.settings.nvim = fixture.root.join("absent").to_str().unwrap().into();
    assert!(
        prepare(&fixture.settings, &fixture.root, &fixture.note(), None)
            .unwrap_err()
            .contains("native Aether editing")
    );
}

#[test]
#[cfg(unix)]
fn absent_tmux_reuses_live_direct_neovim() {
    let mut fixture = Fixture::new(true);
    fixture.settings.tmux = fixture.root.join("absent").to_str().unwrap().into();
    let path = fixture.note();
    let command = prepare(&fixture.settings, &fixture.root, &path, Some(27)).unwrap();
    assert!(command.contains("'--remote-ui'"), "attach to the existing server UI");
    assert!(!command.contains("'--listen'"));
    let calls = std::fs::read_to_string(&fixture.log).unwrap();
    assert!(calls.contains("--remote\n+27\n"));
    assert!(calls.contains(path.to_str().unwrap()));
    assert!(!calls.contains("--listen\n"));
    assert!(!calls.contains("TMUX"));
}

#[test]
#[cfg(unix)]
fn live_server_without_matching_pane_does_not_create_empty_session() {
    let fixture = Fixture::new(true);
    std::fs::write(&fixture.settings.tmux, "#!/bin/sh\nexit 0\n").unwrap();
    let command = prepare(&fixture.settings, &fixture.root, &fixture.note(), None).unwrap();
    assert!(command.contains("'--remote-ui'"));
    let calls = std::fs::read_to_string(&fixture.log).unwrap();
    assert!(calls.contains("--remote\n"));
}

#[test]
#[cfg(unix)]
fn stale_socket_is_removed_before_starting_neovim() {
    use std::os::unix::net::UnixListener;
    let fixture = Fixture::new(false);
    let socket = Path::new(&fixture.settings.socket);
    drop(UnixListener::bind(socket).unwrap());
    assert!(socket.exists());
    prepare(&fixture.settings, &fixture.root, &fixture.note(), None).unwrap();
    assert!(!socket.exists());
}

#[test]
#[cfg(unix)]
fn socket_cleanup_preserves_live_sockets_files_and_symlinks() {
    use std::os::unix::{fs::symlink, net::UnixListener};
    let fixture = Fixture::new(false);
    let socket = fixture.root.join("live.sock");
    let _listener = UnixListener::bind(&socket).unwrap();
    remove_stale_socket(&socket).unwrap();
    assert!(socket.exists());
    let file = fixture.root.join("regular");
    std::fs::write(&file, "preserve").unwrap();
    remove_stale_socket(&file).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "preserve");
    let stale = fixture.root.join("stale.sock");
    drop(UnixListener::bind(&stale).unwrap());
    let link = fixture.root.join("link.sock");
    symlink(&stale, &link).unwrap();
    remove_stale_socket(&link).unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert!(stale.exists());
}

#[test]
#[cfg(unix)]
fn process_timeout_terminates_unresponsive_probe() {
    let started = Instant::now();
    let error =
        run_with_timeout(Path::new("/bin/sleep"), &["10".into()], Duration::from_millis(50))
            .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
#[cfg(unix)]
fn process_output_exceeding_pipe_capacity_does_not_deadlock() {
    let output = run_with_timeout(
        Path::new("/bin/sh"),
        &["-c".into(), "head -c 131072 /dev/zero; head -c 131072 /dev/zero >&2".into()],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 131072);
    assert_eq!(output.stderr.len(), 131072);
}
