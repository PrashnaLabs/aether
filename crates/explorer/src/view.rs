use std::path::{Path, PathBuf};

use crate::buffer::EditorBuffer;
use crate::index::FileIndex;
use crate::search::{GrepMatch, GrepOptions, QuickOpenItem, grep_files, quick_open_matches};
use crate::tree::FileTreeState;

/// Most matches one grep run keeps.
const GREP_LIMIT: usize = 500;

/// Current mode of the explorer's left sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorerSidebarTab {
    Files,
    Grep,
}

/// State managing the entire Global File Explorer.
pub struct ExplorerState {
    pub root: PathBuf,
    pub tree_state: FileTreeState,
    pub tabs: Vec<EditorBuffer>,
    pub active_tab: Option<usize>,
    pub sidebar_tab: ExplorerSidebarTab,
    pub quick_open_open: bool,
    quick_open_query: String,
    pub quick_open_selected: usize,
    quick_open_items: Vec<QuickOpenItem>,
    pub grep_options: GrepOptions,
    pub grep_results: Vec<GrepMatch>,
    /// Bumped per grep run or queued keystroke; results of older runs are dropped.
    pub grep_generation: u64,
    pub grep_running: bool,
    /// Bumped per index load; results of older loads are dropped.
    pub index_generation: u64,
    index: FileIndex,
    tree_generation: u64,
    tree_reveal: Option<PathBuf>,
}

impl ExplorerState {
    pub fn new(root: PathBuf) -> Self {
        let tree_state = FileTreeState::new(root.clone());
        Self {
            root,
            tree_state,
            tabs: Vec::new(),
            active_tab: None,
            sidebar_tab: ExplorerSidebarTab::Files,
            quick_open_open: false,
            quick_open_query: String::new(),
            quick_open_selected: 0,
            quick_open_items: Vec::new(),
            grep_options: GrepOptions::default(),
            grep_results: Vec::new(),
            grep_generation: 0,
            grep_running: false,
            index_generation: 0,
            index: FileIndex::default(),
            tree_generation: 0,
            tree_reveal: None,
        }
    }

    /// Captures a refresh job. Run the returned closure off the UI thread.
    /// Revealing a newly created path happens after its folders have been read.
    pub fn begin_tree_refresh(
        &mut self,
        reveal: Option<PathBuf>,
    ) -> (u64, impl FnOnce() -> (FileTreeState, FileTreeState) + Send + use<>) {
        self.tree_generation += 1;
        if reveal.is_some() {
            self.tree_reveal = reveal;
        }
        let reveal = self.tree_reveal.clone();
        let before = self.tree_state.clone();
        (self.tree_generation, move || {
            let mut refreshed = before.clone();
            refreshed.refresh();
            if let Some(path) = reveal {
                refreshed.reveal(&path);
            }
            (before, refreshed)
        })
    }

    /// A newer request supersedes older disk results.
    pub fn finish_tree_refresh(&mut self, generation: u64, result: (FileTreeState, FileTreeState)) {
        if generation == self.tree_generation {
            self.tree_reveal = None;
            self.tree_state.apply_refresh(&result.0, result.1);
        }
    }

    /// Takes the file index of load `generation` unless a newer load started since,
    /// re-matching Quick Open against it. Returns whether it was taken.
    pub fn set_index(&mut self, generation: u64, index: FileIndex) -> bool {
        if generation != self.index_generation {
            return false;
        }
        self.index = index;
        self.quick_open_items = quick_open_matches(
            &self.root,
            &self.index.files,
            self.index.lower_files(),
            &self.quick_open_query,
        );
        self.quick_open_selected =
            self.quick_open_selected.min(self.quick_open_items.len().saturating_sub(1));
        true
    }

    /// Whether git ignores `path` or a folder above it.
    pub fn is_ignored(&self, path: &Path) -> bool {
        path.ancestors().take_while(|p| *p != self.root).any(|p| self.index.ignored.contains(p))
    }

    /// Opens a file from disk into a tab, or selects it if already open.
    pub fn open_file(&mut self, path: PathBuf) -> std::io::Result<()> {
        if let Some(existing_idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active_tab = Some(existing_idx);
            self.reveal(&path);
            return Ok(());
        }

        let buffer = EditorBuffer::open(path.clone())?;
        self.tabs.push(buffer);
        self.active_tab = Some(self.tabs.len() - 1);
        self.reveal(&path);
        Ok(())
    }

    /// Closes a tab at `index`. Returns true if closed.
    pub fn close_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active_tab = None;
        } else if let Some(active) = self.active_tab {
            if active >= index && active > 0 {
                self.active_tab = Some(active - 1);
            } else if active >= self.tabs.len() {
                self.active_tab = Some(self.tabs.len() - 1);
            }
        }
        true
    }

    /// Re-checks every tab against disk; returns the reloaded tabs' ids and new text.
    pub fn check_disk(&mut self) -> Vec<(u64, String)> {
        self.tabs.iter_mut().filter_map(|t| Some((t.id, t.check_disk()?))).collect()
    }

    /// Tab labels: the file name, plus its folder when another open file shares the name.
    pub fn tab_labels(&self) -> Vec<String> {
        self.tabs
            .iter()
            .map(|tab| {
                let clash = self.tabs.iter().any(|t| t.title == tab.title && t.path != tab.path);
                match tab.path.parent().and_then(|p| p.file_name()) {
                    Some(dir) if clash => format!("{} · {}", tab.title, dir.to_string_lossy()),
                    _ => tab.title.clone(),
                }
            })
            .collect()
    }

    /// Expands `path`'s folders up to the root and selects it.
    pub fn reveal(&mut self, path: &Path) {
        self.tree_state.reveal(path);
    }

    /// Points tabs at or under `from` to the same place under `to`.
    pub fn retarget_tabs(&mut self, from: &Path, to: &Path) {
        for tab in &mut self.tabs {
            let Ok(rest) = tab.path.strip_prefix(from) else {
                continue;
            };
            // join("") adds a trailing slash, file then fails to open
            tab.path = if rest.as_os_str().is_empty() { to.to_path_buf() } else { to.join(rest) };
            if let Some(name) = tab.path.file_name() {
                tab.title = name.to_string_lossy().to_string();
            }
        }
    }

    pub fn active_buffer(&self) -> Option<&EditorBuffer> {
        self.active_tab.and_then(|idx| self.tabs.get(idx))
    }

    pub fn active_buffer_mut(&mut self) -> Option<&mut EditorBuffer> {
        self.active_tab.and_then(|idx| self.tabs.get_mut(idx))
    }

    /// Resolves the folder associated with current context:
    /// parent of active file, or selected folder, or workspace root.
    pub fn active_folder(&self) -> Option<PathBuf> {
        if let Some(buf) = self.active_buffer()
            && let Some(parent) = buf.path.parent()
            && parent.is_dir()
        {
            return Some(parent.to_path_buf());
        }
        if let Some(ref sel) = self.tree_state.selected {
            if sel.is_dir() {
                return Some(sel.clone());
            } else if let Some(p) = sel.parent()
                && p.is_dir()
            {
                return Some(p.to_path_buf());
            }
        }
        if self.root.is_dir() { Some(self.root.clone()) } else { None }
    }

    /// Starts a grep run: its generation and the blocking search to run off the
    /// UI thread. An empty query just clears the results.
    pub fn begin_grep(&mut self) -> Option<(u64, impl FnOnce() -> Vec<GrepMatch> + Send + use<>)> {
        self.grep_generation += 1;
        if self.grep_options.query.trim().is_empty() {
            self.grep_results.clear();
            self.grep_running = false;
            return None;
        }
        self.grep_running = true;
        let (root, files, options) =
            (self.root.clone(), self.index.files.clone(), self.grep_options.clone());
        Some((self.grep_generation, move || grep_files(&root, &files, &options, GREP_LIMIT)))
    }

    /// Takes the matches of run `generation` unless a newer run started since.
    pub fn finish_grep(&mut self, generation: u64, matches: Vec<GrepMatch>) {
        if generation == self.grep_generation {
            self.grep_results = matches;
            self.grep_running = false;
        }
    }

    /// Sets the Quick Open query and re-matches the file index against it.
    pub fn set_quick_open_query(&mut self, query: String) {
        self.quick_open_items =
            quick_open_matches(&self.root, &self.index.files, self.index.lower_files(), &query);
        self.quick_open_query = query;
        self.quick_open_selected = 0;
    }

    /// The best Quick Open matches for the current query.
    pub fn quick_open_items(&self) -> &[QuickOpenItem] {
        &self.quick_open_items
    }
}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
