use aether_git::{GitOp, Stash};
use aether_ui::{row_style, tabular};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Div, Entity, KeyboardButton, Role,
    SharedString, Window, div, prelude::*, rems,
};

use super::diff::back_button;
use super::history::{age, date};
use super::list::ROW_HEIGHT;
use super::rows::{stash_key, stash_label};
use super::widgets::hover_actions;
use crate::app::{ChatApp, format_full};

fn apply_op(stash: &Stash) -> GitOp {
    GitOp::StashApply { index: stash.index, sha: stash.sha.clone() }
}

fn pop_op(stash: &Stash) -> GitOp {
    GitOp::StashPop { index: stash.index, sha: stash.sha.clone() }
}

fn drop_op(stash: &Stash) -> GitOp {
    GitOp::StashDrop { index: stash.index, sha: stash.sha.clone() }
}

/// Whether a click on a stash row applies it: Space does; Enter and the mouse open it.
pub(super) fn applies(ev: &ClickEvent) -> bool {
    matches!(ev, ClickEvent::Keyboard(k) if k.button == KeyboardButton::Space)
}

/// The question before dropping all `n` stashes.
pub(super) fn clear_prompt(n: usize) -> String {
    match n {
        1 => "Drop 1 stash?".into(),
        n => format!("Drop all {} stashes?", format_full(n as u64)),
    }
}

impl ChatApp {
    /// Stashes the changes under the commit box's message, else git's default one.
    pub(super) fn stash_changes(
        &mut self,
        include_untracked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = self.git.message.read(cx).value().trim().to_string();
        self.run_git(GitOp::Stash { message, include_untracked }, window, cx);
    }

    /// The stash a row stands for, as last read: its index and commit go to git together.
    fn stash_at(&self, key: &str) -> Option<Stash> {
        self.git_status()?.stashes.iter().find(|s| stash_key(s) == key).cloned()
    }

    fn run_stash(
        &mut self,
        key: &str,
        op: fn(&Stash) -> GitOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(stash) = self.stash_at(key) {
            self.run_git(op(&stash), window, cx);
        }
    }

    /// Asks before dropping a stash.
    pub(super) fn confirm_drop_stash(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(stash) = self.stash_at(key).filter(|_| self.git.job.is_none()) else {
            return;
        };
        let Some((folder, top)) = self.git_target() else {
            return;
        };
        let view = cx.entity();
        let title = SharedString::from(format!("Drop stash {}?", stash.message));
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, op) = (view.clone(), drop_op(&stash));
            let (folder, top) = (folder.clone(), top.clone());
            alert
                .confirm()
                .title(title.clone())
                .description("This can't be undone.")
                .ok_text("Drop stash")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let (folder, top, op) = (folder.clone(), top.clone(), op.clone());
                    view.update(cx, |this, cx| this.run_git_in(folder, top, op, None, window, cx));
                    true
                })
        });
    }

    /// Asks before dropping every stash; git drops them only while they're
    /// still the ones asked about.
    pub(super) fn confirm_clear_stashes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(status) = self.git_status().filter(|_| self.git.job.is_none()) else {
            return;
        };
        let shas: Vec<String> = status.stashes.iter().map(|s| s.sha.clone()).collect();
        let Some((folder, top)) = self.git_target().filter(|_| !shas.is_empty()) else {
            return;
        };
        let view = cx.entity();
        let title = SharedString::from(clear_prompt(shas.len()));
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, op) = (view.clone(), GitOp::StashClear(shas.clone()));
            let (folder, top) = (folder.clone(), top.clone());
            alert
                .confirm()
                .title(title.clone())
                .description("This can't be undone.")
                .ok_text("Drop all")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let (folder, top, op) = (folder.clone(), top.clone(), op.clone());
                    view.update(cx, |this, cx| this.run_git_in(folder, top, op, None, window, cx));
                    true
                })
        });
    }

    /// The open stash's header: message, branch and age, with Apply, Pop and Drop.
    pub(super) fn stash_header(
        &self,
        stash: &Stash,
        key: &SharedString,
        cx: &mut Context<Self>,
    ) -> Div {
        let back = back_button(cx);
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.sidebar_border);
        let job = self.git.job.as_ref().map(|job| &job.op);
        let current = self.stash_at(key);
        let on = match stash.branch.as_str() {
            "" => String::new(),
            branch => format!("On {branch} · "),
        };
        let action =
            |name: &str, label: &'static str, icon: Lucide, op: Option<fn(&Stash) -> GitOp>| {
                let running =
                    current.as_ref().is_some_and(|s| job == Some(&op.unwrap_or(drop_op)(s)));
                let key = key.clone();
                aether_ui::button(SharedString::from(format!("git-stash-view-{name}")))
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .label(label)
                    .accessibility_label(format!(
                        "{} stash {}",
                        label.trim_end_matches('…'),
                        stash.message
                    ))
                    .loading(running)
                    .disabled(current.is_none() || (job.is_some() && !running))
                    .on_click(cx.listener(move |this, _, window, cx| match op {
                        Some(op) => this.run_stash(&key, op, window, cx),
                        None => this.confirm_drop_stash(&key, window, cx),
                    }))
            };
        v_flex()
            .flex_none()
            .px_2()
            .py_1()
            .gap_0p5()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(back)
                    .child(
                        Icon::new(Lucide::Archive).size(rems(0.875)).flex_none().text_color(muted),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_medium()
                            .child(stash.message.clone()),
                    ),
            )
            .child(
                h_flex()
                    .pl(rems(2.))
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{on}{}", age(stash.time))),
                    )
                    .child(action("apply", "Apply", Lucide::ArchiveRestore, Some(apply_op)))
                    .child(action("pop", "Pop", Lucide::PackageOpen, Some(pop_op)))
                    .child(action("drop", "Drop…", Lucide::Trash, None)),
            )
    }

    /// A stash row: its message, branch and age, with Apply, Pop and Drop on
    /// hover and in its menu. Enter or a click opens it, Space applies it,
    /// Delete drops it.
    pub(super) fn stash_row(
        &self,
        ix: usize,
        key: &SharedString,
        view: &Entity<Self>,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let Some(stash) = self.git_status().and_then(|s| s.stashes.get(ix)) else {
            return div().into_any_element();
        };
        let focus = self.git.focus.get(key).cloned();
        let focused = focus.as_ref().is_some_and(|f| f.is_focused(window));
        let job = self.git.job.as_ref().map(|job| &job.op);
        let busy = job.is_some();
        let muted = cx.theme().muted_foreground;
        let age = age(stash.time);
        let group = SharedString::from(format!("git-group-{key}"));
        let running =
            job.is_some_and(|job| [apply_op, pop_op, drop_op].iter().any(|op| *job == op(stash)));
        let show_action = focused || running || self.git.cursor.as_ref() == Some(key);
        let button =
            |name: &str, icon: Lucide, tip: &'static str, op: Option<fn(&Stash) -> GitOp>| {
                let (view, key) = (view.clone(), key.clone());
                let running = job.is_some_and(|job| *job == op.unwrap_or(drop_op)(stash));
                aether_ui::button(SharedString::from(format!("git-stash-{name}-{key}")))
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .accessibility_label(format!("{tip} {}", stash.message))
                    .tooltip(tip)
                    .tab_stop(false)
                    .loading(running)
                    .disabled(busy && !running)
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        view.update(cx, |this, cx| match op {
                            Some(op) => this.run_stash(&key, op, window, cx),
                            None => this.confirm_drop_stash(&key, window, cx),
                        });
                    })
            };
        let click = {
            let (view, key) = (view.clone(), key.clone());
            move |ev: &ClickEvent, window: &mut Window, cx: &mut App| {
                let apply = applies(ev);
                view.update(cx, |this, cx| match apply {
                    true => this.run_stash(&key, apply_op, window, cx),
                    false => this.open_diff(key.clone(), window, cx),
                });
            }
        };
        let menu = {
            let (view, key, stash) = (view.clone(), key.clone(), stash.clone());
            move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                stash_menu(menu, &view, &key, &stash, busy)
            }
        };
        let on = match stash.branch.as_str() {
            "" => String::new(),
            branch => format!("On {branch} · "),
        };
        let tip = format!("{}\n{on}{}", stash.message, date(stash.time));
        row_style(
            h_flex()
                .id(SharedString::from(format!("git-row-{key}")))
                .when_some(focus, |el, focus| el.track_focus(&focus))
                .role(Role::Button)
                .aria_label(stash_label(stash, &age)),
            false,
            cx,
        )
        .group(group.clone())
        .w_full()
        .h(rems(ROW_HEIGHT))
        .pl_3()
        .pr_2()
        .gap_2()
        .items_center()
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(Icon::new(Lucide::Archive).size(rems(0.875)).flex_none().text_color(muted))
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_1p5()
                .items_baseline()
                .overflow_hidden()
                .child(
                    div()
                        .flex_none()
                        .max_w_full()
                        .truncate()
                        .text_sm()
                        .child(stash.message.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(muted)
                        .child(stash.branch.clone()),
                ),
        )
        .child(hover_actions(
            group,
            show_action,
            div().text_xs().text_color(muted).map(tabular).child(age),
            h_flex()
                .child(button("apply", Lucide::ArchiveRestore, "Apply stash", Some(apply_op)))
                .child(button("pop", Lucide::PackageOpen, "Pop stash", Some(pop_op)))
                .child(button("drop", Lucide::Trash, "Drop stash", None)),
            cx,
        ))
        .on_click(click)
        .on_key_down(self.row_keys(key, view))
        .context_menu(menu)
        .into_any_element()
    }
}

/// Right-click menu of a stash row.
fn stash_menu(
    menu: PopupMenu,
    view: &Entity<ChatApp>,
    key: &SharedString,
    stash: &Stash,
    busy: bool,
) -> PopupMenu {
    let copy = |label: &'static str, text: String| {
        PopupMenuItem::new(label).icon(Icon::new(Lucide::Copy)).on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
        })
    };
    let item = |label: &'static str, icon: Lucide, op: Option<fn(&Stash) -> GitOp>| {
        let (view, key) = (view.clone(), key.clone());
        PopupMenuItem::new(label).icon(Icon::new(icon)).disabled(busy).on_click(
            move |_, window, cx| {
                view.update(cx, |this, cx| match op {
                    Some(op) => this.run_stash(&key, op, window, cx),
                    None => this.confirm_drop_stash(&key, window, cx),
                })
            },
        )
    };
    menu.item(item("Apply stash", Lucide::ArchiveRestore, Some(apply_op)))
        .item(item("Pop stash", Lucide::PackageOpen, Some(pop_op)))
        .separator()
        .item(copy("Copy message", stash.message.clone()))
        .item(copy("Copy ref", format!("stash@{{{}}}", stash.index)))
        .separator()
        .item(item("Drop stash…", Lucide::Trash, None))
}

#[cfg(test)]
#[path = "../../tests/unit/git_panel/stash.rs"]
mod tests;
