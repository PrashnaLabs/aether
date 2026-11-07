pub(crate) mod model;
mod view;

use std::collections::HashMap;
use std::path::PathBuf;

use aether_agent::SavedCard;
use aether_core::ChatLayout;
use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};

use crate::app::{ChatApp, Page, now_ms};
use crate::project_page::model::{CardStatus, card_status};
use crate::side_panel::{SideTab, Surface};
use model::{BoardCard, Col, Turn, auto_move, next_id, slot, tidy};

impl ChatApp {
    pub(crate) fn board_add(
        &mut self,
        title: String,
        body: String,
        col: Col,
        project: String,
        chat: Option<i64>,
    ) -> i64 {
        let id = next_id(&self.cards);
        let position = slot(&self.cards, id, col, None);
        self.cards.push(BoardCard {
            id,
            title,
            body,
            col,
            position,
            chat,
            project,
            updated_at: now_ms(),
        });
        self.board_save(id);
        id
    }

    /// `homes`: worktree folder -> its project.
    pub(crate) fn board_tidy(&mut self, homes: &HashMap<String, String>) {
        let Some(retired) = tidy(&mut self.cards, homes) else { return };
        let Some(store) = &self.store else { return };
        for id in retired {
            store.delete_card(id);
        }
        for card in &self.cards {
            store.save_card(SavedCard::from(card));
        }
    }

    pub(crate) fn board_save(&self, id: i64) {
        if let (Some(store), Some(card)) = (&self.store, self.cards.iter().find(|c| c.id == id)) {
            store.save_card(SavedCard::from(card));
        }
    }

    pub(crate) fn board_move(&mut self, id: i64, col: Col, before: Option<i64>) {
        let position = slot(&self.cards, id, col, before);
        let Some(card) = self.cards.iter_mut().find(|c| c.id == id) else { return };
        card.col = col;
        card.position = position;
        card.updated_at = now_ms();
        self.board_save(id);
    }

    pub(crate) fn board_delete(&mut self, id: i64) {
        self.cards.retain(|c| c.id != id);
        if let Some(store) = &self.store {
            store.delete_card(id);
        }
    }

    pub(crate) fn board_link(&mut self, id: i64, chat: i64) {
        let position = slot(&self.cards, id, Col::Doing, None);
        let Some(card) = self.cards.iter_mut().find(|c| c.id == id) else { return };
        card.chat = Some(chat);
        if card.col != Col::Doing {
            card.col = Col::Doing;
            card.position = position;
        }
        card.updated_at = now_ms();
        self.board_save(id);
    }

    pub(crate) fn board_on_turn(&mut self, chat: i64, turn: Turn) {
        let moves: Vec<(i64, Col)> = self
            .cards
            .iter()
            .filter(|c| c.chat == Some(chat))
            .filter_map(|c| auto_move(c.col, turn).map(|to| (c.id, to)))
            .collect();
        for (id, to) in moves {
            self.board_move(id, to, None);
        }
    }

    pub(crate) fn board_status(&self, card: &BoardCard, now: i64) -> Option<CardStatus> {
        let chat = self.chats.iter().find(|c| Some(c.id) == card.chat && !c.archived)?;
        Some(card_status(
            chat.status(now),
            chat.messages.last().is_some_and(|m| m.failed),
            chat.wait_label(),
        ))
    }

    pub(crate) fn open_board(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Board {
            self.open_page(Page::Chat, cx);
            self.composer.update(cx, |c, cx| c.focus(window, cx));
        } else {
            self.open_page(Page::Board, cx);
            if let Some(id) = self.board_selected {
                self.board_pick(id, window, cx);
            }
        }
    }

    pub(crate) fn board_pick(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.board_commit_in(window, cx);
        let Some(card) = self.cards.iter().find(|c| c.id == id) else { return };
        let (title, body) = (card.title.clone(), card.body.clone());
        self.board_selected = Some(id);
        self.board_title.update(cx, |s, cx| s.set_value(title, window, cx));
        self.board_body.update(cx, |s, cx| s.set_value(body, window, cx));
        cx.notify();
    }

    pub(crate) fn board_commit_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(title) = self.board_commit(cx) {
            self.board_title.update(cx, |s, cx| s.set_value(title, window, cx));
        }
    }

    pub(crate) fn board_commit(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let id = self.board_selected?;
        let title = self.board_title.read(cx).value().trim().to_string();
        let body = self.board_body.read(cx).value().to_string();
        let card = self.cards.iter_mut().find(|c| c.id == id)?;
        if title.is_empty() && body.trim().is_empty() && card.chat.is_none() {
            self.board_delete(id);
            self.board_selected = None;
            cx.notify();
            return None;
        }
        let restore = title.is_empty().then(|| card.title.clone());
        let title = restore.clone().unwrap_or(title);
        if card.title != title || card.body != body {
            card.title = title;
            card.body = body;
            card.updated_at = now_ms();
            self.board_save(id);
            cx.notify();
        }
        restore
    }

    pub(crate) fn open_board_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.board_tab = true;
        if self.settings.chat_layout == ChatLayout::Tabbed {
            let host = self.host_of(window);
            self.set_side_tab(host, SideTab::Board);
        } else {
            self.select_surface(Surface::Board, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn close_board_tab(&mut self, cx: &mut Context<Self>) {
        self.board_commit(cx);
        self.board_tab = false;
        if self.side_tab == SideTab::Board {
            self.side_tab = SideTab::Agent;
        }
        cx.notify();
    }

    pub(crate) fn board_new_card(&mut self, col: Col, window: &mut Window, cx: &mut Context<Self>) {
        let project =
            self.current_project_folder().map(|p| p.display().to_string()).unwrap_or_default();
        let id = self.board_add("New card".into(), String::new(), col, project, None);
        self.board_pick(id, window, cx);
        self.board_title.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
    }

    pub(crate) fn board_run(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.board_commit_in(window, cx);
        let Some(card) = self.cards.iter().find(|c| c.id == id) else { return };
        let folder = if card.project.is_empty() {
            self.settings.workspace_root.clone()
        } else {
            card.project.clone()
        };
        if folder.is_empty() {
            window.push_notification(
                Notification::error("Pick a project folder or set a workspace root first."),
                cx,
            );
            return;
        }
        let text = if card.body.trim().is_empty() {
            card.title.clone()
        } else {
            format!("{}\n\n{}", card.title, card.body)
        };
        self.new_chat_in(PathBuf::from(folder), window, cx);
        let ix = self.active;
        let chat = self.chats[ix].id;
        self.submit(ix, text, vec![], window, cx);
        self.board_link(id, chat);
        self.open_page(Page::Board, cx);
    }

    pub(crate) fn board_open_chat(
        &mut self,
        chat: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.chats.iter().position(|c| c.id == chat) else { return };
        self.select(ix, window, cx);
        self.open_page(Page::Chat, cx);
    }
}
