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
fn resume_command_opens_nvim_then_the_cli_in_a_per_chat_tmux_session() {
    let s = aether_core::Settings::default();
    let folder = std::path::Path::new("/tmp/it's here");
    let cmd = resume_command(&s, aether_core::Provider::Claude, "abc", 7, folder).unwrap();
    assert!(cmd.contains("tmux has-session -t '=aether-7' "));
    assert!(cmd.contains(r"new-session -d -s aether-7 -c '/tmp/it'\''s here' nvim"));
    assert!(cmd.contains(r"split-window -h -t '=aether-7:' -c '/tmp/it'\''s here' ''\''claude'\'' --resume '\''abc'\'''"));
    assert!(cmd.contains("env -u TMUX -u TMUX_PANE tmux attach-session -t '=aether-7'"));
    assert!(cmd.ends_with("else 'claude' --resume 'abc'; fi"));

    let codex = resume_command(&s, aether_core::Provider::Codex, "t1", 7, folder).unwrap();
    assert!(codex.ends_with("else 'codex' resume 't1'; fi"));
    assert_eq!(resume_command(&s, aether_core::Provider::Claude, "", 7, folder), None);
    assert_eq!(resume_command(&s, aether_core::Provider::OpenRouter, "x", 7, folder), None);
}
