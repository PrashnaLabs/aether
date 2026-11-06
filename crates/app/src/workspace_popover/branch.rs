use aether_git::{Branches, GitOp, worktree::Listed};
use aether_ui::{field, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, IconName, Sizable};
use gpui_kit::{AnyElement, App, Context, SharedString, Window, div, prelude::*, px};

use super::model::{
    branch_block_reason, branch_row_label, branch_view_rows, newest_chat_in, refusal_reason,
};
use super::rows::{Where, icon, row, text};
use super::{BRANCH_SEARCH_CONTEXT, PopConfirm, PopView};
use crate::app::ChatApp;
use crate::git_panel::{PickRow, first_pick};

impl ChatApp {
    pub(super) fn open_branch_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(top) = self.git_status().map(|s| s.top.clone()) else {
            return;
        };
        self.workspace_pop.branches = None;
        self.workspace_pop.branch_query.update(cx, |q, cx| q.set_value("", window, cx));
        self.sync_trees(cx);
        let read = top.clone();
        let task = cx.background_executor().spawn(async move { aether_git::branches(&read) });
        cx.spawn(async move |this, cx| {
            let read = task.await;
            this.update(cx, |this, cx| {
                if this.git_status().is_some_and(|s| s.top == top) {
                    this.workspace_pop.branches = Some((top, read.map_err(SharedString::from)));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        self.pop_view(PopView::Branch, window, cx);
    }

    /// The listed branches, once the worktrees are listed too, so a branch
    /// checked out elsewhere never shows as a plain checkout.
    fn branch_list(&self) -> Option<&Result<Branches, SharedString>> {
        let top = &self.git_status()?.top;
        let list = self.workspace_pop.branches.as_ref().filter(|(t, _)| t == top);
        list.filter(|_| self.trees_listed()).map(|(_, b)| b)
    }

    fn branch_rows_now(&self, cx: &App) -> Vec<PickRow> {
        let query = self.workspace_pop.branch_query.read(cx).value();
        match self.branch_list() {
            Some(Ok(branches)) => branch_view_rows(branches, &query),
            _ => Vec::new(),
        }
    }

    fn branch_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.workspace_pop.branch_query.read(cx).value().trim().to_string();
        if query.is_empty() {
            return;
        }
        let rows = self.branch_rows_now(cx);
        let exact = rows.iter().position(|r| matches!(r, PickRow::Local(b) if *b == query));
        if let Some(ix) = exact.or_else(|| first_pick(&rows, &query)) {
            self.pick_branch(rows[ix].clone(), window, cx);
        }
    }

    fn pick_branch(&mut self, row: PickRow, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace_pop.error = None;
        let at = self.pop_where();
        let (Some(folder), Some(status)) = (at.folder, self.git_status()) else {
            return;
        };
        let (top, current) = (status.top.clone(), status.branch.clone());
        if let PickRow::Local(branch) = &row
            && let Some(tree) = self
                .trees()
                .iter()
                .find(|t| t.branch.as_ref() == Some(branch) && t.path != top)
                .cloned()
        {
            return self.open_branch_tree(tree, window, cx);
        }
        if let Some(why) = branch_block_reason(self.agent_in(&folder), at.folder_ok) {
            self.workspace_pop.error = Some(why.into());
            return cx.notify();
        }
        let name = self.workspace_pop.branch_query.read(cx).value().trim().to_string();
        let op = match row {
            PickRow::Create => GitOp::CreateBranch { name, base: None },
            PickRow::Local(branch) if Some(&branch) != current.as_ref() => GitOp::Checkout(branch),
            PickRow::Remote(branch) => GitOp::Track(branch),
            _ => return self.pop_view(PopView::Main, window, cx),
        };
        let busy = self.git.busy();
        self.run_git_in(folder.clone(), top, op, None, window, cx);
        if busy || !self.git.busy() {
            let why = refusal_reason(busy, self.switching_folder(&folder));
            self.workspace_pop.error = Some(why.into());
            return cx.notify();
        }
        self.pop_view(PopView::Main, window, cx);
    }

    fn open_branch_tree(&mut self, tree: Listed, window: &mut Window, cx: &mut Context<Self>) {
        self.close_workspace_pop(window, cx);
        let root = &self.settings.workspace_root;
        let chats: Vec<_> = self
            .chats
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.archived)
            .filter_map(|(ix, c)| Some((ix, c.folder(root)?, c.activity())))
            .collect();
        match newest_chat_in(&chats, &tree.path) {
            Some(ix) => self.select(ix, window, cx),
            None => self.open_worktree_chat(tree.path, tree.main, window, cx),
        }
    }

    fn new_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_workspace_pop(window, cx);
        self.toggle_changes(true, window, cx);
        // its refocus would pull focus out of the picker's field on the next paint
        self.git.refocus = false;
        self.open_tree_picker(window, cx);
    }

    pub(super) fn pop_branch(&mut self, at: Where, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let query = self.workspace_pop.branch_query.read(cx).value();
        let current = self.git_status().and_then(|s| s.branch.clone());
        let busy = at.folder.as_ref().is_some_and(|f| self.agent_in(f));
        let off = branch_block_reason(busy, at.folder_ok);
        let rows = self.branch_rows_now(cx);
        let n = rows.iter().filter(|r| !matches!(r, PickRow::Caption(_))).count();
        let h = self.workspace_pop.rows(n + 2, cx)[1..].to_vec();
        let muted = cx.theme().muted_foreground;
        let note = match self.branch_list() {
            None => Some(("Reading branches…".to_string(), muted)),
            Some(Err(e)) => Some((format!("Couldn't list branches: {e}"), theme::danger_text(cx))),
            Some(Ok(_)) if rows.is_empty() => Some(("No branches match.".into(), muted)),
            Some(Ok(_)) => None,
        };
        let line = |text: String, color| {
            div().px(px(10.)).py(px(4.)).text_xs().text_color(color).child(text).into_any_element()
        };
        let mut out = vec![
            div()
                .key_context(BRANCH_SEARCH_CONTEXT)
                .on_action(
                    cx.listener(|this, _: &PopConfirm, window, cx| this.branch_confirm(window, cx)),
                )
                .child(
                    field(&self.workspace_pop.branch_query)
                        .small()
                        .icon(IconName::Search)
                        .aria_label("Search or create a branch"),
                )
                .into_any_element(),
        ];
        out.extend(off.map(|why| line(why.to_string(), muted)));
        out.extend(note.map(|(text, color)| line(text, color)));
        let mut handles = h.iter();
        for (ix, pick) in rows.into_iter().enumerate() {
            if let PickRow::Caption(caption) = pick {
                out.push(
                    div()
                        .px(px(10.))
                        .pt(px(6.))
                        .text_xs()
                        .text_color(muted)
                        .child(caption)
                        .into_any_element(),
                );
                continue;
            }
            let at = match &pick {
                PickRow::Local(b) => self.branch_tree(b).filter(|at| at.starts_with("in ")),
                _ => None,
            };
            let (glyph, title, trail) = match &pick {
                PickRow::Create => (
                    Lucide::Plus,
                    format!("Create \u{201c}{}\u{201d}", query.trim()),
                    Some(format!("from {}", current.as_deref().unwrap_or("HEAD"))),
                ),
                PickRow::Remote(b) => (Lucide::Cloud, b.clone(), None),
                PickRow::Local(b) => (Lucide::GitBranch, b.clone(), at.clone()),
                _ => continue,
            };
            let Some(handle) = handles.next() else { break };
            let is_current = matches!(&pick, PickRow::Local(b) if Some(b) == current.as_ref());
            let row_off = off.filter(|_| at.is_none());
            let label = branch_row_label(&pick, &query, current.as_deref(), at.as_deref());
            out.push(
                row(("pop-branch-row", ix), handle, label.into(), row_off, cx)
                    .child(icon(glyph, cx))
                    .child(text(title, None, cx))
                    .children(trail.map(|t| div().flex_none().text_xs().text_color(muted).child(t)))
                    .when(at.is_some(), |r| r.child(icon(Lucide::ExternalLink, cx)))
                    .when(is_current, |r| r.child(icon(Lucide::Check, cx)))
                    .when_some(row_off, |r, why| {
                        r.tooltip(move |window, cx| Tooltip::new(why).build(window, cx))
                    })
                    .when(row_off.is_none(), |r| {
                        r.on_click(cx.listener(move |this, _, window, cx| {
                            this.pick_branch(pick.clone(), window, cx)
                        }))
                    })
                    .into_any_element(),
            );
        }
        let tree_off = (!at.folder_ok).then_some("Folder not found");
        out.push(
            row("pop-new-worktree", &h[n], "New worktree…".into(), tree_off, cx)
                .child(icon(Lucide::GitBranchPlus, cx))
                .child(text("New worktree…", tree_off.map(Into::into), cx))
                .when(tree_off.is_none(), |r| {
                    r.on_click(cx.listener(|this, _, window, cx| this.new_worktree(window, cx)))
                })
                .into_any_element(),
        );
        out
    }
}
