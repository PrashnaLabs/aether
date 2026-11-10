use std::path::{Path, PathBuf};
use std::rc::Rc;

use aether_core::home_dir;
use gpui_kit::{App, Global, Window};
use markdown::mdast::Node;

/// An existing file or folder named in a reply, with its 1-indexed line and column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLink {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

/// Opens file links from replies; returns false to leave the link to the OS.
pub struct OpenFileLink(pub Rc<dyn Fn(FileLink, &mut Window, &mut App) -> bool>);

impl Global for OpenFileLink {}

/// Opens web links in the app's browser; returns false to use the OS fallback.
pub struct OpenWebLink(pub Rc<dyn Fn(&str, &mut Window, &mut App) -> bool>);

impl Global for OpenWebLink {}

/// Resolves `href` (a local `file://` URL, an absolute, `~/` or `cwd`-relative path, with an
/// optional `:12`, `:12:3`, `:12-20`, `#L12`, `#L12-L20` or `#L12C3` suffix) to an
/// existing file or folder. Other schemes, like `https:`, are `None`.
pub fn parse_file_link(href: &str, cwd: Option<&Path>) -> Option<FileLink> {
    let href = href.trim();
    let raw = match href.strip_prefix("file://") {
        Some(rest) => match rest.strip_prefix("localhost").unwrap_or(rest) {
            path if path.starts_with('/') => drive_path(percent_decode(path)?),
            _ => return None,
        },
        None if has_scheme(href) => return None,
        None => href.to_string(),
    };
    if let Some(path) = resolve(&raw, cwd) {
        return Some(FileLink { path, line: None, col: None });
    }
    let (path, line, col) = split_position(&raw)?;
    resolve(path, cwd).map(|path| FileLink { path, line: Some(line), col })
}

/// Only these schemes may reach the OS opener from a reply link.
pub(crate) fn openable(href: &str) -> bool {
    let scheme = href.trim().split_once(':').map(|(s, _)| s);
    scheme.is_some_and(|s| {
        s.eq_ignore_ascii_case("http")
            || s.eq_ignore_ascii_case("https")
            || s.eq_ignore_ascii_case("mailto")
    })
}

/// Opens files in Explorer and web links in the app browser, with an OS fallback
/// for `http(s)`/`mailto` only; other schemes never reach the OS.
pub(crate) fn open_link(href: &str, cwd: Option<&Path>, window: &mut Window, cx: &mut App) {
    let open = cx.try_global::<OpenFileLink>().map(|open| open.0.clone());
    let opened = match (open, parse_file_link(href, cwd)) {
        (Some(open), Some(link)) => open(link, window, cx),
        _ => false,
    };
    if opened {
        return;
    }
    let open_web = cx.try_global::<OpenWebLink>().map(|open| open.0.clone());
    if let Some(open) = open_web
        && open(href, window, cx)
    {
        return;
    }
    if openable(href) {
        cx.open_url(href);
    }
}

/// Opens `link` through the app's [`OpenFileLink`], if one is installed.
pub(crate) fn open_file(link: FileLink, window: &mut Window, cx: &mut App) {
    if let Some(open) = cx.try_global::<OpenFileLink>().map(|open| open.0.clone()) {
        open(link, window, cx);
    }
}

/// `text` with each inline code span that names an existing file wrapped in a link to it,
/// e.g. `` `src/a.rs:4` `` to ``[`src/a.rs:4`](</abs/src/a.rs#L4>)``. Spans in link text,
/// with whitespace, or flag-like are left alone.
pub(crate) fn link_inline_code(text: &str, cwd: &Path) -> String {
    if !text.contains('`') {
        return text.to_string();
    }
    let Ok(root) = markdown::to_mdast(text, &markdown::ParseOptions::gfm()) else {
        return text.to_string();
    };
    let mut spans = Vec::new();
    code_spans(&root, &mut spans);
    let (mut out, mut at) = (String::with_capacity(text.len()), 0);
    for (start, end, code) in spans {
        let Some(dest) = code_link(code, cwd) else { continue };
        out.push_str(&text[at..start]);
        out.push_str(&format!("[{}](<{dest}>)", &text[start..end]));
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

fn code_spans<'a>(node: &'a Node, out: &mut Vec<(usize, usize, &'a str)>) {
    match node {
        Node::InlineCode(code) => {
            if let Some(at) = &code.position {
                out.push((at.start.offset, at.end.offset, &code.value));
            }
        }
        Node::Link(_) | Node::LinkReference(_) => {}
        node => node.children().into_iter().flatten().for_each(|child| code_spans(child, out)),
    }
}

fn code_link(code: &str, cwd: &Path) -> Option<String> {
    if code.starts_with('-')
        || code.contains(char::is_whitespace)
        || !code.contains(char::is_alphanumeric)
    {
        return None;
    }
    let link = parse_file_link(code, Some(cwd))?;
    // a backslash would escape in a markdown destination, and Windows accepts `/`
    let path = link.path.to_str()?;
    let path = if cfg!(windows) { path.replace('\\', "/") } else { path.to_string() };
    if path.contains(['<', '>', '\\']) {
        return None;
    }
    Some(match (link.line, link.col) {
        (Some(line), Some(col)) => format!("{path}#L{line}C{col}"),
        (Some(line), None) => format!("{path}#L{line}"),
        _ => path.to_string(),
    })
}

/// The `/C:/x` a file URL holds for `C:\x` is `C:/x`; anything else is unchanged.
fn drive_path(path: String) -> String {
    match path.as_bytes() {
        [b'/', drive, b':', ..] if cfg!(windows) && drive.is_ascii_alphabetic() => path[1..].into(),
        _ => path,
    }
}

fn resolve(path: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let path = match path.strip_prefix("~/") {
        Some(rest) => home_dir().join(rest),
        None if Path::new(path).is_absolute() => PathBuf::from(path),
        None if !path.is_empty() => cwd?.join(path),
        None => return None,
    };
    let path: PathBuf = path.components().collect();
    path.exists().then_some(path)
}

/// `a.rs:12`, `a.rs:12:3`, `a.rs:12-20`, `a.rs#L12`, `a.rs#L12-L20`, `a.rs#L12C3` to
/// the path, line and column.
fn split_position(s: &str) -> Option<(&str, usize, Option<usize>)> {
    if let Some((path, at)) = s.rsplit_once("#L") {
        return match at.split_once('C') {
            Some((line, col)) => Some((path, line.parse().ok()?, Some(col.parse().ok()?))),
            None => Some((path, range_start(at)?, None)),
        };
    }
    let (rest, last) = s.rsplit_once(':')?;
    if let Some((path, line)) = rest.rsplit_once(':')
        && let (Ok(line), Ok(col)) = (line.parse(), last.parse())
    {
        return Some((path, line, Some(col)));
    }
    Some((rest, range_start(last)?, None))
}

/// `12`, `12-20` or `12-L20` to 12.
fn range_start(s: &str) -> Option<usize> {
    let (start, end) = s.split_once('-').unwrap_or((s, "0"));
    end.trim_start_matches('L').parse::<usize>().ok()?;
    start.parse().ok()
}

/// A URL scheme like `https:`; `a.rs:12` is a path with a line.
fn has_scheme(s: &str) -> bool {
    s.split_once(':').is_some_and(|(scheme, rest)| {
        scheme.len() > 1
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
            && !rest.starts_with(|c: char| c.is_ascii_digit())
    })
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(bytes.len()), 0);
    while i < bytes.len() {
        let hex = s.get(i + 1..i + 3).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
        match (bytes[i], hex) {
            (b'%', Some(hex)) => {
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
#[path = "../../tests/unit/chat/links.rs"]
mod tests;
