use aether_agent::usage_history::{self, UsageHistory, UsageProvider};
use aether_ui::Segmented;
use aether_ui::theme::Corners;
use chrono::NaiveDate;
use gpui_kit::component::{ActiveTheme, StyledExt};
use gpui_kit::gpui::{KeyDownEvent, MouseMoveEvent, PathBuilder, relative};
use gpui_kit::{
    App, Bounds, Context, Div, FontWeight, SharedString, canvas, div, fill, point, prelude::*, px,
    size,
};

use crate::app::ChatApp;

use super::format::{format_day_short, format_tokens_compact, format_usd};
use super::{
    CHART_GUTTER, CHART_HEIGHT, CHART_PLOT_TOP, CHART_TICKS, UsageMetric, provider_mark,
    usage_provider_color, usage_provider_colors,
};

/// One cubic segment of a smoothed series boundary, in window pixels.
pub(crate) struct CurveSegment {
    pub from: (f32, f32),
    pub c1: (f32, f32),
    pub c2: (f32, f32),
    pub to: (f32, f32),
}

impl ChatApp {
    pub(super) fn render_usage_chart_column(
        &self,
        history: &UsageHistory,
        cx: &mut Context<Self>,
    ) -> Div {
        let metric = self.usage_metric;
        let theme = cx.theme();
        let dark = theme.is_dark();
        let fg = theme.foreground;
        let muted = theme.muted_foreground;

        let metric_idx = match metric {
            UsageMetric::Cost => 0,
            UsageMetric::Tokens => 1,
        };
        let toggle = Segmented::new("usage-metric")
            .compact()
            .option("COST")
            .option("TOKENS")
            .selected(metric_idx)
            .on_change(cx.listener(|this, idx, _, cx| {
                let next = if *idx == 0 { UsageMetric::Cost } else { UsageMetric::Tokens };
                if this.usage_metric != next {
                    this.usage_metric = next;
                    cx.notify();
                }
            }));

        let mut legend = div().flex().flex_wrap().items_center().gap(px(12.0));
        for provider in UsageProvider::ALL {
            let color = usage_provider_color(provider, dark);
            legend = legend.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .child(provider_mark(provider, 11.0, color))
                    .child(div().text_xs().text_color(muted).child(provider.label())),
            );
        }

        let days = usage_history::enumerate_days(history.since_day, history.until_day);
        div()
            .flex_1()
            .min_w(px(320.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child(match metric {
                                UsageMetric::Cost => "Daily cost",
                                UsageMetric::Tokens => "Daily processed tokens",
                            }),
                    )
                    .child(toggle)
                    .child(legend),
            )
            .child(self.render_usage_chart(history, &days, cx))
            .child(
                div()
                    .pl(px(CHART_GUTTER + 8.0))
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(
                        days.first().copied().map(format_day_short).unwrap_or_default(),
                    ))
                    .child(SharedString::from(
                        days.get(days.len() / 2).copied().map(format_day_short).unwrap_or_default(),
                    ))
                    .child(SharedString::from(
                        days.last().copied().map(format_day_short).unwrap_or_default(),
                    )),
            )
    }

    fn render_usage_chart(
        &self,
        history: &UsageHistory,
        days: &[NaiveDate],
        cx: &mut Context<Self>,
    ) -> Div {
        let metric = self.usage_metric;
        let day_count = days.len();
        let series: Vec<[f64; UsageProvider::COUNT]> = days
            .iter()
            .map(|day| {
                let slice = history.day(*day);
                UsageProvider::ALL.map(|provider| {
                    slice
                        .map(|s| {
                            let entry = s.by_provider[provider.index()];
                            match metric {
                                UsageMetric::Cost => entry.cost_usd,
                                UsageMetric::Tokens => entry.total_tokens as f64,
                            }
                        })
                        .unwrap_or(0.0)
                })
            })
            .collect();

        let peak = series.iter().flat_map(|bands| bands.iter().copied()).fold(0.0_f64, f64::max);
        let (scale_max, ticks) = nice_scale(peak, CHART_TICKS);
        let format_value = move |value: f64| match metric {
            UsageMetric::Cost => format_usd(value),
            UsageMetric::Tokens => format_tokens_compact(value),
        };
        let to_fraction = move |value: f64| -> f32 {
            if scale_max <= 0.0 {
                1.0
            } else {
                1.0 - (value / scale_max) as f32 * (1.0 - CHART_PLOT_TOP / CHART_HEIGHT)
            }
        };

        let muted = cx.theme().muted_foreground;
        let mut gutter = div().relative().w(px(CHART_GUTTER)).h(px(CHART_HEIGHT)).flex_none();
        for tick in &ticks {
            gutter = gutter.child(
                div()
                    .absolute()
                    .right(px(0.0))
                    .top(px((to_fraction(*tick) * CHART_HEIGHT - 7.0).max(0.0)))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(if *tick == 0.0 {
                        "0".to_owned()
                    } else {
                        format_value(*tick)
                    })),
            );
        }

        let hover = self.usage_chart_hover.filter(|index| *index < day_count);
        let colors = usage_provider_colors(cx);
        let bounds_cell = self.usage_chart_bounds.clone();
        let paint_series = series.clone();
        let paint_ticks = ticks.clone();
        let grid_color = cx.theme().border;
        let hover_color = cx.theme().muted_foreground.opacity(0.6);
        let accent = cx.theme().ring;

        let plot_canvas = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                bounds_cell.set(Some(bounds));
                let width = f32::from(bounds.size.width);
                let height = f32::from(bounds.size.height);
                let to_y = |value: f64| bounds.origin.y + px(to_fraction(value) * height);
                for tick in &paint_ticks {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(bounds.origin.x, to_y(*tick)),
                            size(bounds.size.width, px(1.0)),
                        ),
                        grid_color,
                    ));
                }
                if paint_series.is_empty() {
                    return;
                }

                let step = if paint_series.len() <= 1 {
                    0.0
                } else {
                    width / (paint_series.len() - 1) as f32
                };
                let mut layers: Vec<(usize, f64)> = (0..colors.len())
                    .map(|provider| {
                        (provider, paint_series.iter().map(|bands| bands[provider]).sum::<f64>())
                    })
                    .filter(|(_, sum)| *sum > 0.0)
                    .collect();
                layers.sort_by(|a, b| b.1.total_cmp(&a.1));

                let curves: Vec<(usize, Vec<CurveSegment>)> = layers
                    .iter()
                    .map(|(provider, _)| {
                        let points: Vec<(f32, f32)> = paint_series
                            .iter()
                            .enumerate()
                            .map(|(index, bands)| {
                                (
                                    f32::from(bounds.origin.x) + index as f32 * step,
                                    f32::from(to_y(bands[*provider])),
                                )
                            })
                            .collect();
                        (*provider, smooth_curve(&points))
                    })
                    .collect();

                let bottom = bounds.origin.y + bounds.size.height;
                for (provider, segments) in &curves {
                    let Some(first) = segments.first() else {
                        continue;
                    };
                    let mut area = PathBuilder::fill();
                    area.move_to(point(px(first.from.0), px(first.from.1)));
                    for segment in segments {
                        area.cubic_bezier_to(
                            point(px(segment.to.0), px(segment.to.1)),
                            point(px(segment.c1.0), px(segment.c1.1)),
                            point(px(segment.c2.0), px(segment.c2.1)),
                        );
                    }
                    area.line_to(point(bounds.origin.x + bounds.size.width, bottom));
                    area.line_to(point(bounds.origin.x, bottom));
                    area.close();
                    if let Ok(path) = area.build() {
                        window.paint_path(path, colors[*provider].opacity(0.12));
                    }
                }
                for (provider, segments) in &curves {
                    let Some(first) = segments.first() else {
                        continue;
                    };
                    let mut line = PathBuilder::stroke(px(2.0));
                    line.move_to(point(px(first.from.0), px(first.from.1)));
                    for segment in segments {
                        line.cubic_bezier_to(
                            point(px(segment.to.0), px(segment.to.1)),
                            point(px(segment.c1.0), px(segment.c1.1)),
                            point(px(segment.c2.0), px(segment.c2.1)),
                        );
                    }
                    if let Ok(path) = line.build() {
                        window.paint_path(path, colors[*provider]);
                    }
                }

                if let Some(index) = hover {
                    let x = bounds.origin.x + px(index as f32 * step);
                    window.paint_quad(fill(
                        Bounds::new(
                            point(x, bounds.origin.y + px(CHART_PLOT_TOP)),
                            size(px(1.0), bounds.size.height - px(CHART_PLOT_TOP)),
                        ),
                        hover_color,
                    ));
                }
            },
        );

        let plot = div()
            .id("usage-chart-plot")
            .relative()
            .flex_1()
            .min_w(px(0.0))
            .h(px(CHART_HEIGHT))
            .tab_index(0)
            .focus_visible(|style| style.border_1().border_color(accent))
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let Some(bounds) = this.usage_chart_bounds.get() else {
                    return;
                };
                if day_count == 0 || f32::from(bounds.size.width) <= 0.0 {
                    return;
                }
                let fraction =
                    ((event.position.x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0);
                let index = ((fraction * day_count.saturating_sub(1) as f32).round() as usize)
                    .min(day_count - 1);
                if this.usage_chart_hover != Some(index) {
                    this.usage_chart_hover = Some(index);
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered && this.usage_chart_hover.is_some() {
                    this.usage_chart_hover = None;
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if day_count == 0 {
                    return;
                }
                let last = day_count - 1;
                let next = match event.keystroke.key.as_str() {
                    "left" => {
                        Some(this.usage_chart_hover.map_or(last, |index| index.saturating_sub(1)))
                    }
                    "right" => {
                        Some(this.usage_chart_hover.map_or(0, |index| (index + 1).min(last)))
                    }
                    "home" => Some(0),
                    "end" => Some(last),
                    "escape" if this.usage_chart_hover.is_some() => None,
                    _ => return,
                };
                cx.stop_propagation();
                if this.usage_chart_hover != next {
                    this.usage_chart_hover = next;
                    cx.notify();
                }
            }))
            .child(plot_canvas.size_full())
            .when_some(
                hover.and_then(|index| days.get(index).map(|day| (index, *day))),
                |element, (index, day)| {
                    element.child(usage_chart_readout(
                        history,
                        day,
                        if day_count <= 1 { 0.0 } else { index as f32 / (day_count - 1) as f32 },
                        metric,
                        cx,
                    ))
                },
            );

        div().flex().gap(px(8.0)).child(gutter).child(plot)
    }
}

fn usage_chart_readout(
    history: &UsageHistory,
    day: NaiveDate,
    fraction: f32,
    metric: UsageMetric,
    cx: &App,
) -> Div {
    let slice = history.day(day);
    let value = |provider: UsageProvider| {
        slice
            .map(|s| {
                let entry = s.by_provider[provider.index()];
                match metric {
                    UsageMetric::Cost => entry.cost_usd,
                    UsageMetric::Tokens => entry.total_tokens as f64,
                }
            })
            .unwrap_or(0.0)
    };
    let format_value = |v: f64| match metric {
        UsageMetric::Cost => format_usd(v),
        UsageMetric::Tokens => format_tokens_compact(v),
    };

    let theme = cx.theme();
    let dark = theme.is_dark();
    let border = theme.border;
    let fg = theme.foreground;
    let muted = theme.muted_foreground;
    let bg = theme.popover;

    let mut readout = div()
        .absolute()
        .top(px(0.0))
        .when(fraction <= 0.6, |el| el.left(relative(fraction)))
        .when(fraction > 0.6, |el| el.right(relative(1.0 - fraction)))
        .min_w(px(170.0))
        .px(px(10.0))
        .py(px(8.0))
        .corner(px(8.0))
        .border_1()
        .border_color(border)
        .bg(bg)
        .shadow_md()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .text_xs()
        .child(
            div().text_color(muted).font_medium().child(SharedString::from(format_day_short(day))),
        );
    let mut total = 0.0;
    for provider in UsageProvider::ALL {
        let amount = value(provider);
        total += amount;
        let color = usage_provider_color(provider, dark);
        readout = readout.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(provider_mark(provider, 11.0, color))
                .child(div().flex_1().text_color(muted).child(provider.label()))
                .child(div().text_color(fg).child(SharedString::from(format_value(amount)))),
        );
    }
    readout.child(
        div()
            .mt(px(2.0))
            .pt(px(4.0))
            .border_t_1()
            .border_color(border)
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(div().flex_1().text_color(muted).child("Total"))
            .child(
                div().font_medium().text_color(fg).child(SharedString::from(format_value(total))),
            ),
    )
}

/// Monotone cubic tangents (Fritsch–Carlson).
pub(crate) fn monotone_tangents(points: &[(f32, f32)]) -> Vec<f32> {
    let count = points.len();
    if count < 2 {
        return vec![0.0];
    }
    let mut slopes = Vec::with_capacity(count - 1);
    for index in 0..count - 1 {
        let dx = points[index + 1].0 - points[index].0;
        let dy = points[index + 1].1 - points[index].1;
        slopes.push(if dx == 0.0 { 0.0 } else { dy / dx });
    }

    let mut tangents = vec![0.0; count];
    tangents[0] = slopes[0];
    tangents[count - 1] = slopes[count - 2];
    for index in 1..count - 1 {
        let previous = slopes[index - 1];
        let next = slopes[index];
        tangents[index] = if previous * next <= 0.0 { 0.0 } else { (previous + next) / 2.0 };
    }

    for index in 0..count - 1 {
        let slope = slopes[index];
        if slope == 0.0 {
            tangents[index] = 0.0;
            tangents[index + 1] = 0.0;
            continue;
        }
        let a = tangents[index] / slope;
        let b = tangents[index + 1] / slope;
        let magnitude = a * a + b * b;
        if magnitude > 9.0 {
            let scale = 3.0 / magnitude.sqrt();
            tangents[index] = scale * a * slope;
            tangents[index + 1] = scale * b * slope;
        }
    }
    tangents
}

/// Smoothed polyline through `points`, as explicit cubic control points.
pub(crate) fn smooth_curve(points: &[(f32, f32)]) -> Vec<CurveSegment> {
    if points.len() < 2 {
        return Vec::new();
    }
    let tangents = monotone_tangents(points);
    let mut segments = Vec::with_capacity(points.len() - 1);
    for index in 0..points.len() - 1 {
        let from = points[index];
        let to = points[index + 1];
        let dx = to.0 - from.0;
        segments.push(CurveSegment {
            from,
            c1: (from.0 + dx / 3.0, from.1 + tangents[index] * dx / 3.0),
            c2: (to.0 - dx / 3.0, to.1 - tangents[index + 1] * dx / 3.0),
            to,
        });
    }
    segments
}

/// A scale whose maximum is a readable 1/2/5 × 10ⁿ step at or above the peak.
pub(crate) fn nice_scale(peak: f64, count: usize) -> (f64, Vec<f64>) {
    if peak <= 0.0 {
        return (0.0, vec![0.0]);
    }
    let raw_step = peak / count as f64;
    let magnitude = 10.0_f64.powf(raw_step.log10().floor());
    let normalized = raw_step / magnitude;
    let step = if normalized > 5.0 {
        10.0
    } else if normalized > 2.0 {
        5.0
    } else if normalized > 1.0 {
        2.0
    } else {
        1.0
    } * magnitude;
    let max = (peak / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut value = 0.0;
    while value <= max + step * 1e-6 {
        ticks.push(value);
        value += step;
    }
    (max, ticks)
}

#[cfg(test)]
#[path = "../../tests/unit/usage_view/chart.rs"]
mod tests;
