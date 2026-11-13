use aether_ui::Segmented;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Disableable, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::{AnyElement, Context, FontWeight, Hsla, Window, div, prelude::*, px, rgb};

use super::shared::{avatar, context_bar, money, status_dot};
use crate::app::{Chat, ChatApp, now_ms};
use crate::org::model::{Head, Org};
use crate::org::{OrgOverlay, OrgSel, OrgTab};

const AMBER: u32 = 0xf5a524;
const W_WHO: f32 = 190.;
const W_CARDS: f32 = 120.;
const W_TEAM: f32 = 96.;
const W_SPEND: f32 = 130.;
const W_CTX: f32 = 120.;
const W_MENU: f32 = 34.;

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    Everyone,
    Heads,
    Interns,
}

fn cell(w: f32) -> gpui_kit::Div {
    div().w(px(w)).flex_none().min_w_0()
}

fn row(cx: &Context<ChatApp>) -> gpui_kit::Div {
    h_flex()
        .gap(px(12.))
        .items_center()
        .px(px(20.))
        .py(px(9.))
        .border_b_1()
        .border_color(cx.theme().border)
}

fn labelled_bar(label: String, color: Option<Hsla>, bar: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap(px(3.))
        .child(div().text_size(px(11.5)).when_some(color, |d, c| d.text_color(c)).child(label))
        .child(bar)
}

impl ChatApp {
    pub(crate) fn render_org_roster(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let org = self.org_current().cloned().unwrap_or_default();
        let filter = window.use_keyed_state("org-roster-filter", cx, |_, _| Filter::Everyone);
        let shown = *filter.read(cx);
        let app = cx.entity();
        let seg = Segmented::new("org-roster-filter")
            .compact()
            .option("Everyone")
            .option("Heads")
            .option("Interns")
            .selected(shown as usize)
            .on_change(move |ix, _, cx| {
                let pick = [Filter::Everyone, Filter::Heads, Filter::Interns][*ix];
                filter.update(cx, |f, _| *f = pick);
                app.update(cx, |_, cx| cx.notify());
            });
        let mut rows = vec![];
        for (ix, head) in org.heads.iter().enumerate() {
            if shown != Filter::Interns {
                rows.push(self.roster_head_row(ix, head, cx));
            }
            if shown != Filter::Heads {
                let workers = self.org_workers_of(head.chat);
                rows.extend(workers.into_iter().map(|w| self.roster_worker_row(w, cx)));
            }
        }
        let theme = cx.theme();
        let header = row(cx)
            .bg(theme.background)
            .text_size(px(11.))
            .text_color(theme.muted_foreground)
            .child(div().w(px(7.)).flex_none())
            .child(cell(W_WHO).child("WHO"))
            .child(div().flex_1().child("DOING NOW"))
            .child(cell(W_CARDS).child("CARDS"))
            .child(cell(W_TEAM).child("TEAM"))
            .child(cell(W_SPEND).child("SPEND TODAY"))
            .child(cell(W_CTX).child("CONTEXT"))
            .child(cell(W_MENU));
        let add = h_flex()
            .px(px(20.))
            .py(px(14.))
            .gap(px(8.))
            .child(aether_ui::button("org-roster-add-head").small().label("+ Add head").on_click(
                cx.listener(|this, _, _, cx| {
                    this.org.overlay = OrgOverlay::HeadEditor(String::new());
                    cx.notify();
                }),
            ))
            .child(aether_ui::button("org-roster-hire").small().label("+ Hire worker").on_click(
                cx.listener(|this, _, _, cx| {
                    this.org_open_hire(None);
                    cx.notify();
                }),
            ));
        let table = v_flex().flex_1().min_w_0().min_h_0().child(header).child(
            v_flex()
                .id("org-roster-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(rows)
                .child(add),
        );
        v_flex()
            .flex_1()
            .min_h_0()
            .child(self.org_header_with(Some(&org), Some(seg.into_any_element()), window, cx))
            .child(self.org_goal_row(&org, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(table)
                    .child(self.roster_side(&org, cx)),
            )
            .into_any_element()
    }

    fn roster_head_row(&self, ix: usize, head: &Head, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, hover) = (theme.muted_foreground, theme.secondary.opacity(0.5));
        let amber: Hsla = rgb(AMBER).into();
        let selected = self.org.selected == Some(OrgSel::Head(head.role.clone()));
        let workers = self.org_workers_of(head.chat).len();
        let cards = head.chat.map(|c| self.org_chat_cards(c)).unwrap_or_default();
        let spent = head.chat.map_or(0., |c| self.org_spend(c));
        let ctx = self.org_head_context(head);
        let rotate_at = self.settings.org_rotate_at;
        let role = head.role.clone();
        let edit = head.role.clone();
        row(cx)
            .id(("org-head", ix))
            .cursor_pointer()
            .when(selected, |d| d.bg(theme.secondary))
            .when(!selected, |d| d.hover(move |s| s.bg(hover)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.org.selected = Some(OrgSel::Head(role.clone()));
                cx.notify();
            }))
            .child(status_dot(self.org_head_status(head), cx))
            .child(
                cell(W_WHO).child(
                    h_flex().gap(px(9.)).items_center().child(avatar(&head.role, 30.)).child(
                        v_flex()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(head.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(muted)
                                    .truncate()
                                    .child(format!("{} / {}", head.provider.key(), head.model)),
                            ),
                    ),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.5))
                    .text_color(muted)
                    .truncate()
                    .child(self.org_head_doing(head)),
            )
            .child(cell(W_CARDS).child(card_pills(&cards, cx)))
            .child(
                cell(W_TEAM)
                    .text_size(px(12.5))
                    .text_color(if workers > head.max_workers { amber } else { muted })
                    .child(format!("{workers}/{} workers", head.max_workers)),
            )
            .child(cell(W_SPEND).text_size(px(11.5)).child(money(spent)))
            .child(cell(W_CTX).child(labelled_bar(
                format!("ctx {ctx}%"),
                (ctx >= rotate_at).then_some(amber),
                context_bar(ctx, rotate_at, cx),
            )))
            .child(
                cell(W_MENU).child(
                    aether_ui::button(("org-head-menu", ix))
                        .ghost()
                        .xsmall()
                        .label("⋯")
                        .tooltip("Edit head")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.org.overlay = OrgOverlay::HeadEditor(edit.clone());
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }

    fn roster_worker_row(&self, chat: &Chat, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, hover) = (theme.muted_foreground, theme.secondary.opacity(0.5));
        let id = chat.id;
        let selected = self.org.selected == Some(OrgSel::Worker(id));
        let cards = self.org_chat_cards(id);
        let busy = chat.busy();
        row(cx)
            .id(("org-worker", id as usize))
            .pl(px(56.))
            .text_size(px(12.5))
            .cursor_pointer()
            .when(selected, |d| d.bg(theme.secondary))
            .when(!selected, |d| d.hover(move |s| s.bg(hover)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.org.selected = Some(OrgSel::Worker(id));
                cx.notify();
            }))
            .child(status_dot(chat.status(now_ms()), cx))
            .child(cell(W_WHO - 36.).child(
                v_flex().child(div().truncate().child(chat.title.clone())).child(
                    div().text_size(px(11.5)).text_color(muted).truncate().child(format!(
                        "{} / {}",
                        chat.provider.key(),
                        chat.provider_model
                    )),
                ),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(muted)
                    .truncate()
                    .child(self.org_chat_doing(chat)),
            )
            .child(cell(W_CARDS).child(card_pills(&cards, cx)))
            .child(cell(W_TEAM).text_color(muted).child("intern"))
            .child(cell(W_SPEND).text_size(px(11.5)).child(money(self.org_spend(id))))
            .child(cell(W_CTX))
            .child(
                cell(W_MENU).child(
                    aether_ui::button(("org-worker-stop", id as usize))
                        .ghost()
                        .xsmall()
                        .label("✕")
                        .tooltip("Stop its turn")
                        .disabled(!busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if let Some(ix) = this.chat_ix(id) {
                                this.stop_chat(ix, cx);
                            }
                        })),
                ),
            )
            .into_any_element()
    }

    fn roster_side(&self, org: &Org, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.org.tab;
        let need = self.org_needs_you().len();
        let tab_button = |id: &'static str, label: String, value: OrgTab| {
            aether_ui::button(id).ghost().small().selected(tab == value).label(label).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.org.tab = value;
                    cx.notify();
                }),
            )
        };
        let tabs = h_flex()
            .gap(px(2.))
            .pb(px(8.))
            .mb(px(12.))
            .border_b_1()
            .border_color(cx.theme().border)
            .child(tab_button("org-tab-needs", format!("Needs you · {need}"), OrgTab::NeedsYou))
            .child(tab_button("org-tab-messages", "Messages".into(), OrgTab::Messages))
            .child(tab_button("org-tab-rules", "Rules".into(), OrgTab::Rules));
        let body = match tab {
            OrgTab::NeedsYou => self.org_needs_you_list(org, cx).into_any_element(),
            OrgTab::Messages => self.org_message_log(org, 99, cx).into_any_element(),
            OrgTab::Rules => self.org_rules_summary(cx).into_any_element(),
        };
        let theme = cx.theme();
        v_flex()
            .id("org-roster-side")
            .w(px(330.))
            .flex_none()
            .h_full()
            .p(px(14.))
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .overflow_y_scroll()
            .child(tabs)
            .child(body)
    }
}

fn card_pills(cards: &[i64], cx: &Context<ChatApp>) -> AnyElement {
    let theme = cx.theme();
    if cards.is_empty() {
        return div()
            .text_size(px(12.))
            .text_color(theme.muted_foreground)
            .child("none")
            .into_any_element();
    }
    h_flex()
        .flex_wrap()
        .gap(px(4.))
        .children(cards.iter().map(|c| {
            crate::project_page::view::pill(format!("#{c}"), theme.secondary, theme.foreground)
        }))
        .into_any_element()
}
