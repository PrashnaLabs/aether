use crate::theme::Corners;
use gpui_kit::rems;
use std::collections::HashSet;
use std::rc::Rc;

use aether_core::{Message, NoticeKind, NoticeState, Step, ToolCall, ToolDetail, ToolStatus};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, button::ButtonVariants, h_flex,
    menu::ContextMenuExt, popover::Popover, v_flex,
};
use gpui_kit::{Anchor, App, ClickEvent, Role, SharedString, Window, div, prelude::*, px};

use super::context_ring::format_tokens;
use super::live::clock;
use super::message::{Handler, copy_button, menu_item};
use super::tool_row::{count, tabular};
use crate::components::Column;
use crate::theme;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Outcome {
    Worked,
    Failed,
    Stopped,
}

impl Outcome {
    /// Failed beats stopped; `stopped` is the user's stop of this run.
    pub fn of(m: &Message, stopped: bool) -> Self {
        let cut = stopped
            || m.steps
                .iter()
                .any(|s| matches!(s, Step::Tool(c) if c.status == ToolStatus::Stopped));
        match () {
            _ if m.failed || m.error.is_some() => Self::Failed,
            _ if cut => Self::Stopped,
            _ => Self::Worked,
        }
    }
}

fn calls(steps: &[Step]) -> impl Iterator<Item = &ToolCall> {
    steps
        .iter()
        .filter_map(|s| if let Step::Tool(c) = s { Some(c) } else { None })
        .filter(|c| c.status != ToolStatus::Denied)
}

/// (tool calls, subagents), nested rows included, denied ones left out.
pub fn tool_counts(steps: &[Step]) -> (usize, usize) {
    let tasks = calls(steps).filter(|c| matches!(c.detail, ToolDetail::Task { .. })).count();
    (calls(steps).count() - tasks, tasks)
}

/// No time is shown when it isn't known, never a guessed one.
pub fn fold_lead(outcome: Outcome, ms: Option<u64>) -> String {
    let (bare, joined) = match outcome {
        Outcome::Worked => ("Worked", "for"),
        Outcome::Failed => ("Failed", "after"),
        Outcome::Stopped => ("Stopped", "after"),
    };
    match ms {
        Some(ms) => format!("{bare} {joined} {}", clock(ms)),
        None => bare.into(),
    }
}

pub fn fold_failed(steps: &[Step]) -> usize {
    steps
        .iter()
        .filter(|s| match s {
            Step::Tool(c) => c.status == ToolStatus::Failed,
            Step::Notice(n) => n.state == NoticeState::Failed,
            Step::Search { results: Some(Err(_)), .. } => true,
            _ => false,
        })
        .count()
}

pub fn fold_counts(steps: &[Step]) -> String {
    let calls = || calls(steps);
    let files = |kind: fn(&ToolDetail) -> Option<&str>| {
        calls().filter_map(|c| kind(&c.detail)).collect::<HashSet<_>>().len()
    };
    let of = |kind: fn(&ToolDetail) -> bool| calls().filter(|c| kind(&c.detail)).count();
    let read = files(|d| match d {
        ToolDetail::Read { path, .. } => Some(path),
        _ => None,
    });
    let edited = files(|d| match d {
        ToolDetail::Edit { path, .. } => Some(path),
        _ => None,
    });
    let searched = of(|d| matches!(d, ToolDetail::Search { .. }))
        + steps.iter().filter(|s| matches!(s, Step::Search { .. })).count();
    let used = of(|d| {
        matches!(d, ToolDetail::Web { .. } | ToolDetail::Other { .. } | ToolDetail::Plain(_))
    });
    [
        (tool_counts(steps).1, "subagent", "subagents", ""),
        (
            steps.iter().filter(|s| matches!(s, Step::Thought(_) | Step::Thinking { .. })).count(),
            "thinking step",
            "thinking steps",
            "",
        ),
        (read, "file", "files", "read "),
        (of(|d| matches!(d, ToolDetail::Command { .. })), "command", "commands", "ran "),
        (edited, "file", "files", "edited "),
        (searched, "time", "times", "searched "),
        (used, "tool", "tools", "used "),
        (
            steps
                .iter()
                .filter(|s| matches!(s, Step::Notice(n) if n.kind == NoticeKind::Hook))
                .count(),
            "hook",
            "hooks",
            "ran ",
        ),
        (
            steps
                .iter()
                .filter_map(|s| if let Step::Skill(name) = s { Some(name) } else { None })
                .collect::<HashSet<_>>()
                .len(),
            "skill",
            "skills",
            "used ",
        ),
    ]
    .into_iter()
    .filter(|(n, ..)| *n > 0)
    .map(|(n, one, many, verb)| format!(" · {verb}{}", count(n, one, many)))
    .collect()
}

pub struct FootFacts {
    pub outcome: Outcome,
    pub duration_ms: Option<u64>,
    pub tools: usize,
    pub subagents: usize,
    pub model: Option<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cached: u64,
    pub cost: Option<String>,
    pub at: Option<String>,
}

pub fn footer_lead(f: &FootFacts) -> String {
    match (f.outcome, f.duration_ms) {
        (Outcome::Worked, Some(ms)) => format!("Worked {}", clock(ms)),
        (other, ms) => fold_lead(other, ms),
    }
}

pub fn footer_details(f: &FootFacts) -> Vec<(&'static str, String)> {
    let tokens = f.tokens_in + f.tokens_out > 0;
    let cached = f.cached as f64 / f.tokens_in.max(1) as f64 * 100.;
    [
        (f.tools > 0).then(|| ("Tool calls", f.tools.to_string())),
        (f.subagents > 0).then(|| ("Subagents", f.subagents.to_string())),
        f.model.clone().map(|model| ("Model", model)),
        tokens.then(|| ("Input", format_tokens(f.tokens_in))),
        tokens.then(|| ("Output", format_tokens(f.tokens_out))),
        (tokens && f.cached > 0).then(|| ("Cached", format!("{cached:.0}%"))),
        f.cost.clone().map(|cost| ("Cost", cost)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A finished reply's work, folded to one line that expands its timeline.
#[derive(IntoElement)]
pub struct FoldLine {
    id: SharedString,
    lead: String,
    counts: String,
    failed: usize,
    failed_lead: bool,
    open: bool,
    on_toggle: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
}

impl FoldLine {
    pub fn new(
        id: impl Into<SharedString>,
        m: &Message,
        outcome: Outcome,
        duration_ms: Option<u64>,
        open: bool,
    ) -> Self {
        Self {
            id: id.into(),
            lead: fold_lead(outcome, duration_ms),
            counts: fold_counts(&m.steps),
            failed: fold_failed(&m.steps),
            failed_lead: outcome == Outcome::Failed,
            open,
            on_toggle: None,
        }
    }

    /// Called with the requested open state when the line is clicked.
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for FoldLine {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let danger = theme::danger_text(cx);
        let theme = cx.theme();
        let (fg, muted, ring) = (theme.foreground, theme.muted_foreground, theme.ring);
        let pressed = theme.secondary_active;
        let (open, lead, counts, failed) = (self.open, self.lead, self.counts, self.failed);
        let spoken = match failed {
            0 => format!("{lead}{counts}"),
            n => format!("{lead}{counts}, {n} failed"),
        };
        h_flex()
            .id(self.id)
            .role(Role::Button)
            .aria_label(SharedString::from(format!("Work log, {spoken}")))
            .aria_expanded(open)
            .gap_1()
            .min_w_0()
            .text_sm()
            .text_color(muted)
            .map(tabular)
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .corner(rems(0.375))
            .cursor_pointer()
            .hover(|s| s.text_color(fg))
            .active(|s| s.text_color(fg).bg(pressed))
            .tab_index(0)
            .focus_visible(|s| s.border_color(ring))
            .child(
                Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight })
                    .small(),
            )
            .child(
                h_flex()
                    .min_w_0()
                    .child(
                        div()
                            .text_color(if self.failed_lead { danger } else { fg })
                            .font_medium()
                            .child(lead),
                    )
                    .child(div().truncate().child(counts))
                    .when(failed > 0, |h| {
                        h.child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .text_color(danger)
                                .child(" · ")
                                .child(Icon::new(Lucide::CircleX).xsmall())
                                .child(format!("{failed} failed")),
                        )
                    }),
            )
            .when_some(self.on_toggle, |h, handler| {
                h.on_click(move |_, window, cx| handler(!open, window, cx))
            })
    }
}

fn footer_details_popover(id: &str, details: Vec<(&'static str, String)>) -> impl IntoElement {
    let trigger = format!("{id}-details-trigger");
    let content = format!("{id}-details");
    Popover::new(SharedString::from(format!("{id}-details-popover")))
        .anchor(Anchor::BottomLeft)
        .arrow(true)
        .trigger(
            crate::components::button(SharedString::from(trigger.clone()))
                .debug_selector(|| trigger)
                .ghost()
                .xsmall()
                .icon(Lucide::Info)
                .accessibility_label("Reply details")
                .tooltip("Reply details"),
        )
        .content(move |_, _, cx| {
            let muted = cx.theme().muted_foreground;
            let content = content.clone();
            v_flex().debug_selector(|| content).w(px(220.)).gap_1().text_xs().map(tabular).children(
                details.iter().map(|(label, value)| {
                    h_flex()
                        .justify_between()
                        .gap_3()
                        .child(div().flex_none().text_color(muted).child(*label))
                        .child(div().min_w_0().truncate().font_semibold().child(value.clone()))
                }),
            )
        })
}

/// The finished reply's footer: outcome, a popover of facts, Copy and Retry, time.
#[derive(IntoElement)]
pub struct TurnFooter {
    id: SharedString,
    outcome: Outcome,
    lead: String,
    details: Vec<(&'static str, String)>,
    at: Option<String>,
    on_retry: Option<Handler>,
    on_copy: Option<Handler>,
    on_save_note: Option<Handler>,
    copied: bool,
}

impl TurnFooter {
    pub fn new(id: impl Into<SharedString>, facts: &FootFacts) -> Self {
        Self {
            id: id.into(),
            outcome: facts.outcome,
            lead: footer_lead(facts),
            details: footer_details(facts),
            at: facts.at.clone(),
            on_retry: None,
            on_copy: None,
            on_save_note: None,
            copied: false,
        }
    }

    /// Shows a Retry button.
    pub fn on_retry(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_retry = Some(Rc::new(handler));
        self
    }

    /// Shows a Copy button beside Retry.
    pub fn on_copy(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_copy = Some(Rc::new(handler));
        self
    }

    /// Adds Save to note to the context menu.
    pub fn on_save_note(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_save_note = Some(Rc::new(handler));
        self
    }

    /// Swaps the Copy button for a Copied tick.
    pub fn copied(mut self, copied: bool) -> Self {
        self.copied = copied;
        self
    }
}

impl RenderOnce for TurnFooter {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let danger_red = theme::danger_text(cx);
        let theme = cx.theme();
        let (muted, success, danger) =
            (theme.muted_foreground, crate::theme::success_text(cx), danger_red);
        let (on_copy, on_retry, copied) = (self.on_copy, self.on_retry, self.copied);
        let (copy, retry, save_note) = (on_copy.clone(), on_retry.clone(), self.on_save_note);
        let (icon, tint) = match self.outcome {
            Outcome::Worked => (Lucide::CircleCheck, success),
            Outcome::Failed => (Lucide::CircleX, danger),
            Outcome::Stopped => (Lucide::Square, muted),
        };
        let failed = self.outcome == Outcome::Failed;
        let facts = h_flex()
            .gap_1()
            .items_center()
            .min_w_0()
            .map(tabular)
            .child(Icon::new(icon).xsmall().text_color(tint))
            .child(
                div()
                    .when(failed, |lead| lead.text_color(danger_red).font_medium())
                    .child(self.lead),
            )
            .when(!self.details.is_empty(), |row| {
                row.child(footer_details_popover(&self.id, self.details))
            });
        Column::new()
            .px_0()
            .flex()
            .flex_col()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .child(facts)
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .items_center()
                            .when_some(on_copy, |row, handler| {
                                row.child(copy_button(&self.id, copied, handler, cx))
                            })
                            .when_some(on_retry, |row, handler| {
                                row.child(
                                    crate::components::button(SharedString::from(format!(
                                        "{}-retry",
                                        self.id
                                    )))
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::RotateCcw)
                                    .accessibility_label("Retry")
                                    .tooltip("Retry")
                                    .on_click(move |ev, window, cx| handler(ev, window, cx)),
                                )
                            })
                            .children(self.at),
                    ),
            )
            .context_menu(move |menu, _, _| {
                let menu = menu_item(menu, "Copy", IconName::Copy, copy.as_ref());
                let menu = menu_item(menu, "Save to note", Lucide::FileText, save_note.as_ref());
                menu_item(menu, "Retry", Lucide::RotateCcw, retry.as_ref())
            })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/turn.rs"]
mod tests;
