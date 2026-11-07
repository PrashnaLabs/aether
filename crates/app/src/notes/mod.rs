//! Personal notes share files across Aether's editor and Neovim/tmux.
pub(crate) mod external;
pub(crate) mod search;
mod search_view;
pub(crate) mod store;
pub(crate) mod view;

use crate::app::{ChatApp, Page};
use aether_core::{NotesEditor, NotesSettings};
use aether_explorer::ExplorerState;
use gpui_kit::component::input::InputState;
use gpui_kit::{App, Context, Entity, SharedString, Window, prelude::*};
use std::path::{Path, PathBuf};
use store::{NoteKind, NoteQuery, Store};

#[derive(Clone)]
pub(crate) enum Form {
    Create(NoteKind),
    Rename(PathBuf),
    ArchiveAge,
    EmptyTrash,
    Capture,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Listing {
    All,
    Trash,
    Templates,
    Archive,
}

pub(crate) struct NotesModel {
    pub(crate) search_ui: search::SearchState,
    pub(crate) focus_tree: bool,
    pub(crate) error: Option<SharedString>,
    pub(crate) message: Option<SharedString>,
    pub(crate) explorer: Option<Entity<ExplorerState>>,
    pub(crate) terminal_connection: Option<SharedString>,
    pub(crate) search: Entity<InputState>,
    pub(crate) name: Entity<InputState>,
    pub(crate) template: Entity<InputState>,
    pub(crate) form: Option<Form>,
    pub(crate) initial: String,
    pub(crate) project: Option<String>,
    /// Active project folder when notes last opened; a change re-scopes project notes.
    project_folder: Option<PathBuf>,
    pub(crate) listing: Listing,
    pub(crate) entries: Vec<(PathBuf, String)>,
    pub(crate) generation: u64,
    pub(crate) loading: bool,
    pub(crate) busy: bool,
    pub(crate) open_generation: u64,
    #[cfg(test)]
    pub(crate) last_note_file: Option<PathBuf>,
}
impl NotesModel {
    pub(crate) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            search_ui: search::SearchState::default(),
            focus_tree: false,
            error: None,
            message: None,
            explorer: None,
            terminal_connection: None,
            search: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Find notes · /s content · /t tag · /b backlinks")
            }),
            name: cx
                .new(|cx| InputState::new(window, cx).placeholder("Note name or relative path")),
            template: cx
                .new(|cx| InputState::new(window, cx).placeholder("Template name (optional)")),
            form: None,
            initial: String::new(),
            project: None,
            project_folder: None,
            listing: Listing::All,
            entries: Vec::new(),
            generation: 0,
            loading: false,
            busy: false,
            open_generation: 0,
            #[cfg(test)]
            last_note_file: None,
        }
    }
}
pub(crate) fn directory(value: &str) -> PathBuf {
    if value == "~" {
        aether_core::home_dir()
    } else if let Some(path) = value.strip_prefix("~/") {
        aether_core::home_dir().join(path)
    } else {
        PathBuf::from(value)
    }
}
impl ChatApp {
    pub(crate) fn notes_store(&self) -> Result<Store, String> {
        let store = Store::new(directory(&self.settings.notes.directory));
        #[cfg(test)]
        let store = if let Some(file) = &self.notes.last_note_file {
            store.with_last_note_file(file.clone())
        } else {
            store
        };
        store.scoped(self.notes.project.as_deref())
    }
    fn notes_store_task(&self) -> Box<dyn FnOnce() -> Result<Store, String> + Send> {
        let root = directory(&self.settings.notes.directory);
        let project = self.notes.project.clone();
        #[cfg(test)]
        let last = self.notes.last_note_file.clone();
        Box::new(move || {
            let store = Store::new(root);
            #[cfg(test)]
            let store = if let Some(last) = last { store.with_last_note_file(last) } else { store };
            store.scoped(project.as_deref())
        })
    }
    pub(crate) fn notes_explorer(&mut self, cx: &mut Context<Self>) -> Entity<ExplorerState> {
        if let Some(exp) = &self.notes.explorer {
            return exp.clone();
        }
        let root = Store::new(directory(&self.settings.notes.directory)).root().to_path_buf();
        let exp = cx.new(|_| ExplorerState::new(root));
        self.notes.explorer = Some(exp.clone());
        self.load_explorer_index(&exp, cx);
        exp
    }
    pub(crate) fn open_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings.notes_enabled {
            return;
        }
        let folder = self.active_project_folder();
        if (self.settings.notes.project_scope || self.notes.project.is_some())
            && self.notes.project_folder.as_ref() != Some(&folder)
        {
            self.notes.project = folder.file_name().map(|s| s.to_string_lossy().to_string());
        }
        self.notes.project_folder = Some(folder);
        self.open_page(Page::Notes, cx);
        let root = self.notes_explorer(cx).read(cx).root.clone();
        self.check_explorer_disk(&root, window, cx);
        self.refresh_notes(cx);
        self.notes.focus_tree = true;
    }
    pub(crate) fn refresh_notes(&mut self, cx: &mut Context<Self>) {
        self.notes.generation += 1;
        let generation = self.notes.generation;
        let build_store = self.notes_store_task();
        let text = self.notes.search.read(cx).value().to_string();
        let listing = self.notes.listing;
        let project = self.notes.project.clone();
        self.notes.loading = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(120)).await;
            if !this.update(cx, |this, _| this.notes.generation == generation).unwrap_or(false) {
                return;
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = build_store()?;
                    let archive_prefix = project
                        .as_ref()
                        .map(|p| PathBuf::from("archive").join("projects").join(p))
                        .unwrap_or_else(|| PathBuf::from("archive"));
                    let store =
                        if listing == Listing::Archive { store.scoped(None)? } else { store };
                    if listing == Listing::Templates {
                        return store.templates().map(|paths| {
                            paths
                                .into_iter()
                                .map(|p| {
                                    let name = p
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string();
                                    (p, name)
                                })
                                .collect()
                        });
                    }
                    if listing == Listing::Trash {
                        return store.trash_list().map(|paths| {
                            paths
                                .into_iter()
                                .map(|p| {
                                    let name = p
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string();
                                    (p, name)
                                })
                                .collect()
                        });
                    }
                    let mut query = NoteQuery::default();
                    if let Some(s) = text.strip_prefix("/s ") {
                        query.content = s.to_string();
                    } else if let Some(s) = text.strip_prefix("/t ") {
                        query.tag = s.to_string();
                    } else if let Some(s) = text.strip_prefix("/b ") {
                        query.backlink = s.to_string();
                    } else {
                        query.filename = text;
                    }

                    query.include_archive = listing == Listing::Archive;
                    store.list(&query).map(|entries| {
                        entries
                            .into_iter()
                            .filter(|entry| {
                                listing != Listing::Archive
                                    || entry.relative_path.starts_with(&archive_prefix)
                            })
                            .map(|entry| (entry.path, entry.relative_path.display().to_string()))
                            .collect::<Vec<_>>()
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                if this.notes.generation != generation {
                    return;
                }
                this.notes.loading = false;
                match result {
                    Ok(entries) => {
                        this.notes.entries = entries;
                        this.notes.error = None;
                    }
                    Err(error) => this.notes.error = Some(error.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    fn reset_notes_directory_input(&mut self, cx: &mut Context<Self>) {
        let Some(window) = cx.active_window() else {
            return;
        };
        let input = self.notes_setting_inputs.get("directory").cloned();
        let value = self.settings.notes.directory.clone();
        cx.defer(move |cx| {
            if let Some(input) = input {
                window
                    .update(cx, |_, window, cx| {
                        input.update(cx, |s, cx| s.set_value(value, window, cx))
                    })
                    .ok();
            }
        });
    }
    pub(crate) fn update_notes_settings(
        &mut self,
        settings: NotesSettings,
        cx: &mut Context<Self>,
    ) {
        if settings.directory.trim().is_empty() {
            self.notes.error = Some("The notes directory cannot be empty.".into());
            self.reset_notes_directory_input(cx);
            cx.notify();
            return;
        }
        if self.settings.notes.directory != settings.directory {
            if self.notes.busy
                || self
                    .notes
                    .explorer
                    .as_ref()
                    .is_some_and(|e| e.read(cx).tabs.iter().any(|b| b.is_dirty))
            {
                self.notes.error = Some("Save or close unsaved notes and wait for the current operation before changing the notes directory.".into());
                self.reset_notes_directory_input(cx);
                cx.notify();
                return;
            }
            if let Some(exp) = self.notes.explorer.take() {
                for buffer in &exp.read(cx).tabs {
                    self.explorer_editors.remove(&buffer.id);
                    self.explorer_markdown_source.remove(&buffer.id);
                }
            }
            self.notes.generation += 1;
            self.notes.open_generation += 1;
            self.notes.entries.clear();
            // Keep an existing external terminal alive until the user closes it.
        }
        if self.settings.notes.project_scope != settings.project_scope {
            self.notes.project = if settings.project_scope {
                self.active_project_folder().file_name().map(|s| s.to_string_lossy().to_string())
            } else {
                None
            };
        }
        self.notes.error = None;
        self.update_settings(cx, |s| s.notes = settings);
        if self.page == Page::Notes {
            self.refresh_notes(cx);
        }
    }
    pub(crate) fn notes_form(
        &mut self,
        form: Form,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.settings.notes_enabled {
            return;
        }
        if self.notes.busy {
            self.open_notes(window, cx);
            self.notes.message = Some("Wait for the current notes operation to finish.".into());
            cx.notify();
            return;
        }
        self.open_notes(window, cx);
        self.notes.initial = initial;
        self.notes.name.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.focus(window, cx);
        });
        self.notes.template.update(cx, |s, cx| s.set_value("", window, cx));
        if let Form::Rename(path) = &form {
            let name = self
                .notes_store()
                .ok()
                .and_then(|s| {
                    path.strip_prefix(s.scope_root()).ok().map(|p| p.display().to_string())
                })
                .unwrap_or_default();
            self.notes.name.update(cx, |s, cx| s.set_value(name, window, cx));
        }
        self.notes.form = Some(form);
        cx.notify();
    }
    pub(crate) fn submit_note_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.notes.form.clone() else {
            return;
        };
        let name = self.notes.name.read(cx).value().to_string();
        let template = self.notes.template.read(cx).value().to_string();
        let initial = self.notes.initial.clone();
        if let Form::Rename(path) = &form
            && !self.note_clean(path, cx)
        {
            return;
        }
        if matches!(form, Form::ArchiveAge | Form::Capture | Form::EmptyTrash)
            && self
                .notes
                .explorer
                .as_ref()
                .is_some_and(|e| e.read(cx).tabs.iter().any(|b| b.is_dirty))
        {
            self.notes.error = Some("Save unsaved notes before this operation.".into());
            cx.notify();
            return;
        }
        self.note_job(window, cx, move |store| match form {
            Form::Create(kind) => store
                .create(
                    kind,
                    &name,
                    (!template.trim().is_empty()).then_some(template.trim()),
                    &initial,
                )
                .map(NoteResult::Open),
            Form::Capture => store.capture(&name, &initial).map(NoteResult::Open),
            Form::EmptyTrash => store
                .empty_trash()
                .map(|n| NoteResult::Message(format!("Deleted {n} trashed notes"))),
            Form::Rename(path) => {
                store.rename(&path, &name).map(|to| NoteResult::Moved { from: path, to })
            }
            Form::ArchiveAge => name
                .parse::<u64>()
                .map_err(|_| "Enter the number of days.".into())
                .and_then(|days| {
                    store
                        .archive_older_than(days)
                        .map(|n| NoteResult::Message(format!("Archived {n} notes")))
                }),
        });
    }
    pub(crate) fn note_clean(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let dirty = self
            .notes
            .explorer
            .as_ref()
            .is_some_and(|exp| exp.read(cx).tabs.iter().any(|b| b.path == path && b.is_dirty));
        if dirty {
            self.notes.error = Some("Save or close this note's unsaved edits first.".into());
            cx.notify();
        }
        !dirty
    }
    pub(crate) fn note_job(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        job: impl FnOnce(Store) -> Result<NoteResult, String> + Send + 'static,
    ) {
        if self.notes.busy {
            return;
        }
        let build_store = self.notes_store_task();
        self.notes.busy = true;
        self.notes.error = None;
        self.notes.message = None;
        cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_executor().spawn(async move { job(build_store()?) }).await;
            this.update_in(cx, |this, window, cx| {
                this.notes.busy = false;
                match result {
                    Ok(result) => {
                        if let Some(exp) = this.notes.explorer.clone() {
                            let root = exp.read(cx).root.clone();
                            this.check_explorer_disk(&root, window, cx);
                            this.refresh_explorer(&exp, cx);
                        }
                        this.notes.form = None;
                        this.notes.initial.clear();
                        match result {
                            NoteResult::Open(path) => {
                                if this.page == Page::Notes {
                                    this.open_note(path, None, window, cx);
                                }
                            }
                            NoteResult::Moved { from, to } => {
                                if let Some(exp) = this.notes.explorer.clone() {
                                    exp.update(cx, |exp, _| {
                                        if let Some(buffer) =
                                            exp.tabs.iter_mut().find(|b| b.path == from)
                                        {
                                            buffer.path = to.clone();
                                            buffer.title = to
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                                .to_string();
                                            let _ = buffer.reload();
                                        }
                                    });
                                }
                                if this.page == Page::Notes {
                                    this.open_note(to, None, window, cx);
                                }
                            }
                            NoteResult::Message(text) => this.notes.message = Some(text.into()),
                        }
                        this.refresh_notes(cx);
                    }
                    Err(error) => this.notes.error = Some(error.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn open_note(
        &mut self,
        path: PathBuf,
        editor: Option<NotesEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_note_at(path, editor, None, window, cx);
    }
    pub(crate) fn open_note_at(
        &mut self,
        path: PathBuf,
        editor: Option<NotesEditor>,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.notes.busy {
            return;
        }
        if editor.unwrap_or(self.settings.notes.editor) == NotesEditor::Neovim
            && !self.note_clean(&path, cx)
        {
            return;
        }
        self.notes.open_generation += 1;
        let request = self.notes.open_generation;
        let method = editor.unwrap_or(self.settings.notes.editor);
        self.page = Page::Notes;
        self.end_hidden_voice(window, cx);
        let settings = self.settings.notes.clone();
        let root = directory(&settings.directory);
        let build_store = self.notes_store_task();
        let terminal_key: SharedString = format!(
            "notes:{}:{}:{}:{}:{}",
            root.display(),
            settings.session,
            settings.socket,
            settings.nvim,
            settings.tmux
        )
        .into();
        self.notes.busy = true;
        cx.spawn_in(window, async move |this, cx| {
            let external = method == NotesEditor::Neovim;
            let result = cx
                .background_executor()
                .spawn(async move {
                    build_store()?.remember(&path)?;
                    let command = if external {
                        Some(external::prepare(&settings, &root, &path, line)?)
                    } else {
                        None
                    };
                    Ok::<_, String>((path, command, root))
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                if this.notes.open_generation != request {
                    return;
                }
                this.notes.busy = false;
                if this.page != Page::Notes {
                    cx.notify();
                    return;
                }
                match result {
                    Ok((path, command, _root)) => {
                        this.notes.error = None;
                        if let Some(command) = command {
                            this.show_notes_terminal(command, terminal_key, window, cx);
                        } else {
                            let folder = this.notes_explorer(cx).read(cx).root.clone();
                            this.check_explorer_disk(&folder, window, cx);
                            this.open_explorer_file_at(path, line, None, window, cx);
                            if let Some(buffer) = this
                                .notes
                                .explorer
                                .as_ref()
                                .and_then(|e| e.read(cx).active_buffer())
                                .filter(|b| b.original.is_empty())
                            {
                                this.explorer_markdown_source.insert(buffer.id);
                            }
                        }
                    }
                    Err(error) => this.notes.error = Some(error.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn show_notes_terminal(
        &mut self,
        command: String,
        connection: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let terminal = self.ensure_global_terminal(window, cx);
        let running = terminal.read(cx).command_running("notes", cx);
        if running
            && self.notes.terminal_connection.as_ref().is_some_and(|active| active != &connection)
        {
            self.notes.error = Some(
                "Close the running notes terminal before changing its editor connection.".into(),
            );
            cx.notify();
            return;
        }
        terminal.update(cx, |terminal, cx| {
            if running {
                terminal.focus_command("notes", cx);
            } else {
                terminal.run_command("notes".into(), "notes".into(), command, window, cx);
            }
            terminal.open = true;
            terminal.full_page = true;
            terminal.set_shown(true, cx);
        });
        self.notes.terminal_connection = Some(connection);
        self.active_terminal_tab = aether_terminal::TerminalTab::Global;
        self.open_page(Page::Terminal, cx);
        let focus = terminal.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }
    pub(crate) fn save_selection_to_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self
            .active_explorer_editor(window, cx)
            .and_then(|editor| {
                let e = editor.read(cx);
                let range = e.selected_range();
                (!range.is_empty()).then(|| e.text().slice(range).to_string())
            })
            .unwrap_or_default();
        if text.is_empty() {
            self.notes.error = Some("Select text in the editor first.".into());
            cx.notify();
            return;
        }
        self.notes_form(Form::Capture, text, window, cx);
    }
    pub(crate) fn new_note_here(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = Store::new(directory(&self.settings.notes.directory)).root().to_path_buf();
        let project_root = self.active_project_folder();
        let target = if folder.starts_with(&root) {
            let relative = folder.strip_prefix(&root).unwrap();
            let mut parts = relative.components();
            self.notes.project = if parts.next().is_some_and(|p| p.as_os_str() == "projects") {
                parts.next().map(|p| p.as_os_str().to_string_lossy().to_string())
            } else {
                None
            };
            folder
        } else {
            self.notes.project = project_root.file_name().map(|s| s.to_string_lossy().to_string());
            let scope = Store::new(root)
                .scoped(self.notes.project.as_deref())
                .ok()
                .map(|s| s.scope_root().to_path_buf());
            scope
                .unwrap_or_default()
                .join(folder.strip_prefix(&project_root).unwrap_or(Path::new("")))
        };
        self.notes_form(Form::Create(NoteKind::Named), String::new(), window, cx);
        if let Ok(store) = self.notes_store()
            && let Ok(relative) = target.strip_prefix(store.scope_root())
        {
            let prefix = if relative.as_os_str().is_empty() {
                String::new()
            } else {
                format!("{}/", relative.display())
            };
            self.notes.name.update(cx, |s, cx| s.set_value(prefix, window, cx));
        }
    }
}
pub(crate) enum NoteResult {
    Open(PathBuf),
    Message(String),
    Moved { from: PathBuf, to: PathBuf },
}

#[cfg(test)]
#[path = "../../tests/unit/notes/mod.rs"]
mod tests;
