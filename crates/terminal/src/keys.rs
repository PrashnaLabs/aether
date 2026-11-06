//! Key presses and pastes to the bytes a shell expects (xterm conventions).

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub cmd: bool,
}

/// Bytes for a key press, with `key` and `key_char` as GPUI reports them;
/// `None` leaves the key to the app.
pub(crate) fn key_bytes(
    key: &str,
    key_char: Option<&str>,
    m: Mods,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    if m.cmd {
        return match key {
            "left" => Some(vec![0x01]),
            "right" => Some(vec![0x05]),
            "backspace" => Some(vec![0x15]),
            _ => None,
        };
    }
    // macOS convention (Terminal.app): Option+arrow moves by word
    if m.alt && !m.shift && !m.ctrl && (key == "left" || key == "right") {
        return Some(if key == "left" { b"\x1bb".to_vec() } else { b"\x1bf".to_vec() });
    }
    if let Some(seq) = special(key, m, app_cursor) {
        return Some(seq.into_bytes());
    }
    let base = match key {
        "enter" => vec![b'\r'],
        "tab" if m.shift => return Some(b"\x1b[Z".to_vec()),
        "tab" => vec![b'\t'],
        "backspace" if m.ctrl => vec![0x08],
        "backspace" => vec![0x7f],
        "escape" => vec![0x1b],
        _ if m.ctrl => vec![ctrl_byte(key)?],
        _ => {
            let plain = if key == "space" { " " } else { key };
            let typed = key_char.or((plain.chars().count() == 1).then_some(plain))?;
            if !m.alt {
                return Some(typed.as_bytes().to_vec());
            }
            // Option types ASCII layout characters (`{` is Option+8 on German keyboards); otherwise it is Meta.
            if typed.is_ascii() && typed != plain && !(m.shift && typed == plain.to_uppercase()) {
                return Some(typed.as_bytes().to_vec());
            }
            let meta = if m.shift { plain.to_uppercase() } else { plain.to_string() };
            meta.into_bytes()
        }
    };
    Some(if m.alt { [&[0x1b][..], &base].concat() } else { base })
}

fn special(key: &str, m: Mods, app_cursor: bool) -> Option<String> {
    let p = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.ctrl as u8;
    let cursor = |c: char| match (p, app_cursor) {
        (1, true) => format!("\x1bO{c}"),
        (1, false) => format!("\x1b[{c}"),
        _ => format!("\x1b[1;{p}{c}"),
    };
    let tilde = |n: u8| if p == 1 { format!("\x1b[{n}~") } else { format!("\x1b[{n};{p}~") };
    let ss3 = |c: char| if p == 1 { format!("\x1bO{c}") } else { format!("\x1b[1;{p}{c}") };
    Some(match key {
        "up" => cursor('A'),
        "down" => cursor('B'),
        "right" => cursor('C'),
        "left" => cursor('D'),
        "home" => cursor('H'),
        "end" => cursor('F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" => ss3('P'),
        "f2" => ss3('Q'),
        "f3" => ss3('R'),
        "f4" => ss3('S'),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        _ => return None,
    })
}

fn ctrl_byte(key: &str) -> Option<u8> {
    if key == "space" {
        return Some(0);
    }
    let mut chars = key.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        '@' | '2' | ' ' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '/' | '7' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

/// Printable keys go through the platform text input (IME, dead keys, Option
/// characters); with Option as Meta, Option+key stays a key press.
pub(crate) fn leave_to_ime(key_char: Option<&str>, m: Mods, option_as_meta: bool) -> bool {
    !m.ctrl
        && !m.cmd
        && !(m.alt && option_as_meta)
        && key_char.is_some_and(|c| !c.is_empty() && !c.chars().any(char::is_control))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollKey {
    PageUp,
    PageDown,
    LineUp,
    LineDown,
    Top,
    Bottom,
}

/// Scrollback keys: Shift+PgUp/PgDn page; on macOS Cmd+Up/Down move a line and
/// Cmd+Home/End jump, elsewhere Shift+Home/End jump.
pub(crate) fn scroll_key(key: &str, m: Mods, mac: bool) -> Option<ScrollKey> {
    let only = |shift: bool, cmd: bool| m == Mods { shift, cmd, ..Mods::default() };
    Some(match key {
        "pageup" if only(true, false) => ScrollKey::PageUp,
        "pagedown" if only(true, false) => ScrollKey::PageDown,
        "up" if mac && only(false, true) => ScrollKey::LineUp,
        "down" if mac && only(false, true) => ScrollKey::LineDown,
        "home" if only(!mac, mac) => ScrollKey::Top,
        "end" if only(!mac, mac) => ScrollKey::Bottom,
        _ => return None,
    })
}

/// A wheel notch as a mouse report at a 0-based cell, for programs that asked for the mouse.
pub(crate) fn wheel_report(up: bool, col: usize, row: usize, sgr: bool) -> Vec<u8> {
    mouse_report(if up { 64 } else { 65 }, col, row, true, sgr)
}

/// xterm mouse report. `code`: button (0 left, 1 middle, 2 right, 64/65 wheel),
/// +4 shift, +8 alt, +16 ctrl, +32 motion.
// ponytail: no UTF-8 (1005) encoding; legacy coordinates cap at 222
pub(crate) fn mouse_report(code: u8, col: usize, row: usize, pressed: bool, sgr: bool) -> Vec<u8> {
    if sgr {
        let end = if pressed { 'M' } else { 'm' };
        return format!("\x1b[<{code};{};{}{end}", col + 1, row + 1).into_bytes();
    }
    // legacy release names no button
    let code = if pressed { code } else { code & 0b1_1100 | 3 };
    let at = |v: usize| (v + 33).min(255) as u8;
    vec![0x1b, b'[', b'M', 32 + code, at(col), at(row)]
}

/// Pasted text; bracketed mode drops ESC so the paste can't end its own bracket.
pub(crate) fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
#[path = "../tests/unit/keys.rs"]
mod tests;
