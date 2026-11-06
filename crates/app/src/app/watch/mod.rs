use aether_core::{Watch, WatchCorner, toggle_watch};
use gpui_kit::component::IconName;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::{App, Context, Entity, Window};

use super::{Chat, ChatApp, Page};
use crate::side_panel::SideTab;

mod view;
pub(crate) use view::WATCH_STRIP;

pub(crate) fn watchable(item: Watch, chat: Option<&Chat>) -> bool {
    chat.is_some_and(|c| !c.archived && (matches!(item, Watch::Chat(_)) || c.is_agent()))
}

pub(crate) fn watch_item(app: &Entity<ChatApp>, id: i64, suffix: &str, cx: &App) -> PopupMenuItem {
    let watched = app.read(cx).is_watched(Watch::Chat(id));
    let app = app.clone();
    PopupMenuItem::new(format!("{}{suffix}", if watched { "Unwatch" } else { "Watch" }))
        .icon(if watched { IconName::EyeOff } else { IconName::Eye })
        .on_click(move |_, _, cx| app.update(cx, |app, cx| app.toggle_watch(Watch::Chat(id), cx)))
}

impl ChatApp {
    pub(crate) fn is_watched(&self, item: Watch) -> bool {
        self.settings.watchlist.contains(&item)
    }

    fn watchable_now(&self, item: Watch) -> bool {
        let id = match item {
            Watch::Chat(id) | Watch::Terminal(id) => id,
        };
        watchable(item, self.chats.iter().find(|c| c.id == id))
    }

    pub(crate) fn toggle_watch(&mut self, item: Watch, cx: &mut Context<Self>) {
        if !self.is_watched(item) && !self.watchable_now(item) {
            return;
        }
        let mut now = false;
        self.update_settings(cx, |s| now = toggle_watch(&mut s.watchlist, item));
        if !now && self.watch_peek == Some(item) {
            self.watch_peek = None;
        }
        self.sync_terminal_watch(cx);
    }

    pub(crate) fn unwatch_chat(&mut self, id: i64, cx: &mut Context<Self>) {
        self.retain_watched(cx, |w| w.chat() != id);
    }

    pub(crate) fn unwatch_terminal(&mut self, id: i64, cx: &mut Context<Self>) {
        self.retain_watched(cx, |w| *w != Watch::Terminal(id));
    }

    pub(crate) fn prune_watchlist(&mut self, cx: &mut Context<Self>) {
        let keep =
            self.settings.watchlist.iter().map(|&w| self.watchable_now(w)).collect::<Vec<_>>();
        let mut keep = keep.into_iter();
        self.retain_watched(cx, |_| keep.next().unwrap_or(false));
    }

    fn retain_watched(&mut self, cx: &mut Context<Self>, mut keep: impl FnMut(&Watch) -> bool) {
        let before = self.settings.watchlist.len();
        let kept = self.settings.watchlist.iter().copied().filter(|w| keep(w)).collect::<Vec<_>>();
        if kept.len() == before {
            return;
        }
        if self.watch_peek.is_some_and(|p| !kept.contains(&p)) {
            self.watch_peek = None;
        }
        self.update_settings(cx, |s| s.watchlist = kept);
        self.sync_terminal_watch(cx);
    }

    pub(crate) fn set_watch_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.update_settings(cx, |s| s.watch_enabled = enabled);
        if !enabled {
            self.watch_open = false;
            self.close_peek(cx);
        }
        self.sync_terminal_watch(cx);
    }

    /// A terminal menu's Watch state; `None` leaves the item out.
    pub(crate) fn terminal_watch(&self, id: i64) -> Option<bool> {
        self.settings.watch_enabled.then(|| self.is_watched(Watch::Terminal(id)))
    }

    pub(crate) fn sync_terminal_watch(&self, cx: &mut Context<Self>) {
        for (&id, drawer) in &self.terminals {
            let watched = self.terminal_watch(id);
            drawer.view.update(cx, |t, cx| {
                if std::mem::replace(&mut t.watched, watched) != watched {
                    cx.notify();
                }
            });
        }
    }

    pub(crate) fn session_visible(&self, id: i64) -> bool {
        let Some(pane) = self.workspace.model.pane_for(id) else { return false };
        let Some(host) = self.workspace.model.window_of(pane) else { return false };
        if host == 0 && self.page != Page::Chat {
            return false;
        }
        let layout = &self.workspace.model.windows[&host];
        !layout.maximized.is_some_and(|maximized| maximized != pane)
            && (layout.shown.is_none_or(|shown| shown == pane)
                || layout.floats.iter().any(|f| f.pane == pane))
    }

    pub(crate) fn settle_peek(&mut self) {
        self.watch_peek = self.watch_peek.filter(|&peek| {
            self.chats.iter().any(|c| c.id == peek.chat())
                && !matches!(peek, Watch::Chat(id) if self.session_visible(id))
        });
        if self.watch_peek.is_none() {
            self.watch_peek_live = None;
        }
    }

    pub(crate) fn peeked_terminal(&self) -> Option<i64> {
        match self.watch_peek {
            Some(Watch::Terminal(id)) => Some(id),
            _ => None,
        }
    }

    pub(crate) fn set_watch_corner(&mut self, corner: WatchCorner, cx: &mut Context<Self>) {
        self.update_settings(cx, |s| {
            s.watch_corner = corner;
            s.watch_peek_bounds = None;
        });
    }

    pub(crate) fn toggle_watch_list(&mut self, cx: &mut Context<Self>) {
        if !self.settings.watch_enabled {
            return;
        }
        self.watch_peek_live = None;
        if self.watch_peek.take().is_none() {
            self.watch_open = !self.watch_open;
        }
        cx.notify();
    }

    pub(crate) fn close_peek(&mut self, cx: &mut Context<Self>) {
        self.watch_peek = None;
        self.watch_peek_live = None;
        cx.notify();
    }

    pub(crate) fn open_watch_item(
        &mut self,
        item: Watch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.watch_open = false;
        cx.notify();
        let Some(ix) = self.chats.iter().position(|c| c.id == item.chat()) else { return };
        match item {
            Watch::Chat(id) if self.session_visible(id) => {
                self.select(ix, window, cx);
                if self.chats[self.active].id == id {
                    self.focus_composer(window, cx);
                }
            }
            Watch::Chat(_) => self.watch_peek = Some(item),
            Watch::Terminal(_) => {
                let Some(view) = self.drawer_for(ix, window, cx) else { return };
                self.watch_peek = Some(item);
                let focus = view.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
        }
    }

    pub(crate) fn jump_to_watched(
        &mut self,
        item: Watch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_peek(cx);
        let Some(ix) = self.chats.iter().position(|c| c.id == item.chat()) else { return };
        self.select(ix, window, cx);
        if item == Watch::Terminal(self.chats[self.active].id) {
            self.open_terminal(window, cx);
        }
    }

    pub(crate) fn pop_out_watched(
        &mut self,
        item: Watch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_peek(cx);
        let id = item.chat();
        let Some(ix) = self.chats.iter().position(|c| c.id == id) else { return };
        let model = &mut self.workspace.model;
        let pane = model.pane_for(id).unwrap_or_else(|| {
            // detach commits before main redraws; remove() would refocus main's first pane
            let focused = model.windows.get(&0).and_then(|w| w.focused);
            let pane = model.open(id, 0);
            model.windows.entry(0).or_default().focused = focused;
            pane
        });
        if model.window_of(pane) == Some(0) {
            self.detach_session(pane, window, cx);
        } else {
            self.select(ix, window, cx);
        }
        if matches!(item, Watch::Terminal(_)) {
            let app = cx.entity();
            // queued after detach_session's deferred commit, so the new host exists
            cx.defer(move |cx| {
                app.update(cx, |app, _| {
                    let model = &app.workspace.model;
                    let host = model.pane_for(id).and_then(|pane| model.window_of(pane));
                    if let Some(host) = host.filter(|host| *host != 0) {
                        app.set_side_tab(host, SideTab::Terminal);
                    }
                })
            });
        }
    }

    pub(crate) fn peek_in_front(&self, id: i64, cx: &App) -> bool {
        self.watch_peek == Some(Watch::Chat(id))
            && self.workspace.windows.get(&0).is_some_and(|h| cx.active_window() == Some(*h))
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/watch.rs"]
mod tests;
