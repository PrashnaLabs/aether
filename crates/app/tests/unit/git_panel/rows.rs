use super::*;
use crate::git_panel::change;

fn open() -> HashSet<Section> {
    HashSet::new()
}

fn shut(sections: &[Section]) -> HashSet<Section> {
    sections.iter().copied().collect()
}

#[test]
fn sections_list_staged_then_changes_with_counts() {
    use ChangeKind::*;
    let changes = [
        change("a.rs", Modified, false),
        change("a.rs", Modified, true),
        change("b.rs", Added, true),
        change("new.txt", Untracked, false),
    ];
    assert_eq!(
        rows(&changes, &[], 0, None, &open()),
        [
            Row::Header(Section::Staged, 2),
            Row::File(1),
            Row::File(2),
            Row::Header(Section::Changes, 2),
            Row::File(0),
            Row::File(3),
        ]
    );
    let keys: Vec<SharedString> = rows(&changes, &[], 0, None, &open())
        .into_iter()
        .map(|r| row_key(&r, &changes, &[], &[], &[]))
        .collect();
    assert_eq!(keys, ["staged", "s:a.rs", "s:b.rs", "changes", "c:a.rs", "c:new.txt"]);
    assert_eq!(
        rows(&changes[..1], &[], 0, None, &open()),
        [Row::Header(Section::Changes, 1), Row::File(0)]
    );
    assert!(rows(&[], &[], 0, None, &open()).is_empty());
    assert_eq!(changed_files(&changes), 3);
    assert_eq!(changed_files(&[]), 0);
}

#[test]
fn accordion_collapses_sections() {
    use ChangeKind::*;
    let changes = [change("a.rs", Modified, true), change("b.rs", Modified, false)];
    // Both expanded
    assert_eq!(
        rows(&changes, &[], 0, None, &open()),
        [
            Row::Header(Section::Staged, 1),
            Row::File(0),
            Row::Header(Section::Changes, 1),
            Row::File(1),
        ]
    );
    // Staged collapsed
    assert_eq!(
        rows(&changes, &[], 0, None, &shut(&[Section::Staged])),
        [Row::Header(Section::Staged, 1), Row::Header(Section::Changes, 1), Row::File(1),]
    );
    // Both collapsed
    assert_eq!(
        rows(&changes, &[], 0, None, &shut(&[Section::Staged, Section::Changes])),
        [Row::Header(Section::Staged, 1), Row::Header(Section::Changes, 1),]
    );
}

#[test]
fn section_collapse_toggles_state() {
    let changes =
        [change("s.rs", ChangeKind::Modified, true), change("u.rs", ChangeKind::Modified, false)];
    // Neither collapsed: 2 headers + 2 files = 4 rows
    assert_eq!(rows(&changes, &[], 0, None, &open()).len(), 4);
    // Staged collapsed: staged header (files omitted) + changes header + unstaged file = 3 rows
    let staged_collapsed = rows(&changes, &[], 0, None, &shut(&[Section::Staged]));
    assert_eq!(staged_collapsed.len(), 3);
    assert_eq!(staged_collapsed[0], Row::Header(Section::Staged, 1));
    assert_eq!(staged_collapsed[1], Row::Header(Section::Changes, 1));
    assert_eq!(staged_collapsed[2], Row::File(1));
    // Both collapsed: 2 headers only = 2 rows
    assert_eq!(rows(&changes, &[], 0, None, &shut(&[Section::Staged, Section::Changes])).len(), 2);
}

#[test]
fn open_diffs_follow_their_file_between_sections() {
    let changes =
        [change("x.rs", ChangeKind::Modified, true), change("y.rs", ChangeKind::Modified, false)];
    assert_eq!(follow(&changes, "s:x.rs", "x.rs"), Some(0));
    assert_eq!(follow(&changes, "c:x.rs", "x.rs"), Some(0));
    assert_eq!(follow(&changes, "c:gone.rs", "gone.rs"), None);
    assert_eq!(key_path("c:dir/a:b.rs"), Some("dir/a:b.rs"));
    assert_eq!(key_path("staged"), None);
}

#[test]
fn arrows_home_and_end_stop_at_the_ends() {
    assert_eq!(step(0, 3, "up"), None);
    assert_eq!(step(0, 3, "down"), Some(1));
    assert_eq!(step(2, 3, "down"), None);
    assert_eq!(step(1, 3, "home"), Some(0));
    assert_eq!(step(1, 3, "end"), Some(2));
    assert_eq!(step(0, 0, "end"), None);
    assert_eq!(step(0, 3, "x"), None);
}

#[test]
fn rows_put_the_name_first() {
    assert_eq!(split_path("src/app/main.rs"), ("main.rs", "src/app"));
    assert_eq!(split_path("README"), ("README", ""));
    assert_eq!(split_path("dir/"), ("dir/", ""));
}

#[test]
fn rows_describe_themselves_in_words() {
    let mut renamed = change("new.rs", ChangeKind::Renamed, true);
    renamed.from = Some("old.rs".into());
    assert_eq!(describe(&renamed), "Renamed from old.rs, staged");
    assert_eq!(describe(&change("a", ChangeKind::Modified, false)), "Modified, not staged");
    assert_eq!(describe(&change("a", ChangeKind::Untracked, false)), "Untracked");
    assert_eq!(stat_words(Stat::Lines(3, 1)).as_deref(), Some("3 added, 1 removed"));
    assert_eq!(stat_words(Stat::Unknown), None);
}

#[test]
fn conflicts_get_their_own_merge_section_first() {
    use ChangeKind::*;
    let changes = [
        change("a.rs", Modified, false),
        change("both.rs", Conflicted, false),
        change("b.rs", Added, true),
        change("new.txt", Untracked, false),
    ];
    let listed = rows(&changes, &[], 0, None, &open());
    assert_eq!(
        listed,
        [
            Row::Header(Section::Merge, 1),
            Row::File(1),
            Row::Header(Section::Staged, 1),
            Row::File(2),
            Row::Header(Section::Changes, 2),
            Row::File(0),
            Row::File(3),
        ]
    );
    let keys: Vec<SharedString> =
        listed.into_iter().map(|r| row_key(&r, &changes, &[], &[], &[])).collect();
    assert_eq!(keys, ["merge", "c:both.rs", "staged", "s:b.rs", "changes", "c:a.rs", "c:new.txt"]);
    assert_eq!(
        rows(&changes, &[], 0, None, &shut(&[Section::Merge])),
        [
            Row::Header(Section::Merge, 1),
            Row::Header(Section::Staged, 1),
            Row::File(2),
            Row::Header(Section::Changes, 2),
            Row::File(0),
            Row::File(3),
        ]
    );
    // a staged conflict is resolved: it leaves Merge Changes
    assert_eq!(section_of(&change("both.rs", Modified, true)), Section::Staged);
    assert_eq!(step(0, keys.len(), "end"), Some(6));
}

#[test]
fn discard_prompts_name_the_file_or_count_the_changes() {
    use ChangeKind::*;
    let (title, text, ok) = discard_prompt(&[change("src/a.rs", Modified, false)]);
    assert_eq!(
        (title.as_str(), text.as_str(), ok),
        ("Discard changes in src/a.rs?", "This can't be undone.", "Discard changes")
    );
    let (title, _, ok) = discard_prompt(&[change("n.txt", Untracked, false)]);
    assert_eq!((title.as_str(), ok), ("Delete untracked file n.txt?", "Delete file"));
    let mixed = [
        change("a.rs", Modified, false),
        change("gone.rs", Deleted, false),
        change("n.txt", Untracked, false),
    ];
    let (title, text, ok) = discard_prompt(&mixed);
    assert_eq!((title.as_str(), ok), ("Discard all 3 changes?", "Discard all"));
    assert_eq!(
        text,
        "Changes to 2 files will be lost and 1 untracked file deleted. This can't be undone."
    );
    let (_, text, _) = discard_prompt(&mixed[..2]);
    assert_eq!(text, "Changes to 2 files will be lost. This can't be undone.");
    let new = [change("x", Untracked, false), change("y", Untracked, false)];
    assert_eq!(discard_prompt(&new).1, "2 untracked files will be deleted. This can't be undone.");
    // `git add -N`: its index copy is empty, so discarding deletes it
    let intent = change("i.rs", Added, false);
    assert!(discard_deletes(&intent) && !discard_deletes(&change("i.rs", Added, true)));
    let (title, _, ok) = discard_prompt(std::slice::from_ref(&intent));
    assert_eq!((title.as_str(), ok), ("Delete file i.rs?", "Delete file"));
    let (_, text, _) = discard_prompt(&[change("a.rs", Modified, false), intent]);
    assert_eq!(
        text,
        "Changes to 1 file will be lost and 1 untracked file deleted. This can't be undone."
    );
}

fn stash(index: usize, sha: &str, message: &str, branch: &str) -> Stash {
    Stash { index, sha: sha.into(), message: message.into(), branch: branch.into(), time: 0 }
}

fn commit(sha: &str, subject: &str, flow: Option<Flow>) -> Commit {
    Commit {
        sha: sha.into(),
        short: sha.into(),
        subject: subject.into(),
        author: "Ada".into(),
        time: 0,
        flow,
    }
}

#[test]
fn stashes_then_history_follow_the_changes() {
    let changes = [change("a.rs", ChangeKind::Modified, false)];
    let stashes = [stash(0, "s0", "wip", "main"), stash(1, "s1", "older", "dev")];
    let commits = [commit("c1", "two", Some(Flow::Outgoing)), commit("c0", "one", None)];
    let listed = rows(&changes, &stashes, 0, Some(&commits), &open());
    assert_eq!(
        listed,
        [
            Row::Header(Section::Changes, 1),
            Row::File(0),
            Row::Header(Section::Stashes, 2),
            Row::Stash(0),
            Row::Stash(1),
            Row::Header(Section::History, 2),
            Row::Commit(0),
            Row::Commit(1),
        ]
    );
    let keys: Vec<SharedString> =
        listed.into_iter().map(|r| row_key(&r, &changes, &stashes, &[], &commits)).collect();
    assert_eq!(
        keys,
        [
            "changes",
            "c:a.rs",
            "stashes",
            "stash-s0",
            "stash-s1",
            "history",
            "commit-c1",
            "commit-c0"
        ]
    );
    assert!(keys.iter().all(|k| key_path(k).is_none() || k.starts_with("c:")));
    assert_eq!(
        rows(&[], &stashes, 0, None, &shut(&[Section::Stashes])),
        [Row::Header(Section::Stashes, 2)]
    );
}

#[test]
fn history_shows_once_there_is_a_commit_and_waits_for_its_read() {
    // no commit yet: no History at all, and no Stashes without stashes
    assert!(rows(&[], &[], 0, None, &open()).is_empty());
    // collapsed (the default) until opened, with nothing read yet
    assert_eq!(
        rows(&[], &[], 0, Some(&[]), &shut(&[Section::History])),
        [Row::Header(Section::History, 0)]
    );
    assert_eq!(
        rows(&[], &[], 0, Some(&[]), &open()),
        [Row::Header(Section::History, 0), Row::HistoryNote]
    );
    assert_eq!(row_key(&Row::HistoryNote, &[], &[], &[], &[]), "history-note");
}

#[test]
fn new_sections_and_rows_describe_themselves() {
    assert_eq!(section_title(Section::Stashes), "Stashes");
    assert_eq!(
        header_label(Section::Stashes, 1, false),
        "Stashes, 1 stash. Press Enter to collapse"
    );
    assert_eq!(header_label(Section::History, 0, true), "History. Press Enter to expand");
    assert_eq!(
        header_label(Section::History, 52, false),
        "History, 52 commits. Press Enter to collapse"
    );
    assert_eq!(
        header_label(Section::Staged, 2, true),
        "Staged Changes, 2 files. Press Enter to expand"
    );
    assert_eq!(
        stash_label(&stash(0, "s", "fix login", "main"), "5m"),
        "fix login, on main, 5m. Press Enter to open it, Space to apply it, Delete to drop it"
    );
    assert_eq!(
        stash_label(&stash(0, "s", "wip", ""), "Yesterday"),
        "wip, Yesterday. Press Enter to open it, Space to apply it, Delete to drop it"
    );
    assert_eq!(
        commit_label(&commit("c", "Fix it", Some(Flow::Outgoing)), "2h"),
        "Fix it, Ada, 2h, outgoing"
    );
    assert_eq!(
        commit_label(&commit("c", "Theirs", Some(Flow::Incoming)), "3d"),
        "Theirs, Ada, 3d, incoming"
    );
    assert_eq!(commit_label(&commit("c", "Old", None), "Oct 12"), "Old, Ada, Oct 12");
}

#[test]
fn worktrees_list_between_stashes_and_history_once_there_is_more_than_main() {
    let stashes = [stash(0, "s0", "wip", "main")];
    let trees = [
        Listed { path: "/code/app".into(), main: true, ..Listed::default() },
        Listed { path: "/w/review".into(), ..Listed::default() },
    ];
    let listed = rows(&[], &stashes, trees.len(), Some(&[]), &shut(&[Section::History]));
    assert_eq!(
        listed,
        [
            Row::Header(Section::Stashes, 1),
            Row::Stash(0),
            Row::Header(Section::Worktrees, 2),
            Row::Worktree(0),
            Row::Worktree(1),
            Row::Header(Section::History, 0),
        ]
    );
    let keys: Vec<SharedString> =
        listed.into_iter().map(|r| row_key(&r, &[], &stashes, &trees, &[])).collect();
    assert_eq!(
        keys,
        ["stashes", "stash-s0", "worktrees", "worktree-/code/app", "worktree-/w/review", "history"]
    );
    assert!(rows(&[], &[], 1, None, &open()).is_empty(), "only the main one: hidden");
    assert_eq!(
        rows(&[], &[], 2, None, &shut(&[Section::Worktrees])),
        [Row::Header(Section::Worktrees, 2)]
    );
    assert_eq!(section_title(Section::Worktrees), "Worktrees");
    assert_eq!(
        header_label(Section::Worktrees, 3, false),
        "Worktrees, 3 worktrees. Press Enter to collapse"
    );
}

fn dir(section: Section, path: &str, name: &str, depth: usize, collapsed: bool) -> Row {
    Row::Dir(Dir { section, path: path.into(), name: name.into(), depth, collapsed })
}

fn tree_changes() -> Vec<Change> {
    use ChangeKind::*;
    vec![
        change("src/app/chat/view.rs", Modified, false),
        change("src/app/chat/mod.rs", Modified, false),
        change("README.md", Modified, false),
        change("build.rs", Untracked, false),
        change("src/lib.rs", Modified, false),
        change("Cargo.toml", Modified, true),
        change("docs/guide/intro.md", Added, true),
        change("src/Zeta.rs", Modified, false),
    ]
}

#[test]
fn tree_puts_folders_first_by_name_and_compacts_lone_subfolders() {
    use Section::*;
    let changes = tree_changes();
    let tree = as_tree(&rows(&changes, &[], 0, None, &open()), &changes, &HashSet::new());
    assert_eq!(
        tree,
        [
            Row::Header(Staged, 2),
            dir(Staged, "docs/guide", "docs/guide", 0, false),
            Row::TreeFile(6, 1),
            Row::TreeFile(5, 0),
            Row::Header(Changes, 6),
            dir(Changes, "src", "src", 0, false),
            dir(Changes, "src/app/chat", "app/chat", 1, false),
            Row::TreeFile(1, 2),
            Row::TreeFile(0, 2),
            Row::TreeFile(4, 1),
            Row::TreeFile(7, 1),
            Row::TreeFile(3, 0),
            Row::TreeFile(2, 0),
        ]
    );
    let keys: Vec<SharedString> =
        tree.iter().map(|r| row_key(r, &changes, &[], &[], &[])).collect();
    assert_eq!(
        keys,
        [
            "staged",
            "staged:docs/guide",
            "s:docs/guide/intro.md",
            "s:Cargo.toml",
            "changes",
            "changes:src",
            "changes:src/app/chat",
            "c:src/app/chat/mod.rs",
            "c:src/app/chat/view.rs",
            "c:src/lib.rs",
            "c:src/Zeta.rs",
            "c:build.rs",
            "c:README.md",
        ]
    );
    assert_eq!(keys.iter().collect::<HashSet<_>>().len(), keys.len());
    assert_eq!(key_path("changes:src/app/chat"), Some("src/app/chat"));
    // the same folder in two sections keeps two keys
    let both = [
        change("src/a.rs", ChangeKind::Modified, true),
        change("src/b.rs", ChangeKind::Modified, false),
    ];
    let keys: Vec<SharedString> =
        as_tree(&rows(&both, &[], 0, None, &open()), &both, &HashSet::new())
            .iter()
            .map(|r| row_key(r, &both, &[], &[], &[]))
            .collect();
    assert_eq!(
        keys,
        ["staged", "staged:src", "s:src/a.rs", "changes", "changes:src", "c:src/b.rs"]
    );
}

#[test]
fn closed_folders_hide_what_is_inside_them() {
    use Section::*;
    let changes = tree_changes();
    let listed = rows(&changes, &[], 0, None, &open());
    let closed = |dirs: &[(Section, &str)]| -> HashSet<(Section, String)> {
        dirs.iter().map(|(s, p)| (*s, p.to_string())).collect()
    };
    let tree = as_tree(&listed, &changes, &closed(&[(Changes, "src")]));
    assert_eq!(
        tree[4..],
        [
            Row::Header(Changes, 6),
            dir(Changes, "src", "src", 0, true),
            Row::TreeFile(3, 0),
            Row::TreeFile(2, 0),
        ]
    );
    let tree = as_tree(&listed, &changes, &closed(&[(Changes, "src/app/chat"), (Staged, "src")]));
    assert_eq!(
        tree[4..],
        [
            Row::Header(Changes, 6),
            dir(Changes, "src", "src", 0, false),
            dir(Changes, "src/app/chat", "app/chat", 1, true),
            Row::TreeFile(4, 1),
            Row::TreeFile(7, 1),
            Row::TreeFile(3, 0),
            Row::TreeFile(2, 0),
        ]
    );
    // a closed section stays a lone caption; stashes and history aren't trees
    let stashes = [stash(0, "s0", "wip", "main")];
    let commits = [commit("c0", "one", None)];
    let listed = rows(&changes, &stashes, 0, Some(&commits), &shut(&[Changes]));
    let tree = as_tree(&listed, &changes, &HashSet::new());
    assert_eq!(
        tree[4..],
        [
            Row::Header(Changes, 6),
            Row::Header(Stashes, 1),
            Row::Stash(0),
            Row::Header(History, 1),
            Row::Commit(0),
        ]
    );
}

#[test]
fn folder_actions_take_the_exact_paths_under_the_folder() {
    use ChangeKind::*;
    let mut renamed = change("src/new.rs", Renamed, true);
    renamed.from = Some("old/old.rs".into());
    let changes = [
        change("src/a.rs", Modified, false),
        change("src/sub/b.rs", Untracked, false),
        change("srcx/c.rs", Modified, false),
        change("src/a.rs", Modified, true),
        renamed,
        change("src/conf.rs", Conflicted, false),
    ];
    let paths = |files: Vec<&Change>| files.iter().map(|c| c.path.clone()).collect::<Vec<_>>();
    assert_eq!(paths(files_under(&changes, Section::Changes, "src")), ["src/a.rs", "src/sub/b.rs"]);
    assert_eq!(paths(files_under(&changes, Section::Changes, "src/sub")), ["src/sub/b.rs"]);
    assert!(files_under(&changes, Section::Changes, "sr").is_empty());
    let stage = |section| stage_op(section, &files_under(&changes, section, "src"));
    assert_eq!(
        stage(Section::Changes),
        GitOp::Stage(vec!["src/a.rs".into(), "src/sub/b.rs".into()])
    );
    assert_eq!(
        stage(Section::Staged),
        GitOp::Unstage(vec!["src/a.rs".into(), "old/old.rs".into(), "src/new.rs".into()])
    );
    assert_eq!(stage(Section::Merge), GitOp::Stage(vec!["src/conf.rs".into()]));
    let discard: Vec<Change> =
        files_under(&changes, Section::Changes, "src").into_iter().cloned().collect();
    let (title, text, ok) = discard_dir_prompt("src", &discard);
    assert_eq!((title.as_str(), ok), ("Discard all 2 changes in src?", "Discard all"));
    assert_eq!(
        text,
        "Changes to 1 file will be lost and 1 untracked file deleted. This can't be undone."
    );
    let (title, _, ok) = discard_dir_prompt("src", &discard[..1]);
    assert_eq!((title.as_str(), ok), ("Discard changes in src/a.rs?", "Discard changes"));
    let Row::Dir(src) = dir(Section::Changes, "src", "src", 0, false) else { unreachable!() };
    assert_eq!(dir_label(&src, 2), "src folder, 2 files. Press Enter to collapse");
}

#[test]
fn left_and_right_fold_folders_or_move_through_the_tree() {
    let changes = tree_changes();
    let listed = rows(&changes, &[], 0, None, &open());
    let tree = as_tree(&listed, &changes, &HashSet::new());
    // 4 Changes, 5 src, 6 src/app/chat, 7 mod.rs, 9 lib.rs, 11 build.rs
    assert_eq!(fold(&tree, 5, "left"), Some(Fold::Toggle));
    assert_eq!(fold(&tree, 6, "left"), Some(Fold::Toggle));
    assert_eq!(fold(&tree, 5, "right"), Some(Fold::Focus(6)));
    assert_eq!(fold(&tree, 7, "left"), Some(Fold::Focus(6)));
    assert_eq!(fold(&tree, 9, "left"), Some(Fold::Focus(5)));
    assert_eq!(fold(&tree, 11, "left"), Some(Fold::Focus(4)));
    assert_eq!(fold(&tree, 7, "right"), None);
    assert_eq!(fold(&tree, 5, "up"), None);
    assert_eq!(fold(&tree, 4, "left"), None, "section captions aren't tree rows");
    assert_eq!(fold(&listed, 1, "left"), None, "nor are list rows");
    let closed = HashSet::from([(Section::Changes, "src".to_string())]);
    let tree = as_tree(&listed, &changes, &closed);
    assert_eq!(fold(&tree, 5, "right"), Some(Fold::Toggle));
    assert_eq!(fold(&tree, 5, "left"), Some(Fold::Focus(4)));
}

#[test]
fn folders_take_the_color_of_their_loudest_change() {
    use ChangeKind::*;
    let changes = [
        change("a", Added, false),
        change("b", Renamed, false),
        change("c", Modified, false),
        change("d", Deleted, false),
        change("e", Conflicted, false),
    ];
    let files: Vec<&Change> = changes.iter().collect();
    assert_eq!(loudest(&files), Some(Conflicted));
    assert_eq!(loudest(&files[..4]), Some(Deleted));
    assert_eq!(loudest(&files[..3]), Some(Modified));
    assert_eq!(loudest(&files[..2]), Some(Renamed));
    assert_eq!(loudest(&files[..1]), Some(Added));
    assert_eq!(loudest(&[]), None);
}
