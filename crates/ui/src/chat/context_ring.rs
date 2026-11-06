use gpui_kit::component::{
    ActiveTheme, StyledExt,
    button::ButtonVariants,
    h_flex,
    popover::Popover,
    progress::{Progress, ProgressCircle},
    v_flex,
};
use gpui_kit::{Anchor, App, Hsla, SharedString, Window, div, prelude::*, px};

use crate::theme;

/// A quota limit window (e.g. 5-hour session or 7-day weekly quota) for the model account.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanQuota {
    pub label: SharedString,
    pub used_percent: f32,
    pub reset_label: Option<SharedString>,
}

impl PlanQuota {
    pub fn new(label: impl Into<SharedString>, used_percent: f32) -> Self {
        Self { label: label.into(), used_percent, reset_label: None }
    }

    pub fn reset_label(mut self, reset: Option<impl Into<SharedString>>) -> Self {
        self.reset_label = reset.map(Into::into);
        self
    }
}

/// Ring showing how much of the model's context window the chat fills.
/// Clicking it opens a popover with the exact counts and quota telemetry.
#[derive(IntoElement)]
pub struct ContextRing {
    used: u64,
    total: u64,
    model: SharedString,
    cost: Option<SharedString>,
    quotas: Vec<PlanQuota>,
    breakdown: Vec<(SharedString, SharedString)>,
    diameter: f32,
}

impl ContextRing {
    pub fn new(used: u64, total: u64) -> Self {
        Self {
            used,
            total,
            model: SharedString::default(),
            cost: None,
            quotas: Vec::new(),
            breakdown: Vec::new(),
            diameter: 20.,
        }
    }

    pub fn diameter(mut self, diameter: f32) -> Self {
        self.diameter = diameter;
        self
    }

    pub fn model(mut self, model: impl Into<SharedString>) -> Self {
        self.model = model.into();
        self
    }

    /// A preformatted chat total, e.g. "1.2M tokens this chat · ≈$0.41".
    pub fn cost(mut self, cost: Option<impl Into<SharedString>>) -> Self {
        self.cost = cost.map(Into::into);
        self
    }

    pub fn quota(mut self, quota: PlanQuota) -> Self {
        self.quotas.push(quota);
        self
    }

    pub fn quotas(mut self, quotas: Vec<PlanQuota>) -> Self {
        self.quotas = quotas;
        self
    }

    pub fn breakdown(mut self, rows: Vec<(SharedString, SharedString)>) -> Self {
        self.breakdown = rows;
        self
    }
}

impl RenderOnce for ContextRing {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let percent = (self.used as f32 / self.total.max(1) as f32 * 100.).clamp(0., 100.);
        let color = level_color(percent, cx);
        let summary: SharedString =
            format!("{} of {} tokens used", format_tokens(self.used), format_tokens(self.total))
                .into();
        let model = self.model;
        let cost = self.cost;
        let quotas = self.quotas;
        let breakdown = self.breakdown;

        Popover::new("context-usage")
            .anchor(Anchor::BottomRight)
            .arrow(true)
            .trigger(
                crate::components::button("context-usage-trigger")
                    .ghost()
                    .compact()
                    .rounded(px(999.))
                    .size(px(self.diameter + 10.))
                    .p_0()
                    .accessibility_label(format!("Context: {:.0}% used", percent))
                    .child(
                        ProgressCircle::new("context-ring")
                            .value(percent)
                            .color(color)
                            .size(px(self.diameter))
                            .flex_none(),
                    ),
            )
            .content(move |_, _, cx| {
                let muted = cx.theme().muted_foreground;
                let color = level_color(percent, cx);
                v_flex()
                    .w(px(280.))
                    .gap_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(div().text_sm().font_semibold().child("Context window"))
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .text_color(color)
                                    .child(format!("{percent:.0}%")),
                            ),
                    )
                    .child(Progress::new("context-bar").value(percent).color(color))
                    .child(
                        v_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(muted)
                            .child(summary.clone())
                            .when(!model.is_empty(), |list| list.child(model.clone()))
                            .when_some(cost.clone(), |list, cost| list.child(cost)),
                    )
                    .when(!quotas.is_empty(), |card| {
                        card.child(div().h(px(1.)).bg(cx.theme().border))
                            .children(quotas.iter().enumerate().map(|(ix, q)| {
                                let q_color = level_color(q.used_percent, cx);
                                v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .child(div().text_xs().font_medium().child(q.label.clone()))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_semibold()
                                                    .text_color(q_color)
                                                    .child(format!("{:.0}%", q.used_percent)),
                                            ),
                                    )
                                    .child(
                                        Progress::new(SharedString::from(format!("quota-bar-{ix}")))
                                            .value(q.used_percent)
                                            .color(q_color),
                                    )
                                    .when_some(q.reset_label.clone(), |row, reset| {
                                        row.child(
                                            div()
                                                .text_size(px(10.))
                                                .text_color(muted)
                                                .child(format!("Resets {reset}")),
                                        )
                                    })
                            }))
                    })
                    .when(!breakdown.is_empty(), |card| {
                        card.child(div().h(px(1.)).bg(cx.theme().border))
                            .child(
                                v_flex()
                                    .gap_1()
                                    .children(breakdown.iter().map(|(lbl, val)| {
                                        h_flex()
                                            .justify_between()
                                            .text_xs()
                                            .child(div().text_color(muted).child(lbl.clone()))
                                            .child(div().font_semibold().child(val.clone()))
                                    })),
                            )
                    })
                    .when(percent >= 70., |card| {
                        card.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("Long chats lose detail near the limit. Start a new chat to free up space."),
                        )
                    })
            })
    }
}

fn level_color(percent: f32, cx: &App) -> Hsla {
    if percent >= 90. {
        theme::danger_text(cx)
    } else if percent >= 80. {
        theme::mango_text(cx)
    } else if percent >= 60. {
        theme::warning_text(cx)
    } else if percent > 0. {
        theme::success_text(cx)
    } else {
        cx.theme().muted_foreground
    }
}

pub fn format_tokens(n: u64) -> String {
    let short = |value: f64, unit: &str| {
        let text = format!("{value:.1}");
        format!("{}{unit}", text.trim_end_matches(".0"))
    };
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => short(n as f64 / 1_000., "k"),
        _ => short(n as f64 / 1_000_000., "M"),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/context_ring.rs"]
mod tests;
