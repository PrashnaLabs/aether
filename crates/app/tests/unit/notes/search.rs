use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "aether-note-search-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn store(&self) -> Store {
        Store::new(self.0.clone())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn files_are_fuzzy_ranked_and_search_is_global() {
    let f = Fixture::new();
    let store = f.store();
    let exact = store.create(NoteKind::Named, "alpha", None, "# Alpha").unwrap();
    store
        .scoped(Some("project"))
        .unwrap()
        .create(NoteKind::Named, "alpha-guide", None, "")
        .unwrap();
    let results = find(store.scoped(Some("other")).unwrap(), "alpha", Mode::Files).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].path, exact);
    assert_eq!(find(store, "alpgd", Mode::Files).unwrap().len(), 1);
}
#[test]
fn grep_returns_each_matching_line_with_context_preview() {
    let f = Fixture::new();
    let store = f.store();
    store
        .create(NoteKind::Named, "guide", None, "first\nneedle one\nthird\nNEEDLE two\nlast\n")
        .unwrap();
    let hits = find(store, "/s needle", Mode::Files).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].line, Some(2));
    assert_eq!(hits[1].line, Some(4));
    let preview = preview(&hits[1]).unwrap();
    assert_eq!(preview.matched_line, Some(4));
    assert!(preview.text.contains("NEEDLE two"));
    assert!(!preview.markdown);
}
#[test]
fn file_markdown_preview_is_read_only_and_empty_grep_has_no_hits() {
    let f = Fixture::new();
    let store = f.store();
    let path = store.create(NoteKind::Named, "readme", None, "# Document\n").unwrap();
    let hit = find(store.clone(), "readme", Mode::Files).unwrap().remove(0);
    assert!(preview(&hit).unwrap().markdown);
    assert_eq!(fs::read_to_string(path).unwrap(), "# Document\n");
    assert!(find(store, "", Mode::Grep).unwrap().is_empty());
}
