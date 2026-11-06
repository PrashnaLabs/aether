use crate::theme::Corners;
use std::ops::Range;

use aether_core::{Step, ToolCall, ToolDetail, ToolStatus};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, button::ButtonVariants, h_flex,
    spinner::Spinner, v_flex,
};
use gpui_kit::{App, Global, Hsla, Role, SharedString, Window, div, prelude::*, px, rems};

use super::activity::{
    ActivityFacts, ActivityGroup, activity_group_open, activity_kind, tool_activity_kind,
    tool_needs_attention,
};
use super::cache::RenderCache;
use super::context_ring::format_tokens;
use super::live::{clock, short_ms, since};
use super::markdown::Markdown;
use super::thinking::first_sentence;
use super::tool_row::{OnOpen, Reflow, ToolRow, row_open, set_row_open, tabular, tool_key};
use crate::theme;

pub const NESTED_ROWS: usize = 5;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CardStatus {
    Starting,
    Running,
    Waiting,
    Background,
    Done,
    Failed,
    Stopped,
}

pub fn card_status(call: &ToolCall, children: &[ToolCall]) -> CardStatus {
    let child_waits = children.iter().any(|c| c.status == ToolStatus::Waiting);
    match call.status {
        ToolStatus::Running | ToolStatus::Waiting | ToolStatus::Background if child_waits => {
            CardStatus::Waiting
        }
        ToolStatus::Waiting => CardStatus::Waiting,
        ToolStatus::Running if call.started_ms.is_none() && children.is_empty() => {
            CardStatus::Starting
        }
        ToolStatus::Running => CardStatus::Running,
        ToolStatus::Background => CardStatus::Background,
        ToolStatus::Failed => CardStatus::Failed,
        ToolStatus::Stopped | ToolStatus::Denied => CardStatus::Stopped,
        ToolStatus::Done | ToolStatus::Unknown => CardStatus::Done,
    }
}

pub fn waiting_line(children: &[ToolCall]) -> Option<String> {
    let call = children.iter().find(|c| c.status == ToolStatus::Waiting)?;
    let title = if call.title.is_empty() { "Tool" } else { call.title.as_str() };
    let line = format!("{title} {}", call.detail.summary());
    Some(format!("Waiting for your approval below · {}", line.trim()))
}

/// Hidden count and the known children to show.
pub fn earlier(reported: u32, known: usize, all: bool) -> (usize, Range<usize>) {
    let total = (reported as usize).max(known);
    let shown = if all { 0..known } else { known.saturating_sub(NESTED_ROWS)..known };
    (total - shown.len(), shown)
}

/// Extend the last-five window to the start of its first group. Its disclosure
/// key then survives as new calls arrive, instead of shifting on every update.
fn activity_window(reported: u32, children: &[ToolCall], all: bool) -> (usize, Range<usize>) {
    let (_, mut shown) = earlier(reported, children.len(), all);
    while shown.start > 0 && shown.start < children.len() {
        let kind = tool_activity_kind(&children[shown.start]);
        if kind.is_none() || kind != tool_activity_kind(&children[shown.start - 1]) {
            break;
        }
        shown.start -= 1;
    }
    ((reported as usize).max(children.len()) - shown.len(), shown)
}

pub fn earlier_label(n: usize) -> String {
    format!("+ {n} earlier tool call{}", if n == 1 { "" } else { "s" })
}

/// Live statuses tick from the row's start; the rest show their final duration, sub-second
/// only for failures, like a tool row.
pub fn card_timer(call: &ToolCall, status: CardStatus, now_ms: Option<i64>) -> Option<String> {
    match status {
        CardStatus::Starting
        | CardStatus::Running
        | CardStatus::Waiting
        | CardStatus::Background => since(call.started_ms, now_ms?),
        CardStatus::Failed => call.ms.map(short_ms),
        _ => call.ms.filter(|ms| *ms >= 1000).map(clock),
    }
}

/// Agent, time, tools and tokens, skipping what's unknown.
pub fn card_facts(agent: &str, timer: Option<String>, tools: u32, tokens: u64) -> String {
    let tools = (tools > 0).then(|| format!("{tools} tool{}", if tools == 1 { "" } else { "s" }));
    let tokens = (tokens > 0).then(|| format!("{} tok", format_tokens(tokens)));
    [(!agent.is_empty()).then(|| agent.to_string()), timer, tools, tokens]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Cards showing every nested row by `{card key}-all`, or the whole prompt by `{card key}-prompt`.
struct CardStates(RenderCache<bool>);

impl Default for CardStates {
    fn default() -> Self {
        Self(RenderCache::new(1024))
    }
}

impl Global for CardStates {}

/// A subagent's Task row and the rows it ran.
#[derive(IntoElement)]
pub struct SubagentCard {
    key: SharedString,
    session: SharedString,
    call: ToolCall,
    children: Vec<ToolCall>,
    cwd: Option<SharedString>,
    on_reflow: Option<Reflow>,
    on_show_ask: Option<OnOpen>,
    on_stop: Option<OnOpen>,
}

impl SubagentCard {
    pub fn new(key: impl Into<SharedString>, call: ToolCall, children: Vec<ToolCall>) -> Self {
        let session = SharedString::default();
        Self {
            key: key.into(),
            session,
            call,
            children,
            cwd: None,
            on_reflow: None,
            on_show_ask: None,
            on_stop: None,
        }
    }

    /// Session the children's row keys are made in, with `tool_key`.
    pub fn session(mut self, session: impl Into<SharedString>) -> Self {
        self.session = session.into();
        self
    }

    pub fn cwd(mut self, cwd: Option<SharedString>) -> Self {
        self.cwd = cwd;
        self
    }

    pub fn on_reflow(mut self, on_reflow: Option<Reflow>) -> Self {
        self.on_reflow = on_reflow;
        self
    }

    /// Scrolls to the approval a child waits on.
    pub fn on_show_ask(mut self, on_show_ask: Option<OnOpen>) -> Self {
        self.on_show_ask = on_show_ask;
        self
    }

    /// Puts Stop in the header while the agent runs.
    pub fn on_stop(mut self, on_stop: Option<OnOpen>) -> Self {
        self.on_stop = on_stop;
        self
    }
}

impl RenderOnce for SubagentCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let status = card_status(&self.call, &self.children);
        let key = self.key.clone();
        let all_key = SharedString::from(format!("{key}-all"));
        let prompt_key = SharedString::from(format!("{key}-prompt"));
        let attention = self.children.iter().any(tool_needs_attention);
        let is_open = row_open(cx, &key)
            .unwrap_or(attention || matches!(status, CardStatus::Waiting | CardStatus::Failed));
        let all = cx.default_global::<CardStates>().0.get(&all_key).copied().unwrap_or(false);
        let whole_prompt =
            cx.default_global::<CardStates>().0.get(&prompt_key).copied().unwrap_or(false);
        let (mango_text, danger_text, info_text, success_text) = (
            theme::mango_text(cx),
            theme::danger_text(cx),
            theme::info_text(cx),
            theme::success_text(cx),
        );
        let theme = cx.theme();
        let (muted, fg, secondary) = (theme.muted_foreground, theme.foreground, theme.secondary);
        let pressed = theme.secondary_active;
        let (accent, accent_text): (Hsla, Hsla) = match status {
            CardStatus::Starting | CardStatus::Running | CardStatus::Background => {
                (gpui_kit::transparent_black(), info_text)
            }
            CardStatus::Waiting => (theme::mango(), mango_text),
            CardStatus::Failed => (theme.danger, danger_text),
            CardStatus::Done => (theme.success.opacity(0.6), success_text),
            CardStatus::Stopped => (theme.border, muted),
        };
        let border = theme.border;
        let mono = theme.mono_font_family.clone();
        let ring = theme.ring;

        let ToolDetail::Task { output, agent, prompt, tools, tokens, activity, .. } =
            self.call.detail.clone()
        else {
            return ToolRow::new(key, self.call).into_any_element();
        };
        let name = if agent.is_empty() { self.call.title.clone() } else { agent };
        let description = self.call.detail.summary();
        let title = if description.is_empty() { name.clone() } else { description };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as i64);
        let facts = card_facts(&name, card_timer(&self.call, status, now), tools, tokens);
        let live = matches!(
            status,
            CardStatus::Starting
                | CardStatus::Running
                | CardStatus::Waiting
                | CardStatus::Background
        );
        let stop = self.on_stop.clone().filter(|_| live).map(|stop| {
            crate::components::button(SharedString::from(format!("agent-stop-{key}")))
                .ghost()
                .xsmall()
                .label("Stop")
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    stop(window, cx);
                })
        });

        let spinner =
            |color: Hsla| Spinner::new().icon(IconName::LoaderCircle).xsmall().color(color);
        let (pill_icon, pill_text) = match status {
            CardStatus::Starting => (spinner(accent_text).into_any_element(), "Starting"),
            CardStatus::Running => (spinner(accent_text).into_any_element(), "Running"),
            CardStatus::Waiting => (
                Icon::new(Lucide::Shield).xsmall().text_color(accent_text).into_any_element(),
                "Needs approval",
            ),
            CardStatus::Background => {
                (div().size(px(6.)).rounded_full().bg(theme.info).into_any_element(), "Background")
            }
            CardStatus::Done => (
                Icon::new(IconName::CircleCheck)
                    .xsmall()
                    .text_color(accent_text)
                    .into_any_element(),
                "Done",
            ),
            CardStatus::Failed => (
                Icon::new(IconName::CircleX).xsmall().text_color(accent_text).into_any_element(),
                "Failed",
            ),
            CardStatus::Stopped => (
                Icon::new(IconName::Square).xsmall().text_color(accent_text).into_any_element(),
                "Stopped",
            ),
        };

        let reflow = self.on_reflow.clone();
        let toggle_key = key.clone();
        let header = v_flex()
            .id(SharedString::from(format!("agent-hdr-{key}")))
            .role(Role::Button)
            .aria_expanded(is_open)
            .tab_index(0)
            .w_full()
            .min_h(rems(2.))
            .px_1()
            .py(px(2.))
            .gap(px(2.))
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .corner(rems(0.375))
            .hover(|s| s.bg(secondary))
            .active(|s| s.bg(pressed))
            .focus_visible(|s| s.border_color(ring))
            .cursor_pointer()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .child(Icon::new(Lucide::Bot).xsmall().text_color(accent_text))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_semibold()
                            .text_color(fg)
                            .child(title),
                    )
                    .children(stop)
                    .child(
                        Icon::new(if is_open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall()
                        .text_color(muted),
                    ),
            )
            .child(
                h_flex()
                    .pl(rems(1.25))
                    .gap_1()
                    .items_center()
                    .text_xs()
                    .text_color(muted)
                    .map(tabular)
                    .child(pill_icon)
                    .child(div().flex_none().text_color(accent_text).child(pill_text))
                    .when(!facts.is_empty(), |d| {
                        d.child("·").child(div().min_w_0().truncate().child(facts))
                    }),
            )
            .on_click(move |_, window, cx| {
                set_row_open(cx, toggle_key.clone(), !is_open);
                match &reflow {
                    Some(reflow) => reflow(cx),
                    None => window.refresh(),
                }
            });

        let summary = match status {
            CardStatus::Running | CardStatus::Background if !activity.is_empty() => Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .text_color(muted)
                    .child(spinner(muted))
                    .child(div().min_w_0().truncate().child(activity.clone()))
                    .into_any_element(),
            ),
            CardStatus::Waiting if !is_open => {
                let show = self.on_show_ask.clone();
                Some(
                    h_flex()
                        .id(SharedString::from(format!("agent-ask-{key}")))
                        .role(Role::Button)
                        .tab_index(0)
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .text_color(mango_text)
                        .corner(rems(0.375))
                        .border_1()
                        .border_color(gpui_kit::transparent_black())
                        .hover(|s| s.bg(secondary))
                        .active(|s| s.bg(pressed))
                        .focus_visible(|s| s.border_color(ring))
                        .cursor_pointer()
                        .child(Icon::new(Lucide::Shield).xsmall())
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .child(waiting_line(&self.children).unwrap_or_default()),
                        )
                        .on_click(move |_, window, cx| {
                            if let Some(show) = &show {
                                show(window, cx);
                            }
                        })
                        .into_any_element(),
                )
            }
            CardStatus::Failed => {
                let error = self.call.error.clone().unwrap_or_default();
                Some(
                    div()
                        .text_xs()
                        .font_family(mono)
                        .text_color(danger_text)
                        .truncate()
                        .child(error.lines().next().unwrap_or_default().to_string())
                        .into_any_element(),
                )
            }
            CardStatus::Done if !is_open && !output.trim().is_empty() => Some(
                div()
                    .text_xs()
                    .text_color(muted)
                    .truncate()
                    .child(first_sentence(&output))
                    .into_any_element(),
            ),
            _ => None,
        };

        let expanded = is_open.then(|| {
            let (hidden, shown) = activity_window(tools, &self.children, all || attention);
            let steps: Vec<_> = self.children[shown].iter().cloned().map(Step::Tool).collect();
            let rows: Vec<_> = steps
                .chunk_by(|a, b| activity_kind(a).is_some() && activity_kind(a) == activity_kind(b))
                .map(|run| {
                    let Step::Tool(first) = &run[0] else { unreachable!() };
                    let group_key: SharedString = format!("{key}-activity-{}", first.id).into();
                    let facts = activity_kind(&run[0]).map(|kind| {
                        ActivityFacts::new(kind, &run.iter().collect::<Vec<_>>(), false)
                    });
                    let open = run.len() == 1 || activity_group_open(cx, &group_key);
                    let children: Vec<_> = run
                        .iter()
                        .filter(|_| open)
                        .filter_map(|s| match s {
                            Step::Tool(c) => Some(
                                ToolRow::new(tool_key(&self.session, &c.id), c.clone())
                                    .cwd(self.cwd.clone())
                                    .on_reflow(self.on_reflow.clone())
                                    .nested(true)
                                    .into_any_element(),
                            ),
                            _ => None,
                        })
                        .collect();
                    if run.len() == 1 {
                        return children.into_iter().next().unwrap();
                    }
                    ActivityGroup::new(group_key, facts.unwrap(), children)
                        .on_reflow(self.on_reflow.clone())
                        .into_any_element()
                })
                .collect();
            let reflow = self.on_reflow.clone();
            let more_key = all_key.clone();
            let long_prompt = prompt.lines().count() > 2 || prompt.len() > 160;
            let (prompt_reflow, prompt_toggle) = (self.on_reflow.clone(), prompt_key.clone());
            let prompt_button = long_prompt.then(|| {
                crate::components::button(SharedString::from(format!("agent-prompt-{key}")))
                    .ghost()
                    .xsmall()
                    .label(if whole_prompt { "Hide prompt" } else { "Show prompt" })
                    .on_click(move |_, window, cx| {
                        cx.default_global::<CardStates>()
                            .0
                            .insert(prompt_toggle.clone(), !whole_prompt);
                        match &prompt_reflow {
                            Some(reflow) => reflow(cx),
                            None => window.refresh(),
                        }
                    })
            });
            v_flex()
                .gap_1()
                .when(!prompt.is_empty(), |v| {
                    v.child(
                        v_flex()
                            .pb_1()
                            .gap_1()
                            .border_b_1()
                            .border_dashed()
                            .border_color(border)
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .when(long_prompt && !whole_prompt, |d| {
                                        d.max_h(rems(2.5)).overflow_hidden()
                                    })
                                    .child(
                                        Markdown::new(
                                            SharedString::from(format!("{key}-prompt-md")),
                                            prompt.clone().into(),
                                        )
                                        .cwd(self.cwd.clone())
                                        .on_reflow(self.on_reflow.clone()),
                                    ),
                            )
                            .children(prompt_button.map(|b| div().child(b))),
                    )
                })
                .when(hidden > 0, |v| {
                    let label =
                        div().px_2().text_xs().text_color(muted).child(earlier_label(hidden));
                    if all {
                        return v.child(label);
                    }
                    v.child(
                        label
                            .id(SharedString::from(format!("agent-more-{key}")))
                            .role(Role::Button)
                            .tab_index(0)
                            .min_h(rems(1.5))
                            .flex()
                            .items_center()
                            .corner(rems(0.375))
                            .border_1()
                            .border_color(gpui_kit::transparent_black())
                            .hover(|s| s.bg(secondary))
                            .active(|s| s.bg(pressed))
                            .focus_visible(|s| s.border_color(ring))
                            .cursor_pointer()
                            .on_click(move |_, window, cx| {
                                cx.default_global::<CardStates>().0.insert(more_key.clone(), true);
                                match &reflow {
                                    Some(reflow) => reflow(cx),
                                    None => window.refresh(),
                                }
                            }),
                    )
                })
                .children(rows)
                .when(status == CardStatus::Done && !output.trim().is_empty(), |v| {
                    v.child(
                        div().pt_1().text_sm().child(
                            Markdown::new(
                                SharedString::from(format!("{key}-result")),
                                output.clone().into(),
                            )
                            .cwd(self.cwd.clone())
                            .on_reflow(self.on_reflow.clone()),
                        ),
                    )
                })
        });

        v_flex()
            .w_full()
            .gap_1()
            .p_2()
            .corner(px(12.))
            .bg(secondary.opacity(0.5))
            .border_1()
            .border_color(accent)
            .child(header)
            .children(summary)
            .children(expanded)
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/subagent.rs"]
mod tests;
