use super::*;

#[test]
fn selection_rows_skip_a_trailing_line_start() {
    let text = Rope::from("one\ntwo\nthree\n");
    assert_eq!(selected_rows(&text, 0..2), (0, 0));
    assert_eq!(selected_rows(&text, 1..9), (0, 2));
    assert_eq!(selected_rows(&text, 4..8), (1, 1));
    assert_eq!(selected_rows(&text, 4..4), (1, 1));
}

#[test]
fn ask_agent_mentions_the_file_or_its_selected_lines() {
    assert_eq!(line_range(7, 7), "L7");
    assert_eq!(line_range(10, 20), "L10-20");
    assert_eq!(file_mention("src/lib.rs", None, "rust"), "@src/lib.rs ");
    assert_eq!(
        file_mention("src/lib.rs", Some((2, 3, "fn a() {}\nfn b() {}")), "rust"),
        "@src/lib.rs:L2-3\n```rust\nfn a() {}\nfn b() {}\n```\n"
    );
}
