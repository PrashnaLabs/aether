use std::io::BufRead;

use aether_core::Update;

pub(crate) const MAX_LINE: usize = 16 * 1024 * 1024;

/// Lines of `reader` with the newline (and a `\r` before it) removed. A line longer than `cap`
/// is skipped without being buffered and yields `None`; reading then goes on with the next.
/// Ends at EOF or the first read error.
pub(crate) fn capped_lines<R: BufRead>(reader: R, cap: usize) -> Lines<R> {
    Lines { reader, cap }
}

pub(crate) struct Lines<R> {
    reader: R,
    cap: usize,
}

impl<R: BufRead> Iterator for Lines<R> {
    type Item = Option<String>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = Vec::new();
        let (mut over, mut seen) = (false, false);
        loop {
            let chunk = self.reader.fill_buf().ok()?;
            if chunk.is_empty() {
                break;
            }
            seen = true;
            let end = chunk.iter().position(|&b| b == b'\n');
            let content = &chunk[..end.unwrap_or(chunk.len())];
            if !over {
                over = line.len() + content.len() > self.cap;
                if over {
                    line = Vec::new();
                } else {
                    line.extend_from_slice(content);
                }
            }
            let used = end.map_or(chunk.len(), |i| i + 1);
            self.reader.consume(used);
            if end.is_some() {
                break;
            }
        }
        if !seen {
            return None;
        }
        if over {
            return Some(None);
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        Some(Some(String::from_utf8_lossy(&line).into_owned()))
    }
}

pub(crate) fn oversized(provider: &str) -> Update {
    let mb = MAX_LINE / (1024 * 1024);
    Update::Warning(format!("{provider} sent an output line over {mb} MB; skipped it"))
}

#[cfg(test)]
#[path = "../tests/unit/lines.rs"]
mod tests;
