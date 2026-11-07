use crate::theme::Corners;
use std::rc::Rc;

use aether_core::{Notice, NoticeKind, NoticeState, Provider, Step, ToolDetail, ToolStatus};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::SelectableText;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, h_flex,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{App, ElementId, Global, Hsla, SharedString, Window, div, prelude::*, px, rems};

use super::cache::RenderCache;
use super::context_ring::format_tokens;
use super::live::{clock, short_ms};
use super::markdown::Markdown;
use super::tool_row::{OnOpen, Reflow, tabular};
use crate::theme;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum NoticeTone {
    Muted,
    Ok,
    Warn,
    Bad,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum NoticeLink {
    Open(String),
    Action(String),
}

/// What a link click does; the app opens URLs and forwards actions to the CLI.
pub type OnLink = Rc<dyn Fn(NoticeLink, &mut Window, &mut App)>;

pub fn lead(kind: NoticeKind, state: NoticeState) -> Option<&'static str> {
    use NoticeKind as K;
    use NoticeState as S;
    Some(match (kind, state) {
        (K::Retry, S::Running) => "Retrying",
        (K::Retry, S::Failed) => "Retry failed",
        (K::Retry, S::Done) => "Recovered",
        (K::Retry, S::Unknown) => "Retry",
        (K::RateLimit, _) => "Rate limited",
        (K::Compaction, S::Running) => "Compacting context",
        (K::Compaction, S::Failed) => "Compaction failed",
        (K::Compaction, S::Done) => "Compacted",
        (K::Compaction, S::Unknown) => "Compaction",
        (K::Hook, S::Failed) => "Hook blocked",
        (K::Hook, _) => "Hook",
        (K::ModelChange, _) => "Model changed",
        (K::ModeChange, _) => "Permission mode",
        (K::Warning, _) => "Warning",
        (K::McpAuth, _) => "Sign-in needed",
        _ => return None,
    })
}

/// The notice text minus what its lead already says.
pub fn notice_body<'a>(lead: Option<&str>, text: &'a str) -> &'a str {
    match lead.and_then(|l| text.strip_prefix(l)) {
        Some(rest) if !rest.starts_with(char::is_alphanumeric) => {
            rest.trim_start_matches([':', ' '])
        }
        _ => text,
    }
}

/// Finished by really compacting. A turn's end also closes abandoned rows as Done, so the
/// text decides; those still read "Compacting context".
pub fn compacted(n: &Notice) -> bool {
    n.kind == NoticeKind::Compaction
        && n.state == NoticeState::Done
        && n.text.starts_with("Compacted")
}

/// "671911 → ~14549 tokens" as "671.9k → ~14.5k tokens".
pub fn short_counts(text: &str) -> String {
    let word = |w: &str| {
        let digits = w.trim_start_matches('~');
        digits.parse::<u64>().map_or(w.to_string(), |n| {
            format!("{}{}", &w[..w.len() - digits.len()], format_tokens(n))
        })
    };
    text.split(' ').map(word).collect::<Vec<_>>().join(" ")
}

pub fn notice_tone(kind: NoticeKind, state: NoticeState) -> NoticeTone {
    use NoticeKind as K;
    match (kind, state) {
        (_, NoticeState::Failed) => NoticeTone::Bad,
        (K::Warning | K::RateLimit | K::McpAuth | K::ModelChange, _) => NoticeTone::Warn,
        (K::Retry | K::Compaction, NoticeState::Done) => NoticeTone::Ok,
        _ => NoticeTone::Muted,
    }
}

/// Seconds left, rounded up; `None` at or past the deadline.
pub fn countdown(until_ms: Option<i64>, now_ms: i64) -> Option<String> {
    let left = until_ms? - now_ms;
    (left > 0).then(|| clock((left as u64).div_ceil(1000) * 1000))
}

/// What a Running notice counts down to: a retry's next attempt, else a limit's reset.
pub fn countdown_verb(kind: NoticeKind) -> &'static str {
    if kind == NoticeKind::Retry { "retry in" } else { "resets in" }
}

pub fn notice_link(url: &str, kind: NoticeKind) -> Option<(String, NoticeLink)> {
    if let Some(action) = url.strip_prefix("aether:") {
        let label = match action {
            "" => return None,
            "trust-project" => "Trust this project".to_string(),
            _ => {
                let words = action.replace(['-', '_'], " ");
                let mut chars = words.chars();
                chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
            }
        };
        return Some((label, NoticeLink::Action(action.to_string())));
    }
    (url.starts_with("http://") || url.starts_with("https://")).then(|| {
        let label = if kind == NoticeKind::McpAuth { "Sign in" } else { "Open link" };
        (label.to_string(), NoticeLink::Open(url.to_string()))
    })
}

fn stopped_tools(steps: &[Step]) -> impl Iterator<Item = &aether_core::ToolCall> {
    steps.iter().filter_map(|s| match s {
        Step::Tool(call) if call.status == ToolStatus::Stopped => Some(call),
        _ => None,
    })
}

pub fn stopped_summary(steps: &[Step]) -> Option<String> {
    let mut counts = [0usize; 4];
    for call in stopped_tools(steps) {
        counts[match call.detail {
            ToolDetail::Command { .. } => 0,
            ToolDetail::Edit { .. } => 1,
            ToolDetail::Task { .. } => 2,
            _ => 3,
        }] += 1;
    }
    let total: usize = counts.iter().sum();
    if total == 0 {
        return None;
    }
    let names = [
        ("command", "commands"),
        ("edit", "edits"),
        ("subagent", "subagents"),
        ("tool call", "tool calls"),
    ];
    let mut parts: Vec<String> = counts
        .iter()
        .zip(names)
        .filter(|(n, _)| **n > 0)
        .map(|(n, (one, many))| format!("{n} {}", if *n == 1 { one } else { many }))
        .collect();
    let last = parts.pop()?;
    let list = if parts.is_empty() { last } else { format!("{} and {last}", parts.join(", ")) };
    Some(format!("{list} {} interrupted", if total == 1 { "was" } else { "were" }))
}

pub fn stopped_lead(steps: &[Step], failed: bool) -> &'static str {
    if failed {
        return "Interrupted";
    }
    let mut calls = stopped_tools(steps).peekable();
    let closed = calls.peek().is_some()
        && calls.all(|c| c.error.as_deref() == Some("Interrupted: Aether closed"));
    if closed { "Interrupted when Aether closed" } else { "Stopped by you" }
}

struct NoticeStates(RenderCache<bool>);

impl Default for NoticeStates {
    fn default() -> Self {
        Self(RenderCache::new(1024))
    }
}

impl Global for NoticeStates {}

fn is_open(cx: &mut App, key: &SharedString) -> bool {
    cx.default_global::<NoticeStates>().0.get(key).copied().unwrap_or(false)
}

fn toggle(
    key: SharedString,
    open: bool,
    reflow: &Option<Reflow>,
    window: &mut Window,
    cx: &mut App,
) {
    cx.default_global::<NoticeStates>().0.insert(key, !open);
    match reflow {
        Some(reflow) => reflow(cx),
        None => window.refresh(),
    }
}

fn kind_icon(kind: NoticeKind, state: NoticeState) -> Lucide {
    match kind {
        NoticeKind::Retry if state == NoticeState::Done => Lucide::CircleCheck,
        NoticeKind::Retry | NoticeKind::RateLimit | NoticeKind::Wake => Lucide::RefreshCw,
        NoticeKind::Compaction => Lucide::Layers,
        NoticeKind::Hook => Lucide::Zap,
        NoticeKind::ModelChange => Lucide::ArrowLeftRight,
        NoticeKind::ModeChange => Lucide::Shield,
        NoticeKind::Warning => Lucide::TriangleAlert,
        NoticeKind::McpAuth => Lucide::Plug,
        _ => Lucide::Info,
    }
}

fn tone_color(tone: NoticeTone, muted: Hsla, cx: &App) -> Hsla {
    match tone {
        NoticeTone::Muted => muted,
        NoticeTone::Ok => theme::success_text(cx),
        NoticeTone::Warn => theme::mango_text(cx),
        NoticeTone::Bad => theme::danger_text(cx),
    }
}

fn quiet_row(cx: &App) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .min_h(rems(1.5))
        .px_2()
        .gap_2()
        .items_center()
        .text_size(rems(0.78125))
        .text_color(cx.theme().muted_foreground)
}

fn spinner(color: Hsla) -> Spinner {
    Spinner::new().icon(IconName::LoaderCircle).xsmall().color(color)
}

/// A notice the CLI reported mid-turn, on one quiet line.
#[derive(IntoElement)]
pub struct NoticeLine {
    key: SharedString,
    notice: Notice,
    now_ms: i64,
    live: bool,
    on_link: Option<OnLink>,
    on_reflow: Option<Reflow>,
    cwd: Option<SharedString>,
}

impl NoticeLine {
    pub fn new(key: impl Into<SharedString>, notice: Notice) -> Self {
        Self {
            key: key.into(),
            notice,
            now_ms: 0,
            live: true,
            on_link: None,
            on_reflow: None,
            cwd: None,
        }
    }

    /// Clock for the retry countdown.
    pub fn now_ms(mut self, now_ms: i64) -> Self {
        self.now_ms = now_ms;
        self
    }

    /// False once the reply is over: a notice still Running then neither spins nor counts down.
    pub fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }

    pub fn on_link(mut self, on_link: Option<OnLink>) -> Self {
        self.on_link = on_link;
        self
    }

    pub fn on_reflow(mut self, on_reflow: Option<Reflow>) -> Self {
        self.on_reflow = on_reflow;
        self
    }

    pub fn cwd(mut self, cwd: Option<SharedString>) -> Self {
        self.cwd = cwd;
        self
    }
}

/// Wake text from the drivers, e.g. `Claude started a turn on its own: background task "x" completed`,
/// as (lead, detail, woken by background work).
pub fn wake_line(text: &str) -> (&'static str, String, bool) {
    let why = text.split_once(" on its own: ").map_or("", |(_, why)| why);
    match why.strip_prefix("background task \"").and_then(|rest| rest.rsplit_once('"')) {
        Some((name, status)) => {
            let lead = match status.trim() {
                "failed" => "Background task failed",
                "stopped" | "killed" => "Background task stopped",
                _ => "Background task finished",
            };
            let mut lines = name.lines().map(str::trim).filter(|l| !l.is_empty());
            let first = lines.next().unwrap_or_default();
            let more = if lines.next().is_some() { "…" } else { "" };
            (lead, format!("{first}{more}"), true)
        }
        None => ("Agent resumed on its own", why.to_string(), false),
    }
}

impl RenderOnce for NoticeLine {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let n = self.notice;
        let muted = cx.theme().muted_foreground;
        if n.kind == NoticeKind::Wake {
            let info = theme::info_text(cx);
            let border = cx.theme().border;
            let (lead, detail, background) = wake_line(&n.text);
            return quiet_row(cx)
                .text_color(info)
                .child(Icon::new(if background { Lucide::Bot } else { Lucide::RefreshCw }).xsmall())
                .child(div().flex_none().font_semibold().child(lead))
                .when(!detail.is_empty(), |d| {
                    d.child(div().min_w_0().truncate().child(format!("· {detail}")))
                })
                .child(div().flex_1().h(px(1.)).bg(border))
                .into_any_element();
        }
        let running = n.state == NoticeState::Running && self.live;
        let color = tone_color(notice_tone(n.kind, n.state), muted, cx);
        let icon = if running {
            spinner(color).into_any_element()
        } else {
            Icon::new(kind_icon(n.kind, n.state)).xsmall().text_color(color).into_any_element()
        };
        let left = running.then(|| countdown(n.until_ms, self.now_ms)).flatten();
        let detail_key = SharedString::from(format!("{}-detail", self.key));
        let open = is_open(cx, &detail_key);
        let link = n.url.as_deref().and_then(|u| notice_link(u, n.kind));
        let title = lead(n.kind, n.state);
        let boundary = compacted(&n);
        let body = notice_body(title, &n.text);
        let body = if boundary { short_counts(body) } else { body.to_string() };
        let border = cx.theme().border;
        let line = quiet_row(cx)
            .child(icon)
            .children(title.map(|l| div().flex_none().font_semibold().text_color(color).child(l)))
            .when(!body.is_empty(), |d| d.child(div().min_w_0().truncate().child(body)))
            .children(left.map(|t| {
                div().flex_none().map(tabular).child(format!("· {} {t}", countdown_verb(n.kind)))
            }))
            .when(n.detail.is_some(), |d| {
                let reflow = self.on_reflow.clone();
                let key = detail_key.clone();
                d.child(
                    crate::components::button(SharedString::from(format!("{}-summary", self.key)))
                        .ghost()
                        .xsmall()
                        .label(if open { "Hide summary" } else { "View summary" })
                        .on_click(move |_, window, cx| {
                            toggle(key.clone(), open, &reflow, window, cx)
                        }),
                )
            })
            .children(link.map(|(label, link)| {
                let on_link = self.on_link.clone();
                crate::components::button(SharedString::from(format!("{}-link", self.key)))
                    .ghost()
                    .xsmall()
                    .label(label)
                    .on_click(move |_, window, cx| {
                        if let Some(on_link) = &on_link {
                            on_link(link.clone(), window, cx);
                        }
                    })
            }))
            .when(boundary, |d| d.child(div().flex_1().h(px(1.)).bg(border)));
        v_flex()
            .w_full()
            .child(line)
            .when_some(n.detail.filter(|_| open), |v, detail| {
                v.child(
                    div().px_2().pl_6().text_sm().child(
                        Markdown::new(detail_key, detail.into())
                            .cwd(self.cwd)
                            .muted(true)
                            .on_reflow(self.on_reflow),
                    ),
                )
            })
            .into_any_element()
    }
}

/// The CLI's startup notices, while it boots.
#[derive(IntoElement)]
pub struct StartupList {
    notices: Vec<Notice>,
    live: bool,
}

impl StartupList {
    pub fn new(notices: Vec<Notice>) -> Self {
        Self { notices, live: true }
    }

    /// False once the reply is over: a step still Running then shows as stale, not spinning.
    pub fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }
}

impl RenderOnce for StartupList {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let mono = cx.theme().mono_font_family.clone();
        let (ok, bad) = (theme::success_text(cx), theme::danger_text(cx));
        v_flex().w_full().gap_0p5().children(self.notices.into_iter().map(|n| {
            let icon = match n.state {
                NoticeState::Running if self.live => spinner(muted).into_any_element(),
                NoticeState::Failed => {
                    Icon::new(IconName::CircleX).xsmall().text_color(bad).into_any_element()
                }
                NoticeState::Done => {
                    Icon::new(IconName::CircleCheck).xsmall().text_color(ok).into_any_element()
                }
                _ => Icon::new(Lucide::Info).xsmall().text_color(muted).into_any_element(),
            };
            quiet_row(cx).child(icon).child(div().flex_none().child(n.text)).children(
                n.detail.map(|d| {
                    div().min_w_0().truncate().font_family(mono.clone()).text_xs().child(d)
                }),
            )
        }))
    }
}

/// Where the user steered the running turn.
#[derive(IntoElement)]
pub struct SteerMarker {
    id: ElementId,
    text: String,
    on_open: Option<Box<dyn Fn(&mut Window, &mut App)>>,
}

impl SteerMarker {
    pub fn new(id: impl Into<ElementId>, text: String) -> Self {
        Self { id: id.into(), text, on_open: None }
    }

    /// Shows the whole message in the side panel.
    pub fn on_open(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_open = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SteerMarker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, secondary) = (theme.muted_foreground, theme.border, theme.secondary);
        let ok = theme::success_text(cx);
        let success = cx.theme().success;
        let line = self.text.split_whitespace().collect::<Vec<_>>().join(" ");
        h_flex()
            .id(self.id)
            .w_full()
            .min_h(rems(2.))
            .px_2()
            .gap_2()
            .items_center()
            .corner(rems(0.375))
            .bg(secondary.opacity(0.5))
            .when_some(self.on_open, |el, open| {
                el.cursor_pointer()
                    .hover(|s| s.bg(secondary))
                    .on_click(move |_, window, cx| open(window, cx))
            })
            .border_1()
            .border_dashed()
            .border_color(border)
            .text_size(rems(0.78125))
            .child(Icon::new(Lucide::ArrowUp).xsmall().text_color(muted))
            .child(div().flex_none().text_color(muted).child("You, while it worked:"))
            .child(div().flex_1().min_w_0().truncate().child(line))
            .child(
                div()
                    .flex_none()
                    .px_2()
                    .corner(rems(0.375))
                    .bg(success.opacity(0.12))
                    .text_xs()
                    .text_color(ok)
                    .child("Delivered"),
            )
    }
}

/// The run was stopped before it finished.
#[derive(IntoElement)]
pub struct StoppedMarker {
    lead: &'static str,
    elapsed: Option<u64>,
    summary: Option<String>,
}

impl StoppedMarker {
    pub fn new(lead: &'static str, elapsed: Option<u64>, summary: Option<String>) -> Self {
        Self { lead, elapsed, summary }
    }
}

impl RenderOnce for StoppedMarker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        quiet_row(cx)
            .child(Icon::new(Lucide::Square).xsmall())
            .child(div().flex_none().font_semibold().child(self.lead))
            .children(
                self.elapsed.map(|t| {
                    div().flex_none().map(tabular).child(format!("after {}", short_ms(t)))
                }),
            )
            .children(self.summary.map(|s| div().min_w_0().truncate().child(format!("· {s}"))))
    }
}

/// Divider where a chat continued in another agent: "Context handoff · from → to".
#[derive(IntoElement)]
pub struct HandoffMarker(aether_core::Handoff);

impl HandoffMarker {
    pub fn new(handoff: aether_core::Handoff) -> Self {
        Self(handoff)
    }
}

impl RenderOnce for HandoffMarker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let h = self.0;
        let (muted, line) = (cx.theme().muted_foreground, cx.theme().border);
        let rule = || div().flex_1().min_w(px(16.)).h(px(1.)).bg(line);
        let agent = |p: Provider, model: String| {
            h_flex()
                .flex_none()
                .gap(px(5.))
                .items_center()
                .child(crate::provider_icon(p).size(px(13.)))
                .child(if model.is_empty() { p.label().to_string() } else { model })
        };
        quiet_row(cx)
            .id("handoff-marker")
            .role(gpui_kit::Role::Group)
            .aria_label(SharedString::from(format!(
                "Context handoff from {} to {}",
                h.from.label(),
                h.to.label()
            )))
            .py(px(10.))
            .gap(px(10.))
            .child(rule())
            .child(Icon::new(Lucide::ArrowLeftRight).xsmall().text_color(muted))
            .child(div().flex_none().child("Context handoff"))
            .child(agent(h.from, h.from_model))
            .child(Icon::new(Lucide::ArrowRight).xsmall().text_color(muted))
            .child(agent(h.to, h.to_model))
            .child(rule())
    }
}

pub type OnHandoff = Rc<dyn Fn(Provider, &mut Window, &mut App)>;

/// One "Continue in X" item per target, for the failed-reply card.
pub fn handoff_items(mut menu: PopupMenu, targets: &[Provider], on_pick: &OnHandoff) -> PopupMenu {
    for &target in targets {
        let on_pick = on_pick.clone();
        menu = menu.item(
            PopupMenuItem::new(format!("Continue in {}", target.label()))
                .icon(crate::provider_icon(target))
                .on_click(move |_, window, cx| on_pick(target, window, cx)),
        );
    }
    menu
}

/// The error that ended the run.
#[derive(IntoElement)]
pub struct FatalCard {
    key: SharedString,
    error: String,
    time: Option<String>,
    can_restart: bool,
    actions: bool,
    on_restart: Option<OnOpen>,
    on_retry: Option<OnOpen>,
    on_copy: Option<OnOpen>,
    on_reflow: Option<Reflow>,
    handoff: Vec<Provider>,
    on_handoff: Option<OnHandoff>,
}

impl FatalCard {
    pub fn new(key: impl Into<SharedString>, error: String) -> Self {
        Self {
            key: key.into(),
            error,
            time: None,
            can_restart: true,
            actions: true,
            on_restart: None,
            on_retry: None,
            on_copy: None,
            on_reflow: None,
            handoff: Vec::new(),
            on_handoff: None,
        }
    }

    pub fn time(mut self, time: Option<String>) -> Self {
        self.time = time;
        self
    }

    /// False for OpenRouter chats, which have no CLI to restart.
    pub fn can_restart(mut self, can_restart: bool) -> Self {
        self.can_restart = can_restart;
        self
    }

    /// False on older replies: Restart and Retry only make sense for the latest turn.
    pub fn actions(mut self, actions: bool) -> Self {
        self.actions = actions;
        self
    }

    pub fn on_restart(mut self, f: Option<OnOpen>) -> Self {
        self.on_restart = f;
        self
    }

    pub fn on_retry(mut self, f: Option<OnOpen>) -> Self {
        self.on_retry = f;
        self
    }

    pub fn on_copy(mut self, f: Option<OnOpen>) -> Self {
        self.on_copy = f;
        self
    }

    pub fn on_reflow(mut self, on_reflow: Option<Reflow>) -> Self {
        self.on_reflow = on_reflow;
        self
    }

    /// Agents this turn can continue in; empty hides the button.
    pub fn handoff(mut self, targets: Vec<Provider>, on_pick: OnHandoff) -> Self {
        self.handoff = targets;
        self.on_handoff = Some(on_pick);
        self
    }
}

impl RenderOnce for FatalCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let details_key = SharedString::from(format!("{}-details", self.key));
        let open = is_open(cx, &details_key);
        let danger = cx.theme().danger;
        let danger_text = theme::danger_text(cx);
        let (muted, mono) = (cx.theme().muted_foreground, cx.theme().mono_font_family.clone());
        let (first, rest) = self.error.split_once('\n').unwrap_or((&self.error, ""));
        let (first, rest) = (first.to_string(), rest.trim().to_string());
        let run = |f: Option<OnOpen>| {
            move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
                if let Some(f) = &f {
                    f(window, cx);
                }
            }
        };
        let id = self.key.clone();
        let reflow = self.on_reflow.clone();
        let toggle_key = details_key;
        let actions = h_flex()
            .gap_2()
            .flex_wrap()
            .when(self.can_restart && self.actions, |d| {
                d.child(
                    crate::components::button(SharedString::from(format!("{id}-restart")))
                        .primary()
                        .small()
                        .icon(Lucide::RefreshCw)
                        .label("Restart agent")
                        .on_click(run(self.on_restart.clone())),
                )
            })
            .when(self.actions, |d| {
                d.child(
                    crate::components::button(SharedString::from(format!("{id}-retry")))
                        .small()
                        .icon(Lucide::RotateCcw)
                        .label("Retry last message")
                        .on_click(run(self.on_retry.clone())),
                )
            })
            .when(self.actions && !self.handoff.is_empty(), |d| {
                let (targets, on_pick) = (self.handoff.clone(), self.on_handoff.clone());
                d.child(
                    crate::components::button(SharedString::from(format!("{id}-handoff")))
                        .ghost()
                        .small()
                        .icon(Lucide::ArrowRight)
                        .label("Continue in another agent")
                        .dropdown_menu(move |menu: PopupMenu, _, _| match &on_pick {
                            Some(on_pick) => handoff_items(menu, &targets, on_pick),
                            None => menu,
                        }),
                )
            })
            .when(!rest.is_empty(), |d| {
                d.child(
                    crate::components::button(SharedString::from(format!("{id}-details")))
                        .ghost()
                        .small()
                        .label(if open { "Hide details" } else { "Show details" })
                        .on_click(move |_, window, cx| {
                            toggle(toggle_key.clone(), open, &reflow, window, cx)
                        }),
                )
            })
            .child(
                crate::components::button(SharedString::from(format!("{id}-copy")))
                    .ghost()
                    .small()
                    .icon(Lucide::Copy)
                    .label("Copy")
                    .on_click(run(self.on_copy.clone())),
            );
        v_flex()
            .w_full()
            .gap_2()
            .p_3()
            .corner(px(12.))
            .border_1()
            .border_color(danger.opacity(0.5))
            .bg(danger.opacity(0.12))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .child(Icon::new(IconName::CircleX).xsmall().text_color(danger_text))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_semibold()
                            .text_color(danger_text)
                            .child(first),
                    )
                    .children(
                        self.time.map(|t| div().flex_none().text_xs().text_color(muted).child(t)),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("The turn stopped. Edits that finished are kept."),
            )
            .when(open && !rest.is_empty(), |v| {
                v.child(
                    div()
                        .p_2()
                        .corner(px(9.))
                        .bg(cx.theme().background.opacity(0.5))
                        .font_family(mono)
                        .text_xs()
                        .text_color(muted)
                        .child(SelectableText::new(SharedString::from(format!("{id}-rest")), rest)),
                )
            })
            .child(actions)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/notice.rs"]
mod tests;
