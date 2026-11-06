use super::*;

fn tool(id: &str, detail: ToolDetail, status: ToolStatus) -> Step {
    Step::Tool(ToolCall { status, ..ToolCall::new(id, "Run command", detail) })
}

fn bash(command: &str) -> ToolDetail {
    ToolDetail::Command {
        command: command.into(),
        cwd: None,
        output: String::new(),
        exit_code: None,
    }
}

fn edit(path: &str) -> ToolDetail {
    ToolDetail::Edit { path: path.into(), diff: String::new(), added: 1, removed: 0 }
}

#[test]
fn live_label_names_the_latest_running_tool() {
    assert_eq!(live_label(&[]), None);
    let steps = [
        tool("1", bash("ls"), ToolStatus::Running),
        tool("2", edit("/p/src/main.rs"), ToolStatus::Running),
        tool("3", bash("pwd"), ToolStatus::Done),
    ];
    assert_eq!(live_label(&steps).as_deref(), Some("Run command · main.rs…"));
    let quiet = [tool("1", bash(""), ToolStatus::Running)];
    assert_eq!(live_label(&quiet).as_deref(), Some("Run command…"));
}

#[test]
fn preview_is_the_first_sentence_without_bold() {
    assert_eq!(
        first_sentence("**Scoping the redesign.** Looking at routes first."),
        "Scoping the redesign."
    );
    assert_eq!(
        first_sentence("No git repo, so I cannot lean on history. Mapping `src` directly."),
        "No git repo, so I cannot lean on history."
    );
    assert_eq!(first_sentence("one line no period\nsecond"), "one line no period");
    assert_eq!(first_sentence("  "), "");
}

#[test]
fn each_thought_has_its_own_duration() {
    assert_eq!(thought_label(Some(3_000)), "Thought for 3s");
    assert_eq!(thought_label(Some(400)), "Thought");
    assert_eq!(thought_label(Some(72_000)), "Thought for 1m 12s");
    assert_eq!(thought_label(None), "Thought");
}

#[test]
fn preview_is_one_line() {
    assert_eq!(
        first_sentence("**Scoping the redesign**\n\nLooking at routes first."),
        "Scoping the redesign"
    );
    assert_eq!(first_sentence("\nFirst one.\nSecond."), "First one.");
    assert_eq!(first_sentence("Title\n\nSecond. Third"), "Title");
}

#[test]
fn live_thoughts_open_and_finished_ones_collapse() {
    assert!(thought_open(true, None, None, false));
    assert!(!thought_open(true, Some(false), None, false));
    assert!(!thought_open(false, None, None, false));
    assert!(!thought_open(false, Some(true), None, false));
    assert!(thought_open(false, Some(false), Some(true), false));
    assert!(thought_open(false, None, None, true));
    assert!(!thought_open(false, None, Some(false), true));
}

#[test]
fn thought_fades_only_mark_overflow_in_each_direction() {
    assert_eq!(thought_fades(px(0.), px(0.), px(20.)), (0., 0.));
    assert_eq!(thought_fades(px(0.), px(100.), px(20.)), (0., 1.));
    assert_eq!(thought_fades(px(-50.), px(100.), px(20.)), (1., 1.));
    assert_eq!(thought_fades(px(-100.), px(100.), px(20.)), (1., 0.));
    assert_eq!(thought_fades(px(-10.), px(100.), px(20.)), (0.5, 1.));
    assert_eq!(thought_fades(px(-90.), px(100.), px(20.)), (1., 0.5));
    assert_eq!(thought_fades(px(10.), px(100.), px(20.)), (0., 1.));
    assert_eq!(thought_fades(px(-110.), px(100.), px(20.)), (1., 0.));
}
