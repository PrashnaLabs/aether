use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aether_test_{name}_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn open_refuses_binary_and_large_files() {
    let dir = temp_dir("buffer_kinds");
    let (text, binary, large) = (dir.join("a.rs"), dir.join("a.png"), dir.join("big.log"));
    std::fs::write(&text, "fn main() {}\n").unwrap();
    std::fs::write(&binary, b"\x89PNG\0\0").unwrap();
    std::fs::File::create(&large).unwrap().set_len(MAX_LEN + 1).unwrap();

    let buffer = EditorBuffer::open(text).unwrap();
    assert_eq!(buffer.original, "fn main() {}\n");
    assert_eq!(buffer.detect_language(), "Rust");
    assert_eq!(EditorBuffer::open(binary).unwrap_err().kind(), ErrorKind::InvalidData);
    assert_eq!(EditorBuffer::open(large).unwrap_err().kind(), ErrorKind::FileTooLarge);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn disk_stamp_changes_with_the_file() {
    let dir = temp_dir("buffer_stamp");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one").unwrap();
    let before = disk_stamp(&file);
    assert!(before.is_some());
    assert_eq!(disk_stamp(&file), before);
    std::fs::write(&file, "three").unwrap();
    assert_ne!(disk_stamp(&file), before);
    std::fs::remove_file(&file).unwrap();
    assert_eq!(disk_stamp(&file), None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_disk_reloads_clean_tabs_and_flags_dirty_or_deleted_ones() {
    let dir = temp_dir("buffer_check");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one").unwrap();
    let mut buffer = EditorBuffer::open(file.clone()).unwrap();
    assert_eq!(buffer.check_disk(), None);

    std::fs::write(&file, "agent edit").unwrap();
    assert_eq!(buffer.check_disk().as_deref(), Some("agent edit"));
    assert_eq!((buffer.original.as_str(), buffer.conflict), ("agent edit", None));

    buffer.is_dirty = true;
    std::fs::write(&file, "another edit").unwrap();
    assert_eq!(buffer.check_disk(), None);
    assert_eq!(buffer.conflict, Some(DiskConflict::Changed));
    buffer.keep_mine();
    assert_eq!(buffer.check_disk(), None);
    assert_eq!(buffer.conflict, None);

    std::fs::remove_file(&file).unwrap();
    assert_eq!(buffer.check_disk(), None);
    assert_eq!(buffer.conflict, Some(DiskConflict::Deleted));

    buffer.save("mine".into()).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
    assert_eq!((buffer.is_dirty, buffer.conflict), (false, None));
    assert_eq!(buffer.check_disk(), None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn racy_covers_recent_and_future_mtimes() {
    let now = SystemTime::now();
    assert!(is_racy(now, now));
    assert!(is_racy(now - Duration::from_millis(1999), now));
    assert!(!is_racy(now - RACY, now));
    assert!(is_racy(now + Duration::from_secs(5), now));
}

/// A same-length write inside one mtime tick, as the stamp sees it.
fn rewrite_keeping_mtime(file: &Path, text: &str, mtime: SystemTime) {
    std::fs::write(file, text).unwrap();
    std::fs::File::options().write(true).open(file).unwrap().set_modified(mtime).unwrap();
}

#[test]
fn racy_stamp_sees_a_same_length_rewrite_right_after_open() {
    let dir = temp_dir("buffer_racy");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one").unwrap();
    let mut buffer = EditorBuffer::open(file.clone()).unwrap();
    let stamp = buffer.stamp.unwrap();
    assert!(stamp.racy);

    rewrite_keeping_mtime(&file, "two", stamp.mtime);
    assert_eq!(disk_stamp(&file), Some(stamp));
    assert_eq!(buffer.check_disk().as_deref(), Some("two"));

    buffer.is_dirty = true;
    rewrite_keeping_mtime(&file, "six", stamp.mtime);
    assert_eq!(buffer.check_disk(), None);
    assert_eq!(buffer.conflict, Some(DiskConflict::Changed));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn settled_stamp_skips_the_read_and_racy_settles_after_a_match() {
    let dir = temp_dir("buffer_settled");
    let file = dir.join("a.txt");
    let old = SystemTime::now() - Duration::from_secs(60);
    rewrite_keeping_mtime(&file, "one", old);
    let mut buffer = EditorBuffer::open(file.clone()).unwrap();
    assert!(!buffer.stamp.unwrap().racy);

    rewrite_keeping_mtime(&file, "two", old);
    assert_eq!(buffer.check_disk(), None);
    assert_eq!(buffer.original, "one");

    buffer.reload().unwrap();
    buffer.stamp.as_mut().unwrap().racy = true;
    assert_eq!(buffer.check_disk(), None);
    assert!(!buffer.stamp.unwrap().racy);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keep_mine_takes_the_disk_text_as_baseline() {
    let dir = temp_dir("buffer_keep");
    let file = dir.join("a.txt");
    std::fs::write(&file, "one").unwrap();
    let mut buffer = EditorBuffer::open(file.clone()).unwrap();
    buffer.is_dirty = true;
    std::fs::write(&file, "agent edit").unwrap();
    assert_eq!(buffer.check_disk(), None);

    buffer.keep_mine();
    assert_eq!((buffer.original.as_str(), buffer.conflict), ("agent edit", None));
    assert_eq!(buffer.stamp, disk_stamp(&file));

    std::fs::remove_file(&file).unwrap();
    buffer.check_disk();
    buffer.keep_mine();
    assert_eq!((buffer.original.as_str(), buffer.stamp), ("agent edit", None));

    let _ = std::fs::remove_dir_all(&dir);
}
