use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// Files past this size open as a message instead of text.
const MAX_LEN: u64 = 10 * 1024 * 1024;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// How close to its mtime a stamp is racy: a same-length write then may not move it.
const RACY: Duration = Duration::from_secs(2);

/// A file's modified time and length, and whether it was [`RACY`] when taken.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DiskStamp {
    mtime: SystemTime,
    len: u64,
    racy: bool,
}

impl PartialEq for DiskStamp {
    /// Same mtime and length; `racy` is about when it was taken, not the file.
    fn eq(&self, other: &Self) -> bool {
        (self.mtime, self.len) == (other.mtime, other.len)
    }
}

/// `path`'s stamp now, `None` once it's gone.
fn disk_stamp(path: &Path) -> Option<DiskStamp> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    Some(DiskStamp { mtime, len: meta.len(), racy: is_racy(mtime, SystemTime::now()) })
}

/// Whether `mtime` is under [`RACY`] before `now`, or after it.
fn is_racy(mtime: SystemTime, now: SystemTime) -> bool {
    now.duration_since(mtime).map_or(true, |age| age < RACY)
}

/// Reads a UTF-8 text file, refusing large (> 10 MB) and binary (NUL in the first 8 KB) ones.
fn read_text(path: &Path) -> std::io::Result<String> {
    if std::fs::metadata(path)?.len() > MAX_LEN {
        return Err(Error::new(ErrorKind::FileTooLarge, "it's larger than 10 MB"));
    }
    let bytes = std::fs::read(path)?;
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return Err(Error::new(ErrorKind::InvalidData, "it's a binary file"));
    }
    String::from_utf8(bytes).map_err(|_| Error::new(ErrorKind::InvalidData, "it isn't UTF-8 text"))
}

/// Why a tab's text no longer matches its file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskConflict {
    /// The file changed on disk under unsaved edits.
    Changed,
    Deleted,
}

/// An open file in the Global File Explorer; its live text is the tab's editor.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorBuffer {
    /// Unique across explorers; keys the tab's editor.
    pub id: u64,
    /// Absolute or relative path to the file on disk.
    pub path: PathBuf,
    /// Display tab title (e.g. filename).
    pub title: String,
    /// Baseline content as read from disk or last saved.
    pub original: String,
    /// Disk stamp at the last open, save or reload.
    pub(crate) stamp: Option<DiskStamp>,
    /// Whether the editor text differs from `original`.
    pub is_dirty: bool,
    pub conflict: Option<DiskConflict>,
}

impl EditorBuffer {
    /// Opens an existing text file from disk into a new EditorBuffer.
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        let stamp = disk_stamp(&path);
        let original = read_text(&path)?;
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".to_string());
        Ok(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            path,
            title,
            original,
            stamp,
            is_dirty: false,
            conflict: None,
        })
    }

    /// Writes `text` to disk as the new baseline.
    pub fn save(&mut self, text: String) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, &text)?;
        self.stamp = disk_stamp(&self.path);
        self.original = text;
        self.is_dirty = false;
        self.conflict = None;
        Ok(())
    }

    /// Re-reads the file as the new baseline and returns its text.
    pub fn reload(&mut self) -> std::io::Result<String> {
        // stamp first: a write landing mid-read shows as a change next check
        let stamp = disk_stamp(&self.path);
        self.original = read_text(&self.path)?;
        self.stamp = stamp;
        self.is_dirty = false;
        self.conflict = None;
        Ok(self.original.clone())
    }

    /// Keeps the editor text over the disk change, taking the disk text as the new
    /// baseline (a gone file keeps the old one); the next save overwrites the file.
    pub fn keep_mine(&mut self) {
        self.stamp = disk_stamp(&self.path);
        if let Ok(text) = read_text(&self.path) {
            self.original = text;
        }
        self.is_dirty = true;
        self.conflict = None;
    }

    /// Compares the file with its stamp, and a racy stamp's text with `original`: a
    /// clean tab reloads and returns the new text, a dirty or deleted one is marked
    /// in conflict.
    pub fn check_disk(&mut self) -> Option<String> {
        // stamp before the read: a write after it gets a newer mtime, so a match can clear racy
        let stamp = disk_stamp(&self.path);
        if stamp == self.stamp {
            let racy = self.stamp.is_some_and(|s| s.racy);
            if !racy || read_text(&self.path).is_ok_and(|text| text == self.original) {
                self.stamp = stamp;
                return None;
            }
        }
        if stamp.is_none() {
            self.conflict = Some(DiskConflict::Deleted);
            return None;
        }
        if !self.is_dirty
            && let Ok(text) = self.reload()
        {
            return Some(text);
        }
        self.conflict = Some(DiskConflict::Changed);
        None
    }

    /// Detects language from file extension.
    pub fn detect_language(&self) -> &'static str {
        match self.path.extension().and_then(|s| s.to_str()).unwrap_or("") {
            "rs" => "Rust",
            "md" | "markdown" => "Markdown",
            "toml" => "TOML",
            "json" => "JSON",
            "js" | "mjs" | "cjs" => "JavaScript",
            "ts" | "tsx" => "TypeScript",
            "html" | "htm" => "HTML",
            "css" => "CSS",
            "sh" | "bash" | "zsh" => "Shell",
            "py" => "Python",
            "go" => "Go",
            "c" | "h" => "C",
            "cpp" | "hpp" | "cc" => "C++",
            "yaml" | "yml" => "YAML",
            "sql" => "SQL",
            _ => "Plain Text",
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/buffer.rs"]
mod tests;
