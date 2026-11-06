use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aether_core::home_dir;

/// Folders the walk skips when the root isn't a git repo.
const SKIPPED_DIRS: &[&str] =
    &[".git", "target", "node_modules", ".superpowers", ".idea", ".vscode", "dist", "build"];

/// Most files the walk collects.
const WALK_CAP: usize = 50_000;

/// What Quick Open and grep search, and what the tree dims.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileIndex {
    /// Paths relative to the root.
    pub files: Arc<[String]>,
    /// `files` lowercased, precomputed so Quick Open doesn't lowercase per keystroke.
    lower_files: Arc<[String]>,
    /// Git-ignored files and folders, absolute.
    pub ignored: HashSet<PathBuf>,
}

impl FileIndex {
    pub(crate) fn lower_files(&self) -> &[String] {
        &self.lower_files
    }

    pub(crate) fn from_files(files: Vec<String>, ignored: HashSet<PathBuf>) -> Self {
        let lower_files: Vec<String> = files.iter().map(|f| f.to_lowercase()).collect();
        Self { files: files.into(), lower_files: lower_files.into(), ignored }
    }
}

/// Lists `root`'s files with git, or by walking it outside a repo.
/// Blocking: run it off the UI thread.
// ponytail: git output stops at 64 MB (~1.5M paths), stream it if a repo needs more
pub fn load_index(root: &Path) -> FileIndex {
    if root == home_dir() || !root.is_dir() {
        return FileIndex::default();
    }
    let Some(listed) = aether_git::ls_files(root, &["-co", "--exclude-standard", "-z"]) else {
        return FileIndex::from_files(walk_files(root), HashSet::new());
    };
    let ignored = ["-z", "--others", "--ignored", "--exclude-standard", "--directory"];
    let ignored = aether_git::ls_files(root, &ignored)
        .map(|out| nul_entries(&out).map(|rel| root.join(rel)).collect())
        .unwrap_or_default();
    FileIndex::from_files(nul_entries(&listed).map(str::to_string).collect(), ignored)
}

/// Entries of `git ls-files -z` output minus a folder's trailing `/`; an entry
/// cut off at the output cap is dropped.
fn nul_entries(out: &str) -> impl Iterator<Item = &str> {
    let complete = out.rsplit_once('\0').map_or("", |(done, _)| done);
    complete.split('\0').filter(|e| !e.is_empty()).map(|e| e.trim_end_matches('/'))
}

/// Files under `root`, relative, skipping [`SKIPPED_DIRS`], at most [`WALK_CAP`].
fn walk_files(root: &Path) -> Vec<String> {
    let (mut files, mut dirs) = (Vec::new(), vec![root.to_path_buf()]);
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_symlink()) {
                continue;
            }
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if !SKIPPED_DIRS.contains(&entry.file_name().to_string_lossy().as_ref()) {
                    dirs.push(path);
                }
            } else if let Ok(rel) = path.strip_prefix(root) {
                files.push(rel.to_string_lossy().to_string());
                if files.len() >= WALK_CAP {
                    return files;
                }
            }
        }
    }
    files
}

#[cfg(test)]
#[path = "../tests/unit/index.rs"]
mod tests;
