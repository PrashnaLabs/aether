use aether_ui::Segmented;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Disableable, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, Div, FocusHandle, FontWeight, Hsla, KeyDownEvent, Role, ScrollHandle,
    SharedString, Stateful, Window, div, prelude::*, px, rgb,
};

use super::shared::{avatar, context_bar, money, status_dot, status_label};
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

const CELL_NAMES: [&str; 8] = [
    "Status",
    "Who",
    "Current activity",
    "Cards",
    "Team",
    "Spend today",
    "Context usage",
    "Actions",
];

fn data_cell(column: usize, value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    semantic_cell(column).aria_label(format!("{}: {value}", CELL_NAMES[column])).aria_value(value)
}

const ACTIVITY_AX_MAX_CHARS: usize = 200;

fn activity_cell(activity: &str) -> Stateful<Div> {
    // Include the column-name prefix in the accessible name's limit.
    let excerpt_limit = ACTIVITY_AX_MAX_CHARS - CELL_NAMES[2].chars().count() - 2;
    let mut chars = activity.chars();
    let mut excerpt: String = chars.by_ref().take(excerpt_limit).collect();
    if chars.next().is_some() {
        excerpt.pop();
        excerpt.push('…');
    }
    data_cell(2, excerpt)
}

fn cell(w: f32, column: usize, value: impl Into<SharedString>) -> Stateful<Div> {
    data_cell(column, value).w(px(w)).flex_none().min_w_0()
}

fn identity_label(title: &str, role: &str, provider: &str, model: &str) -> String {
    let model = if model.is_empty() { "not set" } else { model };
    format!("{title}; role {role}; provider {provider}; model {model}")
}

fn cards_label(cards: &[i64]) -> String {
    if cards.is_empty() {
        "none".into()
    } else {
        cards.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ")
    }
}

fn semantic_cell(column: usize) -> Stateful<Div> {
    div().id(("cell", column)).role(Role::Cell).aria_column_index(column)
}

#[derive(Clone)]
struct RosterRow {
    selection: OrgSel,
    focus: FocusHandle,
    scroll: ScrollHandle,
}

#[derive(Debug, PartialEq)]
enum RowKey {
    Focus(usize),
    Select,
}

fn row_key(key: &str, index: usize, count: usize) -> Option<RowKey> {
    match key {
        "up" => Some(RowKey::Focus(index.saturating_sub(1))),
        "down" => Some(RowKey::Focus((index + 1).min(count.saturating_sub(1)))),
        "enter" | "space" => Some(RowKey::Select),
        _ => None,
    }
}

fn interactive_row(
    id: SharedString,
    index: usize,
    rows: &[RosterRow],
    selected: bool,
    cx: &Context<ChatApp>,
) -> Stateful<Div> {
    let rows = rows.to_vec();
    let focus = rows[index].focus.clone();
    let click_focus = focus.clone();
    let selection = rows[index].selection.clone();
    let ring = cx.theme().foreground;
    let divider = cx.theme().border;
    row(cx)
        // Reserve the focus border and offset its new top/side widths with padding.
        .relative()
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .px(px(19.))
        .pt(px(8.))
        .id(id.clone())
        .accessibility_id(id.clone())
        .debug_selector(move || id.to_string())
        .role(Role::Row)
        .aria_row_index(index + 1)
        .aria_selected(selected)
        .track_focus(&focus.clone().tab_index(0).tab_stop(true))
        .tab_index(0)
        .focus_visible(move |s| s.border_color(ring))
        .child(div().absolute().left_0().right_0().bottom_0().h(px(1.)).bg(divider))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            // Buttons inside the row retain their own arrow and activation handling.
            if !focus.is_focused(window) || event.keystroke.modifiers.modified() {
                return;
            }
            match row_key(&event.keystroke.key, index, rows.len()) {
                Some(RowKey::Focus(to)) => {
                    rows[to].scroll.scroll_to_item(to);
                    rows[to].focus.focus(window, cx);
                    window.prevent_default();
                    cx.notify();
                    cx.stop_propagation();
                }
                Some(RowKey::Select) => {
                    this.org.selected = Some(rows[index].selection.clone());
                    window.prevent_default();
                    cx.stop_propagation();
                    cx.notify();
                }
                None => {}
            }
        }))
        .on_click(cx.listener(move |this, _, window, cx| {
            click_focus.focus(window, cx);
            this.org.selected = Some(selection.clone());
            cx.notify();
        }))
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
        let scroll = window.use_keyed_state("org-roster-scroll", cx, |_, _| ScrollHandle::new());
        let scroll = scroll.read(cx).clone();
        let mut navigation = vec![];
        for head in &org.heads {
            let mut add_row = |selection: OrgSel, key: String| {
                let focus =
                    window.use_keyed_state(SharedString::from(key), cx, |_, cx| cx.focus_handle());
                navigation.push(RosterRow {
                    selection,
                    focus: focus.read(cx).clone(),
                    scroll: scroll.clone(),
                });
            };
            if shown != Filter::Interns {
                add_row(
                    OrgSel::Head(head.role.clone()),
                    format!("org-roster-head-focus-{}", head.role),
                );
            }
            if shown != Filter::Heads {
                for worker in self.org_workers_of(head.chat) {
                    add_row(
                        OrgSel::Worker(worker.id),
                        format!("org-roster-worker-focus-{}", worker.id),
                    );
                }
            }
        }
        let mut rows = vec![];
        for (index, entry) in navigation.iter().enumerate() {
            match &entry.selection {
                OrgSel::Head(role) => {
                    if let Some(head) = org.heads.iter().find(|head| &head.role == role) {
                        rows.push(
                            self.roster_head_row(head, index, &navigation, cx).into_any_element(),
                        );
                    }
                }
                OrgSel::Worker(id) => {
                    if let Some(chat) = self.chats.iter().find(|chat| chat.id == *id) {
                        rows.push(
                            self.roster_worker_row(chat, index, &navigation, cx).into_any_element(),
                        );
                    }
                }
            }
        }
        let theme = cx.theme();
        let header = row(cx)
            .id("org-roster-header")
            .role(Role::Row)
            .aria_row_index(0)
            .bg(theme.background)
            .text_size(px(11.))
            .text_color(theme.muted_foreground)
            .children(
                [
                    ("Status", Some(12.)),
                    ("WHO", Some(W_WHO)),
                    ("DOING NOW", None),
                    ("CARDS", Some(W_CARDS)),
                    ("TEAM", Some(W_TEAM)),
                    ("SPEND TODAY", Some(W_SPEND)),
                    ("CONTEXT", Some(W_CTX)),
                    ("Actions", Some(W_MENU)),
                ]
                .into_iter()
                .enumerate()
                .map(|(column, (label, width))| {
                    div()
                        .id(("header", column))
                        .role(Role::ColumnHeader)
                        .aria_column_index(column)
                        .aria_label(label)
                        .when_some(width, |d, w| d.w(px(w)).flex_none())
                        .when(width.is_none(), |d| d.flex_1())
                        .when(column != 0 && column != 7, |d| d.child(label))
                }),
            );
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
        let table = v_flex()
            .id("org-roster-table")
            .role(Role::Table)
            .aria_label("Organization roster")
            .aria_column_count(8)
            .aria_row_count(navigation.len() + 2)
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(header)
            .child(
                v_flex()
                    .id("org-roster-rows")
                    .track_scroll(&scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .child(
                        div()
                            .id("org-roster-actions")
                            .role(Role::Row)
                            .aria_row_index(navigation.len() + 1)
                            .child(
                                semantic_cell(0)
                                    .aria_label("Organization actions")
                                    .aria_value("Add head; Hire worker")
                                    .a11y_synthetic_children(|builder| {
                                        builder.parent_node().set_column_span(8)
                                    })
                                    .child(add),
                            ),
                    ),
            );
        v_flex()
            .flex_1()
            .min_h_0()
            .child(self.org_header_with(Some(&org), Some(seg.into_any_element()), window, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(table)
                            .child(self.org_goal_row(&org, cx)),
                    )
                    .child(self.roster_side(&org, cx)),
            )
            .into_any_element()
    }

    fn roster_head_row(
        &self,
        head: &Head,
        index: usize,
        navigation: &[RosterRow],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let hover = theme.secondary.opacity(0.5);
        let selected = self.org.selected == Some(OrgSel::Head(head.role.clone()));
        let status = self.org_head_status(head);
        let identity = identity_label(&head.title, &head.role, head.provider.label(), &head.model);
        interactive_row(format!("org-head-{}", head.role).into(), index, navigation, selected, cx)
            .aria_label(format!("{identity}; status {}", status_label(status)))
            .cursor_pointer()
            .when(selected, |d| d.bg(theme.secondary))
            .when(!selected, |d| d.hover(move |s| s.bg(hover)))
            .children(self.roster_head_cells(head, status, cx))
    }

    fn roster_head_cells(
        &self,
        head: &Head,
        status: aether_ui::ChatStatus,
        cx: &Context<Self>,
    ) -> [Stateful<Div>; 8] {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let amber: Hsla = rgb(AMBER).into();
        let workers = self.org_workers_of(head.chat).len();
        let cards = head.chat.map(|c| self.org_chat_cards(c)).unwrap_or_default();
        let spent = head.chat.map_or(0., |c| self.org_spend(c));
        let ctx = self.org_head_context(head);
        let rotate_at = self.settings.org_rotate_at;
        let edit = head.role.clone();
        let doing = self.org_head_doing(head);
        let identity = identity_label(&head.title, &head.role, head.provider.label(), &head.model);
        [
            cell(12., 0, status_label(status)).child(status_dot(status, cx)),
            cell(W_WHO, 1, identity).child(
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
                            div().text_size(px(11.5)).text_color(muted).truncate().child(format!(
                                "{} / {}",
                                head.provider.key(),
                                head.model
                            )),
                        ),
                ),
            ),
            activity_cell(&doing)
                .flex_1()
                .min_w_0()
                .text_size(px(12.5))
                .text_color(muted)
                .truncate()
                .child(doing),
            cell(W_CARDS, 3, cards_label(&cards)).child(card_pills(&cards, cx)),
            cell(W_TEAM, 4, format!("{workers} of {} workers", head.max_workers))
                .text_size(px(12.5))
                .text_color(if workers > head.max_workers { amber } else { muted })
                .child(format!("{workers}/{} workers", head.max_workers)),
            cell(W_SPEND, 5, money(spent))
                .aria_numeric_value(spent)
                .text_size(px(11.5))
                .child(money(spent)),
            cell(W_CTX, 6, format!("{ctx} percent"))
                .aria_numeric_value(f64::from(ctx))
                .aria_min_numeric_value(0.)
                .aria_max_numeric_value(100.)
                .child(labelled_bar(
                    format!("ctx {ctx}%"),
                    (ctx >= rotate_at).then_some(amber),
                    context_bar(ctx, rotate_at, cx),
                )),
            cell(W_MENU, 7, format!("Edit {} head", head.title)).child(
                aether_ui::button(SharedString::from(format!("org-head-menu-{}", head.role)))
                    .ghost()
                    .xsmall()
                    .label("⋯")
                    .accessibility_label(format!("Edit {} head", head.title))
                    .debug_selector(|| format!("org-head-menu-{}", head.role))
                    .min_w(px(24.))
                    .min_h(px(24.))
                    .tooltip("Edit head")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.org.overlay = OrgOverlay::HeadEditor(edit.clone());
                        cx.notify();
                    })),
            ),
        ]
    }

    fn roster_worker_row(
        &self,
        chat: &Chat,
        index: usize,
        navigation: &[RosterRow],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let hover = theme.secondary.opacity(0.5);
        let id = chat.id;
        let selected = self.org.selected == Some(OrgSel::Worker(id));
        let status = chat.status(now_ms());
        let identity =
            identity_label(&chat.title, "worker", chat.provider.label(), &chat.provider_model);
        interactive_row(format!("org-worker-{id}").into(), index, navigation, selected, cx)
            .aria_label(format!("{identity}; status {}", status_label(status)))
            .pl(px(55.))
            .text_size(px(12.5))
            .cursor_pointer()
            .when(selected, |d| d.bg(theme.secondary))
            .when(!selected, |d| d.hover(move |s| s.bg(hover)))
            .children(self.roster_worker_cells(chat, status, cx))
    }

    fn roster_worker_cells(
        &self,
        chat: &Chat,
        status: aether_ui::ChatStatus,
        cx: &Context<Self>,
    ) -> [Stateful<Div>; 8] {
        let muted = cx.theme().muted_foreground;
        let id = chat.id;
        let cards = self.org_chat_cards(id);
        let busy = chat.busy();
        let doing = self.org_chat_doing(chat);
        let spent = self.org_spend(id);
        let identity =
            identity_label(&chat.title, "worker", chat.provider.label(), &chat.provider_model);
        [
            cell(12., 0, status_label(status)).child(status_dot(status, cx)),
            cell(W_WHO - 36., 1, identity).child(
                v_flex().child(div().truncate().child(chat.title.clone())).child(
                    div().text_size(px(11.5)).text_color(muted).truncate().child(format!(
                        "{} / {}",
                        chat.provider.key(),
                        chat.provider_model
                    )),
                ),
            ),
            activity_cell(&doing).flex_1().min_w_0().text_color(muted).truncate().child(doing),
            cell(W_CARDS, 3, cards_label(&cards)).child(card_pills(&cards, cx)),
            cell(W_TEAM, 4, "worker").text_color(muted).child("intern"),
            cell(W_SPEND, 5, money(spent))
                .aria_numeric_value(spent)
                .text_size(px(11.5))
                .child(money(spent)),
            cell(W_CTX, 6, "unavailable"),
            cell(W_MENU, 7, if busy { "Stop turn" } else { "Stop turn unavailable" }).child(
                aether_ui::button(("org-worker-stop", id as usize))
                    .ghost()
                    .xsmall()
                    .label("✕")
                    .accessibility_label(format!("Stop worker {}", chat.title))
                    .debug_selector(|| format!("org-worker-stop-{id}"))
                    .min_w(px(24.))
                    .min_h(px(24.))
                    .tooltip("Stop its turn")
                    .disabled(!busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if let Some(ix) = this.chat_ix(id) {
                            this.stop_chat(ix, cx);
                        }
                    })),
            ),
        ]
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

#[cfg(test)]
#[path = "../../../tests/unit/org/roster.rs"]
mod tests;
