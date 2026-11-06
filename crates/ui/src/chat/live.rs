use crate::theme::Corners;
use aether_core::{Message, NoticeKind, NoticeState, Step, ToolDetail, ToolStatus};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, h_flex, popover::Popover, spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    Anchor, AnyElement, App, FontWeight, Role, SharedString, Window, div, prelude::*, px, rems,
};
use std::rc::Rc;

use super::context_ring::format_tokens;
use super::tool_row::tabular;
use crate::theme;

pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    let m = s / 60;
    match (s, m) {
        (0..60, _) => format!("{s}s"),
        (_, 0..60) => format!("{m}m {:02}s", s % 60),
        _ => format!("{}h {}m", m / 60, m % 60),
    }
}

pub fn short_ms(ms: u64) -> String {
    if ms < 1000 { format!("{ms}ms") } else { clock(ms) }
}

pub fn since(start_ms: Option<i64>, now_ms: i64) -> Option<String> {
    start_ms.map(|start| clock(now_ms.saturating_sub(start).max(0) as u64))
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Wait {
    Approval,
    Answer,
    Input,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Phase {
    Starting,
    Thinking,
    Writing,
    Working,
    Tools(usize),
    Subagents(usize),
    Waiting(Wait),
    Retrying,
    Compacting,
}

pub fn running_notice(m: &Message, kinds: &[NoticeKind]) -> bool {
    m.steps.iter().any(|s| {
        matches!(s, Step::Notice(n) if n.state == NoticeState::Running && kinds.contains(&n.kind))
    })
}

fn running_top(m: &Message, task: bool) -> usize {
    m.steps
        .iter()
        .filter(|s| {
            matches!(s, Step::Tool(c) if c.status == ToolStatus::Running && c.parent.is_none()
                && matches!(c.detail, ToolDetail::Task { .. }) == task)
        })
        .count()
}

pub fn running_subagents(m: &Message) -> usize {
    running_top(m, true)
}

fn text_after_steps(m: &Message) -> bool {
    m.text.len() > m.step_positions.last().copied().unwrap_or(0)
}

pub fn phase(m: &Message, wait: Option<Wait>) -> Phase {
    // one pass: the previous version rescanned steps once per check below
    let (mut tasks, mut tools, mut retry, mut compact) = (0, 0, false, false);
    for s in &m.steps {
        match s {
            Step::Tool(c) if c.status == ToolStatus::Running && c.parent.is_none() => {
                if matches!(c.detail, ToolDetail::Task { .. }) { tasks += 1 } else { tools += 1 }
            }
            Step::Notice(n) if n.state == NoticeState::Running => match n.kind {
                NoticeKind::Retry => retry = true,
                NoticeKind::Compaction => compact = true,
                _ => {}
            },
            _ => {}
        }
    }
    if let Some(wait) = wait {
        Phase::Waiting(wait)
    } else if retry {
        Phase::Retrying
    } else if compact {
        Phase::Compacting
    } else if tasks > 0 {
        Phase::Subagents(tasks)
    } else if tools > 0 {
        Phase::Tools(tools)
    } else if m.live_thinking_ms().is_some() {
        Phase::Thinking
    } else if text_after_steps(m) {
        Phase::Writing
    } else if m.text.is_empty() && m.steps.is_empty() {
        Phase::Starting
    } else {
        Phase::Working
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl Phase {
    pub fn label(&self, provider: &str) -> String {
        match self {
            Self::Starting => format!("Starting {provider}"),
            Self::Thinking => "Thinking".into(),
            Self::Writing => "Writing".into(),
            Self::Working => "Working".into(),
            Self::Tools(n) => format!("Running {}", plural(*n, "tool", "tools")),
            Self::Subagents(n) => format!("Running {}", plural(*n, "subagent", "subagents")),
            Self::Waiting(Wait::Approval) => "Waiting for your approval".into(),
            Self::Waiting(Wait::Answer) => "Waiting for your answer".into(),
            Self::Waiting(Wait::Input) => "Waiting for you".into(),
            Self::Retrying => "Retrying".into(),
            Self::Compacting => "Compacting".into(),
        }
    }
}

const VERBS: [&str; 24] = [
    "Pondering",
    "Brewing",
    "Cogitating",
    "Noodling",
    "Percolating",
    "Mulling",
    "Tinkering",
    "Conjuring",
    "Simmering",
    "Crafting",
    "Musing",
    "Whirring",
    "Churning",
    "Puzzling",
    "Scheming",
    "Forging",
    "Spelunking",
    "Marinating",
    "Untangling",
    "Wrangling",
    "Ruminating",
    "Concocting",
    "Deliberating",
    "Finagling",
];

/// Label while a turn runs; same seed, same verb, so it holds for the whole turn.
pub fn verb(seed: i64) -> &'static str {
    VERBS[seed.unsigned_abs() as usize % VERBS.len()]
}

pub fn wait_detail(source: Option<&str>, running_subagents: usize) -> Option<String> {
    let mut parts = Vec::new();
    parts.extend(source.map(|s| format!("{s} needs approval")));
    if running_subagents > 0 {
        parts.push(format!("{} still running", plural(running_subagents, "subagent", "subagents")));
    }
    (!parts.is_empty()).then(|| format!("· {}", parts.join(" · ")))
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Heartbeat {
    Live,
    Quiet(u64),
    Silent(u64),
}

/// Past this without an event the status line reads "no output" and the sidebar Quiet.
pub const SILENT_MS: u64 = 30_000;

pub fn heartbeat(idle_ms: u64) -> Heartbeat {
    match idle_ms {
        0..4_000 => Heartbeat::Live,
        4_000..SILENT_MS => Heartbeat::Quiet(idle_ms),
        _ => Heartbeat::Silent(idle_ms),
    }
}

impl Heartbeat {
    pub fn text(&self) -> String {
        match self {
            Self::Live => "live".into(),
            Self::Quiet(ms) => format!("quiet {}", clock(*ms)),
            Self::Silent(ms) => format!("no output {}", clock(*ms)),
        }
    }
}

pub const STALL_MS: u64 = 60_000;

/// A pending approval or question is the user's turn, never a stall.
pub fn stalled(idle_ms: u64, pending: bool) -> bool {
    idle_ms >= STALL_MS && !pending
}

pub fn last_event(m: &Message) -> String {
    if text_after_steps(m) {
        return "Writing the reply".into();
    }
    match m.steps.last() {
        Some(Step::Tool(c)) => {
            let summary = c.detail.summary();
            let what = if summary.is_empty() {
                c.title.clone()
            } else {
                format!("{} `{summary}`", c.title)
            };
            let done = !matches!(
                c.status,
                ToolStatus::Running | ToolStatus::Waiting | ToolStatus::Background
            );
            format!("{what} {}", if done { "finished" } else { "started" })
        }
        Some(Step::Thinking { .. } | Step::Thought(_)) => "Thinking".into(),
        Some(Step::Notice(n)) => n.text.clone(),
        Some(Step::Search { query, results, .. }) => {
            format!("{} \"{query}\"", if results.is_some() { "Searched" } else { "Searching" })
        }
        Some(Step::Skill(name)) => format!("Skill {name}"),
        Some(Step::Steer(_)) => "You steered the run".into(),
        _ => "Session started".into(),
    }
}

pub type Act = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct Stall {
    pub last_event: String,
    /// Time since that event.
    pub quiet: String,
    /// Whether the agent's process lives, once checked.
    pub alive: Option<bool>,
}

pub struct LiveFacts {
    pub phase: Phase,
    pub label: String,
    pub detail: Option<String>,
    pub elapsed: Option<String>,
    pub tokens: u64,
    pub retry_in: Option<String>,
    pub idle_ms: u64,
    /// Present while the warning and its popover show.
    pub stall: Option<Stall>,
}

pub struct StallActions {
    pub keep: Act,
    /// Not offered where sending would cancel the reply (no steering).
    pub nudge: Option<Act>,
    /// Button label and action, only where a TUI attaches to the live session.
    pub tui: Option<(SharedString, Act)>,
    pub stop: Act,
    /// Shows the last event's row in the side panel; only when that event is a row.
    pub inspect: Option<Act>,
}

/// The composer's top attachment: live progress, with a warning popover when it goes quiet.
#[derive(IntoElement)]
pub struct StatusLine {
    key: SharedString,
    facts: LiveFacts,
    actions: Option<StallActions>,
}

impl StatusLine {
    pub fn new(key: impl Into<SharedString>, facts: LiveFacts) -> Self {
        Self { key: key.into(), facts, actions: None }
    }

    pub fn on_stall(mut self, actions: StallActions) -> Self {
        self.actions = Some(actions);
        self
    }
}

fn code_spans(text: &str, cx: &App) -> impl IntoElement {
    let (bg, mono) = (cx.theme().muted, cx.theme().mono_font_family.clone());
    h_flex().min_w_0().items_center().gap_1().children(
        text.split('`').enumerate().filter(|(_, t)| !t.is_empty()).map(|(i, part)| {
            let part = div().child(part.to_string());
            match i % 2 {
                1 => part
                    .min_w_0()
                    .truncate()
                    .px_1()
                    .corner(rems(0.25))
                    .bg(bg)
                    .font_family(mono.clone()),
                _ => part.flex_none(),
            }
        }),
    )
}

impl StallActions {
    fn wrapped(&self, wrap: impl Fn(&Act) -> Act) -> Self {
        Self {
            keep: wrap(&self.keep),
            nudge: self.nudge.as_ref().map(&wrap),
            tui: self.tui.as_ref().map(|(label, act)| (label.clone(), wrap(act))),
            stop: wrap(&self.stop),
            inspect: self.inspect.as_ref().map(&wrap),
        }
    }

    fn row(self, key: &SharedString) -> impl IntoElement {
        let id = |name: &str| SharedString::from(format!("{key}-{name}"));
        let (keep, stop) = (self.keep, self.stop);
        let keep_selector = id("keep").to_string();
        h_flex()
            .flex_wrap()
            .gap_1()
            .child(
                crate::components::button(id("keep"))
                    .debug_selector(|| keep_selector)
                    .ghost()
                    .xsmall()
                    .label("Keep waiting")
                    .on_click(move |_, window, cx| keep(window, cx)),
            )
            .children(self.nudge.map(|nudge| {
                crate::components::button(id("nudge"))
                    .ghost()
                    .xsmall()
                    .label("Ask for update")
                    .on_click(move |_, window, cx| nudge(window, cx))
            }))
            .children(self.tui.map(|(label, open)| {
                crate::components::button(id("tui"))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ExternalLink)
                    .label(label)
                    .on_click(move |_, window, cx| open(window, cx))
            }))
            .child(
                crate::components::button(id("stop"))
                    .danger()
                    .xsmall()
                    .icon(Lucide::Square)
                    .label("Stop")
                    .on_click(move |_, window, cx| stop(window, cx)),
            )
    }
}

/// The quiet turn's last event: muted, brighter on hover, and it opens in the side panel
/// when the event is a row.
fn last_event_link(key: &SharedString, text: &str, inspect: Option<Act>, cx: &App) -> AnyElement {
    let fg = cx.theme().foreground;
    h_flex()
        .id(SharedString::from(format!("{key}-last")))
        .min_w_0()
        .overflow_hidden()
        .child(code_spans(text, cx))
        .when_some(inspect, |d, inspect| {
            d.role(Role::Button)
                .tab_index(0)
                .cursor_pointer()
                .hover(move |s| s.text_color(fg))
                .focus_visible(move |s| s.text_color(fg))
                .on_click(move |_, window, cx| inspect(window, cx))
        })
        .into_any_element()
}

/// Stands where the spinner does, in its exact box, so a quiet turn's bar doesn't move.
fn stall_popover(
    key: SharedString,
    stall: Stall,
    actions: StallActions,
    cx: &App,
) -> impl IntoElement {
    let trigger = format!("{key}-stall");
    let card = format!("{key}-stall-card");
    let warn = theme::mango_text(cx);
    Popover::new(SharedString::from(format!("{key}-stall-popover")))
        .anchor(Anchor::BottomLeft)
        .arrow(true)
        .trigger(
            crate::components::button(SharedString::from(trigger.clone()))
                .debug_selector(|| trigger)
                .ghost()
                .xsmall()
                .m(rems(-0.25))
                .icon(Icon::new(Lucide::TriangleAlert).text_color(warn))
                .accessibility_label("No output")
                .tooltip("No output"),
        )
        .content(move |_, _, cx| {
            let (muted, border) = (cx.theme().muted_foreground, cx.theme().border);
            let card = card.clone();
            let popover = cx.entity();
            let actions = actions.wrapped(|act| {
                let (popover, act) = (popover.clone(), act.clone());
                Rc::new(move |window, cx| {
                    popover.update(cx, |popover, cx| popover.dismiss(window, cx));
                    act(window, cx);
                })
            });
            let inspect = actions.inspect.clone();
            v_flex()
                .debug_selector(|| card)
                .w(px(280.))
                .gap_3()
                .child(
                    h_flex()
                        .justify_between()
                        .child(div().text_sm().font_semibold().child("No output"))
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(warn)
                                .map(tabular)
                                .child(stall.quiet.clone()),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(muted)
                        .children(stall.alive.map(|alive| {
                            format!("Process {}", if alive { "alive" } else { "exited" })
                        }))
                        .child(last_event_link(&key, &stall.last_event, inspect, cx)),
                )
                .child(div().h(px(1.)).bg(border))
                .child(actions.row(&key))
        })
}

impl RenderOnce for StatusLine {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self { key, facts, actions } = self;
        let theme = cx.theme();
        let (muted, fg) = (theme.muted_foreground, theme.foreground);
        let waiting = matches!(facts.phase, Phase::Waiting(_));
        let stalled = facts.stall.is_some();
        let icon = |icon, color| Icon::new(icon).xsmall().text_color(color).into_any_element();
        let lead = match (facts.stall.zip(actions), facts.phase) {
            (Some((stall, actions)), _) => {
                stall_popover(key, stall, actions, cx).into_any_element()
            }
            (_, Phase::Waiting(_)) => icon(Lucide::Shield, theme::mango_text(cx)),
            (_, Phase::Retrying) => icon(Lucide::RefreshCw, theme::info_text(cx)),
            _ => {
                Spinner::new().icon(IconName::LoaderCircle).xsmall().color(muted).into_any_element()
            }
        };
        let beat = (!waiting && !stalled)
            .then(|| heartbeat(facts.idle_ms))
            .filter(|beat| *beat != Heartbeat::Live)
            .map(|beat| {
                let color = match beat {
                    Heartbeat::Silent(_) => theme::mango_text(cx),
                    _ => muted,
                };
                h_flex()
                    .ml_auto()
                    .gap_2()
                    .items_center()
                    .text_color(color)
                    .child(div().size_1p5().rounded_full().bg(color))
                    .child(beat.text())
            });
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .px(px(14.))
            .text_size(rems(0.78125))
            .text_color(muted)
            .map(tabular)
            .child(lead)
            .child(div().font_weight(FontWeight::MEDIUM).text_color(fg).child(facts.label))
            .children(facts.detail.map(|d| div().min_w_0().truncate().child(d)))
            .children(facts.elapsed.map(|t| div().child(format!("· {t}"))))
            .when(facts.tokens > 0, |row| {
                row.child("·")
                    .child(Icon::new(Lucide::ArrowDown).xsmall())
                    .child(format!("{} tokens", format_tokens(facts.tokens)))
            })
            .children(facts.retry_in.map(|t| div().child(format!("· retry in {t}"))))
            .children(beat)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/live.rs"]
mod tests;
