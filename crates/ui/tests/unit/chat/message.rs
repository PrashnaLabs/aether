#[test]
fn user_message_renders_its_attachment_strip() {
    let src = include_str!("../../../src/chat/message.rs");
    assert!(src.contains("attachment_strip("));
}

#[test]
fn user_message_collapses_long_text_with_show_more() {
    let src = include_str!("../../../src/chat/message.rs");
    assert!(src.contains("lines().count()"));
    assert!(src.contains("\"Show more\""));
    assert!(src.contains("\"Show less\""));
    assert!(src.contains("-expanded"));
}
