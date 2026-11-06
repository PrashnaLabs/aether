use aether_agent::usage_history::{self, MonthSlice, UsageHistory, UsageProvider};
use aether_ui::theme::Corners;
use chrono::NaiveDate;
use gpui_kit::component::ActiveTheme;
use gpui_kit::rems;
use gpui_kit::{
    AnyElement, App, Bounds, Context, Div, FontWeight, Hsla, SharedString, canvas, div, fill,
    point, prelude::*, px, size,
};

use crate::app::ChatApp;

use super::format::{count_noun, format_month, format_tokens_compact};
use super::list::{
    rank_by_cost, usage_headline_value, usage_list_empty_row, usage_list_header,
    usage_models_drawer, usage_provider_value, usage_provider_values, usage_split_bar,
    usage_value_label,
};
use super::{card_surface, usage_provider_colors};

impl ChatApp {
    pub(super) fn render_usage_month_list(
        &self,
        history: &UsageHistory,
        cx: &mut Context<Self>,
    ) -> Div {
        let by_cost = rank_by_cost(history);
        let colors = usage_provider_colors(cx);
        let month_value = |month: &MonthSlice| {
            if by_cost { month.cost_usd } else { month.total_tokens as f64 }
        };
        let peak = history.months.iter().map(month_value).fold(0.0_f64, f64::max);
        let day_peak = history
            .daily
            .iter()
            .map(|day| if by_cost { day.cost_usd } else { day.total_tokens as f64 })
            .fold(0.0_f64, f64::max);
        let current_month = usage_history::first_of_month(history.until_day);

        let mut rows = div().px(px(20.0)).flex().flex_col();
        if let Some(earliest) = history.months.first().map(|m| m.first_day) {
            let months = usage_history::enumerate_months(earliest, history.until_day);
            let count = months.len();
            for (index, first_day) in months.iter().rev().enumerate() {
                let last = index + 1 == count;
                rows = rows.child(match history.month(*first_day) {
                    Some(month) => {
                        let row_id = SharedString::from(format!("usage-month-models-{first_day}"));
                        let expanded = self.usage_expanded_models.as_ref() == Some(&row_id);
                        let models_control =
                            self.usage_models_control(row_id, &month.top_models, expanded, cx);
                        usage_month_row(
                            history,
                            month,
                            colors,
                            by_cost,
                            peak,
                            day_peak,
                            *first_day == current_month,
                            last,
                            models_control,
                            expanded.then(|| {
                                usage_models_drawer(&month.top_models, month.cost_usd, cx)
                            }),
                            cx,
                        )
                    }
                    None => usage_empty_month_row(*first_day, last, cx),
                });
            }
        } else {
            rows = rows.child(usage_list_empty_row("No agent usage in the last 12 months.", cx));
        }

        let card_bg = card_surface(cx);
        let border = cx.theme().border;
        div()
            .mt(px(20.0))
            .flex_1()
            .min_h_0()
            .pb(px(8.0))
            .corner(rems(0.75))
            .border_1()
            .border_color(border)
            .bg(card_bg)
            .flex()
            .flex_col()
            .child(
                usage_list_header(
                    "Last 12 months".to_owned(),
                    format!(
                        "{} tokens · {}",
                        format_tokens_compact(history.total_tokens as f64),
                        count_noun(history.sessions, "session")
                    ),
                    usage_headline_value(history, by_cost),
                    cx,
                )
                .flex_none(),
            )
            .child(
                div().id("usage-months-scroll").flex_1().min_h_0().overflow_y_scroll().child(rows),
            )
    }
}

fn usage_month_strip(
    history: &UsageHistory,
    first_day: NaiveDate,
    peak: f64,
    by_cost: bool,
    colors: [Hsla; UsageProvider::COUNT],
) -> impl IntoElement {
    let day_count = usage_history::days_in_month(first_day);
    let values: Vec<[f64; UsageProvider::COUNT]> = (0..day_count)
        .map(|offset| {
            let day = first_day + chrono::Days::new(u64::from(offset));
            history
                .day(day)
                .map(|slice| {
                    std::array::from_fn(|i| usage_provider_value(&slice.by_provider[i], by_cost))
                })
                .unwrap_or([0.0; UsageProvider::COUNT])
        })
        .collect();
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if peak <= 0.0 {
                return;
            }
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            let count = values.len().max(1) as f32;
            let gap = 1.0;
            let bar_width = ((width - gap * (count - 1.0)) / count).max(1.0);
            for (index, bands) in values.iter().enumerate() {
                let x = bounds.origin.x + px(index as f32 * (bar_width + gap));
                let mut top = bounds.origin.y + bounds.size.height;
                for (band, color) in bands.iter().zip(colors) {
                    if *band <= 0.0 {
                        continue;
                    }
                    let band_height = ((*band / peak) as f32 * height).max(1.5);
                    top -= px(band_height);
                    if top < bounds.origin.y {
                        top = bounds.origin.y;
                    }
                    window.paint_quad(fill(
                        Bounds::new(point(x, top), size(px(bar_width), px(band_height))),
                        color,
                    ));
                }
            }
        },
    )
    .w(px(168.0))
    .h(px(20.0))
}

#[allow(clippy::too_many_arguments)]
fn usage_month_row(
    history: &UsageHistory,
    month: &MonthSlice,
    colors: [Hsla; UsageProvider::COUNT],
    by_cost: bool,
    peak: f64,
    day_peak: f64,
    current: bool,
    last: bool,
    models_control: Option<AnyElement>,
    drawer: Option<Div>,
    cx: &App,
) -> Div {
    let value = if by_cost { month.cost_usd } else { month.total_tokens as f64 };
    let theme = cx.theme();
    let border = theme.border;
    let fg = theme.foreground;
    let muted = theme.muted_foreground;

    div()
        .py(px(13.0))
        .when(!last, |el| el.border_b_1().border_color(border))
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(8.0))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(SharedString::from(format_month(month.first_day))),
                )
                .when(current, |el| el.child(div().text_xs().text_color(muted).child("so far")))
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(SharedString::from(usage_value_label(
                            month.cost_usd,
                            month.total_tokens,
                            by_cost,
                        ))),
                ),
        )
        .child(
            div()
                .mt(px(2.0))
                .flex()
                .items_end()
                .gap(px(12.0))
                .child(div().flex_1().min_w_0().truncate().text_xs().text_color(muted).child(
                    SharedString::from(format!(
                        "{} tokens · {} · {}",
                        format_tokens_compact(month.total_tokens as f64),
                        count_noun(month.sessions, "session"),
                        count_noun(u64::from(month.active_days), "active day")
                    )),
                ))
                .child(usage_month_strip(history, month.first_day, day_peak, by_cost, colors)),
        )
        .child(div().mt(px(9.0)).child(usage_split_bar(
            colors,
            if peak <= 0.0 { 0.0 } else { (value / peak) as f32 },
            &month.by_provider,
            by_cost,
            cx,
        )))
        .child(
            div()
                .mt(px(7.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(usage_provider_values(&month.by_provider, by_cost, cx))
                .child(div().flex_1())
                .when_some(models_control, |el, control| el.child(control)),
        )
        .when_some(drawer, |el, d| el.child(d))
}

fn usage_empty_month_row(first_day: NaiveDate, last: bool, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .py(px(11.0))
        .when(!last, |el| el.border_b_1().border_color(theme.border))
        .flex()
        .items_baseline()
        .gap(px(8.0))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(SharedString::from(format_month(first_day))),
        )
        .child(div().flex_1())
        .child(div().text_xs().text_color(theme.muted_foreground.opacity(0.6)).child("No activity"))
}
