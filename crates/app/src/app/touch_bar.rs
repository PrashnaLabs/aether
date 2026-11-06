//! Native controls use the window's current model focus, not the last rendered chat.
use super::{ChatApp, Page};
use crate::session_workspace::WindowId;
use gpui_kit::{Context, Window};

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod native;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ButtonAction {
    NewChat,
    Terminal,
    Files,
    Changes,
    Commands,
    Settings,
}

impl ButtonAction {
    const ALL: [Self; 6] =
        [Self::NewChat, Self::Terminal, Self::Files, Self::Changes, Self::Commands, Self::Settings];
}

impl ChatApp {
    fn touch_bar_target(&self, host: WindowId) -> Option<i64> {
        if host == 0 && !matches!(self.page, Page::Chat | Page::Explorer) {
            return None;
        }
        let layout = self.workspace.model.windows.get(&host)?;
        let pane = layout.focused?;
        if !layout.panes().contains(&pane) {
            return None;
        }
        let id = *self.workspace.model.panes.get(&pane)?;
        self.chats.iter().any(|chat| chat.id == id).then_some(id)
    }

    fn touch_bar_elapsed(&self, host: WindowId) -> Option<String> {
        let id = self.touch_bar_target(host)?;
        let chat = self.chats.iter().find(|chat| chat.id == id)?;
        if !chat.busy() || chat.turn_at <= 0 {
            return None;
        }
        let now = crate::app::now_ms();
        let elapsed_ms = (now - chat.turn_at).max(0) as u64;
        Some(aether_ui::clock(elapsed_ms))
    }

    fn touch_bar_enabled(&self, host: WindowId, action: ButtonAction) -> bool {
        let target = self.touch_bar_target(host);
        if host != 0 && target.is_none() {
            return false;
        }
        if matches!(action, ButtonAction::NewChat | ButtonAction::Commands | ButtonAction::Settings)
        {
            return true;
        }
        let Some(ix) = target.and_then(|id| self.chats.iter().position(|chat| chat.id == id))
        else {
            return false;
        };
        let chat = &self.chats[ix];
        match action {
            ButtonAction::Terminal | ButtonAction::Files => self.is_agent_chat(chat),
            ButtonAction::Changes => {
                self.is_agent_chat(chat)
                    && !chat.worktree_removed()
                    && !self.worktree_missing(ix)
                    && self.git_repo_for(chat).is_some()
            }
            ButtonAction::NewChat | ButtonAction::Commands | ButtonAction::Settings => {
                unreachable!()
            }
        }
    }

    fn run_touch_bar_action(
        &mut self,
        host: WindowId,
        action: ButtonAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Revalidate after scheduling: a session may have moved since the tap.
        if !self.touch_bar_enabled(host, action) {
            return;
        }
        if matches!(action, ButtonAction::NewChat | ButtonAction::Settings) {
            if host == 0 {
                match action {
                    ButtonAction::NewChat => self.new_chat_in_current_project(window, cx),
                    ButtonAction::Settings => self.open_page(Page::Settings, cx),
                    _ => unreachable!(),
                }
            } else if let Some(main) = self.workspace.windows.get(&0).copied() {
                // App-wide actions belong to the main workspace and its native window.
                let app = cx.weak_entity();
                cx.defer(move |cx| {
                    let _ = main.update(cx, |_, window, cx| {
                        let _ = app.update(cx, |app, cx| {
                            match action {
                                ButtonAction::NewChat => {
                                    app.new_chat_in_current_project(window, cx)
                                }
                                ButtonAction::Settings => app.open_page(Page::Settings, cx),
                                _ => unreachable!(),
                            }
                            window.activate_window();
                        });
                    });
                });
            }
            return;
        }
        let page = self.page;
        if host != 0 {
            // Detached windows always show sessions, even while the main window
            // shows Settings or another page. Reuse the existing session controls
            // with that context, then restore the main window's navigation.
            self.page = Page::Chat;
        }
        let mut run = |app: &mut Self, cx: &mut Context<Self>| match action {
            ButtonAction::Terminal => app.toggle_terminal(window, cx),
            ButtonAction::Files => app.toggle_explorer_page(window, cx),
            ButtonAction::Changes => app.toggle_changes(true, window, cx),
            ButtonAction::Commands => app.toggle_palette(window, cx),
            ButtonAction::NewChat | ButtonAction::Settings => unreachable!(),
        };
        if let Some(id) = self.touch_bar_target(host) {
            self.in_session(id, cx, run);
        } else {
            run(self, cx);
        }
        if host != 0 {
            self.page = page;
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn sync_touch_bar(&self, host: WindowId, window: &Window, cx: &mut Context<Self>) {
        use futures::StreamExt;
        let enabled = ButtonAction::ALL.map(|action| self.touch_bar_enabled(host, action));
        let elapsed = self.touch_bar_elapsed(host);
        let handle = window.window_handle();
        native::sync(window, enabled, elapsed, |mut events| {
            cx.spawn(async move |app, cx| {
                while let Some(action) = events.next().await {
                    if handle
                        .update(cx, |_, window, cx| {
                            let _ = app.update(cx, |app, cx| {
                                app.run_touch_bar_action(host, action, window, cx);
                            });
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/touch_bar.rs"]
mod tests;
