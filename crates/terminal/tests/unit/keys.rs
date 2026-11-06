use super::*;

const NONE: Mods = Mods { shift: false, alt: false, ctrl: false, cmd: false };
const SHIFT: Mods = Mods { shift: true, ..NONE };
const ALT: Mods = Mods { alt: true, ..NONE };
const CTRL: Mods = Mods { ctrl: true, ..NONE };
const CMD: Mods = Mods { cmd: true, ..NONE };

fn k(key: &str, key_char: Option<&str>, m: Mods) -> Option<Vec<u8>> {
    key_bytes(key, key_char, m, false)
}

#[test]
fn plain_keys() {
    assert_eq!(k("enter", Some("\n"), NONE).unwrap(), b"\r");
    assert_eq!(k("tab", Some("\t"), NONE).unwrap(), b"\t");
    assert_eq!(k("tab", Some("\t"), SHIFT).unwrap(), b"\x1b[Z");
    assert_eq!(k("backspace", None, NONE).unwrap(), [0x7f]);
    assert_eq!(k("backspace", None, CTRL).unwrap(), [0x08]);
    assert_eq!(k("escape", None, NONE).unwrap(), [0x1b]);
    assert_eq!(k("space", Some(" "), NONE).unwrap(), b" ");
    assert_eq!(k("a", Some("a"), NONE).unwrap(), b"a");
    assert_eq!(k("a", Some("A"), SHIFT).unwrap(), b"A");
    assert_eq!(k("!", Some("!"), NONE).unwrap(), b"!");
    assert_eq!(k("e", Some("é"), NONE).unwrap(), "é".as_bytes());
}

#[test]
fn arrows_follow_app_cursor_and_modifiers() {
    assert_eq!(k("up", None, NONE).unwrap(), b"\x1b[A");
    assert_eq!(key_bytes("up", None, NONE, true).unwrap(), b"\x1bOA");
    assert_eq!(key_bytes("home", None, NONE, true).unwrap(), b"\x1bOH");
    assert_eq!(k("end", None, NONE).unwrap(), b"\x1b[F");
    assert_eq!(k("up", None, SHIFT).unwrap(), b"\x1b[1;2A");
    assert_eq!(key_bytes("left", None, CTRL, true).unwrap(), b"\x1b[1;5D");
    let all = Mods { shift: true, alt: true, ctrl: true, cmd: false };
    assert_eq!(k("right", None, all).unwrap(), b"\x1b[1;8C");
    assert_eq!(k("down", None, ALT).unwrap(), b"\x1b[1;3B");
}

#[test]
fn editing_and_function_keys() {
    assert_eq!(k("insert", None, NONE).unwrap(), b"\x1b[2~");
    assert_eq!(k("delete", None, NONE).unwrap(), b"\x1b[3~");
    assert_eq!(k("delete", None, SHIFT).unwrap(), b"\x1b[3;2~");
    assert_eq!(k("pageup", None, NONE).unwrap(), b"\x1b[5~");
    assert_eq!(k("pagedown", None, CTRL).unwrap(), b"\x1b[6;5~");
    assert_eq!(k("f1", None, NONE).unwrap(), b"\x1bOP");
    assert_eq!(k("f4", None, NONE).unwrap(), b"\x1bOS");
    assert_eq!(k("f1", None, CTRL).unwrap(), b"\x1b[1;5P");
    assert_eq!(k("f5", None, NONE).unwrap(), b"\x1b[15~");
    assert_eq!(k("f12", None, SHIFT).unwrap(), b"\x1b[24;2~");
}

#[test]
fn control_keys() {
    assert_eq!(k("c", None, CTRL).unwrap(), [0x03]);
    assert_eq!(k("a", None, CTRL).unwrap(), [0x01]);
    assert_eq!(k("z", None, CTRL).unwrap(), [0x1a]);
    assert_eq!(k("c", None, Mods { shift: true, ..CTRL }).unwrap(), [0x03]);
    assert_eq!(k("space", Some(" "), CTRL).unwrap(), [0x00]);
    assert_eq!(k("@", None, CTRL).unwrap(), [0x00]);
    assert_eq!(k("[", None, CTRL).unwrap(), [0x1b]);
    assert_eq!(k("\\", None, CTRL).unwrap(), [0x1c]);
    assert_eq!(k("]", None, CTRL).unwrap(), [0x1d]);
    assert_eq!(k("^", None, CTRL).unwrap(), [0x1e]);
    assert_eq!(k("_", None, CTRL).unwrap(), [0x1f]);
    assert_eq!(k("/", None, CTRL).unwrap(), [0x1f]);
    assert_eq!(k("?", None, CTRL).unwrap(), [0x7f]);
    assert_eq!(k("1", None, CTRL), None);
    assert_eq!(k("c", None, Mods { alt: true, ..CTRL }).unwrap(), [0x1b, 0x03]);
}

#[test]
fn option_is_meta_except_for_ascii_layout_characters() {
    assert_eq!(k("b", Some("∫"), ALT).unwrap(), b"\x1bb");
    assert_eq!(k("b", Some("ı"), Mods { shift: true, ..ALT }).unwrap(), b"\x1bB");
    assert_eq!(k("backspace", None, ALT).unwrap(), [0x1b, 0x7f]);
    assert_eq!(k("enter", Some("\n"), ALT).unwrap(), b"\x1b\r");
    assert_eq!(k("5", Some("["), ALT).unwrap(), b"[");
    assert_eq!(k("l", Some("@"), ALT).unwrap(), b"@");
    assert_eq!(k("left", None, ALT).unwrap(), b"\x1bb");
    assert_eq!(k("right", None, ALT).unwrap(), b"\x1bf");
}

#[test]
fn command_keys_edit_the_line_or_belong_to_the_app() {
    assert_eq!(k("left", None, CMD).unwrap(), [0x01]);
    assert_eq!(k("right", None, CMD).unwrap(), [0x05]);
    assert_eq!(k("backspace", None, CMD).unwrap(), [0x15]);
    assert_eq!(k("k", None, CMD), None);
    assert_eq!(k("c", None, CMD), None);
}

#[test]
fn printable_keys_go_to_the_input_method() {
    assert!(leave_to_ime(Some("a"), NONE, false));
    assert!(leave_to_ime(Some("A"), SHIFT, false));
    assert!(leave_to_ime(Some(" "), NONE, false));
    assert!(leave_to_ime(Some("´"), ALT, false));
    assert!(!leave_to_ime(Some("∫"), ALT, true));
    assert!(!leave_to_ime(None, CTRL, false));
    assert!(!leave_to_ime(Some("k"), CMD, false));
    assert!(!leave_to_ime(Some("\n"), NONE, false));
    assert!(!leave_to_ime(Some("\t"), NONE, false));
    assert!(!leave_to_ime(None, NONE, false));
}

#[test]
fn scrollback_keys_by_platform() {
    assert_eq!(scroll_key("pageup", SHIFT, true), Some(ScrollKey::PageUp));
    assert_eq!(scroll_key("pagedown", SHIFT, false), Some(ScrollKey::PageDown));
    assert_eq!(scroll_key("pageup", NONE, true), None);
    assert_eq!(scroll_key("up", CMD, true), Some(ScrollKey::LineUp));
    assert_eq!(scroll_key("down", CMD, true), Some(ScrollKey::LineDown));
    assert_eq!(scroll_key("up", CMD, false), None);
    assert_eq!(scroll_key("home", CMD, true), Some(ScrollKey::Top));
    assert_eq!(scroll_key("end", SHIFT, false), Some(ScrollKey::Bottom));
    assert_eq!(scroll_key("end", SHIFT, true), None);
}

#[test]
fn wheel_reports_in_sgr_and_legacy_encoding() {
    assert_eq!(wheel_report(true, 0, 0, true), b"\x1b[<64;1;1M");
    assert_eq!(wheel_report(false, 9, 4, true), b"\x1b[<65;10;5M");
    assert_eq!(wheel_report(true, 0, 0, false), [0x1b, b'[', b'M', 96, 33, 33]);
    assert_eq!(wheel_report(false, 300, 2, false), [0x1b, b'[', b'M', 97, 255, 35]);
}

#[test]
fn mouse_reports_press_release_and_drag() {
    assert_eq!(mouse_report(0, 4, 2, true, true), b"\x1b[<0;5;3M");
    assert_eq!(mouse_report(0, 4, 2, false, true), b"\x1b[<0;5;3m");
    assert_eq!(mouse_report(32 + 2 + 16, 0, 0, true, true), b"\x1b[<50;1;1M");
    assert_eq!(mouse_report(1, 0, 0, true, false), [0x1b, b'[', b'M', 33, 33, 33]);
    // legacy release: button bits become 3, modifiers stay
    assert_eq!(mouse_report(2 + 8, 1, 1, false, false), [0x1b, b'[', b'M', 32 + 11, 34, 34]);
}

#[test]
fn paste_brackets_only_when_asked() {
    assert_eq!(paste_bytes("ls\n", true), b"\x1b[200~ls\n\x1b[201~");
    assert_eq!(paste_bytes("a\x1b[201~b", true), b"\x1b[200~a[201~b\x1b[201~");
    assert_eq!(paste_bytes("a\r\nb\nc", false), b"a\rb\rc");
}
