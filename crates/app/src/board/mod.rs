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
use model::{BoardCard, Col, Note, Turn, auto_move, next_id, slot, tidy};

impl ChatApp {
    pub(crate) fn board_add(
        &mut self,
        title: String,
        body: String,
        col: Col,
        project: String,
        chat: Option<i64>,
        by_agent: bool,
    ) -> i64 {
        let id = next_id(&self.cards);
        let position = slot(&self.cards, id, col, None);
        self.cards.push(BoardCard {
            id,
            title,
            body,
            col,
            position,
            chats: chat.into_iter().collect(),
            project,
            updated_at: now_ms(),
            by_agent,
            labels: vec![],
            blocked_by: vec![],
            notes: vec![],
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
        if col == Col::Done {
            self.board_unblock(id);
        }
    }

    /// Drops `id` from every card's blockers once it is done or deleted.
    fn board_unblock(&mut self, id: i64) {
        let waiting: Vec<i64> =
            self.cards.iter().filter(|c| c.blocked_by.contains(&id)).map(|c| c.id).collect();
        for card in self.cards.iter_mut().filter(|c| waiting.contains(&c.id)) {
            card.blocked_by.retain(|b| *b != id);
            card.updated_at = now_ms();
        }
        for card in waiting {
            self.board_save(card);
        }
    }

    pub(crate) fn board_toggle_pick(&mut self, id: i64) {
        if !self.board_picks.remove(&id) {
            self.board_picks.insert(id);
        }
    }

    pub(crate) fn board_delete(&mut self, id: i64) {
        self.cards.retain(|c| c.id != id);
        if let Some(store) = &self.store {
            store.delete_card(id);
        }
        self.board_unblock(id);
    }

    pub(crate) fn board_link(&mut self, id: i64, chat: i64) {
        let position = slot(&self.cards, id, Col::Doing, None);
        let Some(card) = self.cards.iter_mut().find(|c| c.id == id) else { return };
        if !card.linked(chat) {
            card.chats.push(chat);
        }
        if card.col != Col::Doing {
            card.col = Col::Doing;
            card.position = position;
        }
        card.updated_at = now_ms();
        self.board_save(id);
    }

    /// Keeps the cards of a deleted chat, without the link.
    pub(crate) fn board_unlink(&mut self, chat: i64) {
        let ids: Vec<i64> = self.cards.iter().filter(|c| c.linked(chat)).map(|c| c.id).collect();
        for id in ids {
            if let Some(card) = self.cards.iter_mut().find(|c| c.id == id) {
                card.chats.retain(|c| *c != chat);
                card.updated_at = now_ms();
            }
            self.board_save(id);
        }
    }

    pub(crate) fn board_on_turn(&mut self, chat: i64, turn: Turn) {
        let busy = |id: i64| id != chat && self.chats.iter().any(|c| c.id == id && c.busy());
        let moves: Vec<(i64, Col)> = self
            .cards
            .iter()
            .filter(|c| c.linked(chat))
            // a card with several agents reaches Review only when the last one finishes
            .filter(|c| matches!(turn, Turn::Started) || !c.chats.iter().any(|id| busy(*id)))
            .filter_map(|c| auto_move(c.col, turn).map(|to| (c.id, to)))
            .collect();
        if let Turn::Finished { failed } = turn {
            self.board_note_result(chat, failed);
        }
        for (id, to) in moves {
            self.board_move(id, to, None);
        }
    }

    /// Files a finished agent's final reply on each card it is linked to.
    fn board_note_result(&mut self, chat: i64, failed: bool) {
        let Some(reply) =
            self.chats.iter().find(|c| c.id == chat).and_then(|c| {
                c.messages.iter().rev().find(|m| m.role == aether_core::Role::Assistant)
            })
        else {
            return;
        };
        let mut text = reply.text.trim().to_string();
        if failed {
            let error = reply.error.as_deref().unwrap_or("failed");
            text = format!("Failed: {error}\n\n{text}").trim().to_string();
        }
        if text.is_empty() {
            return;
        }
        // ponytail: 20k char cap keeps cards.db small; full text stays in the chat
        if let Some((cut, _)) = text.char_indices().nth(20_000) {
            text.truncate(cut);
            text.push('…');
        }
        let now = now_ms();
        let ids: Vec<i64> = self.cards.iter().filter(|c| c.linked(chat)).map(|c| c.id).collect();
        for card in self.cards.iter_mut().filter(|c| ids.contains(&c.id)) {
            card.notes.push(Note { at: now, by: format!("agent {chat}"), text: text.clone() });
            card.updated_at = now;
        }
        for id in ids {
            self.board_save(id);
        }
    }

    pub(crate) fn board_status(&self, card: &BoardCard, now: i64) -> Option<CardStatus> {
        let live = |id: &&i64| self.chats.iter().any(|c| c.id == **id && !c.archived);
        let pick = card
            .chats
            .iter()
            .filter(live)
            .find(|id| self.chats.iter().any(|c| c.id == **id && c.busy()))
            .or_else(|| card.chats.iter().rev().find(live))?;
        let chat = self.chats.iter().find(|c| c.id == *pick)?;
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
        if title.is_empty() && body.trim().is_empty() && card.chats.is_empty() {
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
        let id = self.board_add("New card".into(), String::new(), col, project, None, false);
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

    pub(crate) fn board_set_labels(&mut self, id: i64, text: &str, cx: &mut Context<Self>) {
        let labels = parse_label_text(text);
        let Some(card) = self.cards.iter_mut().find(|c| c.id == id) else { return };
        if card.labels == labels {
            return;
        }
        card.labels = labels;
        card.updated_at = now_ms();
        self.board_save(id);
        cx.notify();
    }

    /// Keeps only open cards that exist; a card can't wait on itself or on one waiting on it.
    pub(crate) fn board_set_blockers(&mut self, id: i64, text: &str, cx: &mut Context<Self>) {
        let allowed: Vec<i64> = self
            .cards
            .iter()
            .filter(|c| c.id != id && c.col != Col::Done && !c.blocked_by.contains(&id))
            .map(|c| c.id)
            .collect();
        let blocked_by = parse_id_text(text, &allowed);
        let Some(card) = self.cards.iter_mut().find(|c| c.id == id) else { return };
        if card.blocked_by == blocked_by {
            return;
        }
        card.blocked_by = blocked_by;
        card.updated_at = now_ms();
        self.board_save(id);
        cx.notify();
    }

    pub(crate) fn board_add_note(&mut self, id: i64, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        let Some(card) = self.cards.iter_mut().find(|c| c.id == id) else { return };
        if text.is_empty() {
            return;
        }
        let now = now_ms();
        card.notes.push(Note { at: now, by: "user".into(), text: text.to_string() });
        card.updated_at = now;
        self.board_save(id);
        cx.notify();
    }
}

/// Comma separated; trimmed, lowercase, deduplicated.
fn parse_label_text(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for label in text.split(',').map(|l| l.trim().to_lowercase()) {
        if !label.is_empty() && !out.contains(&label) {
            out.push(label);
        }
    }
    out
}

/// "#3, 5 7": ids found in `allowed`, in typed order, deduplicated.
fn parse_id_text(text: &str, allowed: &[i64]) -> Vec<i64> {
    let mut out = Vec::new();
    for part in text.split(|c: char| c == ',' || c.is_whitespace()) {
        if let Ok(id) = part.trim_start_matches('#').parse::<i64>()
            && allowed.contains(&id)
            && !out.contains(&id)
        {
            out.push(id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_trimmed_lowercase_and_unique() {
        assert_eq!(parse_label_text(" Bug, UI ,,bug,  "), ["bug", "ui"]);
        assert!(parse_label_text(" , ").is_empty());
    }

    #[test]
    fn ids_keep_only_allowed_in_typed_order() {
        assert_eq!(parse_id_text("#3, 5 3 x 9", &[5, 3]), [3, 5]);
        assert!(parse_id_text("", &[1]).is_empty());
    }
}
