use super::*;
use crate::git_panel::change;

fn sized(lines: usize, binary: bool, cut: bool) -> Diff {
    Diff { raw: "x\n".repeat(lines).into(), rows: Rc::default(), lines, binary, cut, acting: None }
}

#[test]
fn diffs_know_binary_size_and_rows() {
    let (rows, lines, binary, cut) =
        prepare_diff("diff --git a/x b/x\n@@ -1 +1 @@\n-\told\n+new\n");
    assert_eq!((lines, binary, cut), (4, false, false));
    assert!(matches!(&rows[1], DiffRow::Line { text, .. } if text.as_ref() == "    old"));
    let (_, _, binary, _) =
        prepare_diff("diff --git a/p.png b/p.png\nBinary files a/p.png and b/p.png differ\n");
    assert!(binary);
    let (rows, ..) = prepare_diff("diff --git a/x b/x\nold mode 100644\nnew mode 100755\n");
    assert!(rows.is_empty());
    assert!(sized(DIFF_LINES + 1, false, false).too_large());
    assert!(!sized(1, false, false).too_large());
}

#[test]
fn only_whole_text_diffs_of_modified_files_get_hunk_actions() {
    use ChangeKind::*;
    let text = sized(3, false, false);
    assert!(hunk_actions(&change("a.rs", Modified, false), &text));
    assert!(hunk_actions(&change("a.rs", Modified, true), &text));
    for kind in [Added, Deleted, Renamed, Copied, TypeChanged, Untracked, Conflicted] {
        assert!(!hunk_actions(&change("a.rs", kind, false), &text), "{kind:?}");
    }
    let modified = change("a.rs", Modified, false);
    assert!(!hunk_actions(&modified, &sized(3, true, false)));
    assert!(!hunk_actions(&modified, &sized(3, false, true)));
    assert!(!hunk_actions(&modified, &sized(DIFF_LINES + 1, false, false)));
}
