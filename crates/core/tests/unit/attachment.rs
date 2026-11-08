use super::*;
use crate::Message;

fn att(name: &str, mime: &str, size: u64) -> Attachment {
    Attachment {
        name: name.into(),
        path: PathBuf::from("/c/attachments/1").join(name),
        mime: mime.into(),
        size,
    }
}

#[test]
fn limits_and_direct_chat_types_are_checked_on_add() {
    assert_eq!(
        check_attachment(10, "a.png", 1, "image/png", false),
        Err("Up to 10 attachments".into())
    );
    assert_eq!(
        check_attachment(0, "big.mov", 21 * 1024 * 1024, "application/octet-stream", false),
        Err("big.mov is over 20 MB".into())
    );
    assert_eq!(
        check_attachment(0, "x.zip", 10, "application/zip", true),
        Err("x.zip: this chat can only take images, PDFs and text files".into())
    );
    assert_eq!(
        check_attachment(0, "shot.png", 15 * 1024 * 1024, "image/png", true),
        Err("shot.png is over 10 MB".into())
    );
    assert_eq!(check_attachment(9, "x.zip", 10, "application/zip", false), Ok(()));
}

#[test]
fn mime_comes_from_the_extension() {
    assert_eq!(mime_for(Path::new("a/Shot.PNG")), "image/png");
    assert_eq!(mime_for(Path::new("doc.pdf")), "application/pdf");
    assert_eq!(mime_for(Path::new("main.rs")), "text/plain");
    assert_eq!(mime_for(Path::new("blob")), "application/octet-stream");
}

#[test]
fn claude_takes_small_images_natively_and_the_rest_by_path() {
    let (small, big, pdf) = (
        att("a.png", "image/png", 1024),
        att("b.png", "image/png", 4 * 1024 * 1024),
        att("c.pdf", "application/pdf", 10),
    );
    let (native, paths) =
        native_image_split(Provider::Claude, &[small.clone(), big.clone(), pdf.clone()]);
    assert_eq!(native, vec![small.clone()]);
    assert_eq!(paths, vec![big.clone(), pdf.clone()]);
    let (native, paths) = native_image_split(Provider::Antigravity, &[small.clone(), pdf.clone()]);
    assert!(native.is_empty());
    assert_eq!(paths, vec![small.clone(), pdf.clone()]);
    let tiff = att("t.tiff", "image/tiff", 1);
    let (native, paths) = native_image_split(Provider::Claude, &[tiff.clone()]);
    assert!(native.is_empty() && paths == vec![tiff.clone()]);
    let (native, paths) =
        native_image_split(Provider::Codex, &[big.clone(), tiff.clone(), pdf.clone()]);
    assert_eq!(native, vec![big]);
    assert_eq!(paths, vec![tiff, pdf]);
}

#[test]
fn codex_takes_only_web_images_natively() {
    let files = [
        att("a.png", "image/png", 1),
        att("b.jpg", "image/jpeg", 1),
        att("c.gif", "image/gif", 1),
        att("d.webp", "image/webp", 1),
        att("e.heic", "image/heic", 1),
        att("f.bmp", "image/bmp", 1),
    ];
    let (native, paths) = native_image_split(Provider::Codex, &files);
    assert_eq!(native, files[..4]);
    assert_eq!(paths, files[4..]);
}

#[test]
fn pi_takes_only_web_images_natively() {
    let files = [
        att("a.png", "image/png", 1),
        att("b.webp", "image/webp", 1),
        att("c.heic", "image/heic", 1),
        att("d.pdf", "application/pdf", 1),
    ];
    let (native, paths) = native_image_split(Provider::Pi, &files);
    assert_eq!(native, files[..2]);
    assert_eq!(paths, files[2..]);
}

#[test]
fn path_block_lists_each_file() {
    assert_eq!(
        attached_files_block(&[att("a.png", "image/png", 3)]),
        format!(
            "\n\nAttached files:\n- {} (image/png)",
            PathBuf::from("/c/attachments/1").join("a.png").display()
        )
    );
    assert_eq!(attached_files_block(&[]), "");
}

#[test]
fn claude_stops_taking_images_natively_past_20_mb_a_message() {
    let cap = CLAUDE_IMAGE_BYTES;
    let files = [
        att("a.png", "image/png", cap),
        att("b.png", "image/png", cap),
        att("c.png", "image/png", cap),
        att("d.png", "image/png", cap),
        att("e.png", "image/png", cap),
        att("f.png", "image/png", cap),
        att("g.png", "image/png", 1024 * 1024),
    ];
    let (native, paths) = native_image_split(Provider::Claude, &files);
    assert_eq!(native, files[..5]);
    assert_eq!(paths, files[5..]);
    let (native, paths) = native_image_split(Provider::Codex, &files);
    assert_eq!((native.len(), paths.len()), (7, 0));
}

#[test]
fn title_text_drops_the_path_block() {
    let block = attached_files_block(&[att("a.png", "image/png", 3)]);
    assert_eq!(without_attached_files(&format!("fix this{block}")), "fix this");
    assert_eq!(without_attached_files(&block), "");
    assert_eq!(without_attached_files("plain"), "plain");
}

#[test]
fn size_limits_are_inclusive_at_the_boundary() {
    let mb = 1024 * 1024;
    let ok = |size, mime, direct| check_attachment(0, "f", size, mime, direct);
    assert_eq!(ok(20 * mb, "application/octet-stream", false), Ok(()));
    assert_eq!(ok(20 * mb + 1, "application/octet-stream", false), Err("f is over 20 MB".into()));
    assert_eq!(ok(10 * mb, "image/png", true), Ok(()));
    assert_eq!(ok(10 * mb + 1, "image/png", true), Err("f is over 10 MB".into()));
}

#[test]
fn tenth_attachment_fits_and_eleventh_does_not() {
    assert_eq!(check_attachment(9, "a", 1, "image/png", false), Ok(()));
    assert_eq!(
        check_attachment(10, "a", 1, "image/png", false),
        Err("Up to 10 attachments".into())
    );
}

#[test]
fn claude_image_cap_is_inclusive() {
    let at = att("a.png", "image/png", CLAUDE_IMAGE_BYTES);
    let over = att("b.png", "image/png", CLAUDE_IMAGE_BYTES + 1);
    let (native, paths) = native_image_split(Provider::Claude, &[at.clone(), over.clone()]);
    assert_eq!(native, vec![at]);
    assert_eq!(paths, vec![over]);
}

#[test]
fn claude_message_total_cap_is_inclusive() {
    let exact = [
        att("a.png", "image/png", CLAUDE_IMAGE_BYTES),
        att("b.png", "image/png", CLAUDE_IMAGE_BYTES),
        att("c.png", "image/png", CLAUDE_IMAGE_BYTES),
        att("d.png", "image/png", CLAUDE_IMAGE_BYTES),
        att("e.png", "image/png", CLAUDE_IMAGE_BYTES),
        att("f.png", "image/png", CLAUDE_MESSAGE_IMAGE_BYTES - 5 * CLAUDE_IMAGE_BYTES),
    ];
    let (native, paths) = native_image_split(Provider::Claude, &exact);
    assert_eq!((native.len(), paths.len()), (6, 0));
    let mut over = exact.to_vec();
    over.push(att("g.png", "image/png", 1));
    let (native, paths) = native_image_split(Provider::Claude, &over);
    assert_eq!((native.len(), paths.len()), (6, 1));
}

#[test]
fn message_with_attachments_round_trips() {
    let mut m = Message::user("look");
    m.attachments = vec![att("a.png", "image/png", 3), att("b.pdf", "application/pdf", 9)];
    let back: Message = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back.attachments, m.attachments);
    assert_eq!(back.text, "look");
}

#[test]
fn mime_table_is_covered() {
    let mime = |p: &str| mime_for(Path::new(p));
    assert_eq!(mime("a.jpg"), "image/jpeg");
    assert_eq!(mime("a.jpeg"), "image/jpeg");
    assert_eq!(mime("a.gif"), "image/gif");
    assert_eq!(mime("a.webp"), "image/webp");
    assert_eq!(mime("a.tif"), "image/tiff");
    assert_eq!(mime("a.tiff"), "image/tiff");
    assert_eq!(mime("a.bmp"), "image/bmp");
    assert_eq!(mime("a.heic"), "image/heic");
    assert_eq!(mime("a.json"), "application/json");
    assert_eq!(mime("a.md"), "text/markdown");
    assert_eq!(mime("a.csv"), "text/csv");
    assert_eq!(mime("a.html"), "text/html");
    for ext in [
        "txt", "rs", "ts", "tsx", "js", "jsx", "py", "toml", "yaml", "yml", "sh", "go", "swift",
        "java", "c", "h", "cpp", "css", "sql", "log",
    ] {
        assert_eq!(mime(&format!("a.{ext}")), "text/plain", "{ext}");
    }
    assert_eq!(mime("A.JPEG"), "image/jpeg");
    assert_eq!(mime("a.zip"), "application/octet-stream");
}

#[test]
fn opencode_takes_web_images_natively() {
    let png = att("a.png", "image/png", 1);
    let pdf = att("b.pdf", "application/pdf", 1);
    let (native, paths) = native_image_split(Provider::OpenCode, &[png.clone(), pdf.clone()]);
    assert_eq!((native, paths), (vec![png], vec![pdf]));
}
