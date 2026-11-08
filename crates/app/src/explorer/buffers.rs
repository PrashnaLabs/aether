use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aether_explorer::{DiskConflict, ExplorerState};
use gpui_kit::component::button::{ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{EditorState, InputEvent, Position, Rope, RopeExt};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{WindowExt, h_flex};
use gpui_kit::{App, Context, Entity, Focusable, SharedString, Window, prelude::*};

use crate::app::ChatApp;

pub(super) fn language_from_path(path: &Path) -> &'static str {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "rs" => "rust",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" | "markdown" => "markdown",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "html" | "htm" => "html",
        "css" => "css",
        "sh" | "bash" | "zsh" => "shell",
        "py" => "python",
        "c" | "h" => "c",
        "cpp" | "hpp" => "cpp",
        "sql" => "sql",
        _ => "",
    }
}

/// 0-based first and last line a selection covers; one ending at a line start stops above it.
fn selected_rows(text: &Rope, range: Range<usize>) -> (usize, usize) {
    let start = text.offset_to_point(range.start).row;
    let end = text.offset_to_point(range.end);
    (start, if end.column == 0 && end.row > start { end.row - 1 } else { end.row })
}

/// `L10`, or `L10-20` for a span of 1-based lines.
fn line_range(start: usize, end: usize) -> String {
    if start == end { format!("L{start}") } else { format!("L{start}-{end}") }
}

/// What Ask Agent adds to the composer: `@rel`, or `@rel:L10-20` and those lines fenced.
fn file_mention(rel: &str, lines: Option<(usize, usize, &str)>, lang: &str) -> String {
    match lines {
        None => format!("@{rel} "),
        Some((start, end, text)) => {
            format!("@{rel}:{}\n```{lang}\n{text}\n```\n", line_range(start, end))
        }
    }
}

/// Replaces the editor's text undoably, keeping its cursor (clamped) and scroll.
fn replace_keeping_cursor(
    editor: &Entity<EditorState>,
    text: String,
    window: &mut Window,
    cx: &mut App,
) {
    editor.update(cx, |e, cx| {
        let (at, scroll) = (e.cursor_position(), e.scroll_offset());
        e.replace_all(text, window, cx);
        let offset = e.text().position_to_offset(&at);
        e.set_selected_range(offset..offset, cx);
        e.set_scroll_offset(scroll, cx);
    });
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().to_string()
}

impl ChatApp {
    /// Tab `ix`'s editor, made from its on-disk text on first use.
    pub(crate) fn tab_editor(
        &mut self,
        explorer: &Entity<ExplorerState>,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<EditorState>> {
        let buf = explorer.read(cx).tabs.get(ix)?;
        let id = buf.id;
        if let Some((editor, _)) = self.explorer_editors.get(&id) {
            return Some(editor.clone());
        }
        let (lang, text) = (language_from_path(&buf.path), buf.original.clone());
        let editor = cx.new(|cx| {
            EditorState::new(window, cx).line_number(true).language(lang).default_value(text)
        });
        let exp = explorer.clone();
        let sub = cx.subscribe(&editor, move |_, editor, ev: &InputEvent, cx| {
            if let InputEvent::Change = ev {
                exp.update(cx, |exp, cx| {
                    if let Some(buf) = exp.tabs.iter_mut().find(|t| t.id == id) {
                        buf.is_dirty = editor.read(cx).text() != &buf.original;
                    }
                });
                cx.notify();
            }
        });
        self.explorer_editors.insert(id, (editor.clone(), sub));
        Some(editor)
    }

    /// The active tab's editor in this chat's explorer.
    pub(crate) fn active_explorer_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<EditorState>> {
        let explorer = self.active_project_explorer(cx);
        let ix = explorer.read(cx).active_tab?;
        self.tab_editor(&explorer, ix, window, cx)
    }

    fn explorer_editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.explorer_editors.values().any(|(e, _)| e.focus_handle(cx).contains_focused(window, cx))
    }

    fn focus_active_explorer_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let explorer = self.active_project_explorer(cx);
        if explorer.read(cx).active_buffer().is_some_and(|buffer| {
            super::editor::is_markdown_file(&buffer.path)
                && !self.explorer_markdown_source.contains(&buffer.id)
        }) {
            window.blur(cx);
            return;
        }
        if let Some(editor) = self.active_explorer_editor(window, cx) {
            editor.update(cx, |e, cx| e.focus(window, cx));
        }
    }

    /// Opens a file in the explorer as a tab.
    pub(crate) fn open_explorer_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_explorer_file_at(path, None, None, window, cx);
    }

    /// Opens a file in the explorer with the cursor on 1-indexed `line` and `col`.
    pub(crate) fn open_explorer_file_at(
        &mut self,
        path: PathBuf,
        line: Option<usize>,
        col: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preview_image(&path, window, cx) {
            return;
        }
        if self.page != crate::app::Page::Notes {
            self.open_explorer_panel(window, cx);
        }
        let refocus = self.explorer_editor_focused(window, cx);
        let name = file_name(&path);
        let explorer = self.active_project_explorer(cx);
        if let Err(err) = explorer.update(cx, |exp, _| exp.open_file(path)) {
            window
                .push_notification(Notification::error(format!("Couldn't open {name}: {err}")), cx);
            return cx.notify();
        }
        if let Some(editor) = self.active_explorer_editor(window, cx) {
            if let Some(line) = line {
                if let Some(buffer) = explorer.read(cx).active_buffer() {
                    self.explorer_markdown_source.insert(buffer.id);
                }
                // a new editor has no layout to scroll yet; its first paint follows the caret
                let col = col.unwrap_or(1).saturating_sub(1);
                let at = Position::new(line.saturating_sub(1) as u32, col as u32);
                editor.update(cx, |e, cx| e.set_cursor_position(at, window, cx));
            } else if refocus {
                self.focus_active_explorer_editor(window, cx);
            }
        }
        cx.notify();
    }

    /// Closes these tabs, asking first about any with unsaved edits.
    pub(crate) fn close_explorer_tabs(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let explorer = self.active_project_explorer(cx);
        let (dirty, clean): (Vec<PathBuf>, Vec<PathBuf>) = paths
            .into_iter()
            .partition(|path| explorer.read(cx).tabs.iter().any(|t| &t.path == path && t.is_dirty));
        self.drop_explorer_tabs(&explorer, &clean, false, window, cx);
        let title = match dirty.as_slice() {
            [] => return,
            [path] => format!("Save changes to {}?", file_name(path)),
            many => format!("Save changes to {} files?", many.len()),
        };
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let finish = {
                let (view, explorer, dirty) = (view.clone(), explorer.clone(), dirty.clone());
                Rc::new(move |save: bool, window: &mut Window, cx: &mut App| {
                    view.update(cx, |this, cx| {
                        this.drop_explorer_tabs(&explorer, &dirty, save, window, cx)
                    })
                })
            };
            let button = |id: &'static str, label: &'static str, save: Option<bool>| {
                let finish = finish.clone();
                aether_ui::button(id).label(label).on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    if let Some(save) = save {
                        finish(save, window, cx);
                    }
                })
            };
            let on_enter = finish.clone();
            alert
                .title(SharedString::from(title.clone()))
                .description("Your changes will be lost if you don't save them.")
                .on_ok(move |_, window, cx| {
                    on_enter(true, window, cx);
                    true
                })
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(button("close-tab-cancel", "Cancel", None).ghost())
                        .child(button("close-tab-discard", "Don't save", Some(false)).danger())
                        .child(button("close-tab-save", "Save", Some(true)).primary()),
                )
        });
    }

    /// Closes `explorer`'s open tabs among `paths` and drops their editors, without
    /// asking; a tab whose save fails stays open.
    pub(crate) fn drop_explorer_tabs(
        &mut self,
        explorer: &Entity<ExplorerState>,
        paths: &[PathBuf],
        save: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let refocus = self.explorer_editor_focused(window, cx);
        for path in paths {
            let Some(id) = explorer.read(cx).tabs.iter().find(|t| &t.path == path).map(|t| t.id)
            else {
                continue;
            };
            if save && !self.save_explorer_tab(explorer, id, window, cx) {
                continue;
            }
            explorer.update(cx, |exp, _| {
                if let Some(ix) = exp.tabs.iter().position(|t| t.id == id) {
                    exp.close_tab(ix);
                }
            });
            self.explorer_editors.remove(&id);
            self.explorer_markdown_source.remove(&id);
        }
        if refocus {
            self.focus_active_explorer_editor(window, cx);
        }
        cx.notify();
    }

    /// Closes the active tab, asking first if it has unsaved edits.
    pub(crate) fn close_active_explorer_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path =
            self.active_project_explorer(cx).read(cx).active_buffer().map(|b| b.path.clone());
        if let Some(path) = path {
            self.close_explorer_tabs(vec![path], window, cx);
        }
    }

    /// Closes every tab but the one at `path`.
    pub(crate) fn close_other_explorer_tabs(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let explorer = self.active_project_explorer(cx);
        let others =
            explorer.read(cx).tabs.iter().map(|t| t.path.clone()).filter(|p| p != path).collect();
        self.close_explorer_tabs(others, window, cx);
    }

    /// Selects the next (or previous) tab, wrapping around.
    pub(crate) fn cycle_explorer_tab(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let explorer = self.active_project_explorer(cx);
        let (len, active) = {
            let exp = explorer.read(cx);
            (exp.tabs.len(), exp.active_tab)
        };
        if let Some(active) = active.filter(|_| len > 1) {
            let next = if forward { (active + 1) % len } else { (active + len - 1) % len };
            self.set_active_explorer_tab(next, window, cx);
        }
    }

    /// Sets the active tab by index.
    pub(crate) fn set_active_explorer_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let explorer = self.active_project_explorer(cx);
        explorer.update(cx, |exp, _| {
            if index < exp.tabs.len() {
                exp.active_tab = Some(index);
                let path = exp.tabs[index].path.clone();
                exp.reveal(&path);
            }
        });
        self.focus_active_explorer_editor(window, cx);
        cx.notify();
    }

    /// Writes tab `id`'s editor text to its file, showing a failure; true once saved.
    fn save_explorer_tab(
        &mut self,
        explorer: &Entity<ExplorerState>,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(ix) = explorer.read(cx).tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        let text = match self.explorer_editors.get(&id) {
            Some((editor, _)) => editor.read(cx).text().to_string(),
            None => explorer.read(cx).tabs[ix].original.clone(),
        };
        let name = explorer.read(cx).tabs[ix].title.clone();
        let saved = explorer.update(cx, |exp, _| exp.tabs[ix].save(text));
        match &saved {
            Ok(()) => {
                if self.page == crate::app::Page::Notes {
                    self.refresh_notes(cx);
                } else {
                    self.refresh_git(false, cx);
                }
            }
            Err(err) => window
                .push_notification(Notification::error(format!("Couldn't save {name}: {err}")), cx),
        }
        cx.notify();
        saved.is_ok()
    }

    /// Saves the active tab, asking first when its file changed on disk since it was read.
    pub(crate) fn save_active_explorer_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let explorer = self.active_project_explorer(cx);
        let Some(buf) = explorer.read(cx).active_buffer() else {
            return;
        };
        let (id, name) = (buf.id, buf.title.clone());
        if buf.conflict != Some(DiskConflict::Changed) {
            self.save_explorer_tab(&explorer, id, window, cx);
            return;
        }
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, explorer) = (view.clone(), explorer.clone());
            alert
                .confirm()
                .title(SharedString::from(format!("Overwrite {name}?")))
                .description("It changed on disk since you opened it. Saving replaces that version with yours.")
                .ok_text("Overwrite")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| this.save_explorer_tab(&explorer, id, window, cx));
                    true
                })
        });
    }

    /// Reverts the active tab to its on-disk baseline (undoable).
    pub(crate) fn revert_active_explorer_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let explorer = self.active_project_explorer(cx);
        let Some(text) = explorer.read(cx).active_buffer().map(|b| b.original.clone()) else {
            return;
        };
        if let Some(editor) = self.active_explorer_editor(window, cx) {
            replace_keeping_cursor(&editor, text, window, cx);
        }
        cx.notify();
    }

    /// Re-reads the active tab from disk over its edits (undoable).
    pub(crate) fn reload_active_explorer_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let explorer = self.active_project_explorer(cx);
        let reloaded = explorer
            .update(cx, |exp, _| exp.active_buffer_mut().map(|b| (b.title.clone(), b.reload())));
        let text = match reloaded {
            Some((_, Ok(text))) => text,
            Some((name, Err(err))) => {
                let note = Notification::error(format!("Couldn't reload {name}: {err}"));
                window.push_notification(note, cx);
                return cx.notify();
            }
            None => return,
        };
        if let Some(editor) = self.active_explorer_editor(window, cx) {
            replace_keeping_cursor(&editor, text, window, cx);
        }
        cx.notify();
    }

    /// Keeps the active tab's edits over its file's disk change.
    pub(crate) fn keep_active_explorer_file(&mut self, cx: &mut Context<Self>) {
        let explorer = self.active_project_explorer(cx);
        let editor =
            explorer.read(cx).active_buffer().and_then(|b| self.explorer_editors.get(&b.id));
        let editor = editor.map(|(editor, _)| editor.clone());
        explorer.update(cx, |exp, cx| {
            if let Some(buf) = exp.active_buffer_mut() {
                buf.keep_mine();
                if let Some(editor) = editor {
                    // file gone: closing must still ask, the text lives only here
                    buf.is_dirty = !buf.path.exists() || editor.read(cx).text() != &buf.original;
                }
            }
        });
        cx.notify();
    }

    /// Re-checks `folder`'s explorer tabs against disk: changed clean tabs reload
    /// keeping their cursor, changed dirty or deleted ones get the banner.
    pub(crate) fn check_explorer_disk(
        &mut self,
        folder: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let explorer = self
            .notes
            .explorer
            .as_ref()
            .filter(|e| e.read(cx).root == folder)
            .cloned()
            .or_else(|| self.project_explorers.get(folder).cloned());
        let Some(explorer) = explorer else {
            return;
        };
        for (id, text) in explorer.update(cx, |exp, _| exp.check_disk()) {
            if let Some((editor, _)) = self.explorer_editors.get(&id) {
                replace_keeping_cursor(editor, text, window, cx);
            }
        }
        cx.notify();
    }

    /// What Ask Agent inserts for `path`, relative to this chat's folder; the
    /// active tab's exact selected text comes along fenced.
    pub(crate) fn explorer_file_mention(&mut self, path: &Path, cx: &mut Context<Self>) -> String {
        let rel =
            path.strip_prefix(self.active_project_folder()).unwrap_or(path).display().to_string();
        let explorer = self.active_project_explorer(cx);
        let editor = explorer
            .read(cx)
            .active_buffer()
            .filter(|b| b.path == path)
            .and_then(|b| self.explorer_editors.get(&b.id))
            .map(|(editor, _)| editor.read(cx));
        let lines = editor.filter(|e| !e.selected_range().is_empty()).map(|e| {
            let (start, end) = selected_rows(e.text(), e.selected_range());
            (start + 1, end + 1, e.selected_text().to_string())
        });
        let lines = lines.as_ref().map(|(start, end, text)| (*start, *end, text.as_str()));
        file_mention(&rel, lines, language_from_path(path))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/explorer/buffers.rs"]
mod tests;
