use super::*;
use aether_core::{ToolDetail, ToolStatus};

#[test]
fn scrub_replaces_home_cwd_and_emails() {
    let raw = r#"{"cwd":"/private/tmp/aether-fx/src","file":"/Users/iris/.claude/x.json","who":"iris.h+dev@gmail.com","at":"a @ b","pkg":"@scope/x"}"#;
    assert_eq!(
        scrub(raw, "/Users/iris", &["/tmp/aether-fx", "/private/tmp/aether-fx"]),
        r#"{"cwd":"/work/src","file":"~/.claude/x.json","who":"user@example.com","at":"a @ b","pkg":"@scope/x"}"#
    );
}

#[test]
fn scrub_keeps_json_escapes_before_an_email() {
    assert_eq!(scrub(r"a\nme@x.io.", "/h", &[]), r"a\nuser@example.com.");
}

#[test]
fn scrub_skips_relative_and_root_cwds() {
    let raw = r#"{"f":"a.b/c","cwd":"/work"}"#;
    assert_eq!(scrub(raw, "/h", &[".", "/", "", "rel/dir"]), raw);
}

#[cfg(unix)]
#[test]
fn recorder_scrubs_dot_cwd_and_missing_absolute_cwd() {
    let dir = std::env::temp_dir().join(format!("aether-rec-cwd-{}", std::process::id()));
    let text = |cwd: &str| {
        let rec = Recorder::create(&dir, "pi", Path::new(cwd)).unwrap();
        rec.line(r#"{"a":"x.y/z","p":"/nonexistent/proj/f"}"#);
        let mut files: Vec<_> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        let out = std::fs::read_to_string(files.last().unwrap()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        out
    };
    assert_eq!(text("."), "{\"a\":\"x.y/z\",\"p\":\"/nonexistent/proj/f\"}\n");
    assert_eq!(text("/nonexistent/proj"), "{\"a\":\"x.y/z\",\"p\":\"/work/f\"}\n");
}

#[cfg(unix)]
#[test]
fn recorder_appends_scrubbed_lines_and_marks() {
    let dir = std::env::temp_dir().join(format!("aether-rec-test-{}", std::process::id()));
    let rec = Recorder::create(&dir, "pi", Path::new("/tmp/fx")).unwrap();
    rec.line(r#"{"type":"agent_start","cwd":"/tmp/fx"}"#);
    rec.line(&serde_json::json!({"aether": "steer", "text": "also"}).to_string());
    let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("pi-") && name.ends_with(".jsonl"), "{name}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "{\"type\":\"agent_start\",\"cwd\":\"/work\"}\n{\"aether\":\"steer\",\"text\":\"also\"}\n"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn last_calls_keeps_the_latest_snapshot_per_id() {
    let call = |id: &str, status| {
        Update::Tool(ToolCall { status, ..ToolCall::new(id, "Run", ToolDetail::default()) })
    };
    let out = [
        call("a", ToolStatus::Running),
        call("b", ToolStatus::Running),
        call("a", ToolStatus::Done),
    ];
    let last: Vec<(String, ToolStatus)> =
        last_calls(&out).into_iter().map(|c| (c.id, c.status)).collect();
    assert_eq!(last, [("a".into(), ToolStatus::Done), ("b".into(), ToolStatus::Running)]);
}

#[cfg(unix)]
#[test]
fn recorder_creates_private_files_and_directories() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("aether-rec-mode-{}", std::process::id()));
    let rec = Recorder::create(&dir, "pi", Path::new("/tmp")).unwrap();
    let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(file).unwrap().permissions().mode() & 0o777, 0o600);
    drop(rec);
    std::fs::remove_dir_all(dir).unwrap();
}
