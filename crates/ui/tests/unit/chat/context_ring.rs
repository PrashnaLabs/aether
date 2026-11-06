use super::*;

#[test]
fn formats_token_counts_compactly() {
    assert_eq!(format_tokens(950), "950");
    assert_eq!(format_tokens(12_345), "12.3k");
    assert_eq!(format_tokens(200_000), "200k");
    assert_eq!(format_tokens(1_048_576), "1M");
    assert_eq!(format_tokens(2_500_000), "2.5M");
}
