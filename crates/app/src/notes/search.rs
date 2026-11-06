//! Read-only global note discovery and previews, separate from Explorer state.
use super::*;
use gpui_kit::{FocusHandle, UniformListScrollHandle};
use std::fs;

pub(crate) const CONTEXT: &str = "NotesSearch";
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    #[default]
    Files,
    Grep,
}
#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub path: PathBuf,
    pub label: String,
    pub line: Option<usize>,
    pub snippet: String,
}
#[derive(Clone, Debug)]
pub(crate) struct Preview {
    pub text: String,
    pub first_line: usize,
    pub matched_line: Option<usize>,
    pub markdown: bool,
    pub truncated: bool,
}
#[derive(Default)]
pub(crate) struct SearchState {
    pub open: bool,
    pub mode: Mode,
    pub generation: u64,
    pub preview_generation: u64,
    pub selected: usize,
    pub results: Vec<Hit>,
    pub loading: bool,
    pub preview_loading: bool,
    pub preview: Option<Preview>,
    pub error: Option<String>,
    pub restore: Option<FocusHandle>,
    pub scroll: UniformListScrollHandle,
}
fn effective(query: &str, mode: Mode) -> (Mode, &str) {
    if let Some(query) = query.strip_prefix("/s ") {
        (Mode::Grep, query.trim())
    } else {
        (mode, query.trim())
    }
}
fn score(path: &str, query: &str) -> Option<usize> {
    let path = path.to_lowercase();
    let query = query.to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let name = Path::new(&path).file_name().unwrap_or_default().to_string_lossy();
    if name == query {
        return Some(0);
    }
    if name.starts_with(&query) {
        return Some(1);
    }
    if name.contains(&query) {
        return Some(2);
    }
    if path.contains(&query) {
        return Some(3);
    }
    let mut chars = query.chars().filter(|c| !c.is_whitespace());
    let mut next = chars.next();
    for c in path.chars() {
        if Some(c) == next {
            next = chars.next();
            if next.is_none() {
                return Some(4);
            }
        }
    }
    None
}
pub(crate) fn find(store: Store, query: &str, mode: Mode) -> Result<Vec<Hit>, String> {
    let store = store.scoped(None)?;
    let (mode, query) = effective(query, mode);
    let mut filter = NoteQuery::default();
    if let Some(tag) = query.strip_prefix("/t ") {
        filter.tag = tag.trim().into();
    }
    if let Some(name) = query.strip_prefix("/b ") {
        filter.backlink = name.trim().into();
    }
    let special = !filter.tag.is_empty() || !filter.backlink.is_empty();
    if mode == Mode::Files && query.is_empty() {
        filter.recent = Some(50);
    }
    let entries = store.list(&filter)?;
    let mut hits = Vec::new();
    if mode == Mode::Grep && query.is_empty() {
        return Ok(Vec::new());
    }
    let needle = query.to_lowercase();
    for entry in entries {
        let label =
            entry.path.strip_prefix(store.root()).unwrap_or(&entry.path).display().to_string();
        if mode == Mode::Files {
            if let Some(rank) = if special { Some(0) } else { score(&label, query) } {
                hits.push((
                    rank,
                    Hit { path: entry.path, label, line: None, snippet: String::new() },
                ));
            }
        } else {
            if fs::metadata(&entry.path).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
                continue;
            }
            let Ok(text) = fs::read_to_string(&entry.path) else {
                continue;
            };
            for (row, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    hits.push((
                        0,
                        Hit {
                            path: entry.path.clone(),
                            label: label.clone(),
                            line: Some(row + 1),
                            snippet: line.trim().chars().take(160).collect(),
                        },
                    ));
                    if hits.len() >= 200 {
                        break;
                    }
                }
            }
            if hits.len() >= 200 {
                break;
            }
        }
    }
    if !query.is_empty() {
        hits.sort_by(|(a, x), (b, y)| {
            a.cmp(b).then(x.label.cmp(&y.label)).then(x.line.cmp(&y.line))
        });
    }
    Ok(hits.into_iter().take(200).map(|(_, hit)| hit).collect())
}
pub(crate) fn preview(hit: &Hit) -> Result<Preview, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    let file = fs::File::open(&hit.path).map_err(|e| e.to_string())?;
    let limit = if hit.line.is_some() { 4 * 1024 * 1024 } else { 512 * 1024 };
    let mut limited = file.take(limit + 1);
    limited.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let truncated = bytes.len() > limit as usize;
    bytes.truncate(limit as usize);
    if bytes.contains(&0) {
        return Err("Binary files cannot be previewed.".into());
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let markdown =
        hit.line.is_none()
            && hit.path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                s.eq_ignore_ascii_case("md") || s.eq_ignore_ascii_case("markdown")
            });
    if let Some(line) = hit.line {
        let start = line.saturating_sub(9);
        let lines: Vec<_> = text.lines().skip(start).take(19).collect();
        return Ok(Preview {
            text: lines.join("\n"),
            first_line: start + 1,
            matched_line: Some(line),
            markdown: false,
            truncated,
        });
    }
    Ok(Preview { text, first_line: 1, matched_line: None, markdown, truncated })
}
impl ChatApp {
    pub(crate) fn toggle_notes_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.notes.search_ui.open {
            self.close_notes_search(window, cx);
            return;
        }
        if !self.settings.notes_enabled {
            return;
        }
        let restore = if self.palette_open {
            self.palette_restore.take().or_else(|| window.focused(cx))
        } else {
            window.focused(cx)
        };
        self.palette_open = false;
        self.notes.search_ui.open = true;
        self.notes.search_ui.restore = restore;
        self.notes.search_ui.mode = Mode::Files;
        self.notes.search.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.queue_notes_search(cx);
        cx.notify();
    }
    pub(crate) fn close_notes_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.notes.search_ui.open = false;
        self.notes.search_ui.generation += 1;
        self.notes.search_ui.preview_generation += 1;
        if let Some(focus) = self.notes.search_ui.restore.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }
    pub(crate) fn queue_notes_search(&mut self, cx: &mut Context<Self>) {
        if !self.notes.search_ui.open {
            return;
        }
        let query = self.notes.search.read(cx).value().to_string();
        let mode = self.notes.search_ui.mode;
        let build = self.notes_store_task();
        self.notes.search_ui.generation += 1;
        let generation = self.notes.search_ui.generation;
        self.notes.search_ui.preview_generation += 1;
        self.notes.search_ui.preview = None;
        self.notes.search_ui.preview_loading = false;
        self.notes.search_ui.results.clear();
        self.notes.search_ui.selected = 0;
        self.notes.search_ui.loading = true;
        self.notes.search_ui.error = None;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(120)).await;
            if !this
                .update(cx, |this, _| {
                    this.notes.search_ui.open && this.notes.search_ui.generation == generation
                })
                .unwrap_or(false)
            {
                return;
            }
            let result =
                cx.background_executor().spawn(async move { find(build()?, &query, mode) }).await;
            this.update(cx, |this, cx| {
                if !this.notes.search_ui.open || this.notes.search_ui.generation != generation {
                    return;
                }
                this.notes.search_ui.loading = false;
                match result {
                    Ok(hits) => {
                        this.notes.search_ui.results = hits;
                        this.notes
                            .search_ui
                            .scroll
                            .scroll_to_item(0, gpui_kit::ScrollStrategy::Top);
                        this.preview_note_result(cx);
                    }
                    Err(error) => this.notes.search_ui.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn preview_note_result(&mut self, cx: &mut Context<Self>) {
        let Some(hit) = self.notes.search_ui.results.get(self.notes.search_ui.selected).cloned()
        else {
            return;
        };
        self.notes.search_ui.preview_generation += 1;
        let generation = self.notes.search_ui.preview_generation;
        self.notes.search_ui.preview = None;
        self.notes.search_ui.preview_loading = true;
        self.notes.search_ui.error = None;
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { preview(&hit) }).await;
            this.update(cx, |this, cx| {
                if !this.notes.search_ui.open
                    || this.notes.search_ui.preview_generation != generation
                {
                    return;
                }
                this.notes.search_ui.preview_loading = false;
                match result {
                    Ok(preview) => this.notes.search_ui.preview = Some(preview),
                    Err(error) => this.notes.search_ui.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(crate) fn move_note_result(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.notes.search_ui.results.len();
        if len == 0 {
            return;
        }
        self.notes.search_ui.selected =
            (self.notes.search_ui.selected as isize + delta).rem_euclid(len as isize) as usize;
        self.notes
            .search_ui
            .scroll
            .scroll_to_item(self.notes.search_ui.selected, gpui_kit::ScrollStrategy::Nearest);
        self.preview_note_result(cx);
        cx.notify();
    }
    pub(crate) fn open_note_result(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.notes.search_ui.loading || self.notes.busy {
            return;
        }
        let Some(hit) = self.notes.search_ui.results.get(self.notes.search_ui.selected).cloned()
        else {
            return;
        };
        self.close_notes_search(window, cx);
        self.notes.project = None;
        self.notes.listing = Listing::All;
        self.open_note_at(hit.path, None, hit.line, window, cx);
    }
}
#[cfg(test)]
#[path = "../../tests/unit/notes/search.rs"]
mod tests;
