use super::*;
use alacritty_terminal::vte::ansi::Rgb;
use std::time::Instant;

#[test]
fn cursor_setting_picks_the_shape_and_changes_live() {
    use alacritty_terminal::event::VoidListener;
    let dims = TermSize::new(10, 10);
    let mut term = Term::new(term_config(TerminalCursor::Bar), &dims, VoidListener);
    assert_eq!(term.cursor_style().shape, CursorShape::Beam);
    term.set_options(term_config(TerminalCursor::Underline));
    assert_eq!(term.cursor_style().shape, CursorShape::Underline);
    term.set_options(term_config(TerminalCursor::Block));
    assert_eq!(term.cursor_style().shape, CursorShape::Block);
}

#[test]
fn shell_prefers_the_account_then_env_then_sh() {
    let yes = |_: &str| true;
    assert_eq!(shell_program(Some("/bin/zsh"), Some("/bin/bash"), yes), "/bin/zsh");
    assert_eq!(shell_program(None, Some("/bin/bash"), yes), "/bin/bash");
    assert_eq!(
        shell_program(Some("/gone/fish"), Some("/bin/bash"), |p| p != "/gone/fish"),
        "/bin/bash"
    );
    assert_eq!(shell_program(Some("zsh"), Some(""), yes), "/bin/sh");
    assert_eq!(shell_program(None, None, yes), "/bin/sh");
}

#[test]
fn account_shell_reads_an_absolute_path() {
    if let Some(shell) = account_shell() {
        assert!(shell.starts_with('/'), "{shell}");
    }
}

#[test]
fn env_sets_terminal_type_path_and_a_missing_locale() {
    let env = shell_env("/usr/bin:/bin".into(), |_| false);
    assert_eq!(env["TERM"], "xterm-256color");
    assert_eq!(env["COLORTERM"], "truecolor");
    assert_eq!(env["TERM_PROGRAM"], "aether");
    assert_eq!(env["PATH"], "/usr/bin:/bin");
    assert_eq!(env["LANG"], "en_US.UTF-8");
    let env = shell_env(String::new(), |k| k == "LC_CTYPE");
    assert!(!env.contains_key("LANG"));
}

#[cfg(unix)]
#[test]
fn other_terminals_variables_are_dropped() {
    let names = [
        "ITERM_SESSION_ID",
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
        "HOME",
        "LC_TERMINAL",
        "VSCODE_PID",
        "KITTY_WINDOW_ID",
    ];
    let found = foreign_terminal_vars(names.iter().map(|s| s.to_string()));
    assert_eq!(
        found,
        [
            "ITERM_SESSION_ID",
            "KITTY_WINDOW_ID",
            "LC_TERMINAL",
            "TERM_PROGRAM_VERSION",
            "VSCODE_PID"
        ]
    );
    let (program, args) = launch("/bin/zsh", vec!["-l".into()], vec!["A".into(), "B".into()]);
    assert_eq!(program, "/usr/bin/env");
    assert_eq!(args, ["-u", "A", "-u", "B", "/bin/zsh", "-l"]);
    assert_eq!(
        launch("/bin/zsh", vec!["-l".into()], vec![]),
        ("/bin/zsh".to_string(), vec!["-l".to_string()])
    );
}

#[test]
#[ignore = "spawns a real shell"]
fn smoke_shell_output_reaches_the_grid() {
    let size = WindowSize { num_lines: 8, num_cols: 40, cell_width: 8, cell_height: 18 };
    let palette = Palette::new(false, Rgb { r: 0, g: 0, b: 0 }, Rgb { r: 255, g: 255, b: 255 });
    let args = vec!["-c".into(), "echo hi".into()];
    let session = Session::start(Path::new("/tmp"), "/bin/sh".into(), args, size, palette).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !session.notices().any(|n| matches!(n, Notice::Exited(_))) {
        assert!(Instant::now() < deadline, "shell never exited");
        std::thread::sleep(Duration::from_millis(20));
    }
    let frame = super::super::grid::snapshot(&session.term.lock(), &palette, false);
    assert_eq!(frame.rows[0].text, "hi");
}

#[test]
fn color_request_returns_active_theme_fg_and_bg() {
    use super::super::palette::rgb;
    let palette = Palette::new(true, rgb(0xcdd6f4), rgb(0x1e1e2e));
    assert_eq!(palette.index(256), rgb(0xcdd6f4)); // OSC 10 foreground
    assert_eq!(palette.index(257), rgb(0x1e1e2e)); // OSC 11 background
}

#[test]
fn session_update_theme_updates_palette_and_flags_dirty() {
    use super::super::palette::rgb;
    let initial_palette = Palette::new(true, rgb(0x111111), rgb(0x222222));
    let new_palette = Palette::new(false, rgb(0x333333), rgb(0x444444));
    let (tx, rx) = mpsc::channel();
    let shared = Arc::new(Shared {
        dirty: AtomicBool::new(false),
        exited: AtomicBool::new(false),
        notices: tx,
        pty: OnceLock::new(),
        size: Mutex::new(WindowSize {
            num_lines: 10,
            num_cols: 10,
            cell_width: 10,
            cell_height: 10,
        }),
        palette: Mutex::new(initial_palette),
    });
    let config = Config { scrolling_history: 100, ..Config::default() };
    let dims = TermSize::new(10, 10);
    let term = Arc::new(FairMutex::new(Term::new(config, &dims, Listener(shared.clone()))));
    let session = Session {
        term,
        shell: "test".into(),
        shared: shared.clone(),
        notices: rx,
        pid: 0,
        master: -1,
        io: None,
    };
    assert!(!session.take_dirty());
    session.update_theme(new_palette);
    assert!(session.take_dirty());
    assert_eq!(shared.palette.lock().unwrap().index(256), rgb(0x333333));
    assert_eq!(shared.palette.lock().unwrap().index(257), rgb(0x444444));
    // no PTY thread: never killpg(0), our own group
    assert!(!session.terminate());
}

#[cfg(unix)]
#[test]
fn command_args_pass_command_verbatim() {
    let cmd = r#"echo 'a b' && FOO="$HOME" ls | wc -l"#;
    assert_eq!(command_args("/bin/zsh", cmd), vec!["-lc".to_string(), cmd.to_string()]);
}

#[test]
fn powershell_is_told_from_cmd_by_program_name() {
    assert!(is_powershell(r"C:\Program Files\PowerShell\7\pwsh.exe"));
    assert!(is_powershell("powershell"));
    assert!(is_powershell("/usr/bin/pwsh"));
    assert!(!is_powershell(r"C:\Windows\System32\cmd.exe"));
}

#[cfg(windows)]
#[test]
fn windows_shells_run_a_command_and_quote_the_program() {
    assert_eq!(
        command_args(r"C:\x\pwsh.exe", "dir"),
        ["-NoLogo".to_string(), "-Command".into(), "dir".into()]
    );
    assert_eq!(command_args("cmd.exe", "dir /b"), ["/C".to_string(), "dir /b".into()]);
    assert_eq!(login_args("cmd.exe"), Vec::<String>::new());
    let (program, args) = launch(r"C:\Program Files\x\pwsh.exe", vec![], vec!["A".into()]);
    assert_eq!(program, "\"C:\\Program Files\\x\\pwsh.exe\"");
    assert!(args.is_empty());
}

#[cfg(windows)]
#[test]
fn windows_shell_output_reaches_the_grid() {
    let size = WindowSize { num_lines: 8, num_cols: 60, cell_width: 8, cell_height: 18 };
    let palette = Palette::new(false, Rgb { r: 0, g: 0, b: 0 }, Rgb { r: 255, g: 255, b: 255 });
    let session =
        Session::spawn_command(&std::env::temp_dir(), "echo aether-ok", size, palette).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !session.notices().any(|n| matches!(n, Notice::Exited(_))) {
        assert!(Instant::now() < deadline, "shell never exited");
        std::thread::sleep(Duration::from_millis(50));
    }
    let frame = super::super::grid::snapshot(&session.term.lock(), &palette, false);
    let rows: Vec<_> = frame.rows.iter().map(|r| r.text.as_str()).collect();
    assert!(rows.iter().any(|r| r.contains("aether-ok")), "{rows:?}");
}
