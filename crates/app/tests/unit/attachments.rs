use super::*;

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aether-att-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn stored_files_get_numbered_copies_under_their_chat() {
    let root = tmp("store");
    let src = root.join("in.txt");
    fs::write(&src, "hi").unwrap();
    let a = store_file(&root, 7, &src).unwrap();
    let b = store_file(&root, 7, &src).unwrap();
    assert_eq!(a.path, root.join("7").join("1-in.txt"));
    assert_eq!(b.path, root.join("7").join("2-in.txt"));
    assert_eq!((a.name.as_str(), a.mime.as_str(), a.size), ("in.txt", "text/plain", 2));
}

#[test]
fn tiff_is_stored_as_png() {
    let root = tmp("tiff");
    let mut bytes = Vec::new();
    image::RgbaImage::new(1, 1)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Tiff)
        .unwrap();
    let a = store_bytes(&root, 1, "shot.tiff", &bytes).unwrap();
    assert_eq!(a.mime, "image/png");
    assert_eq!(a.name, "shot.png");
    assert!(a.path.extension().is_some_and(|e| e == "png"));
}

#[test]
fn bmp_is_stored_as_png() {
    let root = tmp("bmp");
    let mut bytes = Vec::new();
    image::RgbaImage::new(1, 1)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Bmp)
        .unwrap();
    let a = store_bytes(&root, 1, "shot.bmp", &bytes).unwrap();
    assert_eq!(a.mime, "image/png");
    assert_eq!(a.name, "shot.png");
}

#[test]
fn oversized_tiff_is_rejected_before_decoding() {
    let root = tmp("bomb");
    let mut bytes = Vec::new();
    image::RgbaImage::new(16_385, 1)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Tiff)
        .unwrap();
    assert!(store_bytes(&root, 1, "bomb.tiff", &bytes).is_err());
    assert!(!root.join("1").exists());
}

#[test]
fn sweep_keeps_referenced_files_and_unreadable_chats() {
    let root = tmp("sweep");
    for (chat, file) in [("1", "1-a.txt"), ("1", "2-b.txt"), ("2", "1-c.txt"), ("3", "1-d.txt")] {
        fs::create_dir_all(root.join(chat)).unwrap();
        fs::write(root.join(chat).join(file), "x").unwrap();
    }
    let kept = HashSet::from([root.join("1").join("1-a.txt")]);
    sweep(&root, &HashSet::from([1, 2]), &HashSet::from([1]), &kept);
    assert!(root.join("1").join("1-a.txt").exists());
    assert!(!root.join("1").join("2-b.txt").exists());
    assert!(root.join("2").join("1-c.txt").exists());
    assert!(!root.join("3").exists());
}

#[test]
fn forks_get_their_own_copies() {
    let root = tmp("fork");
    let src = root.join("in.txt");
    fs::write(&src, "hi").unwrap();
    let a = store_file(&root, 1, &src).unwrap();
    let copies = copy_all(&root, 9, &[a]);
    assert_eq!(copies[0].path, root.join("9").join("1-in.txt"));
    assert!(copies[0].path.exists());
}

#[test]
fn pasted_images_are_numbered() {
    assert_eq!(pasted_name(3, "png"), "Pasted image 3.png");
}

#[test]
fn numbers_are_never_reused_after_a_removal() {
    let root = tmp("reuse");
    let src = root.join("in.txt");
    fs::write(&src, "one").unwrap();
    let a = store_file(&root, 1, &src).unwrap();
    fs::write(&src, "two").unwrap();
    let b = store_file(&root, 1, &src).unwrap();
    fs::remove_file(&a.path).unwrap();
    fs::write(&src, "three").unwrap();
    let c = store_file(&root, 1, &src).unwrap();
    assert_eq!(fs::read_to_string(&b.path).unwrap(), "two");
    assert_ne!(c.path, b.path);
    assert_eq!(fs::read_to_string(&c.path).unwrap(), "three");
}
