use std::rc::Rc;

use aether_ui::{OpenFileLink, OpenWebLink};
use gpui_kit::Context;

use crate::app::ChatApp;

/// Opens file links from replies in the active agent chat's Explorer.
pub(super) fn install_file_links(cx: &mut Context<ChatApp>) {
    let view = cx.entity().downgrade();
    cx.set_global(OpenWebLink(Rc::new(move |href, window, cx| {
        if !crate::open_in::web_url(href) {
            return false;
        }
        view.update(cx, |this, cx| {
            if this.notes.search_ui.open {
                this.close_notes_search(window, cx);
            }
            this.page = super::Page::Chat;
            this.open_browser_tab(window, cx);
            this.browser.update(cx, |browser, cx| browser.go(href, cx));
            cx.notify();
            true
        })
        .unwrap_or(false)
    })));
    let view = cx.entity().downgrade();
    cx.set_global(OpenFileLink(Rc::new(move |link, window, cx| {
        view.update(cx, |this, cx| {
            if this.preview_image(&link.path, window, cx) {
                return true;
            }
            let agent = this.chats.get(this.active).is_some_and(|chat| this.is_agent_chat(chat));
            if agent {
                this.open_in_explorer_at(link.path, link.line, link.col, window, cx);
            }
            agent
        })
        .unwrap_or(false)
    })));
}
