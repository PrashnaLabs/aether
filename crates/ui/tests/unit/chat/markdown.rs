use super::*;

#[test]
fn links_resolve_only_when_settled_with_backticks_and_cwd() {
    assert!(wants_link(false, true, false, "see `a.rs`", true));
    assert!(!wants_link(true, true, false, "see `a.rs`", true));
    assert!(!wants_link(false, false, false, "see `a.rs`", true));
    assert!(!wants_link(false, true, true, "see `a.rs`", true));
    assert!(!wants_link(false, true, false, "plain", true));
    assert!(!wants_link(false, true, false, "see `a.rs`", false));
}

#[test]
fn code_blocks_are_uncapped_unless_the_container_requests_a_limit() {
    let normal = Markdown::new("normal", "```rust\nlet x = 1;\n```".into());
    let style = code_block_style("monospace".into(), normal.code_block_max_height);
    assert!(style.max_size.height.is_none());
    assert!(style.overflow.y.is_none());
    let thinking = normal.code_block_max_height(px(340.));
    let style = code_block_style("monospace".into(), thinking.code_block_max_height);
    assert_eq!(style.max_size.height, Some(px(340.).into()));
    assert_eq!(style.overflow.y, Some(Overflow::Scroll));
}

#[test]
fn oversized_messages_are_clipped_for_view() {
    let short = "hello\nworld";
    assert_eq!(clip_for_view(short), short);
    let big = "line of text\n".repeat(30_000);
    let clipped = clip_for_view(&big);
    assert!(clipped.len() < 256 * 1024 + 100);
    assert!(clipped.ends_with("KB more)*"));
    assert!(clipped.contains("line of text\n\n*… message truncated in view"));
    let wide = "é".repeat(200_000);
    assert!(clip_for_view(&wide).starts_with('é'));
}
