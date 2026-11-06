use std::path::PathBuf;

use super::*;
use crate::{Change, ChangeKind, Stat, diff, run_args};

#[test]
fn a_hunk_patch_is_its_file_headers_and_its_own_lines() {
    let head =
        "diff --git a/f b/f\nold mode 100644\nnew mode 100755\nindex 1..2\n--- a/f\n+++ b/f\n";
    let one = "@@ -1,2 +1,2 @@\n-a\r\n+A\r\n b\r\n";
    let two = "@@ -9 +9 @@ h\n-l\n+L\n\\ No newline at end of file\n";
    let text = format!("{head}{one}{two}");
    let kept = "diff --git a/f b/f\nindex 1..2\n--- a/f\n+++ b/f\n";
    assert_eq!(hunk_patch(&text, 0).unwrap(), format!("{kept}{one}"));
    assert_eq!(hunk_patch(&text, 1).unwrap(), format!("{kept}{two}"));
    assert_eq!(hunk_patch(&text, 2), None);
    assert_eq!(hunk_patch(head, 0), None);
    assert_eq!(hunk_patch("", 0), None);
    let other = "diff --git a/g b/g\n--- a/g\n+++ b/g\n@@ -1 +1 @@\n-x\n+y\n";
    let both = format!("{head}{one}{other}");
    assert_eq!(hunk_patch(&both, 1).unwrap(), other);
}

#[test]
fn hunks_git_cant_apply_safely_are_refused_before_running_it() {
    let nowhere = Path::new("/nonexistent");
    let bare = "diff --git f f\n--- f\n+++ f\n@@ -1 +1 @@\n-a\n+++ b/f\n";
    assert!(apply_hunk(nowhere, bare, 0, HunkOp::Revert).unwrap_err().contains("prefixes"));
    let lossy = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+caf\u{FFFD}\n";
    assert!(apply_hunk(nowhere, lossy, 0, HunkOp::Stage).unwrap_err().contains("UTF-8"));
    assert_eq!(apply_hunk(nowhere, lossy, 1, HunkOp::Stage).unwrap_err(), MOVED);
}

const LINES: &str = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n";

fn git(top: &Path, args: &[&str]) -> String {
    let out = run_args(top, args, QUICK).unwrap();
    assert!(out.ok, "git {args:?}: {}", out.stderr);
    out.stdout
}

/// A repo whose one commit holds `path` with `text`, ignoring the user's config that changes diffs.
fn repo(name: &str, path: &str, text: &str) -> PathBuf {
    let top = std::env::temp_dir().join(format!("aether-hunk-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&top);
    std::fs::create_dir_all(top.join(path).parent().unwrap()).unwrap();
    git(&top, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("user.name", "a"),
        ("user.email", "a@b"),
        ("core.hooksPath", "/dev/null"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
        ("diff.noprefix", "false"),
        ("diff.mnemonicPrefix", "false"),
    ] {
        git(&top, &["config", key, value]);
    }
    std::fs::write(top.join(path), text).unwrap();
    git(&top, &["add", "."]);
    git(&top, &["commit", "-qm", "first"]);
    top
}

fn file_diff(top: &Path, path: &str, staged: bool) -> String {
    let change = Change {
        path: path.into(),
        from: None,
        kind: ChangeKind::Modified,
        staged,
        stat: Stat::Unknown,
    };
    diff(top, &change).unwrap()
}

#[test]
fn staging_unstaging_and_reverting_a_hunk_leave_the_others() {
    let path = "dir/ä b.txt";
    let top = repo("ops", path, LINES);
    let index = || git(&top, &["show", &format!(":{path}")]);
    let edited = "A\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nL\n";
    std::fs::write(top.join(path), edited).unwrap();

    apply_hunk(&top, &file_diff(&top, path, false), 0, HunkOp::Stage).unwrap();
    assert_eq!(index(), LINES.replacen('a', "A", 1));
    assert_eq!(std::fs::read_to_string(top.join(path)).unwrap(), edited);

    apply_hunk(&top, &file_diff(&top, path, true), 0, HunkOp::Unstage).unwrap();
    assert_eq!(index(), LINES);

    apply_hunk(&top, &file_diff(&top, path, false), 1, HunkOp::Stage).unwrap();
    apply_hunk(&top, &file_diff(&top, path, false), 0, HunkOp::Revert).unwrap();
    assert_eq!(index(), LINES.replace('l', "L"));
    assert_eq!(std::fs::read_to_string(top.join(path)).unwrap(), LINES.replace('l', "L"));
    let _ = std::fs::remove_dir_all(&top);
}

#[test]
fn hunks_keep_crlf_and_a_missing_final_newline() {
    let crlf = LINES.trim_end().replace('\n', "\r\n");
    let top = repo("crlf", "crlf.txt", &crlf);
    let edited = crlf.replacen('a', "A", 1).replace('l', "L");
    std::fs::write(top.join("crlf.txt"), &edited).unwrap();
    let text = file_diff(&top, "crlf.txt", false);
    assert!(text.contains("\\ No newline at end of file") && text.contains("+A\r\n"), "{text}");

    apply_hunk(&top, &text, 1, HunkOp::Stage).unwrap();
    assert_eq!(git(&top, &["show", ":crlf.txt"]), crlf.replace('l', "L"));
    apply_hunk(&top, &file_diff(&top, "crlf.txt", false), 0, HunkOp::Stage).unwrap();
    assert_eq!(git(&top, &["show", ":crlf.txt"]), edited);
    let _ = std::fs::remove_dir_all(&top);
}

#[test]
fn a_hunk_that_moved_says_so() {
    let top = repo("moved", "f.txt", LINES);
    std::fs::write(top.join("f.txt"), LINES.replacen('a', "A", 1)).unwrap();
    let text = file_diff(&top, "f.txt", false);
    apply_hunk(&top, &text, 0, HunkOp::Stage).unwrap();
    assert_eq!(apply_hunk(&top, &text, 0, HunkOp::Stage).unwrap_err(), MOVED);
    std::fs::write(top.join("f.txt"), LINES.replacen('a', "Z", 1)).unwrap();
    assert_eq!(apply_hunk(&top, &text, 0, HunkOp::Revert).unwrap_err(), MOVED);
    assert_eq!(std::fs::read_to_string(top.join("f.txt")).unwrap(), LINES.replacen('a', "Z", 1));
    let _ = std::fs::remove_dir_all(&top);
}

#[test]
fn a_users_noprefix_config_still_gets_hunk_actions() {
    let top = repo("noprefix", "a.txt", LINES);
    git(&top, &["config", "diff.noprefix", "true"]);
    std::fs::write(top.join("a.txt"), LINES.replacen('a', "A", 1)).unwrap();
    let text = file_diff(&top, "a.txt", false);
    assert!(text.contains("+++ b/a.txt"), "{text}");
    apply_hunk(&top, &text, 0, HunkOp::Stage).unwrap();
    assert_eq!(git(&top, &["show", ":a.txt"]), LINES.replacen('a', "A", 1));
    let _ = std::fs::remove_dir_all(&top);
}

#[cfg(unix)]
#[test]
fn a_hunk_leaves_the_files_mode_change_alone() {
    use std::os::unix::fs::PermissionsExt;
    let top = repo("mode", "run.sh", LINES);
    git(&top, &["config", "core.fileMode", "true"]);
    let file = top.join("run.sh");
    std::fs::write(&file, LINES.replacen('a', "A", 1).replace('l', "L")).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    let index_mode = || git(&top, &["ls-files", "--stage", "run.sh"])[..6].to_string();
    let mode_change = || git(&top, &["diff", "--summary"]).contains("mode change 100644 => 100755");

    apply_hunk(&top, &file_diff(&top, "run.sh", false), 0, HunkOp::Stage).unwrap();
    assert_eq!(index_mode(), "100644");
    assert!(mode_change());
    apply_hunk(&top, &file_diff(&top, "run.sh", false), 0, HunkOp::Revert).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), LINES.replacen('a', "A", 1));
    assert_eq!(index_mode(), "100644");
    assert!(mode_change());
    let _ = std::fs::remove_dir_all(&top);
}
