use super::*;

#[test]
fn terminal_page_enum_and_toggle_state() {
    let mut page = Page::Chat;
    // Toggling terminal page from Chat moves to Terminal
    if page == Page::Terminal {
        page = Page::Chat;
    } else {
        page = Page::Terminal;
    }
    assert_eq!(page, Page::Terminal);

    // Toggling terminal page from Terminal moves back to Chat
    if page == Page::Terminal {
        page = Page::Chat;
    } else {
        page = Page::Terminal;
    }
    assert_eq!(page, Page::Chat);
}

#[test]
fn resume_command_runs_cli_directly_in_the_project_folder() {
    let s = aether_core::Settings::default();
    let folder = std::path::Path::new("/tmp/it's here");
    for (provider, cli) in [
        (Provider::Claude, "'claude' --resume"),
        (Provider::Codex, "'codex' resume"),
        (Provider::Pi, "'pi' --session"),
        (Provider::OpenCode, "'opencode' --session"),
        (Provider::Antigravity, "'agy' --conversation"),
    ] {
        let cmd = resume_command(&s, provider, "session'1", folder).unwrap();
        assert_eq!(cmd, format!(r"cd '/tmp/it'\''s here' && {cli} 'session'\''1'"));
    }
    assert_eq!(resume_command(&s, Provider::Claude, "", folder), None);
    assert_eq!(resume_command(&s, Provider::OpenRouter, "x", folder), None);
}

#[test]
fn resume_command_quotes_custom_binary_paths() {
    let s = aether_core::Settings {
        codex_binary: "/tmp/agent's bin/codex".into(),
        ..Default::default()
    };
    assert_eq!(
        resume_command(&s, Provider::Codex, "abc", std::path::Path::new("/tmp/project")),
        Some(r"cd '/tmp/project' && '/tmp/agent'\''s bin/codex' resume 'abc'".into())
    );
}
