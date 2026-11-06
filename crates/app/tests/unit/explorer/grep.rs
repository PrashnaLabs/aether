use super::*;

#[test]
fn match_byte_offsets_become_char_columns() {
    assert_eq!(char_col("let x = 1;", 4), 5);
    assert_eq!(char_col("é = needle", 5), 5);
    assert_eq!(char_col("🦀 x", 5), 3);
    assert_eq!(char_col("", 0), 1);
}
