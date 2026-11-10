use super::*;
use serde_json::json;

fn edit(detail: &ToolDetail) -> (&str, &str, u32, u32) {
    let ToolDetail::Edit { path, diff, added, removed } = detail else { panic!("{detail:?}") };
    (path, diff, *added, *removed)
}

#[test]
fn names_classify_case_insensitively_and_mcp_tools_stay_other() {
    let kind = |name: &str, input: Value| {
        let (title, detail) = classify(name, &input);
        let kind = format!("{detail:?}");
        (title, kind.split([' ', '(']).next().unwrap().to_string())
    };
    let k = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(
        kind("Bash", json!({"command": "ls", "description": "List files"})),
        k("List files", "Command")
    );
    assert_eq!(kind("shell", json!({"command": "ls"})), k("Run command", "Command"));
    for name in ["Edit", "multiedit", "str_replace"] {
        assert_eq!(kind(name, json!({})), k("Edit file", "Edit"), "{name}");
    }
    assert_eq!(kind("write", json!({})), k("Write file", "Edit"));
    assert_eq!(kind("apply_patch", json!({})), k("Apply patch", "Edit"));
    assert_eq!(kind("Read", json!({})), k("Read file", "Read"));
    assert_eq!(kind("view", json!({})), k("Read file", "Read"));
    assert_eq!(kind("Grep", json!({})), k("Search files", "Search"));
    assert_eq!(kind("Glob", json!({})), k("Find files", "Search"));
    assert_eq!(kind("ls", json!({})), k("List files", "Search"));
    assert_eq!(kind("WebFetch", json!({})), k("Fetch", "Web"));
    assert_eq!(kind("WebSearch", json!({})), k("Web search", "Web"));
    assert_eq!(kind("Task", json!({"description": "Find callers"})), k("Find callers", "Task"));
    assert_eq!(kind("agent", json!({})), k("Run agent", "Task"));
    assert_eq!(kind("TodoWrite", json!({})), k("Update to-dos", "Plan"));
    assert_eq!(kind("mcp__github__create_thread", json!({})), k("Create thread", "Other"));
    assert_eq!(kind("mcp__fs__read", json!({})), k("Read", "Other"));
    assert_eq!(kind("ToolSearch", json!({})), k("Tool search", "Other"));
}

#[test]
fn inputs_fill_typed_fields_across_cli_spellings() {
    let (_, d) = classify("bash", &json!({"command": "cargo test", "workdir": "/p"}));
    assert_eq!(
        d,
        ToolDetail::Command {
            command: "cargo test".into(),
            cwd: Some("/p".into()),
            output: String::new(),
            exit_code: None
        }
    );
    let (_, d) = classify("read", &json!({"filePath": "src/a.rs", "offset": 10, "limit": 40}));
    assert_eq!(d, ToolDetail::Read { path: "src/a.rs".into(), range: Some("lines 10–49".into()) });
    let (_, d) = classify("Read", &json!({"file_path": "a.rs"}));
    assert_eq!(d, ToolDetail::Read { path: "a.rs".into(), range: None });
    let (_, d) = classify("Grep", &json!({"pattern": "fn main", "path": "src"}));
    assert_eq!(
        d,
        ToolDetail::Search {
            pattern: "fn main".into(),
            path: Some("src".into()),
            matches: None,
            output: String::new()
        }
    );
    let (_, d) = classify("WebFetch", &json!({"url": "https://a.dev", "prompt": "x"}));
    assert_eq!(d, ToolDetail::Web { target: "https://a.dev".into(), output: String::new() });
    let (_, d) = classify(
        "TodoWrite",
        &json!({"todos": [
            {"content": "Read", "status": "completed"},
            {"content": "Write", "status": "in_progress"},
            {"content": "Ship", "status": "pending"},
            {"content": "Drop", "status": "cancelled"},
        ]}),
    );
    let item = |text: &str, status| PlanItem { text: text.into(), status };
    assert_eq!(
        d,
        ToolDetail::Plan {
            items: vec![
                item("Read", PlanStatus::Done),
                item("Write", PlanStatus::Active),
                item("Ship", PlanStatus::Pending),
                item("Drop", PlanStatus::Cancelled),
            ]
        }
    );
    let (_, d) = classify("mcp__x__go", &json!({"q": "a".repeat(5000)}));
    let ToolDetail::Other { name, input, .. } = d else { panic!() };
    assert_eq!(name, "mcp__x__go");
    assert!(
        input.starts_with("{\n  \"q\"") && input.chars().count() <= 4_001 && input.ends_with('…'),
        "{}",
        input.len()
    );
    let (_, d) = classify("mystery", &Value::Null);
    assert_eq!(
        d,
        ToolDetail::Other { name: "mystery".into(), input: String::new(), output: String::new() }
    );
}

#[test]
fn edits_synthesize_unpositioned_diffs_and_writes_positioned_ones() {
    let (_, d) = classify(
        "Edit",
        &json!({"file_path": "a.rs", "old_string": "fn a() {\n    1\n}", "new_string": "fn a() {\n    2\n    3\n}"}),
    );
    assert_eq!(edit(&d), ("a.rs", " fn a() {\n-    1\n+    2\n+    3\n }\n", 2, 1));
    let (_, d) = classify(
        "MultiEdit",
        &json!({"file_path": "a.rs", "edits": [
            {"old_string": "a", "new_string": "b"},
            {"old_string": "c", "new_string": ""},
        ]}),
    );
    assert_eq!(edit(&d), ("a.rs", "-a\n+b\n@@\n-c\n", 1, 2));
    let (_, d) = classify("edit", &json!({"path": "p.rs", "oldText": "x", "newText": "y"}));
    assert_eq!(edit(&d), ("p.rs", "-x\n+y\n", 1, 1));
    let (_, d) = classify("Write", &json!({"file_path": "n.txt", "content": "a\nb\n"}));
    assert_eq!(edit(&d), ("n.txt", "@@ -0,0 +1,2 @@\n+a\n+b\n", 2, 0));
}

#[test]
fn big_diffs_keep_true_counts_but_cap_lines() {
    let content: String = (0..1000).map(|i| format!("line {i}\n")).collect();
    let (_, d) = classify("Write", &json!({"file_path": "big", "content": content}));
    let (_, diff, added, removed) = edit(&d);
    assert_eq!((added, removed), (1000, 0));
    assert_eq!(diff.lines().count(), 401);
    assert_eq!(diff.lines().last(), Some("… 601 more lines not shown"));
    let old: String = (0..3000).map(|i| format!("{i}\n")).collect();
    let new: String = (0..3000).map(|i| format!("{}\n", i * 7)).collect();
    let (_, d) = classify("Edit", &json!({"file_path": "f", "old_string": old, "new_string": new}));
    let (_, diff, added, removed) = edit(&d);
    assert!(added >= 2000 && removed >= 2000 && diff.lines().count() == 401, "{added} {removed}");
}

#[test]
fn patches_replace_the_synthesized_diff() {
    let (_, mut d) =
        classify("Edit", &json!({"file_path": "n.txt", "old_string": "a", "new_string": "b"}));
    apply_hunks(
        &mut d,
        &json!([{"oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1, "lines": ["-a", "+b"]}]),
    );
    assert_eq!(edit(&d), ("n.txt", "@@ -1,1 +1,1 @@\n-a\n+b\n", 1, 1));
    apply_hunks(&mut d, &json!([]));
    assert_eq!(edit(&d).1, "@@ -1,1 +1,1 @@\n-a\n+b\n");
    let patch = "Index: n.txt\n===\n--- n.txt\n+++ n.txt\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n";
    apply_patch_text(&mut d, patch);
    assert_eq!(edit(&d), ("n.txt", patch, 1, 1));
    let (_, d) = classify(
        "apply_patch",
        &json!({"patchText": "*** Begin Patch\n*** Update File: src/x.rs\n@@\n-a\n+b\n*** End Patch"}),
    );
    assert_eq!(edit(&d).0, "src/x.rs");
    assert_eq!((edit(&d).2, edit(&d).3), (1, 1));
}

#[test]
fn long_output_keeps_head_and_tail_on_line_boundaries() {
    assert_eq!(head_tail("short\n"), "short\n");
    let text: String = (0..5000).map(|i| format!("row {i:04}\n")).collect();
    let cut = head_tail(&text);
    assert!(cut.chars().count() <= 16_100, "{}", cut.len());
    assert!(cut.starts_with("row 0000\n") && cut.ends_with("row 4999\n"));
    let marker = cut.lines().find(|l| l.starts_with('…')).unwrap();
    let kept = cut.lines().filter(|l| l.starts_with("row")).count();
    assert_eq!(marker, format!("… {} lines omitted …", 5000 - kept));
    let wide = "x".repeat(40_000);
    let cut = head_tail(&wide);
    assert!(
        cut.starts_with("xxx")
            && cut.contains("\n… 1 line omitted …\n")
            && cut.chars().count() <= 16_100
    );
    let unicode = "é".repeat(20_000);
    assert!(head_tail(&unicode).chars().count() <= 16_100);
}

#[test]
fn results_fill_output_exit_code_and_error_line() {
    let mut call = start("t", "Bash", &json!({"command": "ls missing-dir"}));
    finish(&mut call, "Exit code 2\n\"missing-dir\": No such file or directory (os error 2)", true);
    assert_eq!(call.status, ToolStatus::Failed);
    assert_eq!(
        call.error.as_deref(),
        Some("\"missing-dir\": No such file or directory (os error 2)")
    );
    let ToolDetail::Command { output, exit_code, .. } = &call.detail else { panic!() };
    assert_eq!(
        (output.as_str(), *exit_code),
        ("\"missing-dir\": No such file or directory (os error 2)", Some(2))
    );
    let mut call = start("t", "bash", &json!({"command": "ls"}));
    finish(&mut call, "(Bash completed with no output)", false);
    assert_eq!((call.status, call.error.clone()), (ToolStatus::Done, None));
    assert_eq!(
        call.detail,
        ToolDetail::Command {
            command: "ls".into(),
            cwd: None,
            output: String::new(),
            exit_code: None
        }
    );
    let mut call = start("t", "bash", &json!({"command": "false"}));
    finish(&mut call, "boom\n\nCommand exited with code 1", true);
    let ToolDetail::Command { exit_code, .. } = &call.detail else { panic!() };
    assert_eq!((*exit_code, call.error.as_deref()), (Some(1), Some("boom")));
    let mut call =
        start("t", "Edit", &json!({"file_path": "a", "old_string": "x", "new_string": "y"}));
    finish(
        &mut call,
        "<tool_use_error>String to replace not found in file.\nString: x</tool_use_error>",
        true,
    );
    assert_eq!(call.error.as_deref(), Some("String to replace not found in file."));
    assert_eq!(edit(&call.detail).1, "-x\n+y\n");
}

#[test]
fn output_drops_ansi_codes_and_overwritten_progress() {
    let mut call = start("t", "bash", &json!({"command": "npm i"}));
    finish(&mut call, "\x1b[32mok\x1b[0m\n10%\r50%\r100%\nend\r\n", false);
    let ToolDetail::Command { output, .. } = &call.detail else { panic!() };
    assert_eq!(output, "ok\n100%\nend\n");
    let mut call = start("t", "bash", &json!({"command": "x"}));
    finish(&mut call, "\x1b[31merror: bad\x1b[0m", true);
    assert_eq!(call.error.as_deref(), Some("error: bad"));
}

#[test]
fn finals_keep_streamed_output_and_read_exit_codes_only_on_failure() {
    let mut call = start("t", "bash", &json!({"command": "x"}));
    set_output(&mut call.detail, "partial");
    finish(&mut call, "", false);
    assert!(matches!(&call.detail, ToolDetail::Command { output, .. } if output == "partial"));
    let mut call = start("t", "bash", &json!({"command": "x"}));
    finish(&mut call, "Exit code 3\nfine\nexited with code 4", false);
    let ToolDetail::Command { output, exit_code, .. } = &call.detail else { panic!() };
    assert_eq!((output.as_str(), *exit_code), ("Exit code 3\nfine\nexited with code 4", None));
}

#[test]
fn huge_inputs_are_clipped_and_skip_synthesis() {
    let big = "line\n".repeat(50_000);
    let (_, d) = classify("Write", &json!({"file_path": "big", "content": big}));
    assert_eq!(edit(&d), ("big", "… Diff too large to show\n", 50_000, 0));
    let (_, d) = classify("Edit", &json!({"file_path": "f", "old_string": big, "new_string": "x"}));
    assert_eq!(edit(&d), ("f", "… Diff too large to show\n", 1, 50_000));
    let (_, d) = classify(
        "Edit",
        &json!({"file_path": "f", "old_string": "a", "new_string": "b".repeat(5000)}),
    );
    let added = edit(&d).1.lines().nth(1).unwrap();
    assert_eq!((added.chars().count(), added.ends_with('…')), (1_502, true));
    let (_, d) = classify("Write", &json!({"file_path": "e", "content": ""}));
    assert_eq!(edit(&d), ("e", "… Empty file\n", 0, 0));
    let (_, d) = classify("bash", &json!({"command": "y".repeat(20_000)}));
    assert!(matches!(&d, ToolDetail::Command { command, .. } if command.chars().count() == 8_001));
    let (title, d) = classify("Task", &json!({"description": "z".repeat(5000)}));
    assert!(
        matches!(&d, ToolDetail::Task { description, .. } if description.chars().count() == 2_001)
    );
    assert!(title.chars().count() <= 201);
}

#[test]
fn hunk_lines_that_look_like_headers_still_count() {
    let (_, mut d) =
        classify("Edit", &json!({"file_path": "a.sql", "old_string": "x", "new_string": "y"}));
    apply_hunks(
        &mut d,
        &json!([{"oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1, "lines": ["--- old comment", "+++ new comment"]}]),
    );
    assert_eq!((edit(&d).2, edit(&d).3), (1, 1));
    apply_patch_text(
        &mut d,
        "--- a/a.sql\n+++ b/a.sql\n@@ -1,2 +1,1 @@\n--- gone\n-- kept\n+-- kept\n",
    );
    assert_eq!((edit(&d).2, edit(&d).3), (1, 2));
}

#[test]
fn search_results_count_matches() {
    let count = |text: &str| {
        let mut call = start("t", "Grep", &json!({"pattern": "b"}));
        finish(&mut call, text, false);
        let ToolDetail::Search { matches, output, .. } = call.detail else { panic!() };
        assert_eq!(output, text);
        matches
    };
    assert_eq!(count("Found 3 files\na\nb\nc"), Some(3));
    assert_eq!(count("No files found"), Some(0));
    assert_eq!(count("a.rs:1:b\nb.rs:2:b\n\n"), Some(2));
    assert_eq!(count(""), Some(0));
}

#[test]
fn result_text_joins_blocks_and_skips_images() {
    assert_eq!(result_text(&json!("hi")), "hi");
    assert_eq!(
        result_text(
            &json!([{"type": "text", "text": "a"}, {"type": "image"}, {"type": "text", "text": "b"}])
        ),
        "a\nb"
    );
    assert_eq!(result_text(&Value::Null), "");
}

fn call(id: &str, out: &str, status: ToolStatus) -> Update {
    let mut call = start(id, "bash", &json!({"command": "x"}));
    set_output(&mut call.detail, out);
    call.status = status;
    Update::Tool(call)
}

#[test]
fn throttle_holds_rapid_snapshots_and_flushes_them_in_order() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let t = Throttle::new(tx);
    t.send(call("a", "1", ToolStatus::Running));
    t.send(call("a", "2", ToolStatus::Running));
    t.send(call("a", "3", ToolStatus::Running));
    t.send(Update::Delta("x".into()));
    t.send(call("b", "1", ToolStatus::Running));
    t.send(call("b", "2", ToolStatus::Running));
    t.send(call("b", "3", ToolStatus::Done));
    let got: Vec<Update> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(
        got,
        [
            call("a", "1", ToolStatus::Running),
            call("a", "3", ToolStatus::Running),
            Update::Delta("x".into()),
            call("b", "1", ToolStatus::Running),
            call("b", "3", ToolStatus::Done),
        ]
    );
    t.send(call("c", "1", ToolStatus::Running));
    t.send(call("c", "2", ToolStatus::Running));
    std::thread::sleep(Duration::from_millis(300));
    let got: Vec<Update> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(got, [call("c", "1", ToolStatus::Running), call("c", "2", ToolStatus::Running)]);
}

#[test]
fn only_the_clis_question_tools_read_as_questions() {
    let input = serde_json::json!({"question": "Which?"});
    assert_eq!(classify("AskUserQuestion", &input).0, "Ask question");
    assert_eq!(classify("question", &input).0, "Ask question");
    assert_eq!(classify("mcp__docs__ask_question", &input).0, "Ask question");
    assert_eq!(classify("search_questions", &input).0, "Search questions");
}

#[test]
fn subagent_tools_are_tasks_with_type_and_prompt() {
    for (name, input) in [
        (
            "Agent",
            json!({"description": "Find styles", "prompt": "Look in src", "subagent_type": "Explore"}),
        ),
        (
            "subagent",
            json!({"description": "Find styles", "prompt": "Look in src", "agent": "Explore"}),
        ),
    ] {
        let (_, detail) = classify(name, &input);
        assert!(
            matches!(detail, ToolDetail::Task { ref description, ref agent, ref prompt, .. }
            if description == "Find styles" && agent == "Explore" && prompt == "Look in src"),
            "{name}"
        );
    }
}

#[test]
fn tool_result_images_are_saved_as_attachments() {
    let dir = std::env::temp_dir().join(format!("aether-img-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = "iVBORw0KGgo="; // base64 of the PNG signature, enough for a file
    let content = json!([
        {"type": "text", "text": "shot"},
        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": png}},
        {"type": "image", "data": png, "mimeType": "image/png"},
    ]);
    let saved = save_images(&content, Some(&dir));
    assert_eq!(saved.len(), 2);
    assert!(
        saved.iter().all(|a| a.mime == "image/png" && a.path.exists() && a.path.starts_with(&dir))
    );
    assert!(save_images(&content, None).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn saved_images_never_share_a_path_and_keep_their_bytes() {
    let dir = std::env::temp_dir().join(format!("aether-img-seq-{}", std::process::id()));
    let content = json!([{"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png"}]);
    let saved: Vec<_> = (0..50).flat_map(|_| save_images(&content, Some(&dir))).collect();
    let paths: std::collections::HashSet<_> = saved.iter().map(|a| &a.path).collect();
    assert_eq!(paths.len(), 50, "calls in one millisecond overwrote each other");
    let a = &saved;
    let signature = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    assert_eq!(std::fs::read(&a[0].path).unwrap(), signature);
    assert_eq!(a[0].size, 8);
    let odd = json!([{"type": "image", "data": "!!", "mimeType": "image/png"},
        {"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/tiff"}]);
    assert!(save_images(&odd, Some(&dir)).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn child_activity_counts_new_children_only() {
    let mut task = start("t", "Agent", &json!({"description": "d"}));
    let child = start("c", "Read", &json!({"file_path": "src/a.css"}));
    child_activity(&mut task, &child, true);
    child_activity(&mut task, &child, false);
    assert!(
        matches!(task.detail, ToolDetail::Task { tools: 1, ref activity, .. } if activity == "Read file src/a.css")
    );
}

#[test]
fn notebook_edit_is_an_edit_of_the_notebook() {
    let (title, detail) =
        classify("NotebookEdit", &json!({"notebook_path": "/w/a.ipynb", "new_source": "x = 1"}));
    assert_eq!(title, "Edit notebook");
    let (path, _, added, _) = edit(&detail);
    assert_eq!((path, added), ("/w/a.ipynb", 1));
}

#[test]
fn a_cloned_throttle_flushes_the_held_snapshot_before_its_own_update() {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let t = Throttle::new(tx);
    let watchdog = t.clone();
    t.send(call("a", "1", ToolStatus::Running));
    t.send(call("a", "2", ToolStatus::Running));
    watchdog.send(Update::Final(String::new()));
    let got: Vec<Update> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(
        got,
        [
            call("a", "1", ToolStatus::Running),
            call("a", "2", ToolStatus::Running),
            Update::Final(String::new())
        ]
    );
}

fn marker_count(text: &str) -> usize {
    text.matches(" bytes earlier output hidden\n").count()
}

fn hidden(text: &str) -> usize {
    text.strip_prefix("… ").unwrap().split(' ').next().unwrap().parse().unwrap()
}

#[test]
fn output_under_the_cap_is_left_alone() {
    let mut text = "x".repeat(OUTPUT_CAP);
    cap_output(&mut text);
    assert_eq!(text.len(), OUTPUT_CAP);
    assert_eq!(marker_count(&text), 0);
}

#[test]
fn output_over_the_cap_keeps_the_tail_behind_one_marker() {
    let mut text: String = (0..20_000).map(|i| format!("line {i}\n")).collect();
    let total = text.len();
    let last = text.lines().last().unwrap().to_string();
    cap_output(&mut text);
    let (marker, body) = text.split_once('\n').unwrap();
    assert_eq!(marker_count(&text), 1);
    assert!(body.len() <= OUTPUT_CAP && body.len() > OUTPUT_CAP - 2048, "{}", body.len());
    assert!(body.starts_with("line "), "cut on a line boundary: {body:.20}");
    assert!(body.ends_with(&format!("{last}\n")));
    assert_eq!(hidden(marker), total - body.len());
}

#[test]
fn repeated_appends_keep_one_marker_and_an_exact_count() {
    let mut detail = ToolDetail::Command {
        command: "yes".into(),
        cwd: None,
        output: String::new(),
        exit_code: None,
    };
    let chunk = "0123456789abcdef\n".repeat(1000);
    for _ in 0..50 {
        append_output(&mut detail, &chunk);
    }
    let ToolDetail::Command { output, .. } = &detail else { panic!() };
    let (marker, body) = output.split_once('\n').unwrap();
    assert_eq!(marker_count(output), 1);
    assert!(body.len() <= OUTPUT_CAP);
    assert_eq!(hidden(marker) + body.len(), 50 * chunk.len());
}

#[test]
fn output_is_cut_on_a_char_boundary() {
    let mut text = "é".repeat(OUTPUT_CAP);
    cap_output(&mut text);
    let (marker, body) = text.split_once('\n').unwrap();
    assert!(body.chars().all(|c| c == 'é'));
    assert_eq!(hidden(marker) + body.len(), 2 * OUTPUT_CAP);
}

#[test]
fn append_output_ignores_details_without_output() {
    let mut detail = ToolDetail::Read { path: "a".into(), range: None };
    append_output(&mut detail, "more");
    assert_eq!(detail, ToolDetail::Read { path: "a".into(), range: None });
}
