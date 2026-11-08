use super::*;

#[test]
fn explorer_state_manages_tabs_and_resolves_active_folder() {
    let dir = std::env::temp_dir().join(format!(
        "aether_test_view_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let sub = dir.join("crates").join("app");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("app.rs");
    std::fs::write(&file, "code").unwrap();

    let mut state = ExplorerState::new(dir.clone());
    state.open_file(file.clone()).unwrap();
    assert_eq!(state.tabs.len(), 1);
    assert_eq!(state.active_tab, Some(0));
    assert_eq!(state.active_folder(), Some(sub));

    // Opening same file selects existing tab without duplicating
    state.open_file(file.clone()).unwrap();
    assert_eq!(state.tabs.len(), 1);

    assert!(state.close_tab(0));
    assert_eq!(state.tabs.len(), 0);
    assert_eq!(state.active_tab, None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn explorer_instances_maintain_separate_roots_and_tabs() {
    let dir_a = std::env::temp_dir().join("aether_test_scope_a");
    let dir_b = std::env::temp_dir().join("aether_test_scope_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    let file_a = dir_a.join("file_a.txt");
    let file_b = dir_b.join("file_b.txt");
    std::fs::write(&file_a, "A").unwrap();
    std::fs::write(&file_b, "B").unwrap();

    let mut exp_a = ExplorerState::new(dir_a.clone());
    let mut exp_b = ExplorerState::new(dir_b.clone());

    exp_a.open_file(file_a).unwrap();
    exp_b.open_file(file_b).unwrap();

    assert_eq!(exp_a.tabs.len(), 1);
    assert_eq!(exp_a.tabs[0].title, "file_a.txt");
    assert_eq!(exp_b.tabs.len(), 1);
    assert_eq!(exp_b.tabs[0].title, "file_b.txt");

    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
}

#[test]
fn tab_labels_add_the_folder_only_when_names_clash() {
    let buffer = |path: &str| {
        let path = std::path::PathBuf::from(path);
        let title = path.file_name().unwrap().to_string_lossy().to_string();
        EditorBuffer {
            id: 0,
            path,
            title,
            original: String::new(),
            stamp: None,
            is_dirty: false,
            conflict: None,
        }
    };
    let mut state = ExplorerState::new(PathBuf::from("/nonexistent-aether-root"));
    state.tabs = vec![buffer("/r/app/mod.rs"), buffer("/r/ui/mod.rs"), buffer("/r/lib.rs")];
    assert_eq!(state.tab_labels(), ["mod.rs · app", "mod.rs · ui", "lib.rs"]);
}

#[test]
fn quick_open_matches_are_cached_per_query_and_index() {
    let root = PathBuf::from("/nonexistent-aether-root");
    let mut state = ExplorerState::new(root.clone());
    state.set_quick_open_query("app".into());
    assert!(state.quick_open_items().is_empty());

    let files: Vec<String> = vec!["README.md".into(), "src/app.rs".into()];
    state.set_index(0, FileIndex::from_files(files, [root.join("target")].into_iter().collect()));
    assert_eq!(state.quick_open_items().len(), 1);
    assert_eq!(state.quick_open_items()[0].path, root.join("src/app.rs"));
    state.set_quick_open_query(String::new());
    assert_eq!(state.quick_open_items().len(), 2);

    assert!(state.is_ignored(&root.join("target/debug/x")));
    assert!(!state.is_ignored(&root.join("src/app.rs")));
}

#[test]
fn grep_drops_results_of_superseded_runs() {
    let mut state = ExplorerState::new(PathBuf::from("/nonexistent-aether-root"));
    assert!(state.begin_grep().is_none());
    state.grep_options.query = "x".into();
    let (old, _) = state.begin_grep().unwrap();
    let (new, _) = state.begin_grep().unwrap();
    let hit = |line_number| GrepMatch {
        path: PathBuf::from("a"),
        line_number,
        line_text: String::new(),
        match_indices: (0, 1),
    };
    state.finish_grep(old, vec![hit(1)]);
    assert!(state.grep_running && state.grep_results.is_empty());
    state.finish_grep(new, vec![hit(2)]);
    assert!(!state.grep_running);
    assert_eq!(state.grep_results[0].line_number, 2);
}

#[test]
fn index_drops_results_of_superseded_loads() {
    let root = PathBuf::from("/nonexistent-aether-root");
    let mut state = ExplorerState::new(root.clone());
    state.index_generation = 2;
    let index = |file: &str| FileIndex::from_files(vec![file.to_string()], Default::default());
    assert!(!state.set_index(1, index("old.rs")));
    assert!(state.quick_open_items().is_empty());
    assert!(state.set_index(2, index("new.rs")));
    assert_eq!(state.quick_open_items()[0].path, root.join("new.rs"));
}

#[test]
fn renamed_paths_retarget_their_tabs() {
    let buffer = |path: &str| EditorBuffer {
        id: 0,
        path: PathBuf::from(path),
        title: String::new(),
        original: String::new(),
        stamp: None,
        is_dirty: false,
        conflict: None,
    };
    let mut state = ExplorerState::new(PathBuf::from("/nonexistent-aether-root"));
    state.tabs = vec![buffer("/r/dir/a.rs"), buffer("/r/other.rs"), buffer("/r/dirt.rs")];
    state.retarget_tabs(Path::new("/r/dir"), Path::new("/r/new"));
    state.retarget_tabs(Path::new("/r/other.rs"), Path::new("/r/x.rs"));
    let paths: Vec<_> = state.tabs.iter().map(|t| t.path.to_str().unwrap()).collect();
    let moved = Path::new("/r/new").join("a.rs");
    assert_eq!(paths, [moved.to_str().unwrap(), "/r/x.rs", "/r/dirt.rs"]);
    assert_eq!(state.tabs[1].title, "x.rs");
}
