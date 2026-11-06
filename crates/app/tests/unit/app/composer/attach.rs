use super::*;
use gpui_kit::{ExternalPaths, ImageFormat};

#[test]
fn text_paste_is_left_to_the_textarea() {
    assert!(matches!(paste_kind(&ClipboardItem::new_string("hi".into())), PasteKind::Text));
}

#[test]
fn image_and_file_pastes_are_taken() {
    let png = Image::from_bytes(ImageFormat::Png, vec![1]);
    assert!(matches!(
        paste_kind(&ClipboardItem::new_image(&png)),
        PasteKind::Image(i) if i.format() == ImageFormat::Png
    ));
    let files =
        ClipboardItem { entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths::default())] };
    assert!(matches!(paste_kind(&files), PasteKind::Files(_)));
}

#[test]
fn batch_stores_files_and_reports_the_first_refusal() {
    let dir = std::env::temp_dir().join(format!("aether-batch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.txt"), dir.join("b.zip"));
    std::fs::write(&a, "hi").unwrap();
    std::fs::write(&b, "zz").unwrap();
    let (stored, err) = store_batch(&dir.join("root"), 3, 0, true, vec![b, a]);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].name, "a.txt");
    assert_eq!(err.as_deref(), Some("b.zip: this chat can only take images, PDFs and text files"));
    let (stored, err) = store_batch(&dir.join("root"), 3, 10, true, vec![dir.join("a.txt")]);
    assert!(stored.is_empty());
    assert_eq!(err.as_deref(), Some("Up to 10 attachments"));
}

#[test]
fn overflow_past_the_cap_is_split_off() {
    let file = |n: usize| Attachment {
        name: n.to_string(),
        path: PathBuf::new(),
        mime: String::new(),
        size: 0,
    };
    let mut stored: Vec<_> = (0..4).map(file).collect();
    let over = split_overflow(&mut stored, 8);
    assert_eq!((stored.len(), over.len()), (2, 2));
    let mut stored = vec![file(0)];
    assert!(split_overflow(&mut stored, 3).is_empty());
}

#[test]
fn a_direct_send_refuses_files_it_cannot_inline() {
    let file = |name: &str, mime: &str, size: u64| Attachment {
        name: name.into(),
        path: name.into(),
        mime: mime.into(),
        size,
    };
    let big = file("big.png", "image/png", 11 * 1024 * 1024);
    let zip = file("a.zip", "application/zip", 1);
    let ok = file("a.png", "image/png", 1);
    assert_eq!(first_refusal(&[ok.clone(), big.clone()], false), None);
    assert_eq!(first_refusal(&[ok.clone(), zip.clone()], false), None);
    assert_eq!(
        first_refusal(&[ok.clone(), big, zip.clone()], true).unwrap(),
        "big.png is over 10 MB"
    );
    assert_eq!(
        first_refusal(&[zip], true).unwrap(),
        "a.zip: this chat can only take images, PDFs and text files"
    );
    assert_eq!(first_refusal(&vec![ok; 11], false).unwrap(), "Up to 10 attachments");
}

#[test]
fn a_leading_bang_marks_a_shell_command() {
    assert_eq!(shell_command("!ls -la"), Some("ls -la"));
    assert_eq!(shell_command("! git status"), Some("git status"));
    assert_eq!(shell_command("!cd a\nls"), Some("cd a\nls"));
    assert_eq!(shell_command("!"), None);
    assert_eq!(shell_command("!  "), None);
    assert_eq!(shell_command("fix this!"), None);
    assert_eq!(shell_command(""), None);
}

#[test]
fn only_cli_commands_are_detected_by_their_leading_token() {
    let names = ["review", "init"];
    let is = |t: &str| is_cli_command(t, names.into_iter());
    assert!(is("/review"));
    assert!(is("  /review check this"));
    assert!(is("/init\nmore"));
    assert!(!is("/reviewer"));
    assert!(!is("please /review"));
    assert!(!is("review"));
    assert!(!is(""));
}
