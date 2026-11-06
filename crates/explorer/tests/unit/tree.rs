use super::*;

#[test]
fn tree_lists_folders_lazily_and_flattens_open_ones() {
    let dir = std::env::temp_dir().join(format!(
        "aether_test_tree_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(dir.join("src/sub")).unwrap();
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::create_dir_all(dir.join("target")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::write(dir.join("src/sub/deep.rs"), "").unwrap();
    std::fs::write(dir.join("b.txt"), "").unwrap();
    std::fs::write(dir.join("A.txt"), "").unwrap();

    let names = |state: &FileTreeState| -> Vec<(String, usize)> {
        state.rows().iter().map(|n| (n.name.clone(), n.depth)).collect()
    };
    let row = |name: &str, depth: usize| (name.to_string(), depth);

    let mut state = FileTreeState::new(dir.clone());
    assert_eq!(names(&state), [row("src", 0), row("target", 0), row("A.txt", 0), row("b.txt", 0)]);
    assert!(!state.listings.contains_key(&dir.join("src")));
    assert!(!state.has_expanded_folders());
    state.reveal(&dir.join("src/sub/deep.rs"));
    state.toggle_expand(&dir.join("target"));
    assert!(state.has_expanded_folders());
    assert!(state.is_expanded(&dir.join("src")));
    assert!(state.is_expanded(&dir.join("src/sub")));
    assert!(state.is_expanded(&dir.join("target")));
    assert!(names(&state).contains(&row("deep.rs", 2)));
    assert!(!names(&state).iter().any(|(name, _)| name == ".git"));
    state.collapse_all();
    assert!(!state.has_expanded_folders());
    assert_eq!(state.rows().len(), 4);
    assert_eq!(state.selected, Some(dir.join("src/sub/deep.rs")));
    state.collapse_all();
    assert_eq!(state.rows().len(), 4);

    state.toggle_expand(&dir.join("src"));
    assert_eq!(names(&state)[..3], [row("src", 0), row("sub", 1), row("main.rs", 1)]);
    state.toggle_expand(&dir.join("src"));
    assert_eq!(state.rows().len(), 4);

    state.reveal(&dir.join("src/sub/deep.rs"));
    assert_eq!(state.selected, Some(dir.join("src/sub/deep.rs")));
    assert_eq!(names(&state)[..3], [row("src", 0), row("sub", 1), row("deep.rs", 2)]);

    std::fs::write(dir.join("src/sub/new.rs"), "").unwrap();
    std::fs::remove_dir_all(dir.join("target")).unwrap();
    state.refresh();
    assert!(names(&state).contains(&row("new.rs", 2)));
    assert!(!names(&state).contains(&row("target", 0)));

    let _ = std::fs::remove_dir_all(&dir);
}
