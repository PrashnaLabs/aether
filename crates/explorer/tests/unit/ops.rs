use super::*;

#[test]
fn file_ops_never_clobber_and_reject_paths_as_names() {
    let dir = std::env::temp_dir().join(format!(
        "aether_test_ops_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    let file = create_file(&dir, "a.txt").unwrap();
    std::fs::write(&file, "keep").unwrap();
    assert_eq!(create_file(&dir, "a.txt").unwrap_err().kind(), ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep");

    let folder = create_folder(&dir, "sub").unwrap();
    assert!(folder.is_dir());
    assert!(create_folder(&dir, "sub").is_err());

    for bad in ["", "..", "x/y", "/etc/z"] {
        assert_eq!(create_file(&dir, bad).unwrap_err().kind(), ErrorKind::InvalidInput);
    }

    std::fs::write(dir.join("b.txt"), "other").unwrap();
    assert_eq!(rename(&file, "b.txt").unwrap_err().kind(), ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "other");
    let moved = rename(&file, "c.txt").unwrap();
    assert_eq!(moved, dir.join("c.txt"));
    assert_eq!(std::fs::read_to_string(&moved).unwrap(), "keep");
    assert!(!file.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn case_only_rename_works_on_either_kind_of_disk() {
    let dir = std::env::temp_dir().join(format!(
        "aether_test_ops_case_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = create_file(&dir, "Foo.txt").unwrap();
    let insensitive = dir.join("foo.txt").exists();

    assert_eq!(rename(&file, "foo.txt").unwrap(), dir.join("foo.txt"));
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["foo.txt"]);
    assert_eq!(dir.join("Foo.txt").exists(), insensitive);

    std::fs::hard_link(dir.join("foo.txt"), dir.join("link.txt")).unwrap();
    assert!(rename(&dir.join("foo.txt"), "link.txt").is_err());

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn trash_sends_files_and_folders_to_the_recycle_bin() {
    let dir = std::env::temp_dir().join(format!("aether-trash-{}", std::process::id()));
    let folder = dir.join("it's a folder");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("inner.txt"), "x").unwrap();
    let file = dir.join("a file.txt");
    std::fs::write(&file, "x").unwrap();
    trash(&file).unwrap();
    trash(&folder).unwrap();
    assert!(!file.exists() && !folder.exists());
    assert!(trash(&dir.join("missing.txt")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
