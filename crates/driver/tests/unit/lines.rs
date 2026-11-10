use super::*;
use std::io::{BufReader, Cursor};

fn read(input: &[u8], cap: usize) -> Vec<Option<String>> {
    capped_lines(Cursor::new(input.to_vec()), cap).collect()
}

fn some(s: &str) -> Option<String> {
    Some(s.to_string())
}

#[test]
fn normal_lines_come_back_without_newlines() {
    assert_eq!(read(b"a\r\nbb\n\nccc\n", 16), [some("a"), some("bb"), some(""), some("ccc")]);
}

#[test]
fn eof_without_a_trailing_newline_keeps_the_last_line() {
    assert_eq!(read(b"a\nlast", 16), [some("a"), some("last")]);
    assert_eq!(read(b"", 16), []);
}

#[test]
fn an_oversized_line_is_skipped_and_the_next_one_still_reads() {
    let mut input = vec![b'x'; 100];
    input.extend_from_slice(b"\nok\n");
    assert_eq!(read(&input, 10), [None, some("ok")]);
    assert_eq!(read(b"0123456789\nab\n", 10), [some("0123456789"), some("ab")]);
}

#[test]
fn an_oversized_line_split_across_tiny_buffers_is_skipped() {
    let mut input = vec![b'x'; 100];
    input.extend_from_slice(b"\nok");
    let reader = BufReader::with_capacity(7, Cursor::new(input));
    assert_eq!(capped_lines(reader, 10).collect::<Vec<_>>(), [None, some("ok")]);
}

#[test]
fn an_oversized_line_at_eof_is_reported_once() {
    assert_eq!(read(&[b'x'; 50], 10), [None]);
}

#[test]
fn invalid_utf8_is_replaced_not_fatal() {
    assert_eq!(read(b"a\xffb\nnext\n", 16), [some("a\u{fffd}b"), some("next")]);
}
