use super::*;

impl ChatApp {
    pub(crate) fn session_snapshots(
        &self,
        cx: &App,
    ) -> std::collections::BTreeMap<i64, crate::session_workspace::SessionSnapshot> {
        use crate::session_workspace::SessionSnapshot;
        self.chats
            .iter()
            .map(|chat| {
                let ui = self.session_views.get(&chat.id);
                let fields = if self.ui_owner == Some(chat.id) {
                    Some((
                        &self.composer,
                        &self.scroller,
                        &self.draft_editor,
                        self.editing,
                        self.composer_history_index,
                        &self.composer_draft,
                    ))
                } else {
                    ui.map(|ui| {
                        (
                            &ui.composer,
                            &ui.scroller,
                            &ui.draft_editor,
                            ui.editing,
                            ui.composer_history_index,
                            &ui.composer_draft,
                        )
                    })
                };
                let mut snapshot =
                    if let Some((composer, scroller, editor, editing, history, history_draft)) =
                        fields
                    {
                        SessionSnapshot {
                            text: composer.read(cx).value().to_string(),
                            selection: composer.read(cx).selected_range(),
                            editor: editor.as_ref().map(|e| e.read(cx).value().to_string()),
                            editor_selection: editor
                                .as_ref()
                                .map(|e| e.read(cx).selected_range())
                                .unwrap_or_default(),
                            editing,
                            history,
                            history_draft: history_draft.clone(),
                            scroll: scroller.read(cx).snapshot(),
                            ..Default::default()
                        }
                    } else {
                        self.workspace.restored_drafts.get(&chat.id).cloned().unwrap_or_else(|| {
                            SessionSnapshot { text: chat.draft.clone(), ..Default::default() }
                        })
                    };
                snapshot.pending = chat.pending.clone();
                snapshot.edit_stash = chat.edit_stash.clone();
                (chat.id, snapshot)
            })
            .collect()
    }
    pub(super) fn apply_restored_session(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = self.workspace.restored_drafts.remove(&id) else { return };
        fn selection(text: &str, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
            let clamp = |at: usize| {
                let mut at = at.min(text.len());
                while !text.is_char_boundary(at) {
                    at -= 1;
                }
                at
            };
            let start = clamp(range.start);
            let end = clamp(range.end);
            start.min(end)..start.max(end)
        }
        let range = selection(&snapshot.text, snapshot.selection);
        self.composer.update(cx, |s, cx| {
            s.set_value(snapshot.text, window, cx);
            s.set_selected_range(range, cx);
        });
        self.editing = snapshot.editing.filter(|(chat, i)| {
            *chat == id
                && self.chats[self.active]
                    .messages
                    .get(*i)
                    .is_some_and(|m| m.role == aether_core::Role::User)
        });
        self.composer_history_index = snapshot.history;
        self.composer_draft = snapshot.history_draft;
        if let Some(text) = snapshot.editor {
            let range = selection(&text, snapshot.editor_selection);
            let editor =
                cx.new(|cx| EditorState::new(window, cx).language("markdown").default_value(text));
            editor.update(cx, |s, cx| s.set_selected_range(range, cx));
            self.draft_editor = Some(editor);
        }
        self.reset_rows(cx);
        self.scroller.update(cx, |s, cx| s.restore_snapshot(snapshot.scroll, cx));
    }
    pub(crate) fn rebind_session_ui(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui_owner != Some(id) && !self.session_views.contains_key(&id) {
            self.ensure_session_ui(id, window, cx);
        } else {
            self.rebind_existing_session_ui(id, window, cx);
        }
    }
    pub(super) fn rebind_existing_session_ui(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.in_session(id, cx, |app, cx| {
            use gpui_kit::EntityInputHandler;
            app.composer.update(cx, |state, cx| state.unmark_text(window, cx));
            let text = app.composer.read(cx).value().to_string();
            let selection = app.composer.read(cx).selected_range();
            let scroll = app.scroller.read(cx).snapshot();
            let editor = app
                .draft_editor
                .as_ref()
                .map(|e| (e.read(cx).value().to_string(), e.read(cx).selected_range()));
            let next = SessionState::new(window, cx);
            app.composer = next.composer;
            app.scroller = next.scroller;
            app.selector = next.selector;
            app.access_menu = next.access_menu;
            app.project_menu = next.project_menu;
            app.headline_project_menu = next.headline_project_menu;
            app.worktree_menu = next.worktree_menu;
            let worktree_name = app.worktree_name_input.read(cx).value().to_string();
            app.worktree_name_input = next.worktree_name_input;
            app.worktree_name_input
                .update(cx, |state, cx| state.set_value(worktree_name, window, cx));
            app.workspace_pop = next.workspace_pop;
            app.palette = next.palette;
            app.palette_query = next.palette_query;
            app.palette_open = false;
            app.palette_restore = None;
            app.palette_rows.clear();
            app.worktree_base_select = None;
            app.composer.update(cx, |state, cx| {
                state.set_value(text, window, cx);
                state.set_selected_range(selection, cx);
            });
            if let Some((text, selection)) = editor {
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx).language("markdown").default_value(text)
                });
                editor.update(cx, |state, cx| state.set_selected_range(selection, cx));
                app.draft_editor = Some(editor);
            }
            app.rows.clear();
            app.reset_rows(cx);
            app.scroller.update(cx, |state, cx| state.restore_snapshot(scroll, cx));
            app.suggestions = None;
            app.suggestions_dismissed = None;
            app.bind_session_widgets(id, window, cx);
            app.sync_selector(cx);
            app.rebind_session_requests(id, window, cx);
            cx.notify();
        });
    }
}
