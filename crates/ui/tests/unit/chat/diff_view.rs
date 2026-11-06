use super::*;

fn line(change: Change, old: Option<u32>, new: Option<u32>, text: &str) -> DiffRow {
    DiffRow::Line { change, old, new, text: text.into() }
}

#[test]
fn positioned_hunks_number_both_sides() {
    let rows = parse("@@ -3,3 +3,4 @@ fn main\n a\n-b\n+c\n+d\n e\n");
    assert_eq!(
        rows,
        [
            DiffRow::Hunk("@@ -3,3 +3,4 @@ fn main".into()),
            line(Change::Context, Some(3), Some(3), "a"),
            line(Change::Removed, Some(4), None, "b"),
            line(Change::Added, None, Some(4), "c"),
            line(Change::Added, None, Some(5), "d"),
            line(Change::Context, Some(5), Some(6), "e"),
        ]
    );
}

#[test]
fn synthesized_edits_have_no_numbers_and_bare_breaks() {
    let rows = parse(" ctx\n-old\n+new\n@@\n--- not a header\n+++ nor this\n");
    assert_eq!(
        rows,
        [
            line(Change::Context, None, None, "ctx"),
            line(Change::Removed, None, None, "old"),
            line(Change::Added, None, None, "new"),
            DiffRow::Hunk("".into()),
            line(Change::Removed, None, None, "-- not a header"),
            line(Change::Added, None, None, "++ nor this"),
        ]
    );
}

#[test]
fn file_headers_show_only_for_multi_file_patches() {
    let one = "Index: a.rs\n===\n--- a.rs\n+++ a.rs\n@@ -1,1 +1,1 @@\n-x\n+y\n\\ No newline at end of file\n";
    assert_eq!(
        parse(one),
        [
            DiffRow::Hunk("@@ -1,1 +1,1 @@".into()),
            line(Change::Removed, Some(1), None, "x"),
            line(Change::Added, None, Some(1), "y"),
        ]
    );
    let two = "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-x\n+y\n--- a/b.rs\n+++ b/b.rs\n@@ -0,0 +1,1 @@\n+z\n";
    let rows = parse(two);
    assert_eq!(rows[0], DiffRow::File("a.rs".into()));
    assert_eq!(rows[4], DiffRow::File("b.rs".into()));
    assert_eq!(rows[6], line(Change::Added, None, Some(1), "z"));
    let codex = "*** Begin Patch\n*** Update File: src/x.rs\n@@ fn go\n-a\n+b\n*** Add File: src/y.rs\n+new\n*** End Patch";
    assert_eq!(
        parse(codex),
        [
            DiffRow::File("src/x.rs".into()),
            DiffRow::Hunk("@@ fn go".into()),
            line(Change::Removed, None, None, "a"),
            line(Change::Added, None, None, "b"),
            DiffRow::File("src/y.rs".into()),
            line(Change::Added, None, None, "new"),
        ]
    );
}

#[test]
fn patch_path_names_the_first_file_even_alone() {
    assert_eq!(patch_path("--- a.rs\n+++ a.rs\n@@ -1 +1 @@\n-x\n+y\n").as_deref(), Some("a.rs"));
    let two = "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-x\n+y\n--- a/b.rs\n+++ b/b.rs\n";
    assert_eq!(patch_path(two).as_deref(), Some("a.rs"));
    assert_eq!(
        patch_path("*** Begin Patch\n*** Update File: src/x.rs\n@@\n-a\n+b\n").as_deref(),
        Some("src/x.rs")
    );
    assert_eq!(patch_path("--- a/gone.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n"), None);
    assert_eq!(patch_path("@@ -1 +1 @@\n-x\n+++ y\n"), None);
}

#[test]
fn truncated_hunks_end_at_the_note() {
    let rows = parse("@@ -0,0 +1,900 @@\n+a\n+b\n… 898 more lines\n");
    assert_eq!(rows.last(), Some(&DiffRow::Note("898 more lines".into())));
    assert_eq!(rows[2], line(Change::Added, None, Some(2), "b"));
}

#[test]
fn word_marks_cover_the_changed_middle() {
    assert_eq!(word_marks("        See work", "        See selected work"), Some((12..12, 12..21)));
    assert_eq!(word_marks("let a = 1;", "let b = 2;"), Some((4..9, 4..9)));
    assert_eq!(word_marks("naïve one", "naïve two"), Some((7..10, 7..10)));
    assert_eq!(word_marks("abc", "xyz"), None);
}

#[test]
fn line_marks_pair_removed_and_added_runs() {
    let rows = parse("@@ -1,2 +1,2 @@\n-let a = 1;\n-x\n+let b = 2;\n+y\n");
    assert_eq!(line_marks(&rows), [None, Some(4..9), None, Some(4..9), None]);
}

#[test]
fn hunk_headers_read_line_and_context() {
    assert_eq!(
        hunk_title("@@ -40,6 +44,7 @@ function Stats"),
        Some(("Line 44".into(), Some("function Stats".into())))
    );
    assert_eq!(hunk_title("@@ -1 +1 @@"), Some(("Line 1".into(), None)));
    assert_eq!(hunk_title("@@"), None);
}

#[test]
fn gaps_between_hunks_count_hidden_lines() {
    let rows = parse("@@ -1,3 +1,3 @@\n a\n-b\n+c\n d\n@@ -10,2 +10,2 @@\n x\n-y\n+z\n");
    assert_eq!(gaps(&rows), [Gap { before_row: 5, old_start: 4, new_start: 4, lines: 6 }]);
}

#[test]
fn split_pairs_removed_and_added_runs() {
    let rows = parse("@@ -1,4 +1,5 @@\n a\n-b\n-c\n+x\n+y\n+z\n d\n");
    assert_eq!(
        split_rows(&rows),
        [
            Split::Full(0),
            Split::Pair(Some(1), Some(1)),
            Split::Pair(Some(2), Some(4)),
            Split::Pair(Some(3), Some(5)),
            Split::Pair(None, Some(6)),
            Split::Pair(Some(7), Some(7))
        ]
    );
}

#[test]
fn gaps_do_not_cross_files() {
    let rows = parse(
        "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n\
         diff --git a/z b/z\n--- a/z\n+++ b/z\n@@ -20,2 +20,2 @@\n-x\n+y\n w\n",
    );
    assert!(gaps(&rows).is_empty());
}

fn lines(range: std::ops::Range<u32>) -> Rc<Vec<DiffRow>> {
    Rc::new(range.map(|n| line(Change::Context, Some(n), Some(n), "k")).collect())
}

#[test]
fn splice_fills_expanded_gaps_and_marks_the_rest() {
    let rows = parse("@@ -1,3 +1,3 @@\n a\n-b\n+c\n d\n@@ -10,2 +10,2 @@\n x\n-y\n+z\n");
    let (open, none) = (Opened::new(), Opened::from([(5, Ok(lines(4..10)))]));
    let (kept, pending) = splice(&rows, &open);
    assert_eq!(kept, *rows);
    assert_eq!(pending.keys().copied().collect::<Vec<_>>(), [5]);
    assert_eq!(pending[&5].unavailable, None);
    let (kept, pending) = splice(&rows, &none);
    assert_eq!(kept.len(), rows.len() + 6);
    assert!(pending.is_empty());
    assert!(matches!(kept[5], DiffRow::Line { old: Some(4), .. }));
    assert!(matches!(kept[11], DiffRow::Hunk(_)));
    let gone = Opened::from([(5, Err(Unavailable::NoFile))]);
    let (kept, pending) = splice(&rows, &gone);
    assert_eq!(kept, *rows);
    assert_eq!(pending[&5].unavailable, Some(Unavailable::NoFile));
}

#[test]
fn pending_gap_sits_on_the_hunk_row_in_split_view() {
    let rows = parse("@@ -1,3 +1,3 @@\n a\n-b\n+c\n d\n@@ -10,2 +10,2 @@\n x\n-y\n+z\n");
    let (kept, pending) = splice(&rows, &Opened::new());
    let at = *pending.keys().next().unwrap();
    assert!(split_rows(&kept).contains(&Split::Full(at)));
}
