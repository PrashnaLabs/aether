use super::*;

#[test]
fn paths_show_relative_to_the_chat_folder() {
    let cwd = Some("/tmp/proj");
    assert_eq!(relative_path("/tmp/proj/src/a.rs", cwd), "src/a.rs");
    assert_eq!(relative_path("/private/tmp/proj/a.rs", cwd), "a.rs");
    assert_eq!(relative_path("/tmp/project/a.rs", cwd), "/tmp/project/a.rs");
    assert_eq!(relative_path("/tmp/proj", cwd), "/tmp/proj");
    assert_eq!(relative_path("src/a.rs", None), "src/a.rs");
    assert_eq!(relative_path("/x/a.rs", Some("/x/")), "a.rs");
}

#[test]
fn humanizes_tool_names() {
    assert_eq!(humanize_tool_name("run_command"), "Run command");
    assert_eq!(humanize_tool_name("execute_bash"), "Execute bash");
    assert_eq!(humanize_tool_name("custom_server__search_docs"), "Search docs");
}

#[test]
fn subjects_skip_repeats_and_raw_json() {
    let task = ToolDetail::Task {
        description: "Find callers".into(),
        output: String::new(),
        agent: String::new(),
        prompt: String::new(),
        tools: 0,
        tokens: 0,
        activity: String::new(),
    };
    assert_eq!(subject(&task, "Find callers", None).0, "");
    let other = ToolDetail::Other { name: "x".into(), input: "{}".into(), output: String::new() };
    assert_eq!(subject(&other, "X", None).0, "");
    let search = ToolDetail::Search {
        pattern: "fn main".into(),
        path: Some("/p/src".into()),
        matches: None,
        output: String::new(),
    };
    assert_eq!(subject(&search, "Search files", Some("/p")), ("fn main in src".into(), false));
    let read = ToolDetail::Read { path: "/p/a.rs".into(), range: None };
    assert_eq!(subject(&read, "Read file", Some("/p")), ("a.rs".into(), true));
}

#[test]
fn question_subjects_and_output_extraction() {
    let q = ToolDetail::Other {
        name: "question".into(),
        input: "What database to use?".into(),
        output: String::new(),
    };
    assert_eq!(subject(&q, "Ask question", None).0, "What database to use?");

    let historical_q = ToolDetail::Other {
        name: String::new(),
        input: String::new(),
        output: "User has answered your questions: \"The /about page is built\": \"Bio\"".into(),
    };
    assert_eq!(subject(&historical_q, "Ask question", None).0, "The /about page is built");
    assert_eq!(
        extract_question_from_output(
            "User has answered your questions: \"The /about page is built\": \"Bio\""
        )
        .unwrap(),
        "The /about page is built"
    );
}

#[test]
fn menu_copies_skip_empty_texts() {
    let command = ToolDetail::Command {
        command: "ls".into(),
        cwd: None,
        output: " \n".into(),
        exit_code: Some(0),
    };
    assert_eq!(copies(command), [("Copy command", "ls".to_string())]);
    let search = ToolDetail::Search {
        pattern: "todo".into(),
        path: Some("/repo".into()),
        matches: None,
        output: "a.rs:1".into(),
    };
    assert_eq!(
        copies(search),
        [("Copy path", "/repo".to_string()), ("Copy results", "a.rs:1".to_string())]
    );
    assert!(copies(ToolDetail::Plain("note".into())).is_empty());
}

#[test]
fn menu_files_open_at_the_first_line_shown() {
    let at = |line| Some(("/p/a.rs".to_string(), line));
    let read = |range: Option<&str>| ToolDetail::Read {
        path: "/p/a.rs".into(),
        range: range.map(Into::into),
    };
    assert_eq!(file_at(&read(Some("lines 10–49"))), at(Some(10)));
    assert_eq!(file_at(&read(Some("from line 7"))), at(Some(7)));
    assert_eq!(file_at(&read(Some("lines 0–39"))), at(None));
    assert_eq!(file_at(&read(Some("first 40 lines"))), at(None));
    assert_eq!(file_at(&read(None)), at(None));
    let edit = |diff: &str| ToolDetail::Edit {
        path: "/p/a.rs".into(),
        diff: diff.into(),
        added: 1,
        removed: 1,
    };
    assert_eq!(file_at(&edit("@@ -3,2 +5,2 @@\n-a\n+b")), at(Some(5)));
    assert_eq!(file_at(&edit("@@\n-a\n+b")), at(None));
    assert_eq!(file_at(&ToolDetail::Read { path: String::new(), range: None }), None);
    assert_eq!(file_at(&ToolDetail::Plain("x".into())), None);
}

#[test]
fn output_tail_keeps_the_last_twelve_lines() {
    let out = (1..=20).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
    assert_eq!(tail(&out, OUTPUT_LINES), ("9\n10\n11\n12\n13\n14\n15\n16\n17\n18\n19\n20", 8));
    assert_eq!(tail("a\nb\n", OUTPUT_LINES), ("a\nb\n", 0));
}

#[test]
fn status_words_show_exit_codes_only_when_nonzero() {
    let cmd = |code, status| ToolCall {
        status,
        ..ToolCall::new(
            "c",
            "Run command",
            ToolDetail::Command {
                command: "ls".into(),
                cwd: None,
                output: String::new(),
                exit_code: code,
            },
        )
    };
    assert_eq!(status_word(&cmd(Some(0), ToolStatus::Done)), None);
    assert_eq!(status_word(&cmd(Some(2), ToolStatus::Failed)).as_deref(), Some("Exit 2"));
    assert_eq!(status_word(&cmd(None, ToolStatus::Failed)).as_deref(), Some("Failed"));
    assert_eq!(status_word(&cmd(None, ToolStatus::Waiting)).as_deref(), Some("Needs approval"));
    assert_eq!(status_word(&cmd(None, ToolStatus::Background)).as_deref(), Some("Background"));
}

#[test]
fn only_failed_waiting_and_background_rows_take_colour() {
    use ToolStatus::*;
    assert_eq!(
        [Running, Done, Stopped, Denied, Failed, Waiting, Background].map(tone),
        [
            Tone::Plain,
            Tone::Plain,
            Tone::Plain,
            Tone::Plain,
            Tone::Danger,
            Tone::Warning,
            Tone::Info
        ]
    );
}

#[test]
fn timers_follow_status_and_skip_unknown_starts() {
    let mut c = ToolCall::new("c", "Run", ToolDetail::Plain(String::new()));
    c.started_ms = Some(1_000);
    assert_eq!(timer_text(&c, 13_000).as_deref(), Some("12s"));
    c.status = ToolStatus::Done;
    c.ms = Some(400);
    assert_eq!(timer_text(&c, 13_000), None, "sub-second finished rows hide their time");
    c.ms = Some(4_200);
    assert_eq!(timer_text(&c, 13_000).as_deref(), Some("4s"));
    c.status = ToolStatus::Failed;
    c.ms = Some(400);
    assert_eq!(timer_text(&c, 13_000).as_deref(), Some("400ms"));
    c.status = ToolStatus::Running;
    c.started_ms = None;
    assert_eq!(timer_text(&c, 13_000), None, "rows saved before timers show none");
}

#[test]
fn read_groups_name_their_files_and_new_files_are_detected() {
    let r = |p: &str| ToolCall::new(p, "Read", ToolDetail::Read { path: p.into(), range: None });
    assert_eq!(
        read_group_label(&[r("src/routes/index.tsx"), r("src/components/Header.tsx")]),
        ("Read 2 files".to_string(), "index.tsx, Header.tsx".to_string())
    );
    assert!(is_new_file("@@ -0,0 +1,14 @@\n+:root {"));
    assert!(!is_new_file("@@ -1,2 +1,3 @@\n a"));
}

#[test]
fn finished_rows_keep_their_time_and_skewed_clocks_read_zero() {
    let mut c = ToolCall::new("c", "Run", ToolDetail::Plain(String::new()));
    c.ms = Some(4_200);
    for status in [ToolStatus::Stopped, ToolStatus::Unknown, ToolStatus::Denied] {
        c.status = status;
        assert_eq!(timer_text(&c, 13_000).as_deref(), Some("4s"), "{status:?}");
    }
    c.status = ToolStatus::Running;
    c.started_ms = Some(20_000);
    assert_eq!(timer_text(&c, 13_000).as_deref(), Some("0s"), "a skewed clock never goes negative");
}

#[test]
fn running_rows_open_once_there_is_output_and_waiting_rows_stay_shut() {
    let cmd = |output: &str, status| ToolCall {
        status,
        ..ToolCall::new(
            "c",
            "Run",
            ToolDetail::Command {
                command: "ls".into(),
                cwd: None,
                output: output.into(),
                exit_code: None,
            },
        )
    };
    assert!(!auto_open(&cmd("", ToolStatus::Running)));
    assert!(auto_open(&cmd("a\n", ToolStatus::Running)));
    assert!(
        !auto_open(&cmd("a\n", ToolStatus::Waiting)),
        "the approval card below shows what waits"
    );
    assert!(!auto_open(&cmd("a\n", ToolStatus::Done)));
    assert!(!auto_open(&cmd("a\n", ToolStatus::Background)));
    let edit = |diff: &str| {
        ToolCall::new(
            "e",
            "Edit",
            ToolDetail::Edit { path: "a.rs".into(), diff: diff.into(), added: 1, removed: 0 },
        )
    };
    assert!(auto_open(&edit("@@ -1 +1 @@\n+a")), "a streaming edit shows its diff");
    assert!(!auto_open(&edit("")));
    let other = |input: &str, output: &str| {
        ToolCall::new(
            "o",
            "Fetch",
            ToolDetail::Other {
                name: "mcp__docs__fetch".into(),
                input: input.into(),
                output: output.into(),
            },
        )
    };
    assert!(!auto_open(&other("https://example.com", "")), "input alone is not output");
    assert!(auto_open(&other("https://example.com", "<html>")));
}

#[test]
fn status_pills_pair_their_word_with_an_icon() {
    assert_eq!(pill_icon(ToolStatus::Failed), Some(Lucide::CircleX));
    assert_eq!(pill_icon(ToolStatus::Waiting), Some(Lucide::Shield));
    assert_eq!(pill_icon(ToolStatus::Denied), Some(Lucide::ShieldX));
    assert_eq!(pill_icon(ToolStatus::Stopped), Some(Lucide::Square));
    assert_eq!(pill_icon(ToolStatus::Done), None);
}

#[test]
fn showing_all_output_does_not_count_as_opening_the_row() {
    let mut rows = ToolRows::default();
    rows.open.insert("s-t1".into(), true);
    let all = SharedString::from("s-t2-all");
    rows.toggle_all(all.clone());
    assert!(rows.show_all(&all));
    assert_eq!(rows.opened("s"), HashSet::from(["t1".to_string()]));
    rows.toggle_all(all.clone());
    assert!(!rows.show_all(&all));
}

#[test]
fn command_highlights_parse_once_per_command_and_theme() {
    let mut rows = ToolRows::default();
    let (key, theme) = (SharedString::from("s-t1"), HighlightTheme::default_light());
    let parsed = Cell::new(0);
    let parse = || {
        parsed.set(parsed.get() + 1);
        Vec::new()
    };
    rows.command_styles(&key, "ls", &theme, parse);
    rows.command_styles(&key, "ls", &theme, parse);
    assert_eq!(parsed.get(), 1, "a re-render reuses the parse");
    rows.command_styles(&key, "ls -la", &theme, parse);
    assert_eq!(parsed.get(), 2, "a changed command parses again");
    rows.command_styles(&key, "ls -la", &Arc::new((*theme).clone()), parse);
    assert_eq!(parsed.get(), 3, "a theme switch parses again");
}
