//! Session presentation state. Conversation/driver ownership stays in ChatApp.
mod bindings;
mod snapshot;
use super::*;
use crate::session::{ChatId, CommandTarget};
use aether_ui::{AgentSelectorEvent, ChoiceMenuEvent, WorkspaceSelectEvent};
use gpui_kit::component::input::InputEvent;
use gpui_kit::{AppContext, WeakEntity};

pub(crate) struct SessionState {
    palette: Entity<CommandState>,
    palette_query: Entity<InputState>,
    palette_open: bool,
    palette_restore: Option<FocusHandle>,
    palette_rows: Vec<Vec<Option<PaletteCommand>>>,
    composer: Entity<TextareaState>,
    scroller: Entity<ChatScrollerState>,
    rows: Vec<TranscriptRow>,
    draft_editor: Option<Entity<EditorState>>,
    folded: bool,
    selector: Entity<AgentSelectorState>,
    access_menu: Entity<ChoiceMenuState>,
    project_menu: Entity<ChoiceMenuState>,
    headline_project_menu: Entity<ChoiceMenuState>,
    worktree_menu: Entity<WorkspaceSelectState>,
    worktree_name_input: Entity<InputState>,
    worktree_base_select: Option<Entity<BranchSelectState>>,
    pending_worktree_dialog: bool,
    pending_worktree_dialog_base: Option<String>,
    editing: Option<(i64, usize)>,
    composer_history_index: Option<usize>,
    composer_draft: Option<String>,
    attach_error: Option<SharedString>,
    open_error: Option<SharedString>,
    suggestions: Option<Suggestions>,
    suggestions_dismissed: Option<(usize, char)>,
    copied: Option<(usize, usize)>,
    show_usage_limits: bool,
    workspace_pop: WorkspacePopover,
}
impl SessionState {
    pub(crate) fn is_editing(&self, id: i64, message: usize) -> bool {
        self.editing == Some((id, message))
    }
    fn new(window: &mut Window, cx: &mut Context<ChatApp>) -> Self {
        Self {
            palette: cx.new(|cx| CommandState::new(window, cx)),
            palette_query: cx
                .new(|cx| InputState::new(window, cx).placeholder("Type a command or search…")),
            palette_open: false,
            palette_restore: None,
            palette_rows: Vec::new(),
            composer: cx
                .new(|cx| TextareaState::new(window, cx).auto_grow(1, 10).submit_on_enter(true)),
            scroller: cx.new(|cx| ChatScrollerState::new(0, cx)),
            rows: Vec::new(),
            draft_editor: None,
            folded: false,
            selector: cx.new(|cx| AgentSelectorState::new(window, cx)),
            access_menu: cx.new(ChoiceMenuState::new),
            project_menu: cx.new(ChoiceMenuState::new),
            headline_project_menu: cx.new(|cx| ChoiceMenuState::new(cx).below()),
            worktree_menu: cx.new(|cx| WorkspaceSelectState::new(window, cx)),
            worktree_name_input: cx.new(|cx| InputState::new(window, cx)),
            worktree_base_select: None,
            pending_worktree_dialog: false,
            pending_worktree_dialog_base: None,
            editing: None,
            composer_history_index: None,
            composer_draft: None,
            attach_error: None,
            open_error: None,
            suggestions: None,
            suggestions_dismissed: None,
            copied: None,
            show_usage_limits: false,
            workspace_pop: WorkspacePopover::new(window, cx),
        }
    }
    fn swap(&mut self, app: &mut ChatApp) {
        std::mem::swap(&mut self.palette, &mut app.palette);
        std::mem::swap(&mut self.palette_query, &mut app.palette_query);
        std::mem::swap(&mut self.palette_open, &mut app.palette_open);
        std::mem::swap(&mut self.palette_restore, &mut app.palette_restore);
        std::mem::swap(&mut self.palette_rows, &mut app.palette_rows);
        std::mem::swap(&mut self.composer, &mut app.composer);
        std::mem::swap(&mut self.scroller, &mut app.scroller);
        std::mem::swap(&mut self.rows, &mut app.rows);
        std::mem::swap(&mut self.draft_editor, &mut app.draft_editor);
        std::mem::swap(&mut self.folded, &mut app.folded);
        std::mem::swap(&mut self.selector, &mut app.selector);
        std::mem::swap(&mut self.access_menu, &mut app.access_menu);
        std::mem::swap(&mut self.project_menu, &mut app.project_menu);
        std::mem::swap(&mut self.headline_project_menu, &mut app.headline_project_menu);
        std::mem::swap(&mut self.worktree_menu, &mut app.worktree_menu);
        std::mem::swap(&mut self.worktree_name_input, &mut app.worktree_name_input);
        std::mem::swap(&mut self.worktree_base_select, &mut app.worktree_base_select);
        std::mem::swap(&mut self.pending_worktree_dialog, &mut app.pending_worktree_dialog);
        std::mem::swap(
            &mut self.pending_worktree_dialog_base,
            &mut app.pending_worktree_dialog_base,
        );
        std::mem::swap(&mut self.editing, &mut app.editing);
        std::mem::swap(&mut self.composer_history_index, &mut app.composer_history_index);
        std::mem::swap(&mut self.composer_draft, &mut app.composer_draft);
        std::mem::swap(&mut self.attach_error, &mut app.attach_error);
        std::mem::swap(&mut self.open_error, &mut app.open_error);
        std::mem::swap(&mut self.suggestions, &mut app.suggestions);
        std::mem::swap(&mut self.suggestions_dismissed, &mut app.suggestions_dismissed);
        std::mem::swap(&mut self.copied, &mut app.copied);
        std::mem::swap(&mut self.show_usage_limits, &mut app.show_usage_limits);
        std::mem::swap(&mut self.workspace_pop, &mut app.workspace_pop);
    }
}

#[derive(Clone)]
pub(crate) struct SessionHandle {
    app: Entity<ChatApp>,
    target: CommandTarget,
}
#[derive(Clone)]
pub(crate) struct WeakSessionHandle {
    app: WeakEntity<ChatApp>,
    target: CommandTarget,
}
impl SessionHandle {
    pub(crate) fn downgrade(&self) -> WeakSessionHandle {
        WeakSessionHandle { app: self.app.downgrade(), target: self.target }
    }
    pub(crate) fn update<R: Default, C: AppContext>(
        &self,
        cx: &mut C,
        f: impl FnOnce(&mut ChatApp, &mut Context<ChatApp>) -> R,
    ) -> R {
        self.app.update(cx, |app, cx| {
            if self.target.resolve(&app.chats).is_none() {
                return R::default();
            }
            app.in_session(self.target.chat.0, cx, f).unwrap_or_default()
        })
    }
}
impl WeakSessionHandle {
    pub(crate) fn update<R: Default, C: AppContext>(
        &self,
        cx: &mut C,
        f: impl FnOnce(&mut ChatApp, &mut Context<ChatApp>) -> R,
    ) -> Result<R, ()> {
        let app = self.app.upgrade().ok_or(())?;
        Ok(SessionHandle { app, target: self.target }.update(cx, f))
    }
}
impl ChatApp {
    pub(crate) fn session_handle(&self, cx: &Context<Self>) -> SessionHandle {
        let chat = &self.chats[self.active];
        SessionHandle {
            app: cx.entity(),
            target: CommandTarget { chat: ChatId(chat.id), epoch: chat.epoch },
        }
    }
    pub(crate) fn session_listener<E: ?Sized>(
        &self,
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &E, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        let handle = self.session_handle(cx);
        move |event, window, cx| {
            handle.update(cx, |app, cx| f(app, event, window, cx));
        }
    }
    pub(crate) fn in_session<R>(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Context<Self>) -> R,
    ) -> Option<R> {
        let ix = self.chats.iter().position(|c| c.id == id)?;
        let prev = self.ui_owner;
        let prev_draft_owner = self.draft_owner;
        let prev_active = self.chats.get(self.active).map(|c| c.id);
        let detached = self
            .workspace
            .model
            .pane_for(id)
            .and_then(|pane| self.workspace.model.window_of(pane))
            .is_some_and(|host| host != 0);
        let page = self.page;
        self.swap_session_ui(id);
        self.active = ix;
        self.draft_owner = Some(id);
        // detached windows have no page; main's Project/Settings page mustn't strip their tabs
        if detached {
            self.page = Page::Chat;
        }
        let result = f(self, cx);
        if detached && self.page == Page::Chat {
            self.page = page;
        }
        if let Some(prev) = prev {
            self.swap_session_ui(prev);
        }
        self.draft_owner = prev_draft_owner;
        if let Some(ix) = prev_active.and_then(|id| self.chats.iter().position(|c| c.id == id)) {
            self.active = ix;
        }
        Some(result)
    }
    fn swap_session_ui(&mut self, id: i64) {
        if self.ui_owner == Some(id) {
            return;
        }
        if let Some(mut next) = self.session_views.remove(&id) {
            next.swap(self);
            if let Some(prev) = self.ui_owner
                && self.chats.iter().any(|c| c.id == prev)
            {
                self.session_views.insert(prev, next);
            }
            self.ui_owner = Some(id);
        }
    }
    pub(crate) fn ensure_session_ui(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui_owner == Some(id) || self.session_views.contains_key(&id) {
            if self.session_windows.get(&id) != Some(&window.window_handle().window_id()) {
                self.rebind_existing_session_ui(id, window, cx);
            }
            return;
        }
        let next = SessionState::new(window, cx);
        self.session_views.insert(id, next);
        let prev = self.ui_owner;
        self.swap_session_ui(id);
        let saved_active = self.active;
        if let Some(ix) = self.chats.iter().position(|c| c.id == id) {
            self.active = ix;
            let draft = self.chats[ix].draft.clone();
            self.composer.update(cx, |s, cx| s.set_value(draft, window, cx));
            self.bind_session_widgets(id, window, cx);
            self.reset_rows(cx);
            self.apply_restored_session(id, window, cx);
            self.sync_selector(cx);
        }
        if let Some(prev) = prev {
            self.swap_session_ui(prev);
        }
        self.active = saved_active;
    }
    pub(crate) fn activate_session_ui(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui_owner.is_none() {
            self.ui_owner = Some(id);
            self.bind_session_widgets(id, window, cx);
            self.apply_restored_session(id, window, cx);
        } else {
            self.ensure_session_ui(id, window, cx);
            self.swap_session_ui(id);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/view.rs"]
mod tests;
