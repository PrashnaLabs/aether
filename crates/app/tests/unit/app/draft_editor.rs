use super::*;

#[test]
fn end_position_counts_lines_and_utf16() {
    let p = end_position("a\néé");
    assert_eq!((p.line, p.character), (1, 2));
    let p = end_position("😀");
    assert_eq!((p.line, p.character), (0, 2));
    let p = end_position("");
    assert_eq!((p.line, p.character), (0, 0));
}
