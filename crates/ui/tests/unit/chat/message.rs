#[test]
fn user_message_renders_its_attachment_strip() {
    let src = include_str!("../../../src/chat/message.rs");
    assert!(src.contains("attachment_strip("));
}
