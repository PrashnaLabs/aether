use super::*;

#[test]
fn compaction_env_is_set_on_fresh_and_resumed_claude_commands_only() {
    let mut opts = DriverStartOptions::for_provider(
        &aether_core::Settings::default(),
        Provider::Claude,
        "/tmp",
    );
    opts.binary = std::env::current_exe().unwrap().to_string_lossy().into_owned();
    opts.compaction_window = Some(435_150);
    for args in [vec!["--session-id".into(), "new".into()], vec!["--resume".into(), "old".into()]] {
        let cmd = launch_command(&opts, &args).unwrap();
        let compact: Vec<_> =
            cmd.get_envs().filter(|(key, _)| *key == "CLAUDE_CODE_AUTO_COMPACT_WINDOW").collect();
        assert_eq!(
            compact,
            [(
                std::ffi::OsStr::new("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
                Some(std::ffi::OsStr::new("435150"))
            )]
        );
        assert!(!cmd.get_envs().any(|(key, _)| key == "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"));
    }
    opts.compaction_window = None;
    let cmd = launch_command(&opts, &[]).unwrap();
    assert!(!cmd.get_envs().any(|(key, _)| key == "CLAUDE_CODE_AUTO_COMPACT_WINDOW"));
}
