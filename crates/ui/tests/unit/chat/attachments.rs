use super::*;

#[test]
fn sizes_read_like_finder() {
    assert_eq!(format_size(512), "512 B");
    assert_eq!(format_size(1_250), "1.2 KB");
    assert_eq!(format_size(3_500_000), "3.3 MB");
}

#[test]
fn missing_files_say_so() {
    let mut v = AttachmentView {
        name: "a.png".into(),
        path: "/x/a.png".into(),
        image: true,
        size: 1_250,
        missing: false,
    };
    assert_eq!(attachment_caption(&v), "1.2 KB");
    v.missing = true;
    assert_eq!(attachment_caption(&v), "File missing");
}
