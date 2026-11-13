use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, FontWeight, SharedString, Window, div, prelude::*, px, rgb,
};

use super::shared::{avatar, context_bar, report, spend_bar, status_dot};
use crate::app::{ChatApp, now_ms};
use crate::board::model::BoardCard;
use crate::org::model::{Head, Org};
use crate::org::{OrgOverlay, OrgSel};

const AMBER: u32 = 0xf5a524;
const NODE_W: f32 = 208.;
const GAP: f32 = 18.;

impl ChatApp {
    pub(crate) fn render_org_chart(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let org = self.org_current().cloned().unwrap_or_default();
        let sel = self.org.selected.clone().unwrap_or(OrgSel::Head("CEO".into()));
        let ceo = match org.head("CEO") {
            Some(h) => Some(self.chart_node(h, &sel, cx)),
            None => None,
        };
        let heads: Vec<&Head> = org.heads.iter().filter(|h| h.role != "CEO").collect();
        let line = px(heads.len().saturating_sub(1) as f32 * (NODE_W + GAP));
        let border = cx.theme().border;
        let conn = move |h: f32| div().w(px(2.)).h(px(h)).bg(border);
        let mut row = Vec::new();
        for h in &heads {
            row.push(v_flex().items_center().child(conn(22.)).child(self.chart_node(h, &sel, cx)));
        }
        let canvas = div().id("org-chart").flex_1().min_w_0().min_h_0().overflow_scroll().child(
            v_flex()
                .min_w_full()
                .items_center()
                .pt(px(28.))
                .pb(px(90.))
                .px(px(20.))
                .children(ceo)
                .when(!heads.is_empty(), |c| {
                    c.child(conn(22.)).child(div().h(px(2.)).w(line).bg(border))
                })
                .child(h_flex().gap(px(GAP)).items_start().children(row))
                .child(
                    div().mt(px(22.)).child(
                        aether_ui::button("org-chart-add-head")
                            .small()
                            .outline()
                            .border_dashed()
                            .label("+ Add head")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.org.overlay = OrgOverlay::HeadEditor(String::new());
                                cx.notify();
                            })),
                    ),
                ),
        );
        let inspector = self.chart_inspector(&org, &sel, window, cx);
        let header = self.org_header(Some(&org), window, cx).into_any_element();
        let goal = self.org_goal_row(&org, cx).into_any_element();
        v_flex()
            .flex_1()
            .min_h_0()
            .child(header)
            .child(goal)
            .child(h_flex().flex_1().min_h_0().items_stretch().child(canvas).child(inspector))
            .into_any_element()
    }

    fn chart_workers(&self, head: &Head) -> Vec<usize> {
        let Some(id) = head.chat else { return vec![] };
        (0..self.chats.len())
            .filter(|&ix| self.chats[ix].parent == Some(id) && !self.chats[ix].archived)
            .collect()
    }

    fn chart_card_of(&self, chat: i64) -> Option<&BoardCard> {
        self.cards.iter().find(|c| c.chats.contains(&chat))
    }

    fn chart_node(&self, head: &Head, sel: &OrgSel, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, ring) = (theme.muted_foreground, theme.border, theme.ring);
        let selected = *sel == OrgSel::Head(head.role.clone());
        let spent = head.chat.map_or(0., |id| self.org_spend(id));
        let ctx = self.org_head_context(head);
        let role = head.role.clone();
        let node = v_flex()
            .id(SharedString::from(format!("org-node-{}", head.role)))
            .w(px(NODE_W))
            .px(px(11.))
            .py(px(10.))
            .rounded(px(12.))
            .border_1()
            .border_color(if selected { ring } else { border })
            .bg(theme.secondary)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.org.selected = Some(OrgSel::Head(role.clone()));
                cx.notify();
            }))
            .child(
                h_flex()
                    .gap(px(9.))
                    .items_center()
                    .child(avatar(&head.role, 30.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div().font_weight(FontWeight::SEMIBOLD).child(head.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(muted)
                                    .truncate()
                                    .child(format!("{} / {}", head.provider.label(), head.model)),
                            ),
                    )
                    .child(status_dot(self.org_head_status(head), cx)),
            )
            .child(
                div()
                    .mt(px(7.))
                    .mb(px(6.))
                    .min_h(px(34.))
                    .text_size(px(11.5))
                    .text_color(muted)
                    .line_clamp(2)
                    .child(self.org_head_doing(head)),
            )
            .child(
                h_flex()
                    .justify_between()
                    .mb(px(3.))
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(format!("${spent:.2} of ${:.0}", head.budget_usd))
                    .child(format!("ctx {ctx}%")),
            )
            .child(spend_bar(spent, head.budget_usd, cx));
        let chips: Vec<_> = self
            .chart_workers(head)
            .into_iter()
            .map(|ix| {
                let chat = &self.chats[ix];
                let id = chat.id;
                let card = self.chart_card_of(id).map(|c| format!("#{}", c.id));
                let selected = *sel == OrgSel::Worker(id);
                h_flex()
                    .id(("org-worker", id as u64))
                    .w(px(190.))
                    .gap(px(6.))
                    .items_center()
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if selected { ring } else { border })
                    .bg(theme.background)
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.org.selected = Some(OrgSel::Worker(id));
                        cx.notify();
                    }))
                    .child(status_dot(chat.status(now_ms()), cx))
                    .child(div().flex_1().min_w_0().truncate().child(chat.title.clone()))
                    .children(card.map(|c| div().text_color(muted).child(c)))
            })
            .collect();
        v_flex()
            .items_center()
            .child(node)
            .when(!chips.is_empty(), |c| {
                c.child(div().w(px(2.)).h(px(10.)).mt(px(10.)).bg(border))
                    .child(v_flex().gap(px(5.)).items_center().children(chips))
            })
            .into_any_element()
    }

    fn chart_inspector(
        &self,
        org: &Org,
        sel: &OrgSel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let panel = v_flex()
            .id("org-inspector")
            .w(px(340.))
            .flex_none()
            .p(px(16.))
            .gap(px(12.))
            .overflow_y_scroll()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.sidebar);
        let worker = match sel {
            OrgSel::Worker(id) => self.chat_ix(*id),
            OrgSel::Head(_) => None,
        };
        if let Some(ix) = worker {
            return panel.child(self.chart_worker_panel(org, ix, cx)).into_any_element();
        }
        let role = match sel {
            OrgSel::Head(r) if org.head(r).is_some() => r.as_str(),
            _ => "CEO",
        };
        let Some(head) = org.head(role) else { return panel.into_any_element() };
        let details = self.chart_head_panel(head, window, cx);
        let needs = self.org_needs_you_list(org, cx).into_any_element();
        let log = self.org_message_log(org, 3, cx).into_any_element();
        panel
            .child(details)
            .child(section("Needs you", needs, cx))
            .child(section("Recent messages", log, cx))
            .into_any_element()
    }

    fn chart_head_panel(
        &self,
        head: &Head,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = window;
        let muted = cx.theme().muted_foreground;
        let spent = head.chat.map_or(0., |id| self.org_spend(id));
        let ctx = self.org_head_context(head);
        let rotate_at = self.settings.org_rotate_at;
        let workers = self.chart_workers(head);
        let mut chats: Vec<i64> = workers.iter().map(|&ix| self.chats[ix].id).collect();
        chats.extend(head.chat);
        let cards: Vec<String> = self
            .cards
            .iter()
            .filter(|c| c.chats.iter().any(|id| chats.contains(id)))
            .map(|c| format!("#{}", c.id))
            .collect();
        let reviewer = head.reviewer.as_deref().map_or("You".to_string(), |r| r.to_string());
        let over = head.budget_usd > 0. && spent / head.budget_usd >= 0.8;
        let role = head.role.clone();
        let chat = head.chat;
        v_flex()
            .gap(px(12.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .items_center()
                    .child(avatar(&head.role, 44.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(head.title.clone()),
                            )
                            .child(div().text_size(px(11.5)).text_color(muted).child(format!(
                                "{} / {}",
                                head.provider.label(),
                                head.model
                            ))),
                    )
                    .child(status_dot(self.org_head_status(head), cx)),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .text_size(px(12.5))
                    .child(kv("Doing", self.org_head_doing(head), cx))
                    .child(kv("Owns", div().text_color(muted).child(head.owns.clone()), cx))
                    .child(kv("Reviewed by", reviewer, cx))
                    .child(kv(
                        "Spend today",
                        h_flex()
                            .gap(px(6.))
                            .child(format!("${spent:.2} of ${:.0}", head.budget_usd))
                            .when(over, |c| c.child(pill("80%+"))),
                        cx,
                    ))
                    .child(kv(
                        "Context",
                        v_flex()
                            .gap(px(4.))
                            .child(
                                h_flex()
                                    .gap(px(6.))
                                    .child(format!("{ctx}%"))
                                    .when(ctx >= rotate_at, |c| {
                                        c.child(pill(format!("rotates at {rotate_at}%")))
                                    }),
                            )
                            .child(context_bar(ctx, rotate_at, cx)),
                        cx,
                    ))
                    .child(kv(
                        "Workers",
                        format!("{} of {} allowed", workers.len(), head.max_workers),
                        cx,
                    ))
                    .child(kv(
                        "Cards",
                        if cards.is_empty() { "none".into() } else { cards.join(" ") },
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(aether_ui::button("org-insp-edit").small().label("Edit role").on_click(
                        cx.listener({
                            let role = role.clone();
                            move |this, _, _, cx| {
                                this.org.overlay = OrgOverlay::HeadEditor(role.clone());
                                cx.notify();
                            }
                        }),
                    ))
                    .child(
                        aether_ui::button("org-insp-open")
                            .small()
                            .label("Open chat")
                            .disabled(chat.is_none())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(id) = chat {
                                    this.board_open_chat(id, window, cx);
                                }
                            })),
                    )
                    .when(role != "CEO", |c| {
                        c.child(
                            aether_ui::button("org-insp-message")
                                .small()
                                .label("Message")
                                .on_click(cx.listener({
                                    let role = role.clone();
                                    move |this, _, window, cx| {
                                        this.org.goal_target = Some(role.clone());
                                        this.org.goal.update(cx, |s, cx| s.focus(window, cx));
                                        cx.notify();
                                    }
                                })),
                        )
                    })
                    .child(
                        aether_ui::button("org-insp-rotate")
                            .small()
                            .label("Rotate session")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let done = this.org_rotate(&role, window, cx);
                                report(done, window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn chart_worker_panel(&self, org: &Org, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let chat = &self.chats[ix];
        let id = chat.id;
        let boss = chat
            .parent
            .and_then(|p| org.head_of_chat(p))
            .map_or("You".to_string(), |h| h.title.clone());
        let card = self
            .chart_card_of(id)
            .map_or("none".to_string(), |c| format!("#{} · {}", c.id, c.col.label()));
        let branch = chat.worktree_branch_str();
        let branch = if branch.is_empty() { "none".to_string() } else { branch.to_string() };
        v_flex()
            .gap(px(12.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .items_center()
                    .child(status_dot(chat.status(now_ms()), cx))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(chat.title.clone()),
                    ),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .text_size(px(12.5))
                    .child(kv("Reports to", boss, cx))
                    .child(kv(
                        "Model",
                        format!("{} / {}", chat.provider.label(), chat.provider_model),
                        cx,
                    ))
                    .child(kv("Card", card, cx))
                    .child(kv("Spend", format!("${:.2}", self.org_spend(id)), cx))
                    .child(kv("Worktree", div().text_size(px(11.5)).child(branch), cx)),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(aether_ui::button("org-insp-open").small().label("Open chat").on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.board_open_chat(id, window, cx)
                        }),
                    ))
                    .child(
                        aether_ui::button("org-insp-stop")
                            .small()
                            .danger()
                            .label("Stop")
                            .disabled(!chat.busy())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(ix) = this.chat_ix(id) {
                                    this.stop_chat(ix, cx);
                                }
                            })),
                    ),
            )
            .into_any_element()
    }
}

fn kv(key: &'static str, value: impl IntoElement, cx: &App) -> impl IntoElement {
    h_flex()
        .gap(px(10.))
        .items_start()
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .pt(px(2.))
                .text_color(cx.theme().muted_foreground)
                .child(key),
        )
        .child(div().flex_1().min_w_0().child(value))
}

fn pill(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px(px(6.))
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(AMBER))
        .text_size(px(11.))
        .text_color(rgb(AMBER))
        .child(text.into())
}

fn section(title: &'static str, body: impl IntoElement, cx: &App) -> impl IntoElement {
    v_flex()
        .gap(px(8.))
        .pt(px(12.))
        .border_t_1()
        .border_color(cx.theme().border)
        .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(title))
        .child(body)
}
