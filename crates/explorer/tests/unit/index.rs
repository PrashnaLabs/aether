use super::*;

#[test]
fn from_files_precomputes_lowercase_paths() {
    let index = FileIndex::from_files(
        vec!["README.md".to_string(), "SRC/App.RS".to_string()],
        HashSet::new(),
    );
    assert_eq!(&*index.files, ["README.md", "SRC/App.RS"]);
    assert_eq!(&*index.lower_files, ["readme.md", "src/app.rs"]);
}

#[test]
fn nul_entries_trim_folder_slashes_and_drop_a_cut_off_tail() {
    let entries: Vec<&str> = nul_entries("src/main.rs\0target/\0.env\0").collect();
    assert_eq!(entries, ["src/main.rs", "target", ".env"]);
    let cut: Vec<&str> = nul_entries("a.rs\0b.r").collect();
    assert_eq!(cut, ["a.rs"]);
    assert_eq!(nul_entries("").count(), 0);
}

#[test]
fn walk_skips_build_and_vcs_folders() {
    let dir = std::env::temp_dir().join(format!(
        "aether_test_index_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    for sub in ["src", ".git", "target", "node_modules"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        std::fs::write(dir.join(sub).join("f.txt"), "").unwrap();
    }
    std::fs::write(dir.join("README.md"), "").unwrap();

    let mut files = walk_files(&dir);
    files.sort();
    assert_eq!(files, ["README.md", "src/f.txt"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn walk_skips_symlinks_to_outside_files_and_dirs() {
    use std::os::unix::fs::symlink;
    let tag =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("aether_test_index_link_{tag}"));
    let outside = std::env::temp_dir().join(format!("aether_test_index_outside_{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(dir.join("real.txt"), "ok").unwrap();
    std::fs::write(outside.join("secret.txt"), "secret").unwrap();
    symlink(outside.join("secret.txt"), dir.join("link.txt")).unwrap();
    symlink(&outside, dir.join("linkdir")).unwrap();
    let files = walk_files(&dir);
    assert_eq!(files, vec!["real.txt"]);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
}
