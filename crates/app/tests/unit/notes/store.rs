use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    store: Store,
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "aether-notes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let dir = fs::canonicalize(dir).unwrap();
        let store = Store::new(dir.join("notes")).with_last_note_file(dir.join("last"));
        Self { store, dir }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
#[test]
fn layout_templates_and_preserving_existing_content() {
    let f = Fixture::new();
    let s = &f.store;
    fs::create_dir_all(s.root().join(".templates")).unwrap();
    fs::write(s.root().join(".templates/template.md"), "template\n").unwrap();
    fs::write(s.root().join(".templates/rfc.md"), "RFC\n").unwrap();
    let path = s.create(NoteKind::Named, "hello", None, "exact\n text").unwrap();
    assert_eq!(path, s.root().join("misc/hello.md"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "template\nexact\n text");
    s.create(NoteKind::Named, "hello", None, "ignored").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "template\nexact\n text");
    let doc = s.create(NoteKind::Docs, "design/api", Some("rfc"), "").unwrap();
    assert_eq!(fs::read_to_string(doc).unwrap(), "RFC\n");
    let project = s.scoped(Some("aether")).unwrap();
    assert_eq!(
        project.create(NoteKind::Named, "hello", None, "").unwrap(),
        s.root().join("projects/aether/hello.md")
    );
    assert!(
        s.create(NoteKind::Credentials, "aws", None, "").unwrap().ends_with("creds/aws.creds.md")
    );
    assert!(
        s.create(NoteKind::Password, "aws", None, "").unwrap().ends_with("creds/aws.password.md")
    );
}
#[test]
fn daily_scratchpad_and_timestamp_collision() {
    let f = Fixture::new();
    let s = &f.store;
    let daily = s.create(NoteKind::Daily, "", None, "today").unwrap();
    assert_eq!(s.create(NoteKind::Daily, "", None, "ignored").unwrap(), daily);
    assert_eq!(fs::read_to_string(daily).unwrap(), "today");
    let scratch = s.create(NoteKind::Scratchpad, "", None, "first").unwrap();
    s.create(NoteKind::Scratchpad, "", None, "second").unwrap();
    let text = fs::read_to_string(scratch).unwrap();
    assert_eq!(text.matches("### ").count(), 2);
    assert!(text.starts_with("\n### "));
    assert!(text.find("### ").unwrap() < text.find("first").unwrap());
    assert!(text.contains("first"));
    assert!(text.contains("second"));
    let a = s.create(NoteKind::Timestamped, "", None, "a").unwrap();
    let b = s.create(NoteKind::Timestamped, "", None, "b").unwrap();
    assert_ne!(a, b);
}
#[test]
fn discovery_search_and_recent() {
    let f = Fixture::new();
    let s = &f.store;
    s.create(NoteKind::Named, "one", None, "tags: rust, backend\n[[target]]\nNeedle").unwrap();
    s.create(NoteKind::Named, "two", None, "tags: rusty\nother").unwrap();
    assert_eq!(
        s.list(&NoteQuery {
            content: "needle".into(),
            tag: "rust".into(),
            backlink: "target.md".into(),
            ..Default::default()
        })
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        s.list(&NoteQuery { filename: "two".into(), ..Default::default() }).unwrap().len(),
        1
    );
    assert_eq!(s.list(&NoteQuery { recent: Some(1), ..Default::default() }).unwrap().len(), 1);
}
#[test]
fn trash_restore_preserves_folders_and_refuses_collisions() {
    let f = Fixture::new();
    let s = &f.store;
    let path = s.create(NoteKind::Named, "deep/note", None, "original").unwrap();
    let trash = s.trash(&path).unwrap();
    assert!(!path.exists());
    assert_eq!(s.trash_list().unwrap(), vec![trash.clone()]);
    s.create(NoteKind::Named, "deep/note", None, "conflict").unwrap();
    assert!(s.restore(&trash).is_err());
    assert!(trash.exists());
    fs::remove_file(&path).unwrap();
    assert_eq!(s.restore(&trash).unwrap(), path);
    assert_eq!(fs::read_to_string(path).unwrap(), "original");
    let legacy = s.root().join(".trash/20260101_123456_old.md");
    fs::write(&legacy, "legacy").unwrap();
    assert_eq!(s.restore(&legacy).unwrap(), s.root().join("misc/old.md"));
}
#[test]
fn last_rename_archive_and_empty_cleanup() {
    let f = Fixture::new();
    let s = &f.store;
    let path = s.create(NoteKind::Named, "one", None, "keep").unwrap();
    s.remember(&path).unwrap();
    let moved = s.rename(&path, "folder/two").unwrap();
    assert_eq!(s.last().unwrap(), moved);
    assert_eq!(s.archive(&moved).unwrap(), s.root().join("archive/folder/two.md"));
    s.create(NoteKind::Named, "empty", None, "").unwrap();
    assert_eq!(s.clean_empty().unwrap(), 1);
    assert!(s.list(&NoteQuery::default()).unwrap().is_empty());
}
#[test]
fn traversal_and_symlinks_are_rejected() {
    let f = Fixture::new();
    let s = &f.store;
    assert!(s.scoped(Some("../escape")).is_err());
    assert!(s.create(NoteKind::Named, "../escape", None, "").is_err());
    assert!(s.create(NoteKind::Docs, "/tmp/escape", None, "").is_err());
    #[cfg(unix)]
    {
        fs::create_dir_all(s.root()).unwrap();
        std::os::unix::fs::symlink(&f.dir, s.root().join("link")).unwrap();
        assert!(s.create(NoteKind::Named, "link/escape", None, "").is_err());
    }
}

#[test]
fn capture_appends_exact_text_to_existing_note() {
    let f = Fixture::new();
    let path = f.store.create(NoteKind::Named, "capture", None, "existing").unwrap();
    let selection = "\n  selected\r\ntext\t";
    assert_eq!(f.store.capture("capture", selection).unwrap(), path);
    assert_eq!(fs::read_to_string(path).unwrap(), format!("existing{selection}"));
    let new = f.store.capture("new", selection).unwrap();
    assert_eq!(fs::read_to_string(new).unwrap(), selection);
}

#[test]
#[cfg(unix)]
fn missing_notes_root_resolves_existing_symlink_ancestry() {
    let f = Fixture::new();
    let alias = f.dir.join("alias");
    let actual = f.dir.join("actual");
    fs::create_dir(&actual).unwrap();
    std::os::unix::fs::symlink(&actual, &alias).unwrap();
    let store = Store::new(alias.join("missing/notes"));
    let path = store.create(NoteKind::Named, "works", None, "text").unwrap();
    assert_eq!(path, actual.join("missing/notes/misc/works.md"));
    assert!(store.create(NoteKind::Named, "../../escape", None, "bad").is_err());
    // Links introduced inside the tree must still be rejected.
    std::os::unix::fs::symlink(&f.dir, store.root().join("outside")).unwrap();
    assert!(store.capture("outside/escape", "bad").is_err());
}

#[test]
fn empty_trash_removes_notes_and_metadata() {
    let f = Fixture::new();
    assert_eq!(f.store.empty_trash().unwrap(), 0);
    let note = f.store.create(NoteKind::Named, "discard", None, "text").unwrap();
    f.store.trash(&note).unwrap();
    fs::write(f.store.root().join(".trash/legacy.md"), "legacy").unwrap();
    assert_eq!(f.store.empty_trash().unwrap(), 2);
    assert_eq!(fs::read_dir(f.store.root().join(".trash")).unwrap().count(), 0);
}

#[test]
#[cfg(unix)]
fn empty_trash_never_follows_symlinks() {
    let f = Fixture::new();
    let outside = f.dir.join("outside.md");
    fs::write(&outside, "keep").unwrap();
    fs::create_dir_all(f.store.root().join(".trash")).unwrap();
    std::os::unix::fs::symlink(&outside, f.store.root().join(".trash/link.md")).unwrap();
    assert_eq!(f.store.empty_trash().unwrap(), 0);
    assert_eq!(fs::read_to_string(&outside).unwrap(), "keep");
    fs::remove_dir_all(f.store.root().join(".trash")).unwrap();
    std::os::unix::fs::symlink(&f.dir, f.store.root().join(".trash")).unwrap();
    assert!(f.store.empty_trash().is_err());
    assert!(outside.exists());
}
