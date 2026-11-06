use aether_agent::usage_history::{UsageHistory, UsageProvider};
use aether_ui::{Segmented, overlay_strong};
use gpui_kit::component::{ActiveTheme, StyledExt};
use gpui_kit::gpui::relative;
use gpui_kit::{App, Context, Div, FontWeight, Hsla, SharedString, div, prelude::*, px};

use crate::app::ChatApp;

use super::format::{
    count_noun, format_day_short, format_percent, format_tokens_compact, format_usd,
};
use super::{
    UsageBreakdown, UsageMetric, provider_mark, provider_short_label, usage_provider_color,
};

impl ChatApp {
    pub(super) fn render_usage_summary(
        &self,
        history: &UsageHistory,
        cx: &mut Context<Self>,
    ) -> Div {
        let metric = self.usage_metric;
        let theme = cx.theme();
        let dark = theme.is_dark();
        let fg = theme.foreground;
        let muted = theme.muted_foreground;
        let track_bg = overlay_strong(cx);

        let headline = match metric {
            UsageMetric::Cost => format!("{}*", format_usd(history.cost_usd)),
            UsageMetric::Tokens => format_tokens_compact(history.total_tokens as f64),
        };
        let caption = match metric {
            UsageMetric::Cost => {
                "*Estimated at full API rates where not provider-reported".to_owned()
            }
            UsageMetric::Tokens => format!("{} in window", count_noun(history.sessions, "session")),
        };

        let mut column = div().w(px(290.0)).flex_none().flex().flex_col().gap(px(16.0)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(div().text_xs().text_color(muted).child(match metric {
                    UsageMetric::Cost => "RAW TOKEN COST",
                    UsageMetric::Tokens => "PROCESSED TOKENS",
                }))
                .child(
                    div()
                        .text_size(px(30.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(SharedString::from(headline)),
                )
                .child(div().text_xs().text_color(muted).child(SharedString::from(caption))),
        );

        let mut providers = history.providers.clone();
        if metric == UsageMetric::Tokens {
            providers.sort_by_key(|p| std::cmp::Reverse(p.total_tokens));
        }
        for provider in &providers {
            let color = usage_provider_color(provider.provider, dark);
            let share = match metric {
                UsageMetric::Cost => provider.cost_share,
                UsageMetric::Tokens => provider.token_share,
            };
            let value = match metric {
                UsageMetric::Cost => format_usd(provider.cost_usd),
                UsageMetric::Tokens => format_tokens_compact(provider.total_tokens as f64),
            };
            let detail = match metric {
                UsageMetric::Cost => format!(
                    "{} of cost · {} tokens",
                    format_percent(share),
                    format_tokens_compact(provider.total_tokens as f64)
                ),
                UsageMetric::Tokens => format!(
                    "{} of tokens · {}",
                    format_percent(share),
                    format_usd(provider.cost_usd)
                ),
            };
            column = column.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(provider_mark(provider.provider, 14.0, color))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .font_medium()
                                    .text_color(fg)
                                    .child(provider.provider.label()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_medium()
                                    .text_color(fg)
                                    .child(SharedString::from(value)),
                            ),
                    )
                    .child(
                        div().h(px(4.0)).w_full().rounded_full().bg(track_bg).child(
                            div()
                                .h_full()
                                .w(relative((share as f32).clamp(0.0, 1.0)))
                                .rounded_full()
                                .bg(color),
                        ),
                    )
                    .child(div().text_xs().text_color(muted).child(SharedString::from(detail))),
            );
        }
        if history.providers.is_empty() {
            column = column.child(
                div().text_xs().text_color(muted).child("No agent usage recorded in this window."),
            );
        }
        column
    }

    pub(super) fn render_usage_breakdown(
        &self,
        history: &UsageHistory,
        cx: &mut Context<Self>,
    ) -> Div {
        let breakdown = self.usage_breakdown;
        let fg = cx.theme().foreground;

        let breakdown_idx = match breakdown {
            UsageBreakdown::Model => 0,
            UsageBreakdown::Day => 1,
        };
        let toggle = Segmented::new("usage-breakdown")
            .compact()
            .option("MODEL")
            .option("DAY")
            .selected(breakdown_idx)
            .on_change(cx.listener(|this, idx, _, cx| {
                let next = if *idx == 0 { UsageBreakdown::Model } else { UsageBreakdown::Day };
                if this.usage_breakdown != next {
                    this.usage_breakdown = next;
                    cx.notify();
                }
            }));

        div()
            .flex_1()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child("Breakdown"),
                    )
                    .child(toggle),
            )
            .child(match breakdown {
                UsageBreakdown::Model => usage_model_table(history, cx),
                UsageBreakdown::Day => usage_day_table(history, cx),
            })
    }
}

pub(super) fn usage_metric_strip(history: &UsageHistory, cx: &App) -> Div {
    let active_days = history.daily.iter().filter(|day| day.total_tokens > 0).count();
    let daily_average =
        if active_days == 0 { 0.0 } else { history.total_tokens as f64 / active_days as f64 };
    let observed_input = history.totals.uncached_input + history.totals.cached_input;
    let cached_share = if observed_input == 0 {
        0.0
    } else {
        history.totals.cached_input as f64 / observed_input as f64
    };
    let savings_detail = if history.cost_usd > 0.0 {
        format!("{:.1}× raw cost", history.quality.cache_savings_usd / history.cost_usd)
    } else {
        "vs. full input rates".to_owned()
    };

    let tiles: [(&str, String, String); 5] = [
        (
            "Processed tokens",
            format_tokens_compact(history.total_tokens as f64),
            format!("{} / active day", format_tokens_compact(daily_average)),
        ),
        (
            "Cached input",
            format_tokens_compact(history.totals.cached_input as f64),
            format!("{} of observed input", format_percent(cached_share)),
        ),
        (
            "Uncached input",
            format_tokens_compact(history.totals.uncached_input as f64),
            format!("{} cache writes", format_tokens_compact(history.totals.cache_creation as f64)),
        ),
        (
            "Output",
            format_tokens_compact(history.totals.output as f64),
            format!(
                "Includes {} reasoning",
                format_tokens_compact(history.totals.reasoning as f64)
            ),
        ),
        ("Cache savings", format_usd(history.quality.cache_savings_usd), savings_detail),
    ];

    let theme = cx.theme();
    let border = theme.border;
    let fg = theme.foreground;
    let muted = theme.muted_foreground;

    let mut strip = div().mt(px(24.0)).border_t_1().border_b_1().border_color(border).flex();
    for (index, (label, value, detail)) in tiles.into_iter().enumerate() {
        strip = strip.child(
            div()
                .flex_1()
                .min_w_0()
                .px(px(14.0))
                .py(px(11.0))
                .when(index > 0, |el| el.border_l_1().border_color(border))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(div().text_xs().text_color(muted).truncate().child(label))
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .text_color(fg)
                        .truncate()
                        .child(SharedString::from(value)),
                )
                .child(
                    div().text_xs().text_color(muted).truncate().child(SharedString::from(detail)),
                ),
        );
    }
    strip
}

fn usage_table_empty_row(cx: &App) -> Div {
    div()
        .py(px(24.0))
        .flex()
        .justify_center()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child("No agent usage recorded in this window.")
}

fn usage_cell(width: f32, text: impl Into<SharedString>, color: Hsla) -> Div {
    div().w(px(width)).flex_none().flex().justify_end().text_color(color).child(text.into())
}

fn usage_model_table(history: &UsageHistory, cx: &App) -> Div {
    let theme = cx.theme();
    let dark = theme.is_dark();
    let border = theme.border;
    let fg = theme.foreground;
    let muted = theme.muted_foreground;

    let mut table = div().flex().flex_col().text_xs().child(
        div()
            .pb(px(7.0))
            .border_b_1()
            .border_color(border)
            .flex()
            .items_center()
            .gap(px(12.0))
            .text_color(muted)
            .child(div().flex_1().min_w_0().child("Model"))
            .child(usage_cell(84.0, "Cost", muted))
            .child(usage_cell(64.0, "Share", muted))
            .child(usage_cell(84.0, "Tokens", muted)),
    );
    if history.models.is_empty() {
        return table.child(usage_table_empty_row(cx));
    }
    for model in &history.models {
        let color = usage_provider_color(model.provider, dark);
        table = table.child(
            div()
                .py(px(8.0))
                .border_b_1()
                .border_color(border)
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(7.0))
                        .child(provider_mark(model.provider, 12.0, color))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(fg)
                                .child(SharedString::from(model.model.clone())),
                        ),
                )
                .child(usage_cell(84.0, format_usd(model.cost_usd), fg))
                .child(usage_cell(64.0, format_percent(model.cost_share), muted))
                .child(usage_cell(84.0, format_tokens_compact(model.total_tokens as f64), muted)),
        );
    }
    table
}

fn usage_day_table(history: &UsageHistory, cx: &App) -> Div {
    let theme = cx.theme();
    let border = theme.border;
    let fg = theme.foreground;
    let muted = theme.muted_foreground;

    let mut header = div()
        .pb(px(7.0))
        .border_b_1()
        .border_color(border)
        .flex()
        .items_center()
        .gap(px(8.0))
        .text_xs()
        .text_color(muted)
        .child(div().flex_1().min_w_0().child("Day"));
    for provider in UsageProvider::ALL {
        header = header.child(usage_cell(66.0, provider_short_label(provider), muted));
    }
    header =
        header.child(usage_cell(72.0, "Total", muted)).child(usage_cell(72.0, "Tokens", muted));

    let mut table = div().flex().flex_col().text_xs().child(header);
    if history.daily.is_empty() {
        return table.child(usage_table_empty_row(cx));
    }
    for day in history.daily.iter().rev().take(10) {
        let mut row = div()
            .py(px(8.0))
            .border_b_1()
            .border_color(border)
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(fg)
                    .child(SharedString::from(format_day_short(day.day))),
            );
        for provider in UsageProvider::ALL {
            row = row.child(usage_cell(
                66.0,
                format_usd(day.by_provider[provider.index()].cost_usd),
                muted,
            ));
        }
        table = table.child(
            row.child(usage_cell(72.0, format_usd(day.cost_usd), fg)).child(usage_cell(
                72.0,
                format_tokens_compact(day.total_tokens as f64),
                muted,
            )),
        );
    }
    table
}

pub(super) fn usage_quality_panel(history: &UsageHistory, cx: &App) -> Div {
    let theme = cx.theme();
    let border = theme.border;
    let fg = theme.foreground;
    let muted = theme.muted_foreground;

    let row = |label: &'static str, value: String| {
        div()
            .py(px(8.0))
            .border_b_1()
            .border_color(border)
            .flex()
            .items_center()
            .gap(px(12.0))
            .text_xs()
            .child(div().flex_1().text_color(muted).child(label))
            .child(div().text_color(fg).child(SharedString::from(value)))
    };
    div()
        .w(px(230.0))
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(div().text_xs().font_weight(FontWeight::MEDIUM).text_color(fg).child("Cost quality"))
        .child(
            div()
                .flex()
                .flex_col()
                .child(row(
                    "Provider-reported",
                    format_percent(history.quality.provider_reported_share),
                ))
                .child(row("Model-priced", format_percent(history.quality.model_priced_share)))
                .child(row("Unpriced", format_percent(history.quality.unpriced_share)))
                .child(row("Cache savings", format_usd(history.quality.cache_savings_usd))),
        )
}
