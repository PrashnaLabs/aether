use crate::theme::Corners;
use aether_core::{NoticeKind, NoticeState, Step, ToolCall, ToolDetail, ToolStatus};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, h_flex, spinner::Spinner, v_flex,
};
use gpui_kit::{AnyElement, App, Role, SharedString, Window, div, prelude::*, rems};

use super::live::short_ms;
use super::tool_row::{Reflow, peek_row_open, row_open, set_row_open, tabular};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Thinking,
    Reads,
    Searches,
    Commands,
    Edits,
    Hooks,
    Skills,
    Tools,
}

impl ActivityKind {
    fn label(self) -> &'static str {
        match self {
            Self::Thinking => "Thinking steps",
            Self::Reads => "File reads",
            Self::Searches => "Searches",
            Self::Commands => "Commands",
            Self::Edits => "File edits",
            Self::Hooks => "Hooks",
            Self::Skills => "Skills used",
            Self::Tools => "Tool calls",
        }
    }

    fn icon(self) -> Lucide {
        match self {
            Self::Thinking => Lucide::Lightbulb,
            Self::Reads => Lucide::Files,
            Self::Searches => Lucide::Search,
            Self::Commands => Lucide::SquareTerminal,
            Self::Edits => Lucide::FilePen,
            Self::Hooks => Lucide::Zap,
            Self::Skills => Lucide::BookOpen,
            Self::Tools => Lucide::Wrench,
        }
    }
}

pub(crate) fn tool_needs_attention(call: &ToolCall) -> bool {
    matches!(
        call.status,
        ToolStatus::Failed | ToolStatus::Waiting | ToolStatus::Stopped | ToolStatus::Denied
    ) || call.error.is_some()
        || matches!(call.detail, ToolDetail::Command { exit_code: Some(code), .. } if code != 0)
}

pub fn activity_needs_attention(step: &Step) -> bool {
    match step {
        Step::Tool(c) => tool_needs_attention(c),
        Step::Search { results: Some(Err(_)), .. } => true,
        Step::Notice(n) => n.state == NoticeState::Failed,
        _ => false,
    }
}

/// Operations requiring attention stay outside collapsible activity groups.
pub fn activity_kind(step: &Step) -> Option<ActivityKind> {
    if activity_needs_attention(step) {
        return None;
    }
    Some(match step {
        Step::Thought(_) | Step::Thinking { .. } => ActivityKind::Thinking,
        Step::Skill(_) => ActivityKind::Skills,
        Step::Search { .. } => ActivityKind::Searches,
        Step::Notice(n) if n.kind == NoticeKind::Hook && n.url.is_none() => ActivityKind::Hooks,
        Step::Tool(c) => return tool_activity_kind(c),
        _ => return None,
    })
}

pub(crate) fn tool_activity_kind(call: &ToolCall) -> Option<ActivityKind> {
    if tool_needs_attention(call) {
        return None;
    }
    Some(match call.detail {
        ToolDetail::Read { .. } => ActivityKind::Reads,
        ToolDetail::Search { .. } => ActivityKind::Searches,
        ToolDetail::Command { .. } => ActivityKind::Commands,
        ToolDetail::Edit { .. } => ActivityKind::Edits,
        ToolDetail::Task { .. } => return None,
        _ => ActivityKind::Tools,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityFacts {
    pub kind: ActivityKind,
    pub count: usize,
    pub active: bool,
    pub duration_ms: Option<u64>,
    pub preview: String,
}

impl ActivityFacts {
    pub fn new(kind: ActivityKind, steps: &[&Step], live: bool) -> Self {
        let active = steps.iter().any(|s| match s {
            Step::Thinking { ms: None, .. } => live,
            Step::Tool(c) => matches!(c.status, ToolStatus::Running | ToolStatus::Background),
            Step::Search { results: None, .. } => live,
            Step::Notice(n) => n.state == NoticeState::Running,
            _ => false,
        });
        // A partial sum would imply a duration we do not know. Parallel calls'
        // durations describe tool time, not the wall-clock length of the group.
        let duration_ms = steps
            .iter()
            .try_fold(0u64, |total, s| {
                let ms = match s {
                    Step::Tool(c) => c.ms,
                    Step::Thinking { ms, .. } => *ms,
                    _ => None,
                }?;
                Some(total.saturating_add(ms))
            })
            .filter(|_| !active);
        let preview = steps
            .last()
            .map(|s| match s {
                Step::Tool(c) => c.detail.summary(),
                Step::Thought(text) | Step::Thinking { text, .. } => {
                    super::thinking::first_sentence(text)
                }
                Step::Skill(name) => name.clone(),
                Step::Search { query, .. } => query.clone(),
                Step::Notice(n) => n.text.clone(),
                _ => String::new(),
            })
            .unwrap_or_default();
        Self { kind, count: steps.len(), active, duration_ms, preview }
    }
}

/// Used before constructing children, so collapsed groups do not build tool output or diffs.
/// Closed until opened: quick tools flip running on and off, so following them flickers.
pub fn activity_group_open(cx: &App, key: &SharedString) -> bool {
    peek_row_open(cx, key).unwrap_or(false)
}

/// One quiet disclosure header, with the original rows beneath a shared guide.
#[derive(IntoElement)]
pub struct ActivityGroup {
    key: SharedString,
    facts: ActivityFacts,
    children: Vec<AnyElement>,
    reflow: Option<Reflow>,
}

impl ActivityGroup {
    pub fn new(
        key: impl Into<SharedString>,
        facts: ActivityFacts,
        children: Vec<AnyElement>,
    ) -> Self {
        Self { key: key.into(), facts, children, reflow: None }
    }

    pub fn on_reflow(mut self, reflow: Option<Reflow>) -> Self {
        self.reflow = reflow;
        self
    }
}

impl RenderOnce for ActivityGroup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let open = row_open(cx, &self.key).unwrap_or(false);
        let theme = cx.theme();
        let (muted, fg, border, ring, hover, pressed) = (
            theme.muted_foreground,
            theme.foreground,
            theme.border,
            theme.ring,
            theme.secondary,
            theme.secondary_active,
        );
        let facts = self.facts;
        let duration = facts.duration_ms.map(|ms| format!("{} total", short_ms(ms)));
        let label = format!(
            "{}, {}{}, {}",
            facts.kind.label(),
            facts.count,
            if facts.active { ", active" } else { "" },
            facts.preview
        );
        let key = self.key;
        let reflow = self.reflow;
        let header = h_flex()
            .id(key.clone())
            .role(Role::Button)
            .aria_label(SharedString::from(label))
            .aria_expanded(open)
            .tab_index(0)
            .w_full()
            .min_w_0()
            .min_h(rems(1.75))
            .px_2()
            .gap_2()
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .corner(rems(0.375))
            .text_sm()
            .text_color(muted)
            .cursor_pointer()
            .hover(|s| s.bg(hover).text_color(fg))
            .active(|s| s.bg(pressed))
            .focus_visible(|s| s.border_color(ring))
            .child(Icon::new(facts.kind.icon()).xsmall())
            .child(div().min_w_0().truncate().font_medium().child(facts.kind.label()))
            .child(tabular(div().flex_none().text_xs().child(facts.count.to_string())))
            .child(div().flex_1().min_w_0().truncate().when(!open, |d| d.child(facts.preview)))
            .children(duration.map(|d| tabular(div().flex_none().text_xs().child(d))))
            .when(facts.active, |h| {
                h.child(Spinner::new().icon(IconName::LoaderCircle).xsmall().color(muted))
            })
            .child(
                Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight })
                    .xsmall(),
            )
            .on_click(move |_, window, cx| {
                set_row_open(cx, key.clone(), !open);
                match &reflow {
                    Some(f) => f(cx),
                    None => window.refresh(),
                }
            });
        v_flex().w_full().min_w_0().child(header).when(open, |v| {
            v.child(
                v_flex()
                    .ml_3()
                    .pl_3()
                    .mt_1()
                    .gap_1()
                    .border_l_1()
                    .border_color(border)
                    .children(self.children),
            )
        })
    }
}

/// A finished stretch of work folded under one "Worked for" header; closed until opened.
#[derive(IntoElement)]
pub struct WorkGroup {
    key: SharedString,
    label: String,
    count: usize,
    thought: bool,
    children: Vec<AnyElement>,
    reflow: Option<Reflow>,
}

impl WorkGroup {
    pub fn new(
        key: impl Into<SharedString>,
        label: String,
        count: usize,
        children: Vec<AnyElement>,
    ) -> Self {
        let thought = label.starts_with("Thought");
        Self { key: key.into(), label, count, thought, children, reflow: None }
    }

    pub fn on_reflow(mut self, reflow: Option<Reflow>) -> Self {
        self.reflow = reflow;
        self
    }
}

impl RenderOnce for WorkGroup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let open = row_open(cx, &self.key).unwrap_or(false);
        let theme = cx.theme();
        let (muted, fg, border, ring, hover) =
            (theme.muted_foreground, theme.foreground, theme.border, theme.ring, theme.secondary);
        let (key, reflow) = (self.key, self.reflow);
        let steps = format!("{} steps", self.count);
        let header = h_flex()
            .id(key.clone())
            .role(Role::Button)
            .aria_label(SharedString::from(format!("{}, {steps}", self.label)))
            .aria_expanded(open)
            .tab_index(0)
            .w_full()
            .min_w_0()
            .min_h(rems(1.75))
            .px_2()
            .gap_2()
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .corner(rems(0.375))
            .text_sm()
            .text_color(muted)
            .cursor_pointer()
            .hover(|s| s.bg(hover).text_color(fg))
            .focus_visible(|s| s.border_color(ring))
            .child(Icon::new(if self.thought { Lucide::Lightbulb } else { Lucide::Clock }).xsmall())
            .child(div().flex_none().font_medium().child(self.label))
            .child(tabular(div().flex_1().min_w_0().truncate().text_xs().child(steps)))
            .child(
                Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight })
                    .xsmall(),
            )
            .on_click(move |_, window, cx| {
                set_row_open(cx, key.clone(), !open);
                match &reflow {
                    Some(f) => f(cx),
                    None => window.refresh(),
                }
            });
        v_flex().w_full().min_w_0().child(header).when(open, |v| {
            v.child(
                v_flex()
                    .ml_3()
                    .pl_3()
                    .mt_1()
                    .gap_1()
                    .border_l_1()
                    .border_color(border)
                    .children(self.children),
            )
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/activity.rs"]
mod tests;
