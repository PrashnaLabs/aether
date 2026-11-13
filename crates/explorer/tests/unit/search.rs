use super::*;

fn lower(query: &str, candidate: &str) -> Option<i64> {
    fuzzy_match_path(&query.to_lowercase(), &candidate.to_lowercase(), candidate.len())
}

#[test]
fn fuzzy_matching_prefers_exact_basename_and_contiguous_matches() {
    assert!(lower("app", "crates/app/src/app.rs").is_some());
    assert!(lower("gpui", "crates/ui/src/lib.rs").is_none());

    let score_direct = lower("app", "crates/app/src/app.rs").unwrap();
    let score_sparse = lower("app", "crates/agent/src/process_pipe.rs").unwrap();
    assert!(score_direct > score_sparse);
}

#[test]
fn fuzzy_matching_is_case_insensitive_via_precomputed_lowercase() {
    assert!(lower("APP", "Crates/App/SRC/App.RS").is_some());
    assert_eq!(lower("app", "SRC/App.RS"), lower("app", "src/app.rs"));
}

#[test]
fn quick_open_matches_uses_precomputed_lowercase_paths() {
    let root = Path::new("/r");
    let files = vec!["README.md".to_string(), "SRC/App.RS".to_string()];
    let lower_files: Vec<String> = files.iter().map(|f| f.to_lowercase()).collect();
    let items = quick_open_matches(root, &files, &lower_files, "app");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].rel_path, "SRC/App.RS");
    assert_eq!(items[0].path, root.join("SRC/App.RS"));
}

fn opts(query: &str, case_sensitive: bool, whole_word: bool, is_regex: bool) -> GrepOptions {
    GrepOptions { query: query.to_string(), case_sensitive, whole_word, is_regex }
}

#[test]
fn grep_finds_lines_with_correct_numbers_and_spans() {
    let content = "use crate::app;\npub fn new_chat_in() {}\nlet x = 1;\n";
    let re = grep_regex(&opts("new_chat_in", true, true, false)).unwrap();
    let matches = search_file_content(Path::new("app.rs"), content, &re);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line_number, 2);
    assert_eq!(matches[0].line_text, "pub fn new_chat_in() {}");
    assert_eq!(matches[0].match_indices, (7, 18));
}

#[test]
fn grep_respects_case_insensitivity() {
    let content = "Hello World\nhello world\n";
    let re = grep_regex(&opts("hello", false, false, false)).unwrap();
    assert_eq!(search_file_content(Path::new("test.txt"), content, &re).len(), 2);
}

#[test]
fn grep_files_reads_the_index_and_stops_at_the_limit() {
    let dir = std::env::temp_dir().join(format!(
        "aether_test_grep_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();
    std::fs::write(dir.join("b.rs"), "xzy(\nx.y(\n").unwrap();
    let files = ["a.rs".to_string(), "b.rs".to_string(), "gone.rs".to_string()];

    assert_eq!(grep_files(&dir, &files, &opts("fn", true, false, false), 2).len(), 2);
    assert_eq!(grep_files(&dir, &files, &opts(r"fn \w\(", true, false, true), 500).len(), 3);
    let bad_regex = grep_files(&dir, &files, &opts("x.y(", true, false, true), 500);
    assert_eq!(bad_regex.len(), 1);
    assert_eq!((bad_regex[0].path.clone(), bad_regex[0].line_number), (dir.join("b.rs"), 2));
    assert!(grep_files(&dir, &files, &opts("  ", true, false, false), 500).is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn grep_files_skips_symlinks_and_non_regular_files() {
    let dir = std::env::temp_dir().join(format!(
        "aether_test_grep_symlink_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("real.rs"), "fn target() {}\n").unwrap();
    std::os::unix::fs::symlink(dir.join("real.rs"), dir.join("link.rs")).unwrap();
    let files = ["real.rs".to_string(), "link.rs".to_string()];
    let matches = grep_files(&dir, &files, &opts("target", true, false, false), 500);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].path, dir.join("real.rs"));
    let _ = std::fs::remove_dir_all(&dir);
}
