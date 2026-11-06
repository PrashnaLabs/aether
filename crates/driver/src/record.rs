use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use aether_core::home_dir;
#[cfg(test)]
use aether_core::{ToolCall, Update};

pub struct Recorder {
    file: Mutex<File>,
    home: String,
    cwds: Vec<String>,
}

pub fn from_env(provider: &str, cwd: &Path) -> Option<Arc<Recorder>> {
    let dir = std::env::var_os("AETHER_RECORD").filter(|v| !v.is_empty())?;
    match Recorder::create(Path::new(&dir), provider, cwd) {
        Ok(rec) => Some(Arc::new(rec)),
        Err(e) => {
            eprintln!("aether: AETHER_RECORD: {e}");
            None
        }
    }
}

impl Recorder {
    pub fn create(dir: &Path, provider: &str, cwd: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("{provider}-{ms}.jsonl")))?;
        let mut cwds: Vec<String> =
            cwd.canonicalize().into_iter().map(|p| p.display().to_string()).collect();
        if cwd.is_absolute() {
            cwds.push(cwd.display().to_string());
        }
        Ok(Self { file: Mutex::new(file), home: home_dir().display().to_string(), cwds })
    }

    pub fn line(&self, raw: &str) {
        let cwds: Vec<&str> = self.cwds.iter().map(String::as_str).collect();
        let out = scrub(raw, &self.home, &cwds);
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(format!("{out}\n").as_bytes());
        }
    }
}

pub fn scrub(raw: &str, home: &str, cwds: &[&str]) -> String {
    let mut cwds: Vec<&str> =
        cwds.iter().copied().filter(|c| c.starts_with('/') && *c != "/").collect();
    cwds.sort_by_key(|c| std::cmp::Reverse(c.len()));
    let mut out = raw.to_string();
    for cwd in cwds {
        out = out.replace(cwd, "/work");
    }
    if !home.is_empty() && home != "/" {
        out = out.replace(home, "~");
    }
    scrub_emails(&out)
}

fn scrub_emails(s: &str) -> String {
    let local = |c: u8| c.is_ascii_alphanumeric() || b"._%+-".contains(&c);
    let domain = |c: u8| c.is_ascii_alphanumeric() || c == b'.' || c == b'-';
    let b = s.as_bytes();
    let (mut out, mut last, mut i) = (String::new(), 0, 0);
    while i < b.len() {
        if b[i] != b'@' {
            i += 1;
            continue;
        }
        let mut start = i;
        while start > last && local(b[start - 1]) {
            start -= 1;
        }
        // a JSON escape like \n sits right before the address and must survive
        if start > last && b[start - 1] == b'\\' && start < i && b"ntrbf".contains(&b[start]) {
            start += 1;
        }
        let mut end = i + 1;
        while end < b.len() && domain(b[end]) {
            end += 1;
        }
        while end > i + 1 && matches!(b[end - 1], b'.' | b'-') {
            end -= 1;
        }
        if start < i && s[i + 1..end].contains('.') {
            out.push_str(&s[last..start]);
            out.push_str("user@example.com");
            last = end;
            i = end;
        } else {
            i += 1;
        }
    }
    out.push_str(&s[last..]);
    out
}

#[cfg(test)]
fn fixture_dir(provider: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(provider)
}

#[cfg(test)]
pub(crate) fn fixture(provider: &str, scenario: &str) -> Vec<String> {
    let path = fixture_dir(provider).join(format!("{scenario}.jsonl"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect()
}

#[cfg(test)]
pub(crate) fn fixtures(provider: &str) -> Vec<(String, Vec<String>)> {
    let mut names: Vec<String> = std::fs::read_dir(fixture_dir(provider))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.path().file_stem()?.to_str().map(String::from))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let lines = fixture(provider, &n);
            (n, lines)
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn last_calls(updates: &[Update]) -> Vec<ToolCall> {
    let mut out: Vec<ToolCall> = Vec::new();
    for u in updates {
        if let Update::Tool(call) = u {
            match out.iter_mut().find(|c| c.id == call.id) {
                Some(slot) => *slot = call.clone(),
                None => out.push(call.clone()),
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/record.rs"]
mod tests;
