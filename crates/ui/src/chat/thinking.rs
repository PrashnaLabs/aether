use crate::theme::Corners;
use aether_core::{Step, ToolCall, ToolDetail, ToolStatus};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, h_flex,
    scroll::{ScrollableMask, Scrollbar},
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    App, Axis, Bounds, Global, Hsla, Pixels, Role, ScrollHandle, SharedString, Window, canvas, div,
    fill, linear_color_stop, linear_gradient, point, prelude::*, px, rems, size,
};

use super::cache::RenderCache;
use super::live::{clock, short_ms};
use super::markdown::Markdown;
use super::tool_row::{Reflow, tabular, verbose_rows};

struct ThoughtStates(RenderCache<bool>);

impl Default for ThoughtStates {
    fn default() -> Self {
        Self(RenderCache::new(4096))
    }
}

impl Global for ThoughtStates {}

/// The opening sentence of a thought, for its collapsed preview.
pub fn first_sentence(text: &str) -> String {
    let text = text.replace("**", "");
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let end = line.find(". ").map_or(line.len(), |i| i + 1);
    line[..end].to_string()
}

/// Live rows are open unless closed while live; finished rows are closed unless opened after.
pub fn thought_open(
    live: bool,
    while_live: Option<bool>,
    after: Option<bool>,
    verbose: bool,
) -> bool {
    if live { while_live.unwrap_or(true) } else { after.unwrap_or(verbose) }
}

pub fn thought_label(ms: Option<u64>) -> String {
    ms.filter(|ms| *ms >= 1000)
        .map_or_else(|| "Thought".to_string(), |ms| format!("Thought for {}", short_ms(ms)))
}

/// One thought: a header that expands into the reasoning text.
#[derive(IntoElement)]
pub struct ThoughtRow {
    key: SharedString,
    text: SharedString,
    ms: Option<u64>,
    live: bool,
    cwd: Option<SharedString>,
    on_reflow: Option<Reflow>,
}

impl ThoughtRow {
    /// `ms` is how long it thought, or so far while `live`.
    pub fn new(
        key: impl Into<SharedString>,
        text: SharedString,
        ms: Option<u64>,
        live: bool,
    ) -> Self {
        Self { key: key.into(), text, ms, live, cwd: None, on_reflow: None }
    }

    pub fn cwd(mut self, cwd: Option<SharedString>) -> Self {
        self.cwd = cwd;
        self
    }

    pub fn on_reflow(mut self, on_reflow: Option<Reflow>) -> Self {
        self.on_reflow = on_reflow;
        self
    }
}

impl RenderOnce for ThoughtRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = self.key;
        let live = self.live;
        let live_key = SharedString::from(format!("{key}-live"));
        let state_key = if live { live_key.clone() } else { key.clone() };
        let verbose = verbose_rows(cx);
        let is_open = {
            let states = &mut cx.default_global::<ThoughtStates>().0;
            let while_live = states.get(&live_key).copied();
            let after = states.get(&key).copied();
            thought_open(live, while_live, after, verbose)
        };
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let background = theme.background;
        let border = theme.border;
        let content = self.text.trim().to_string();

        let reflow = self.on_reflow;
        let body_reflow = reflow.clone();
        let header = h_flex()
            .id(SharedString::from(format!("thought-hdr-{key}")))
            .role(Role::Button)
            .aria_expanded(is_open)
            .tab_index(0)
            .min_h(rems(1.5))
            .px_2()
            .gap_2()
            .items_center()
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .corner(rems(0.375))
            .focus_visible(|s| s.border_color(theme.ring))
            .cursor_pointer()
            .text_sm()
            .text_color(muted)
            .hover(|s| s.text_color(theme.foreground))
            .active(|s| s.text_color(theme.foreground).bg(theme.secondary_active))
            .child(if live {
                Spinner::new().icon(IconName::LoaderCircle).xsmall().color(muted).into_any_element()
            } else {
                Icon::new(Lucide::Lightbulb).xsmall().into_any_element()
            })
            .child(if live {
                h_flex()
                    .gap_2()
                    .child(div().child("Thinking"))
                    .children(self.ms.map(|ms| tabular(div().child(clock(ms)))))
                    .into_any_element()
            } else {
                div().child(thought_label(self.ms)).into_any_element()
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(!live && !is_open, |d| d.child(first_sentence(&content))),
            )
            .child(
                Icon::new(if is_open { IconName::ChevronDown } else { IconName::ChevronRight })
                    .xsmall(),
            )
            .on_click(move |_, window, cx| {
                cx.default_global::<ThoughtStates>().0.insert(state_key.clone(), !is_open);
                if let Some(reflow) = &reflow {
                    reflow(cx);
                } else {
                    window.refresh();
                }
            });

        let body_key = SharedString::from(format!("{key}-content"));
        v_flex().w_full().child(header).when(is_open && !content.is_empty(), |v| {
            v.child(
                div()
                    .mt_1()
                    .ml_3()
                    .pl_3p5()
                    .border_l_1()
                    .border_color(border)
                    .text_size(rems(0.84375))
                    .line_height(rems(THOUGHT_LINE_HEIGHT))
                    .child(thought_scroll(
                        body_key.clone(),
                        background,
                        div().py_1().child(
                            Markdown::new(body_key, content.into())
                                .code_block_max_height(px(340.))
                                .cwd(self.cwd)
                                .live(live)
                                .muted(true)
                                .on_reflow(body_reflow),
                        ),
                        window,
                        cx,
                    )),
            )
        })
    }
}

// Match the text's line height so this scales with the app's font size.
const THOUGHT_LINE_HEIGHT: f32 = 1.375;
const THOUGHT_LINES: f32 = 14.;

/// Fade strength reflects how much content remains beyond each edge.
fn thought_fades(offset: Pixels, max: Pixels, height: Pixels) -> (f32, f32) {
    if max <= px(0.) || height <= px(0.) {
        return (0., 0.);
    }
    let top = (f32::from(-offset) / f32::from(height)).clamp(0., 1.);
    let bottom = (f32::from(max + offset) / f32::from(height)).clamp(0., 1.);
    (top, bottom)
}

fn thought_scroll(
    key: SharedString,
    background: Hsla,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let state = window.use_keyed_state(SharedString::from(format!("{key}-scroll")), cx, |_, _| {
        ScrollHandle::new()
    });
    let handle = state.read(cx).clone();
    let fade_handle = handle.clone();
    let fade_height = window.rem_size() * THOUGHT_LINE_HEIGHT;
    div()
        .relative()
        .w_full()
        .min_w_0()
        .child(
            div()
                .id(key.clone())
                .w_full()
                .max_h(rems(THOUGHT_LINES * THOUGHT_LINE_HEIGHT))
                .overflow_y_scroll()
                .track_scroll(&handle)
                .child(div().w_full().child(content)),
        )
        // Keep wheel events inside this viewport until it reaches an edge.
        .child(ScrollableMask::new(Axis::Vertical, &handle).id(key.clone()))
        // Paint after layout so overflow and fades are correct on the first frame,
        // including when markdown finishes parsing in the background.
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let height = fade_height.min(bounds.size.height / 2.);
                    let (top, bottom) =
                        thought_fades(fade_handle.offset().y, fade_handle.max_offset().y, height);
                    for (opacity, y, angle) in
                        [(top, bounds.top(), 0.), (bottom, bounds.bottom() - height, 180.)]
                    {
                        if opacity > 0. {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(bounds.left(), y),
                                    size(bounds.size.width, height),
                                ),
                                linear_gradient(
                                    angle,
                                    linear_color_stop(background.opacity(0.), 0.),
                                    linear_color_stop(background.opacity(opacity), 1.),
                                ),
                            ));
                        }
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        // Keep the scrollbar above the fades.
        .child(div().absolute().inset_0().child(Scrollbar::new(&handle).id(key)))
}

fn tools(steps: &[Step]) -> impl Iterator<Item = &ToolCall> {
    steps.iter().filter_map(|s| match s {
        Step::Tool(call) => Some(call),
        _ => None,
    })
}

/// "Run command · cargo test…" for the latest running tool.
pub fn live_label(steps: &[Step]) -> Option<String> {
    let call = tools(steps).filter(|c| c.status == ToolStatus::Running).last()?;
    let title = if !call.title.is_empty() {
        call.title.as_str()
    } else {
        match &call.detail {
            ToolDetail::Other { name, .. } if name.to_ascii_lowercase().contains("question") => {
                "Ask question"
            }
            _ => "Tool",
        }
    };
    let subject = match &call.detail {
        ToolDetail::Edit { path, .. } | ToolDetail::Read { path, .. } => {
            path.rsplit('/').next().unwrap_or(path).to_string()
        }
        ToolDetail::Other { name, input, .. } if name.to_ascii_lowercase().contains("question") => {
            input.lines().next().unwrap_or_default().trim().to_string()
        }
        ToolDetail::Other { .. } => String::new(),
        detail => detail.summary(),
    };
    Some(if subject.is_empty() || subject == title {
        format!("{title}…")
    } else {
        format!("{title} · {subject}…")
    })
}

#[cfg(test)]
#[path = "../../tests/unit/chat/thinking.rs"]
mod tests;
