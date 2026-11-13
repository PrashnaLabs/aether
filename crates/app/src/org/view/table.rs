use aether_ui::ChatStatus;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::{AnyElement, Context, FontWeight, Hsla, Window, div, prelude::*, px, rgb};

use super::shared::avatar;
use crate::app::ChatApp;
use crate::board::model::Col;
use crate::org::model::{Head, Org};
use crate::org::{OrgOverlay, OrgSel};

const SIDE: f32 = 660.;
const SEAT_W: f32 = 150.;
const SEAT_H: f32 = 130.;
const BOARD: f32 = 260.;

/// Seat `i` of `n` around a square of side `s`: angle from 12 o'clock clockwise, and centre x/y.
pub(crate) fn seat_pos(i: usize, n: usize, s: f32) -> (f32, f32, f32) {
    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n.max(1) as f32;
    let r = s * 270. / 660.;
    (a, s / 2. + r * a.cos(), s / 2. + r * a.sin())
}

fn label(text: impl Into<gpui_kit::SharedString>, muted: Hsla) -> impl IntoElement {
    div().mb(px(10.)).text_size(px(11.)).text_color(muted).child(text.into())
}

fn short(text: &str, max: usize) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() > max {
        format!("{}…", line.chars().take(max).collect::<String>())
    } else {
        line.to_string()
    }
}

impl ChatApp {
    pub(crate) fn render_org_table(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let org = self.org_current().cloned().unwrap_or_default();
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let needs = self.org_needs_you().len();
        v_flex()
            .flex_1()
            .min_h_0()
            .child(self.org_header(Some(&org), window, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(
                        div()
                            .id("org-table-needs")
                            .w(px(280.))
                            .h_full()
                            .flex_none()
                            .p(px(14.))
                            .overflow_y_scroll()
                            .border_r_1()
                            .border_color(border)
                            .child(label(format!("NEEDS YOU · {needs}"), muted))
                            .child(self.org_needs_you_list(&org, cx)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(
                                div()
                                    .id("org-table-center")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_scroll()
                                    .flex()
                                    .justify_center()
                                    .items_center()
                                    .child(self.org_round_table(&org, cx)),
                            )
                            .child(self.org_goal_row(&org, cx)),
                    )
                    .child(
                        div()
                            .id("org-table-talk")
                            .w(px(300.))
                            .h_full()
                            .flex_none()
                            .p(px(14.))
                            .overflow_y_scroll()
                            .border_l_1()
                            .border_color(border)
                            .child(label("TALK AT THE TABLE", muted))
                            .child(self.org_message_log(&org, 50, cx)),
                    ),
            )
            .into_any_element()
    }

    fn org_round_table(&self, org: &Org, cx: &mut Context<Self>) -> impl IntoElement {
        let n = org.heads.len();
        let seats: Vec<_> = org
            .heads
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let (_, x, y) = seat_pos(i, n, SIDE);
                div()
                    .absolute()
                    .left(px(x - SEAT_W / 2.))
                    .top(px(y - SEAT_H / 2.))
                    .child(self.org_seat(h, cx))
            })
            .collect();
        div()
            .relative()
            .flex_none()
            .size(px(SIDE))
            .child(self.org_board_disc(org, cx))
            .children(seats)
            .child(div().absolute().right_0().top_0().child(
                aether_ui::button("org-pull-chair").small().label("+ Pull up a chair").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.org.overlay = OrgOverlay::HeadEditor(String::new());
                        cx.notify();
                    }),
                ),
            ))
    }

    fn org_board_disc(&self, org: &Org, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let cols = Col::ALL.map(|col| {
            let count =
                self.cards.iter().filter(|c| c.col == col && c.project == org.project).count();
            v_flex()
                .items_center()
                .text_size(px(10.5))
                .text_color(theme.muted_foreground)
                .child(
                    div()
                        .text_size(px(17.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.foreground)
                        .child(count.to_string()),
                )
                .child(col.key())
        });
        let pool = format!(
            "{}/{} worker slots · {} build at a time",
            self.org_pool_busy(),
            self.org_pool_cap(),
            self.settings.org_builds
        );
        let inset = (SIDE - BOARD) / 2.;
        v_flex()
            .absolute()
            .left(px(inset))
            .top(px(inset))
            .size(px(BOARD))
            .p(px(30.))
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .items_center()
            .justify_center()
            .text_center()
            .child(
                div().text_size(px(11.)).text_color(theme.muted_foreground).child("CURRENT GOAL"),
            )
            .child(
                div()
                    .my(px(8.))
                    .text_size(px(12.5))
                    .text_color(theme.foreground)
                    .child(short(&org.goal, 90)),
            )
            .child(h_flex().gap(px(6.)).children(cols))
            .child(
                div()
                    .mt(px(12.))
                    .text_size(px(11.5))
                    .text_color(theme.muted_foreground)
                    .child(pool),
            )
    }

    fn org_seat(&self, head: &Head, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let spend = head.chat.map_or(0., |id| self.org_spend(id));
        let ring: Hsla = match self.org_head_status(head) {
            ChatStatus::Starting | ChatStatus::Working { .. } | ChatStatus::Quiet { .. } => {
                rgb(0x58a6ff).into()
            }
            ChatStatus::Waiting | ChatStatus::Retrying => rgb(0xf5a524).into(),
            ChatStatus::Failed => rgb(0xf85149).into(),
            _ => theme.border,
        };
        let doing = short(&self.org_head_doing(head), 34);
        let workers: Vec<bool> = head
            .chat
            .map(|id| {
                self.chats
                    .iter()
                    .filter(|c| c.parent == Some(id) && !c.archived)
                    .map(|c| c.busy())
                    .collect()
            })
            .unwrap_or_default();
        let slots = (0..head.max_workers.max(workers.len())).map(|i| {
            let slot = div().size(px(9.)).rounded(px(3.));
            match workers.get(i) {
                Some(true) => slot.bg(rgb(0xb18cff)),
                Some(false) => slot.bg(theme.border),
                None => slot.bg(theme.border).opacity(0.4),
            }
        });
        let selected = self.org.selected == Some(OrgSel::Head(head.role.clone()));
        let role = head.role.clone();
        v_flex()
            .id(gpui_kit::SharedString::from(format!("org-seat-{}", head.role)))
            .w(px(SEAT_W))
            .h(px(SEAT_H))
            .items_center()
            .text_center()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.org.selected = Some(OrgSel::Head(role.clone()));
                this.org.overlay = OrgOverlay::HeadEditor(role.clone());
                cx.notify();
            }))
            .child(
                div()
                    .size(px(58.))
                    .mb(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_3()
                    .border_color(ring)
                    .bg(theme.secondary)
                    .when(selected, |d| d.shadow_md())
                    .child(avatar(&head.role, 44.)),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(head.title.clone()),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.muted_foreground)
                    .child(format!("{} · ${spend:.2} today", head.model)),
            )
            .child(div().text_size(px(11.)).text_color(theme.foreground).opacity(0.7).child(doing))
            .child(h_flex().mt(px(5.)).gap(px(3.)).children(slots))
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/org/table.rs"]
mod tests;
