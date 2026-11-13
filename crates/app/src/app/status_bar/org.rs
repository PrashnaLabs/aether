use std::path::Path;

use aether_ui::theme::{Corners, warning_text};
use aether_ui::{TriggerBounds, anchored_popup, overlay, popup_height, room};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme, FocusTrapElement, StyledExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, Role, SharedString, Window, div, prelude::*, px,
    rgb,
};

use super::usage::{divider, focusable, footer_row};
use crate::app::ChatApp;
use crate::org::view::shared::{avatar, status_dot};
use crate::workspace_popover::{POP_CONTEXT, PopBack, PopDown, PopUp, Stop, pop_step};

pub(crate) struct OrgPop {
    pub(crate) open: bool,
    trigger: TriggerBounds,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    pub(crate) rows: Vec<FocusHandle>,
}

impl OrgPop {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            trigger: TriggerBounds::default(),
            focus: cx.focus_handle(),
            restore: None,
            rows: Vec::new(),
        }
    }
}

impl ChatApp {
    /// "N heads · busy/cap workers" for the current project's org.
    pub(crate) fn org_status_text(&self) -> Option<String> {
        let org = self.org_current().filter(|_| self.settings.org_enabled)?;
        Some(format!(
            "{} heads · {}/{} workers",
            org.heads.len(),
            self.org_pool_busy(),
            self.org_pool_cap()
        ))
    }

    fn open_org_pop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.org_current().map_or(0, |o| o.heads.len()) + 3;
        let pop = &mut self.org_pop;
        pop.open = true;
        pop.rows = (0..rows).map(|_| cx.focus_handle().tab_stop(true)).collect();
        pop.restore = window.focused(cx);
        pop.focus.focus(window, cx);
        cx.notify();
    }

    fn close_org_pop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pop = &mut self.org_pop;
        if !std::mem::take(&mut pop.open) {
            return;
        }
        if let Some(previous) = pop.restore.take()
            && pop.focus.contains_focused(window, cx)
        {
            previous.focus(window, cx);
        }
        cx.notify();
    }

    fn org_pop_move(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let heads = self.org_current().map_or(0, |o| o.heads.len());
        let needs = !self.org_needs_you().is_empty();
        let order: Vec<usize> =
            (0..heads).chain(needs.then_some(heads)).chain([heads + 1, heads + 2]).collect();
        let rows = &self.org_pop.rows;
        let at = order.iter().position(|&ix| rows.get(ix).is_some_and(|r| r.is_focused(window)));
        if let Some(Stop::Row(to)) = pop_step(at, false, down)
            && let Some(row) = order.get(to).and_then(|&ix| rows.get(ix))
        {
            row.focus(window, cx);
        }
    }

    pub(super) fn render_org_status(
        &self,
        view: &Entity<ChatApp>,
        window: &Window,
        cx: &App,
    ) -> Option<impl IntoElement + use<>> {
        let text = self.org_status_text()?;
        let needs = self.org_needs_you().len();
        let open_view = view.clone();
        let item = h_flex()
            .id("status-org")
            .debug_selector(|| "status-org".into())
            .flex_none()
            .cursor_pointer()
            .items_center()
            .gap(px(4.))
            .px(px(6.))
            .py(px(2.))
            .corner(px(4.))
            .hover(|s| s.bg(cx.theme().tokens.accent.opacity(0.15)))
            .child(
                gpui_kit::svg()
                    .path(Lucide::Users.path())
                    .size(px(12.))
                    .text_color(cx.theme().muted_foreground),
            )
            .child(text)
            .when(needs > 0, |row| {
                row.child(div().child("·"))
                    .child(div().text_color(warning_text(cx)).child(format!("{needs} need you")))
            })
            .when(!self.org_pop.open, |item| {
                // open only: outside mouse-down already closed it, a click would reopen
                item.on_click(move |_, window, cx| {
                    open_view.update(cx, |this, cx| this.open_org_pop(window, cx))
                })
            });
        Some(
            anchored_popup("org-pop-anchor", item, &self.org_pop.trigger, false)
                .when_some(self.render_org_pop(view, window, cx), |p, pop| p.content(pop)),
        )
    }

    fn render_org_pop(
        &self,
        view: &Entity<ChatApp>,
        window: &Window,
        cx: &App,
    ) -> Option<AnyElement> {
        let pop = &self.org_pop;
        let org = self.org_current().filter(|_| pop.open)?;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let max_h = popup_height(room(pop.trigger.get(), false, window), 560.);
        let (out_view, back_view, up_view, down_view) =
            (view.clone(), view.clone(), view.clone(), view.clone());
        let n = org.heads.len();
        let row_focus = |ix: usize| pop.rows.get(ix);
        let project = self
            .org_project()
            .map(|p| self.project_name(Path::new(&p)).to_string())
            .unwrap_or_default();
        let running = self
            .org_chats(org)
            .iter()
            .any(|&id| self.chat_ix(id).is_some_and(|ix| self.chats[ix].busy()));
        let header = h_flex()
            .px(px(8.))
            .pt(px(4.))
            .pb(px(2.))
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(format!("Organization · {project}")),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .debug_selector(|| "org-pop-state".into())
                    .text_color(if running { rgb(0x3fb950).into() } else { muted })
                    .child(if running { "running" } else { "idle" }),
            );
        let heads = org.heads.iter().enumerate().map(|(ix, head)| {
            let view = view.clone();
            let chat = head.chat;
            let workers = self.org_workers_of(head.chat).len();
            let label = format!("{}, {}", head.title, self.org_head_doing(head));
            let row = h_flex().id(SharedString::from(format!("org-pop-head-{ix}")));
            let row = match row_focus(ix) {
                Some(focus) => focusable(row, focus, label, cx),
                None => row,
            };
            row.debug_selector(move || format!("org-pop-head-{ix}"))
                .px(px(8.))
                .py(px(6.))
                .gap_2()
                .corner(px(8.))
                .items_center()
                .cursor_pointer()
                .hover(|s| s.bg(overlay(cx)))
                .child(avatar(&head.role, 22.))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(theme.foreground)
                                        .child(head.title.clone()),
                                )
                                .child(status_dot(self.org_head_status(head), cx)),
                        )
                        .child(div().text_xs().text_color(muted).truncate().child(format!(
                            "{} / {}",
                            head.provider.key(),
                            head.model
                        )))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .truncate()
                                .child(self.org_head_doing(head)),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{workers}/{}", head.max_workers)),
                )
                .on_click(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.close_org_pop(window, cx);
                        if let Some(ix) = chat.and_then(|id| this.chat_ix(id)) {
                            this.select(ix, window, cx);
                        }
                    })
                })
        });
        let slots = format!(
            "{}/{} worker slots · {} build at a time",
            self.org_pool_busy(),
            self.org_pool_cap(),
            self.settings.org_builds
        );
        let needs = self.org_needs_you().len();
        let footer = |id, label, ix: usize, act: fn(&mut ChatApp, &mut Context<ChatApp>)| {
            let view = view.clone();
            let row = footer_row(id, label, cx);
            let row = match row_focus(ix) {
                Some(focus) => focusable(row, focus, label, cx),
                None => row,
            };
            row.cursor_pointer().hover(|s| s.bg(overlay(cx))).on_click(move |_, window, cx| {
                view.update(cx, |this, cx| {
                    this.close_org_pop(window, cx);
                    act(this, cx);
                })
            })
        };
        let need_row = (needs > 0).then(|| {
            let view = view.clone();
            let label = format!("{needs} need you");
            let row = h_flex().id("org-pop-needs");
            let row = match row_focus(n) {
                Some(focus) => focusable(row, focus, label.clone(), cx),
                None => row,
            };
            row.debug_selector(|| "org-pop-needs".into())
                .px(px(8.))
                .py(px(8.))
                .corner(px(8.))
                .items_center()
                .cursor_pointer()
                .hover(|s| s.bg(overlay(cx)))
                .text_color(warning_text(cx))
                .child(label)
                .on_click(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.close_org_pop(window, cx);
                        this.open_org_page(None, cx);
                    })
                })
        });
        let list = v_flex()
            .id("org-pop")
            .debug_selector(|| "org-pop".into())
            .w(px(320.))
            .max_h(px(max_h))
            .overflow_y_scroll()
            .p(px(6.))
            .gap(px(2.))
            .corner(px(12.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .child(header)
            .children(heads)
            .child(div().px(px(8.)).py(px(4.)).text_xs().text_color(muted).child(slots))
            .children(need_row)
            .child(divider(cx))
            .child(footer("org-pop-open", "Open organization", n + 1, |this, cx| {
                this.open_org_page(None, cx)
            }))
            .child(footer("org-pop-hire", "Hire worker", n + 2, |this, cx| {
                this.open_org_page(None, cx);
                this.org_open_hire(None);
                cx.notify();
            }));
        Some(
            div()
                .id("org-pop-host")
                .text_sm()
                .text_color(theme.popover_foreground)
                .track_focus(&pop.focus)
                .role(Role::Dialog)
                .aria_label("Organization")
                .key_context(POP_CONTEXT)
                .on_action(move |_: &PopBack, window, cx| {
                    back_view.update(cx, |this, cx| this.close_org_pop(window, cx))
                })
                .on_action(move |_: &PopUp, window, cx| {
                    up_view.update(cx, |this, cx| this.org_pop_move(false, window, cx))
                })
                .on_action(move |_: &PopDown, window, cx| {
                    down_view.update(cx, |this, cx| this.org_pop_move(true, window, cx))
                })
                .on_mouse_down_out(move |_, window, cx| {
                    out_view.update(cx, |this, cx| this.close_org_pop(window, cx))
                })
                .child(list)
                .focus_trap("org-pop-trap", &pop.focus)
                .into_any_element(),
        )
    }
}
