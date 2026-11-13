use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A file or folder in the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct FileTreeNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
}

/// The tree's open folders, their listings read so far, the selection, and the visible rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileTreeState {
    pub root: PathBuf,
    pub selected: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    listings: HashMap<PathBuf, Vec<FileTreeNode>>,
    rows: Vec<FileTreeNode>,
}

impl FileTreeState {
    pub fn new(root: PathBuf) -> Self {
        let mut state = Self { root: root.clone(), ..Self::default() };
        state.open(&root);
        state.rebuild();
        state
    }

    /// Rows shown, in order: each open folder's children right under it.
    pub fn rows(&self) -> &[FileTreeNode] {
        &self.rows
    }

    pub fn toggle_expand(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.open(path);
        }
        self.rebuild();
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    pub fn has_expanded_folders(&self) -> bool {
        self.expanded.iter().any(|path| path != &self.root)
    }

    /// Leave the invisible root open so its immediate children remain visible.
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
        self.expanded.insert(self.root.clone());
        self.rebuild();
    }

    pub fn select(&mut self, path: PathBuf) {
        self.selected = Some(path);
    }

    /// Opens every folder above `path` up to the root and selects it.
    pub fn reveal(&mut self, path: &Path) {
        let dirs: Vec<PathBuf> = path
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(&self.root))
            .map(Path::to_path_buf)
            .collect();
        for dir in dirs {
            self.open(&dir);
        }
        self.selected = Some(path.to_path_buf());
        self.rebuild();
    }

    /// Re-reads the open folders; one that can't be read closes.
    pub fn refresh(&mut self) {
        self.listings.retain(|dir, _| self.expanded.contains(dir));
        for dir in self.expanded.clone() {
            match list_dir(&dir, self.depth_below(&dir)) {
                Ok(nodes) => {
                    self.listings.insert(dir, nodes);
                }
                Err(_) => {
                    self.listings.remove(&dir);
                    self.expanded.remove(&dir);
                }
            }
        }
        self.rebuild();
    }

    /// Applies disk listings without replacing interactions made during the scan.
    pub(crate) fn apply_refresh(&mut self, before: &Self, mut refreshed: Self) {
        if self.root != refreshed.root {
            return;
        }
        if self.expanded == before.expanded && self.selected == before.selected {
            self.expanded = refreshed.expanded;
            self.selected = refreshed.selected;
        }
        for dir in &before.expanded {
            if self.expanded.contains(dir) && !refreshed.listings.contains_key(dir) {
                self.expanded.remove(dir);
                self.listings.remove(dir);
            }
        }
        for (dir, nodes) in refreshed.listings.drain() {
            if self.expanded.contains(&dir) {
                self.listings.insert(dir, nodes);
            }
        }
        self.listings.retain(|dir, _| self.expanded.contains(dir));
        self.rebuild();
    }

    /// Marks `dir` open, reading its children the first time.
    fn open(&mut self, dir: &Path) {
        if !self.listings.contains_key(dir)
            && let Ok(nodes) = list_dir(dir, self.depth_below(dir))
        {
            self.listings.insert(dir.to_path_buf(), nodes);
        }
        self.expanded.insert(dir.to_path_buf());
    }

    /// Depth of `dir`'s children: 0 for the root's.
    fn depth_below(&self, dir: &Path) -> usize {
        dir.strip_prefix(&self.root).map_or(0, |rel| rel.components().count())
    }

    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        flatten(&self.root, &self.listings, &self.expanded, &mut rows);
        self.rows = rows;
    }
}

/// Appends `dir`'s children, each open folder followed by its own.
fn flatten(
    dir: &Path,
    listings: &HashMap<PathBuf, Vec<FileTreeNode>>,
    expanded: &HashSet<PathBuf>,
    out: &mut Vec<FileTreeNode>,
) {
    for node in listings.get(dir).into_iter().flatten() {
        out.push(node.clone());
        if node.is_dir && expanded.contains(&node.path) {
            flatten(&node.path, listings, expanded, out);
        }
    }
}

/// `dir`'s children minus `.git`, folders first, then by name ignoring case.
fn list_dir(dir: &Path, depth: usize) -> std::io::Result<Vec<FileTreeNode>> {
    let mut nodes = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let path = entry.path();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir() || t.is_symlink() && path.is_dir());
        nodes.push(FileTreeNode { path, name, is_dir, depth });
    }
    nodes.sort_by_cached_key(|n| (!n.is_dir, n.name.to_lowercase()));
    Ok(nodes)
}

#[cfg(test)]
#[path = "../tests/unit/tree.rs"]
mod tests;
