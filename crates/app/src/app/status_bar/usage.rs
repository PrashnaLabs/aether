use aether_agent::usage::{PlanLimit, PlanUsage};
use aether_core::Provider;
use aether_ui::theme::{Corners, danger_text, warning_text};
use aether_ui::{
    TriggerBounds, anchored_popup, overlay, overlay_strong, popup_height, provider_icon, room,
};
use chrono::Utc;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, FocusTrapElement, Icon, IconName, Sizable, StyledExt, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Hsla, Role, SharedString, Stateful, Window,
    div, prelude::*, px, relative,
};

use crate::app::{ChatApp, Page};
use crate::workspace_popover::{POP_CONTEXT, PopBack, PopDown, PopUp, Stop, pop_step};

const PROVIDERS: [Provider; 5] = [
    Provider::Claude,
    Provider::Codex,
    Provider::Antigravity,
    Provider::OpenRouter,
    Provider::OpenCode,
];
const DETAILS: usize = PROVIDERS.len();
const ROWS: usize = DETAILS + 1;

pub(crate) struct UsagePop {
    pub(crate) open: bool,
    pub(crate) card: Option<Provider>,
    trigger: TriggerBounds,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    pub(crate) strip: FocusHandle,
    pub(crate) rows: [FocusHandle; ROWS],
}

impl UsagePop {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            card: None,
            trigger: TriggerBounds::default(),
            focus: cx.focus_handle(),
            restore: None,
            strip: cx.focus_handle().tab_stop(true),
            rows: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
        }
    }
}

fn used(limit: &PlanLimit) -> f32 {
    (100. - limit.percent_left).clamp(0., 100.)
}

fn short_reset(label: &str) -> &str {
    let label = label.strip_prefix("resets ").unwrap_or(label);
    label.strip_prefix("in ").unwrap_or(label)
}

fn reset_minutes(reset: &str) -> Option<u32> {
    if reset == "now" {
        return Some(0);
    }
    reset
        .split_whitespace()
        .map(|part| {
            let unit = part.chars().last()?;
            let n: u32 = part[..part.len() - unit.len_utf8()].parse().ok()?;
            let per = match unit {
                'd' => 1440,
                'h' => 60,
                'm' => 1,
                _ => return None,
            };
            n.checked_mul(per)
        })
        .sum()
}

pub(super) fn short_label(label: &str) -> Option<String> {
    let who = label
        .split_once('(')
        .map(|(_, who)| who.trim_end_matches(')'))
        .or_else(|| label.split_once(" Models").map(|(who, _)| who))?;
    let window = if label.contains("Weekly") { "wk" } else { "5h" };
    Some(format!("{} {window}", who.trim()))
}

pub(super) fn headline(u: &PlanUsage) -> Option<(f32, String, Option<String>)> {
    let soon = |l: &PlanLimit| reset_minutes(short_reset(&l.reset_label)).unwrap_or(u32::MAX);
    let top = u.limits.iter().min_by(|a, b| {
        a.percent_left.total_cmp(&b.percent_left).then_with(|| soon(a).cmp(&soon(b)))
    })?;
    Some((used(top), short_reset(&top.reset_label).to_string(), short_label(&top.label)))
}

pub(super) fn updated_ago(now_ms: i64, at_ms: i64) -> String {
    match (now_ms - at_ms).max(0) / 60_000 {
        0 => "just now".into(),
        m if m < 60 => format!("{m}m ago"),
        m if m < 1440 => format!("{}h ago", m / 60),
        m => format!("{}d ago", m / 1440),
    }
}

fn used_color(used: f32, base: Hsla, cx: &App) -> Hsla {
    if used >= 85. {
        danger_text(cx)
    } else if used >= 60. {
        warning_text(cx)
    } else {
        base
    }
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn balance(u: &PlanUsage) -> Option<String> {
    let credits = u.limits.iter().find(|l| l.label == "Credits")?;
    credits.reset_label.split_whitespace().next().map(str::to_string)
}

/// Used % (None when pay-as-you-go), reset or balance, short label.
pub(super) fn gauge(p: Provider, u: &PlanUsage) -> Option<(Option<f32>, String, Option<String>)> {
    if p == Provider::OpenRouter {
        return Some((None, balance(u)?, None));
    }
    match headline(u) {
        Some((pct, reset, label)) => Some((Some(pct), reset, label)),
        None if p == Provider::OpenCode => Some((None, "Pay-as-you-go".into(), None)),
        None => None,
    }
}

fn refresh_button(id: &'static str, view: &Entity<ChatApp>, cx: &App) -> impl IntoElement {
    let view = view.clone();
    div().debug_selector(move || id.into()).flex_none().child(
        aether_ui::icon_chip(id, false, cx)
            .icon(Lucide::RefreshCw)
            .accessibility_label("Refresh usage")
            .tooltip("Refresh usage")
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                view.update(cx, |this, cx| this.refresh_all_provider_usages(cx));
            }),
    )
}

fn footer_row(id: &'static str, label: &'static str, cx: &App) -> Stateful<Div> {
    h_flex()
        .id(id)
        .debug_selector(move || id.into())
        .px(px(8.))
        .py(px(8.))
        .corner(px(8.))
        .items_center()
        .justify_between()
        .child(label)
        .child(Icon::new(IconName::ChevronRight).small().text_color(cx.theme().muted_foreground))
}

fn focusable(
    row: Stateful<Div>,
    focus: &FocusHandle,
    label: impl Into<SharedString>,
    cx: &App,
) -> Stateful<Div> {
    let ring = overlay(cx);
    row.track_focus(focus)
        .tab_index(0)
        .role(Role::ListItem)
        .aria_label(label)
        .focus_visible(move |s| s.bg(ring))
}

fn is_narrow(window: &Window) -> bool {
    window.viewport_size().width < px(700.)
}

fn slot(p: Provider) -> usize {
    PROVIDERS.iter().position(|&q| q == p).unwrap_or_default()
}

fn pct_text(id: String, pct: f32, cx: &App) -> Div {
    div()
        .flex_none()
        .debug_selector(move || id)
        .text_color(used_color(pct, cx.theme().foreground, cx))
        .child(format!("{pct:.0}%"))
}

fn divider(cx: &App) -> Div {
    div().h(px(1.)).mx(px(8.)).my(px(4.)).bg(cx.theme().border)
}

impl ChatApp {
    fn open_usage_pop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pop = &mut self.usage_pop;
        pop.open = true;
        pop.card = None;
        pop.restore = window.focused(cx);
        pop.focus.focus(window, cx);
        cx.notify();
    }

    fn close_usage_pop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pop = &mut self.usage_pop;
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

    fn usage_pop_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.usage_pop.card.take().is_some() {
            cx.notify();
        } else {
            self.close_usage_pop(window, cx);
        }
    }

    fn usage_pop_move(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let order: Vec<usize> = self.sorted().iter().map(|r| slot(r.0)).chain([DETAILS]).collect();
        let rows = &self.usage_pop.rows;
        let at = order.iter().position(|&ix| rows[ix].is_focused(window));
        if let Some(Stop::Row(to)) = pop_step(at, false, down)
            && let Some(&ix) = order.get(to)
        {
            rows[ix].focus(window, cx);
        }
    }

    fn sorted(&self) -> Vec<(Provider, &PlanUsage, (Option<f32>, String, Option<String>))> {
        let mut rows = self.shown();
        rows.sort_by(|a, b| b.2.0.unwrap_or(0.).total_cmp(&a.2.0.unwrap_or(0.)));
        rows
    }

    fn shown(&self) -> Vec<(Provider, &PlanUsage, (Option<f32>, String, Option<String>))> {
        PROVIDERS
            .iter()
            .filter_map(|&p| {
                let u = self.provider_plan_usages.get(&p)?;
                Some((p, u, gauge(p, u)?))
            })
            .collect()
    }

    pub(super) fn render_usage_strip(
        &self,
        view: &Entity<ChatApp>,
        window: &Window,
        cx: &App,
    ) -> impl IntoElement + use<> {
        // pay-as-you-go rows have no %; they live in the popover only, not the bar
        let items: Vec<_> = self.shown().into_iter().filter(|(_, _, g)| g.0.is_some()).collect();
        let empty = items.is_empty();
        let open_view = view.clone();
        let strip = h_flex()
            .id("usage-strip")
            .debug_selector(|| "usage-strip".into())
            .track_focus(&self.usage_pop.strip)
            .tab_index(0)
            .role(Role::Button)
            .aria_label("Usage")
            .focus_visible(|s| s.bg(overlay(cx)))
            .min_w_0()
            .overflow_hidden()
            .items_center()
            .gap(px(4.))
            .cursor_pointer()
            .child(
                h_flex()
                    .min_w_0()
                    .overflow_hidden()
                    .items_center()
                    .gap(px(10.))
                    .when(empty, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .debug_selector(|| "usage-none".into())
                                .child("No usage"),
                        )
                    })
                    .children(items.into_iter().map(|(p, _, (pct, reset, label))| {
                        h_flex()
                            .flex_none()
                            .items_center()
                            .gap(px(4.))
                            .debug_selector(move || format!("usage-strip-{}", p.key()))
                            .child(provider_icon(p).size(px(12.)))
                            .children(
                                pct.map(|pct| pct_text(format!("usage-pct-{}", p.key()), pct, cx)),
                            )
                            .child(label.unwrap_or(reset))
                    })),
            )
            .when(!self.usage_pop.open, |strip| {
                // open only: outside mouse-down already closed it, a click would reopen
                strip.on_click(move |_, window, cx| {
                    open_view.update(cx, |this, cx| this.open_usage_pop(window, cx))
                })
            });
        // block anchor stretches its child; the row lets the strip hug its content
        let hug = h_flex().min_w_0().child(strip);
        anchored_popup("usage-pop-anchor", hug, &self.usage_pop.trigger, false)
            .flex_1()
            .min_w_0()
            .when_some(self.render_usage_pop(view, window, cx), |p, pop| p.content(pop))
    }

    fn render_usage_pop(
        &self,
        view: &Entity<ChatApp>,
        window: &Window,
        cx: &App,
    ) -> Option<AnyElement> {
        let pop = &self.usage_pop;
        if !pop.open {
            return None;
        }
        let theme = cx.theme();
        let max_h = popup_height(room(pop.trigger.get(), false, window), 560.);
        let (out_view, details_view) = (view.clone(), view.clone());
        let (back_view, up_view, down_view) = (view.clone(), view.clone(), view.clone());
        let rows = self.sorted();
        let list = v_flex()
            .id("usage-pop")
            .debug_selector(|| "usage-pop".into())
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
            .child(
                h_flex()
                    .px(px(8.))
                    .pt(px(4.))
                    .items_center()
                    .gap_2()
                    .child(
                        div().flex_1().font_semibold().text_color(theme.foreground).child("Usage"),
                    )
                    .child(refresh_button("usage-pop-refresh", view, cx)),
            )
            .children(rows.into_iter().map(|(p, _, g)| self.usage_row(p, g, view, cx)))
            .child(divider(cx))
            .child(
                focusable(
                    footer_row("usage-details", "Usage details & history", cx),
                    &pop.rows[DETAILS],
                    "Usage details & history",
                    cx,
                )
                .cursor_pointer()
                .hover(|s| s.bg(overlay(cx)))
                .on_click(move |_, window, cx| {
                    details_view.update(cx, |this, cx| {
                        this.close_usage_pop(window, cx);
                        this.open_page(Page::Usage, cx);
                    })
                }),
            );
        let card = pop
            .card
            .and_then(|p| Some(self.usage_card(p, self.provider_plan_usages.get(&p)?, max_h, cx)));
        let narrow = is_narrow(window);
        Some(
            h_flex()
                .id("usage-pop-host")
                .items_end()
                .gap(px(8.))
                .text_sm()
                .text_color(theme.popover_foreground)
                .track_focus(&pop.focus)
                .role(Role::Dialog)
                .aria_label("Usage")
                .key_context(POP_CONTEXT)
                .on_action(move |_: &PopBack, window, cx| {
                    back_view.update(cx, |this, cx| this.usage_pop_back(window, cx))
                })
                .on_action(move |_: &PopUp, window, cx| {
                    up_view.update(cx, |this, cx| this.usage_pop_move(false, window, cx))
                })
                .on_action(move |_: &PopDown, window, cx| {
                    down_view.update(cx, |this, cx| this.usage_pop_move(true, window, cx))
                })
                .on_mouse_down_out(move |_, window, cx| {
                    out_view.update(cx, |this, cx| this.close_usage_pop(window, cx))
                })
                .when(!(narrow && card.is_some()), |host| host.child(list))
                .children(card)
                .focus_trap("usage-pop-trap", &pop.focus)
                .into_any_element(),
        )
    }

    fn usage_row(
        &self,
        p: Provider,
        (pct, reset, _): (Option<f32>, String, Option<String>),
        view: &Entity<ChatApp>,
        cx: &App,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let key = p.key();
        let view = view.clone();
        let muted = theme.muted_foreground;
        let label = match pct {
            Some(pct) => format!("{}, {pct:.0}% used, {reset}", p.label()),
            None => format!("{}, {reset}", p.label()),
        };
        let name_row = h_flex()
            .gap_2()
            .items_center()
            .child(provider_icon(p).size(px(16.)).flex_none())
            .child(
                div().flex_1().min_w_0().truncate().text_color(theme.foreground).child(p.label()),
            )
            .child(div().flex_none().text_xs().text_color(muted).child(reset))
            .children(pct.map(|pct| pct_text(format!("usage-row-{key}-pct"), pct, cx)))
            .child(Icon::new(IconName::ChevronRight).small().text_color(muted));
        let row = v_flex().id(SharedString::from(format!("usage-row-{key}")));
        focusable(row, &self.usage_pop.rows[slot(p)], label, cx)
            .debug_selector(move || format!("usage-row-{key}"))
            .px(px(8.))
            .py(px(8.))
            .gap(px(2.))
            .corner(px(8.))
            .cursor_pointer()
            .when(self.usage_pop.card == Some(p), |row| row.bg(overlay_strong(cx)))
            .hover(|s| s.bg(overlay(cx)))
            .child(name_row)
            .on_click(move |_, window, cx| {
                view.update(cx, |this, cx| {
                    this.usage_pop.card = (this.usage_pop.card != Some(p)).then_some(p);
                    // narrow: card unmounts the list, focused row with it
                    if is_narrow(window) {
                        this.usage_pop.focus.focus(window, cx);
                    }
                    cx.notify();
                })
            })
    }

    fn usage_card(&self, p: Provider, u: &PlanUsage, max_h: f32, cx: &App) -> Stateful<Div> {
        let metered = p != Provider::OpenRouter;
        let theme = cx.theme();
        let (fg, muted) = (theme.foreground, theme.muted_foreground);
        let updated = self
            .plan_usage_at
            .get(&p)
            .map(|&at| format!("Updated {}", updated_ago(Utc::now().timestamp_millis(), at)));
        v_flex()
            .id("usage-card")
            .debug_selector(|| "usage-card".into())
            .w(px(340.))
            .max_h(px(max_h))
            .overflow_y_scroll()
            .p(px(8.))
            .gap(px(6.))
            .corner(px(12.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .child(
                h_flex()
                    .pl(px(2.))
                    .pr(px(6.))
                    .gap_2()
                    .items_center()
                    .child(provider_icon(p).size(px(16.)).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_semibold()
                            .text_color(fg)
                            .child(p.label()),
                    )
                    .children(
                        updated
                            .map(|text| div().flex_none().text_xs().text_color(muted).child(text)),
                    ),
            )
            .child(
                v_flex()
                    .p(px(10.))
                    .gap(px(10.))
                    .corner(px(10.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(overlay(cx))
                    .when(!u.plan_name.is_empty(), |card| {
                        card.child(div().font_medium().text_color(fg).child(u.plan_name.clone()))
                    })
                    .children(u.limits.iter().enumerate().map(|(ix, l)| {
                        let pct = used(l);
                        v_flex()
                            .gap(px(4.))
                            .debug_selector(move || format!("usage-card-limit-{ix}"))
                            .child(div().text_color(fg).child(l.label.clone()))
                            .when(!metered, |limit| {
                                limit.child(
                                    div().text_xs().text_color(muted).child(l.reset_label.clone()),
                                )
                            })
                            .when(metered, |limit| {
                                limit
                                    .child(
                                        div()
                                            .debug_selector(move || format!("usage-card-bar-{ix}"))
                                            .h(px(6.))
                                            .w_full()
                                            .rounded_full()
                                            .bg(theme.secondary)
                                            .child(
                                                div()
                                                    .h_full()
                                                    .rounded_full()
                                                    .bg(used_color(pct, muted, cx))
                                                    .w(relative(pct / 100.)),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .gap_2()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(format!("{pct:.0}% used"))
                                            .child(capitalized(&l.reset_label)),
                                    )
                            })
                    })),
            )
    }
}
