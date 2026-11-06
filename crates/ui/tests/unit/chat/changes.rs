use super::*;
use aether_core::{NOT_KEPT, ToolCall};

fn edit(id: &str, path: &str, diff: &str, added: u32, removed: u32, status: ToolStatus) -> Step {
    let detail = ToolDetail::Edit { path: path.into(), diff: diff.into(), added, removed };
    let mut call = ToolCall::new(id, "Edit file", detail);
    call.status = status;
    Step::Tool(call)
}

fn file(path: &str, added: u32, removed: u32) -> FileEdits {
    FileEdits { path: path.into(), added, removed, diffs: vec![String::new()] }
}

#[test]
fn edits_group_by_path_in_first_edit_order() {
    let steps = [
        edit("1", "/p/src/b.rs", "@@ -1 +1 @@\n-x\n+y", 1, 1, ToolStatus::Done),
        Step::Thought("hmm".into()),
        edit("2", "/p/a.rs", "+a", 3, 0, ToolStatus::Done),
        edit("3", "/p/src/b.rs", "+z", 2, 0, ToolStatus::Stopped),
        edit("4", "/p/c.rs", "+c", 9, 9, ToolStatus::Denied),
        edit("5", "/p/a.rs", "+d", 9, 9, ToolStatus::Failed),
        edit("6", "", "+e", 1, 0, ToolStatus::Done),
    ];
    let files = reply_edits(&steps, Some("/p"));
    assert_eq!(
        files,
        [
            FileEdits {
                path: "src/b.rs".into(),
                added: 1,
                removed: 1,
                diffs: vec!["@@ -1 +1 @@\n-x\n+y".into()],
            },
            FileEdits { path: "a.rs".into(), added: 3, removed: 0, diffs: vec!["+a".into()] },
        ]
    );
    assert_eq!(edit_totals(&steps), totals(&files));
    assert_eq!(edit_totals(&steps), (4, 1));
}

#[test]
fn dropped_diffs_keep_their_counts_and_show_the_note() {
    let steps = [edit("1", "/p/a.rs", NOT_KEPT, 41, 3, ToolStatus::Done)];
    let files = reply_edits(&steps, Some("/p"));
    assert_eq!((files[0].added, files[0].removed), (41, 3));
    let rows = file_rows(&files[0].diffs);
    assert!(matches!(&rows[..], [DiffRow::Note(note)] if note.starts_with("Not kept")));
}

#[test]
fn each_edit_of_a_file_keeps_its_rows_with_a_break_between() {
    let rows = file_rows(&["-a\n+b".into(), "@@ -9 +9 @@\n-c\n+d".into(), "+e".into()]);
    let hunks: Vec<_> = rows
        .iter()
        .enumerate()
        .filter_map(|(ix, row)| match row {
            DiffRow::Hunk(header) => Some((ix, header.to_string())),
            _ => None,
        })
        .collect();
    assert_eq!(hunks, [(2, "@@ -9 +9 @@".to_string()), (5, String::new())]);
    assert_eq!(rows.len(), 7);
}

#[test]
fn tree_lists_folders_first_and_joins_lone_subfolders() {
    let files = [
        file("README.md", 1, 0),
        file("crates/ui/src/chat/message.rs", 10, 2),
        file("crates/ui/src/chat/changes.rs", 30, 0),
        file("crates/app/src/main.rs", 0, 1),
        file("build.rs", 2, 2),
    ];
    let dir = |path: &str, name: &str, depth, added, removed| EditRow::Dir {
        path: path.into(),
        name: name.into(),
        depth,
        added,
        removed,
        collapsed: false,
    };
    assert_eq!(
        edit_tree(&files, &|_| false),
        [
            dir("crates", "crates", 0, 40, 3),
            dir("crates/app/src", "app/src", 1, 0, 1),
            EditRow::File { ix: 3, depth: 2 },
            dir("crates/ui/src/chat", "ui/src/chat", 1, 40, 2),
            EditRow::File { ix: 2, depth: 2 },
            EditRow::File { ix: 1, depth: 2 },
            EditRow::File { ix: 4, depth: 0 },
            EditRow::File { ix: 0, depth: 0 },
        ]
    );
    assert_eq!(file_order(&files), [3, 2, 1, 4, 0]);
}

#[test]
fn collapsed_folders_hide_their_files_but_keep_totals() {
    let files = [file("src/a.rs", 1, 2), file("src/b.rs", 3, 4), file("top.rs", 5, 0)];
    let rows = edit_tree(&files, &|path| path == "src");
    assert_eq!(
        rows,
        [
            EditRow::Dir {
                path: "src".into(),
                name: "src".into(),
                depth: 0,
                added: 4,
                removed: 6,
                collapsed: true,
            },
            EditRow::File { ix: 2, depth: 0 },
        ]
    );
}

#[test]
fn card_labels_count_files_and_lines() {
    let files = [file("a.rs", 40, 3), file("b.rs", 1, 0)];
    assert_eq!(changes_title(&files), "2 changed files");
    assert_eq!(changes_label(&files), "2 changed files, 41 added, 3 removed");
    assert_eq!(changes_label(&files[1..]), "1 changed file, 1 added, 0 removed");
}

#[test]
fn only_finished_edits_count_as_applied() {
    let edit = |id: &str, status| {
        Step::Tool(ToolCall {
            status,
            ..ToolCall::new(
                id,
                "Edit",
                ToolDetail::Edit {
                    path: format!("{id}.rs"),
                    diff: String::new(),
                    added: 1,
                    removed: 0,
                },
            )
        })
    };
    let steps = [
        edit("a", ToolStatus::Done),
        edit("b", ToolStatus::Stopped),
        edit("c", ToolStatus::Running),
        edit("d", ToolStatus::Waiting),
    ];
    assert_eq!(
        reply_edits(&steps, None).iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["a.rs"]
    );
}
