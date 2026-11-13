use crate::theme::Corners;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

use aether_core::{Message, PlanItem, PlanStatus, Role, Step, ToolCall, ToolDetail, ToolStatus};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::base::{HandleEdge, resize_handle};
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::{ButtonCustomVariant, ButtonVariants},
    h_flex,
    input::{Textarea, TextareaState},
    resizable::resize_handle_appearance,
    spinner::Spinner,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    Action, AnyElement, App, Axis, Bounds, ClickEvent, ClipboardItem, Context, DragMoveEvent,
    Entity, ExternalPaths, Focusable, Hsla, MouseButton, Pixels, Render, SharedString, Window,
    canvas, deferred, div, prelude::*, px, relative, rems,
};

use super::live::{Act, since};
use super::tool_row::tabular;
use crate::components::{ClickHandler, overlay, overlay_strong};

const FOLD_TRANSITION: Duration = Duration::from_millis(260);
const ATTACH_ROW_TOP: f32 = 14.;

#[derive(Clone, Default)]
struct ComposerMeasurements {
    // Paint callbacks and subsequent renders must update the same measurements.
    body: Rc<Cell<Pixels>>,
    tray: Rc<Cell<Pixels>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

type AttachBuilder = Rc<dyn Fn(&App) -> AnyElement>;
type PasteHandler = Rc<dyn Fn(&ClipboardItem, &mut Window, &mut App) -> bool>;
type DropHandler = Rc<dyn Fn(Vec<PathBuf>, &mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposerMode {
    Full,
    Resting,
    Folded,
}

pub fn composer_mode(
    has_messages: bool,
    draft_empty: bool,
    user_scrolled_up: bool,
) -> ComposerMode {
    if has_messages && user_scrolled_up {
        ComposerMode::Folded
    } else if has_messages && draft_empty {
        ComposerMode::Resting
    } else {
        ComposerMode::Full
    }
}

pub fn composer_placeholder(agent: Option<&str>) -> String {
    match agent {
        Some(agent) => format!("Ask {agent} anything, @tag files, / for commands"),
        None => "Ask anything, @tag files, / for commands".to_string(),
    }
}

pub fn fold_bar_text(draft: &str, agent: Option<&str>) -> (String, bool) {
    match draft.lines().map(str::trim).find(|line| !line.is_empty()) {
        Some(line) => (line.to_string(), true),
        None => (
            match agent {
                Some(agent) => format!("Reply to {agent}…"),
                None => "Reply…".to_string(),
            },
            false,
        ),
    }
}

pub fn context_label(used: u64, total: u64) -> Option<String> {
    if used == 0 || total == 0 {
        return None;
    }
    let percent = ((used * 100 + total / 2) / total).min(100);
    Some(format!("{percent}%"))
}

#[derive(Debug, PartialEq, Eq)]
pub struct PlanProgress {
    pub done: usize,
    pub total: usize,
    pub current: Option<String>,
}

pub fn plan_progress(items: &[PlanItem]) -> PlanProgress {
    let counted = || items.iter().filter(|i| i.status != PlanStatus::Cancelled);
    PlanProgress {
        done: counted().filter(|i| i.status == PlanStatus::Done).count(),
        total: counted().count(),
        current: items.iter().find(|i| i.status == PlanStatus::Active).map(|i| i.text.clone()),
    }
}

impl PlanProgress {
    pub fn header(&self) -> String {
        format!("{} of {}", self.done, self.total)
    }
}

pub fn latest_plan(messages: &[Message]) -> Option<&[PlanItem]> {
    messages.iter().rev().flat_map(|m| m.steps.iter().rev()).find_map(|step| match step {
        Step::Tool(ToolCall { detail: ToolDetail::Plan { items }, .. }) => Some(items.as_slice()),
        _ => None,
    })
}

pub fn plan_tray_items(messages: &[Message]) -> Option<&[PlanItem]> {
    let last_has_plan = messages.last().is_some_and(|m| {
        m.steps
            .iter()
            .any(|s| matches!(s, Step::Tool(ToolCall { detail: ToolDetail::Plan { .. }, .. })))
    });
    latest_plan(messages).filter(|items| {
        last_has_plan
            || items.iter().any(|i| {
                matches!(i.status, PlanStatus::Active | PlanStatus::Pending | PlanStatus::Unknown)
            })
    })
}

/// The CLI died on the last reply and the user has not restarted it since.
pub fn dead_agent_shown(
    idle: bool,
    alive: bool,
    last: Option<&Message>,
    restarted_ms: i64,
) -> bool {
    idle && !alive
        && last.is_some_and(|m| {
            m.role == Role::Assistant
                && m.error.as_deref().is_some_and(|e| !e.contains("is not installed"))
                && m.ended_at_ms.is_some_and(|ended| restarted_ms < ended)
        })
}

/// The reply's top-level rows still running in the background.
pub fn background_rows(m: &Message) -> impl Iterator<Item = &ToolCall> {
    m.steps.iter().filter_map(|step| match step {
        Step::Tool(call) if call.parent.is_none() && call.status == ToolStatus::Background => {
            Some(call)
        }
        _ => None,
    })
}

/// `sends`: a running turn will end and send the chip; not once stopped, blocked or idle.
pub fn chip_copy(delivering: bool, sends: bool, provider: &str) -> (&'static str, String) {
    match (delivering, sends) {
        (true, _) => ("Delivering", format!("Waiting for {provider} to pick it up")),
        (false, true) => ("Queued", "Sends when this turn ends".to_string()),
        (false, false) => ("Queued", "Send now to run it".to_string()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Busy {
    Idle,
    Working,
    Waiting,
}

pub fn busy_placeholder(agent: Option<&str>, busy: Busy) -> String {
    match (busy, agent) {
        (Busy::Working, Some(agent)) => format!("Type to steer {agent} while it works"),
        (Busy::Waiting, Some(agent)) => {
            format!("Answer above, or type to tell {agent} what to do instead")
        }
        _ => composer_placeholder(agent),
    }
}

type RowHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;
type IxHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

fn tray_box(cx: &App) -> gpui_kit::Div {
    v_flex().w_full().corner(px(12.)).border_1().border_color(cx.theme().border).bg(overlay(cx))
}

fn spinner(color: Hsla) -> Spinner {
    Spinner::new().icon(IconName::LoaderCircle).xsmall().color(color)
}

fn tray_head(
    id: &'static str,
    state: Entity<bool>,
    open: bool,
    cx: &App,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .w_full()
        .px(px(12.))
        .py(px(8.))
        .gap(px(8.))
        .items_center()
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .corner(px(12.))
        .cursor_pointer()
        .role(gpui_kit::Role::Button)
        .aria_expanded(open)
        .tab_index(0)
        .hover(|s| s.bg(theme.secondary))
        .active(|s| s.bg(theme.secondary_active))
        .focus_visible(|s| s.border_color(theme.ring))
        .child(
            Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall(),
        )
        .on_click(move |_, _, cx| {
            state.update(cx, |open, cx| {
                *open = !*open;
                cx.notify();
            })
        })
}

/// Plan above the card: one line collapsed, every item expanded.
#[derive(IntoElement)]
pub struct PlanTray {
    scope: SharedString,
    items: Vec<PlanItem>,
}

impl PlanTray {
    /// `scope` keeps the tray's open state per chat.
    pub fn new(scope: impl Into<SharedString>, items: Vec<PlanItem>) -> Self {
        Self { scope: scope.into(), items }
    }
}

impl RenderOnce for PlanTray {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = SharedString::from(format!("plan-tray-open-{}", self.scope));
        let state = window.use_keyed_state(key, cx, |_, _| false);
        let open = *state.read(cx);
        let progress = plan_progress(&self.items);
        let success = crate::theme::success_text(cx);
        let theme = cx.theme();
        let (fg, muted, track) = (theme.foreground, theme.muted_foreground, theme.border);
        let fraction = progress.done as f32 / progress.total.max(1) as f32;
        let head =
            tray_head("plan-tray-head", state, open, cx)
                .aria_label(SharedString::from(format!("Plan, {}", progress.header())))
                .text_color(muted)
                .child(Icon::new(Lucide::ListChecks).small())
                .child(div().text_color(fg).font_weight(gpui_kit::FontWeight::MEDIUM).child("Plan"))
                .child(div().map(tabular).child(progress.header()))
                .child(
                    div()
                        .flex_none()
                        .w(px(60.))
                        .h(px(4.))
                        .rounded(px(2.))
                        .bg(track)
                        .overflow_hidden()
                        .child(div().h_full().w(relative(fraction)).bg(success)),
                )
                .children(progress.current.map(|current| {
                    div().flex_1().min_w_0().truncate().child(format!("· {current}"))
                }));
        tray_box(cx).child(head).when(open, |tray| {
            tray.child(
                v_flex()
                    .px(px(12.))
                    .pt(px(4.))
                    .pb(px(8.))
                    .gap(px(4.))
                    .border_t_1()
                    .border_color(track)
                    .children(self.items.into_iter().map(|item| {
                        let color = match item.status {
                            PlanStatus::Active => fg,
                            _ => muted,
                        };
                        let icon = match item.status {
                            PlanStatus::Done => {
                                Icon::new(Lucide::Check).xsmall().text_color(success)
                            }
                            PlanStatus::Cancelled => {
                                Icon::new(Lucide::CircleSlash).xsmall().text_color(muted)
                            }
                            _ => Icon::new(Lucide::Circle).xsmall().text_color(muted),
                        };
                        let active = item.status == PlanStatus::Active;
                        h_flex()
                            .gap(px(8.))
                            .items_start()
                            .child(div().flex_none().pt(px(4.)).child(if active {
                                spinner(fg).into_any_element()
                            } else {
                                icon.into_any_element()
                            }))
                            .child(
                                div()
                                    .min_w_0()
                                    .text_color(color)
                                    .when(item.status == PlanStatus::Done, |d| d.line_through())
                                    .child(item.text),
                            )
                    })),
            )
        })
    }
}

pub struct BackgroundItem {
    pub id: String,
    pub agent: bool,
    pub name: String,
    pub tools: u32,
    pub started_ms: Option<i64>,
    /// The opened task's full card or row, shown in place of its summary.
    pub body: Option<AnyElement>,
}

#[derive(Clone)]
struct PanelResize;

impl Render for PanelResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

/// Background tasks and terminals still running after the reply, beside the chat,
/// with the foreground row picked from the stall line on top.
#[derive(IntoElement)]
pub struct BackgroundPanel {
    items: Vec<BackgroundItem>,
    sections: Vec<(&'static str, AnyElement)>,
    width: f32,
    on_resize: Option<(Rc<dyn Fn(f32, &mut Window, &mut App)>, Act)>,
    now_ms: i64,
    on_open: RowHandler,
    on_stop: RowHandler,
    on_close: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl BackgroundPanel {
    pub fn new(
        items: Vec<BackgroundItem>,
        now_ms: i64,
        on_open: impl Fn(&str, &mut Window, &mut App) + 'static,
        on_stop: impl Fn(&str, &mut Window, &mut App) + 'static,
        on_close: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            items,
            sections: Vec::new(),
            width: 340.,
            on_resize: None,
            now_ms,
            on_open: Rc::new(on_open),
            on_stop: Rc::new(on_stop),
            on_close: Rc::new(on_close),
        }
    }

    /// Width from the left-edge drag handle; `on_end` runs when the drag lets go.
    pub fn resizable(
        mut self,
        width: f32,
        on_resize: impl Fn(f32, &mut Window, &mut App) + 'static,
        on_end: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.width = width;
        self.on_resize = Some((Rc::new(on_resize), Rc::new(on_end)));
        self
    }

    /// A labelled block above the tasks, e.g. the row picked from the stall line.
    pub fn section(mut self, label: &'static str, body: Option<AnyElement>) -> Self {
        self.sections.extend(body.map(|body| (label, body)));
        self
    }
}

impl RenderOnce for BackgroundPanel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, muted, border, hover) =
            (theme.foreground, theme.muted_foreground, theme.border, theme.secondary);
        let mono = theme.mono_font_family.clone();
        let count = self.items.len();
        let close = self.on_close;
        let head = h_flex()
            .h(px(36.))
            .flex_none()
            .border_b_1()
            .border_color(border)
            .px(px(12.))
            .gap(px(8.))
            .items_center()
            .text_sm()
            .child(
                div().text_color(fg).font_weight(gpui_kit::FontWeight::MEDIUM).child("Background"),
            )
            .child(div().text_xs().text_color(muted).map(tabular).child(count.to_string()))
            .child(
                crate::components::button("background-panel-close")
                    .ghost()
                    .xsmall()
                    .ml_auto()
                    .icon(IconName::Close)
                    .tooltip("Hide")
                    .on_click(move |_, window, cx| close(window, cx)),
            );
        let label = |text: &'static str| {
            div().px(px(8.)).pt(px(4.)).text_xs().text_color(muted).child(text)
        };
        let sections = self
            .sections
            .into_iter()
            .map(|(name, body)| v_flex().gap_1().child(label(name)).child(body));
        let rows = self.items.into_iter().enumerate().map(|(i, item)| {
            let (open, stop) = (self.on_open.clone(), self.on_stop.clone());
            let (open_id, stop_id) = (item.id.clone(), item.id);
            let stop = crate::components::button(("background-stop", i))
                .ghost()
                .xsmall()
                .label("Stop")
                .on_click(move |_, window, cx| stop(&stop_id, window, cx));
            if let Some(body) = item.body {
                if item.agent {
                    return body;
                }
                return v_flex()
                    .gap_1()
                    .child(body)
                    .child(h_flex().justify_end().child(stop))
                    .into_any_element();
            }
            let icon = if item.agent { Lucide::Bot } else { Lucide::SquareTerminal };
            let facts = [
                item.agent.then(|| format!("{} tools", item.tools)),
                since(item.started_ms, self.now_ms),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            v_flex()
                .px(px(8.))
                .py(px(6.))
                .gap(px(2.))
                .corner(px(8.))
                .hover(|s| s.bg(hover))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .text_sm()
                        .text_color(muted)
                        .child(spinner(muted))
                        .child(Icon::new(icon).xsmall())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(fg)
                                .when(!item.agent, |d| d.font_family(mono.clone()))
                                .child(item.name),
                        ),
                )
                .child(
                    h_flex()
                        .pl(px(36.))
                        .gap_1()
                        .items_center()
                        .text_xs()
                        .text_color(muted)
                        .child(div().flex_1().min_w_0().truncate().map(tabular).child(facts))
                        .child(
                            crate::components::button(("background-open", i))
                                .ghost()
                                .xsmall()
                                .label("Open")
                                .on_click(move |_, window, cx| open(&open_id, window, cx)),
                        )
                        .child(stop),
                )
                .into_any_element()
        });
        let tasks = (count > 0).then(|| v_flex().gap(px(2.)).child(label("Tasks")).children(rows));
        v_flex()
            .id("background-panel")
            .relative()
            .w(px(self.width))
            .flex_none()
            .h_full()
            .border_l_1()
            .border_color(border)
            .when_some(self.on_resize, |this, (on_resize, end)| {
                let up = end.clone();
                this.on_drag_move(move |e: &DragMoveEvent<Rc<PanelResize>>, window, cx| {
                    on_resize(f32::from(e.bounds.right() - e.event.position.x), window, cx)
                })
                .capture_any_mouse_up(move |_, window, cx| up(window, cx))
                .on_mouse_up_out(MouseButton::Left, move |_, window, cx| end(window, cx))
                .child(
                    resize_handle::<PanelResize, PanelResize>(
                        "background-panel-resize",
                        Axis::Horizontal,
                    )
                    .inside(HandleEdge::Leading)
                    .with_appearance(resize_handle_appearance())
                    .on_drag(PanelResize, |_: Rc<PanelResize>, _, _, cx| cx.new(|_| PanelResize)),
                )
            })
            .child(head)
            .child(
                v_flex()
                    .id("background-panel-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(4.))
                    .pb(px(8.))
                    .gap_3()
                    .children(sections)
                    .children(tasks),
            )
    }
}

/// A message waiting for the CLI: queued behind the turn, or handed over as a steer.
#[derive(IntoElement)]
pub struct QueueChip {
    ix: usize,
    text: SharedString,
    files: usize,
    delivering: bool,
    sends: bool,
    provider: SharedString,
    edit_blocked: bool,
    on_send: IxHandler,
    on_edit: IxHandler,
    on_remove: IxHandler,
    on_open: Option<IxHandler>,
}

impl QueueChip {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ix: usize,
        text: impl Into<SharedString>,
        files: usize,
        delivering: bool,
        sends: bool,
        provider: impl Into<SharedString>,
        edit_blocked: bool,
        on_send: impl Fn(usize, &mut Window, &mut App) + 'static,
        on_edit: impl Fn(usize, &mut Window, &mut App) + 'static,
        on_remove: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            ix,
            text: text.into(),
            files,
            delivering,
            sends,
            provider: provider.into(),
            edit_blocked,
            on_send: Rc::new(on_send),
            on_edit: Rc::new(on_edit),
            on_remove: Rc::new(on_remove),
            on_open: None,
        }
    }
}

impl QueueChip {
    /// Shows the whole message, attachments included, elsewhere.
    pub fn on_open(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_open = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for QueueChip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, muted) = (theme.foreground, theme.muted_foreground);
        let (label, hint) = chip_copy(self.delivering, self.sends, &self.provider);
        let ix = self.ix;
        let (send, edit, remove) = (self.on_send, self.on_edit, self.on_remove);
        h_flex()
            .w_full()
            .px(px(8.))
            .py(px(4.))
            .gap(px(8.))
            .items_center()
            .corner(px(6.))
            .bg(overlay(cx))
            .text_size(rems(0.78125))
            .text_color(muted)
            .child(if self.delivering {
                spinner(muted).into_any_element()
            } else {
                Icon::new(Lucide::Clock).xsmall().into_any_element()
            })
            .child(div().flex_none().child(label))
            .child(
                h_flex()
                    .id(("queue-open", ix))
                    .flex_1()
                    .min_w_0()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(fg)
                            .child(self.text.split_whitespace().collect::<Vec<_>>().join(" ")),
                    )
                    .when(self.files > 0, |row| {
                        row.child(
                            h_flex()
                                .flex_none()
                                .gap_1()
                                .items_center()
                                .child(Icon::new(Lucide::Paperclip).size(px(12.)))
                                .child(self.files.to_string()),
                        )
                    })
                    .when_some(self.on_open, |row, open| {
                        row.cursor_pointer().on_click(move |_, window, cx| open(ix, window, cx))
                    }),
            )
            .child(div().flex_none().max_w(px(220.)).truncate().child(hint))
            .when(!self.delivering, |row| {
                row.child(
                    crate::components::button(("queue-steer", ix))
                        .ghost()
                        .xsmall()
                        .label("Send now")
                        .on_click(move |_, window, cx| send(ix, window, cx)),
                )
                .child(
                    div()
                        .id(("queue-edit-wrap", ix))
                        .flex_none()
                        .when(self.edit_blocked, |wrap| {
                            wrap.tooltip(|window, cx| {
                                Tooltip::new("Clear the composer to edit this").build(window, cx)
                            })
                        })
                        .child(
                            crate::components::button(("queue-edit", ix))
                                .ghost()
                                .xsmall()
                                .label("Edit")
                                .disabled(self.edit_blocked)
                                .on_click(move |_, window, cx| edit(ix, window, cx)),
                        ),
                )
                .child(
                    crate::components::button(("queue-remove", ix))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::X)
                        .accessibility_label("Remove")
                        .tooltip("Remove")
                        .on_click(move |_, window, cx| remove(ix, window, cx)),
                )
            })
    }
}

/// Reply text the next message quotes.
#[derive(IntoElement)]
pub struct QuoteChip {
    text: SharedString,
    on_remove: ClickHandler,
}

impl QuoteChip {
    pub fn new(text: impl Into<SharedString>, on_remove: ClickHandler) -> Self {
        Self { text: text.into(), on_remove }
    }
}

impl RenderOnce for QuoteChip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .w_full()
            .px(px(8.))
            .py(px(4.))
            .gap(px(8.))
            .items_center()
            .corner(px(6.))
            .bg(overlay(cx))
            .text_size(rems(0.78125))
            .text_color(theme.muted_foreground)
            .child(Icon::new(Lucide::TextQuote).xsmall())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.foreground)
                    .child(self.text.split_whitespace().collect::<Vec<_>>().join(" ")),
            )
            .child(
                crate::components::button("quote-remove")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::X)
                    .accessibility_label("Remove quote")
                    .tooltip("Remove quote")
                    .on_click(self.on_remove),
            )
    }
}

/// Shown when the CLI died on the last reply; sending also restarts it.
#[derive(IntoElement)]
pub struct DeadAgentBar {
    provider: SharedString,
    on_restart: ClickHandler,
}

impl DeadAgentBar {
    pub fn new(provider: impl Into<SharedString>, on_restart: ClickHandler) -> Self {
        Self { provider: provider.into(), on_restart }
    }
}

impl RenderOnce for DeadAgentBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let danger = crate::theme::danger_text(cx);
        h_flex()
            .w_full()
            .px(px(12.))
            .py(px(4.))
            .gap(px(8.))
            .items_center()
            .text_size(rems(0.8125))
            .text_color(danger)
            .child(Icon::new(Lucide::CircleX).small())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(format!("{} stopped. Your message will restart it.", self.provider)),
            )
            .child(
                crate::components::button("dead-agent-restart")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::RefreshCw)
                    .label("Restart now")
                    .on_click(self.on_restart),
            )
    }
}

fn shell_hint(cx: &App) -> AnyElement {
    let color = crate::theme::success_text(cx);
    h_flex()
        .px(px(6.))
        .gap(px(6.))
        .items_center()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(Lucide::SquareTerminal).small().text_color(color))
        .child(div().font_weight(gpui_kit::FontWeight::MEDIUM).text_color(color).child("Command"))
        .child(div().min_w_0().truncate().child("Runs in a new terminal tab"))
        .into_any_element()
}

/// One-line stand-in for the card body while the transcript is scrolled up.
pub fn fold_bar(
    icon: Icon,
    text: SharedString,
    is_draft: bool,
    model: SharedString,
    on_expand: ClickHandler,
    on_jump: ClickHandler,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let text_color = if is_draft { theme.foreground } else { theme.muted_foreground };
    h_flex()
        .id("composer-fold-bar")
        .w_full()
        .h(px(46.))
        .pl(px(16.))
        .pr(px(7.))
        .gap(px(10.))
        .items_center()
        .cursor_text()
        .on_click(on_expand)
        .child(div().flex().text_color(theme.muted_foreground).child(icon.small()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(13.5))
                .text_color(text_color)
                .child(text),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted_foreground)
                .whitespace_nowrap()
                .child(model),
        )
        .child(
            h_flex()
                .id("composer-scroll-end")
                .h(px(32.))
                .px(px(12.))
                .gap(px(5.))
                .items_center()
                .rounded_full()
                .bg(overlay_strong(cx))
                .hover(|s| s.bg(theme.foreground.opacity(0.13)))
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(theme.foreground)
                .whitespace_nowrap()
                .on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    on_jump(event, window, cx);
                })
                .child(Icon::new(IconName::ArrowDown).xsmall())
                .child("Scroll to end"),
        )
        .into_any_element()
}

/// Message input card: text area, then a toolbar of leading controls on the
/// left and trailing controls plus an action button on the right, with an
/// optional tray underneath.
#[derive(IntoElement)]
pub struct Composer {
    input: Entity<TextareaState>,
    leading: Vec<AnyElement>,
    trailing: Vec<AnyElement>,
    action: Option<AnyElement>,
    popup: Option<AnyElement>,
    header: Option<AnyElement>,
    status: Option<AnyElement>,
    tray: Option<AnyElement>,
    bar: Option<AnyElement>,
    attachments: Option<AttachBuilder>,
    attach_scope: u64,
    on_paste: Option<PasteHandler>,
    on_drop_paths: Option<DropHandler>,
    mode: ComposerMode,
    disabled: bool,
    attention: bool,
    shell: bool,
    recording: bool,
    full_width: bool,
}

impl Composer {
    pub fn new(input: &Entity<TextareaState>) -> Self {
        Self {
            input: input.clone(),
            leading: Vec::new(),
            trailing: Vec::new(),
            action: None,
            popup: None,
            header: None,
            status: None,
            tray: None,
            bar: None,
            attachments: None,
            attach_scope: 0,
            on_paste: None,
            on_drop_paths: None,
            mode: ComposerMode::Full,
            disabled: false,
            attention: false,
            shell: false,
            recording: false,
            full_width: false,
        }
    }

    pub fn full_width(mut self, full: bool) -> Self {
        self.full_width = full;
        self
    }

    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading.push(element.into_any_element());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Mango border while the agent waits on an answer.
    pub fn attention(mut self, attention: bool) -> Self {
        self.attention = attention;
        self
    }

    /// The draft is a shell command: terminal look, and the agent controls give way.
    pub fn shell(mut self, shell: bool) -> Self {
        self.shell = shell;
        self
    }

    /// Dictation is listening: red border over every other state.
    pub fn recording(mut self, recording: bool) -> Self {
        self.recording = recording;
        self
    }

    /// Sits just above the card, e.g. an "editing message" banner.
    pub fn header(mut self, element: impl IntoElement) -> Self {
        self.header = Some(element.into_any_element());
        self
    }

    /// Live status attached to the card's top edge, mirroring the bottom tray.
    pub fn status(mut self, element: impl IntoElement) -> Self {
        self.status = Some(element.into_any_element());
        self
    }

    /// Floats above the card, left-aligned, e.g. a suggestion menu.
    pub fn popup(mut self, element: impl IntoElement) -> Self {
        self.popup = Some(element.into_any_element());
        self
    }

    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing.push(element.into_any_element());
        self
    }

    pub fn action(mut self, element: impl IntoElement) -> Self {
        self.action = Some(element.into_any_element());
        self
    }

    pub fn mode(mut self, mode: ComposerMode) -> Self {
        self.mode = mode;
        self
    }

    /// Strip tucked under the card; hidden while folded.
    pub fn tray(mut self, element: impl IntoElement) -> Self {
        self.tray = Some(element.into_any_element());
        self
    }

    /// Replaces the card body while folded.
    pub fn bar(mut self, element: impl IntoElement) -> Self {
        self.bar = Some(element.into_any_element());
        self
    }

    /// Rendered above the text area. The row keeps the last builder until it
    /// has collapsed, so removing the final item shrinks the card smoothly.
    pub fn attachments(mut self, build: impl Fn(&App) -> AnyElement + 'static) -> Self {
        self.attachments = Some(Rc::new(build));
        self
    }

    /// Names whose attachments the row shows (e.g. the chat); changing it snaps instead of animating.
    pub fn attach_scope(mut self, scope: u64) -> Self {
        self.attach_scope = scope;
        self
    }

    /// Sees every paste first; `true` consumes it.
    pub fn on_paste(
        mut self,
        handler: impl Fn(&ClipboardItem, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_paste = Some(Rc::new(handler));
        self
    }

    /// Files dropped on the card; it takes the accent border while they hover.
    pub fn on_drop_paths(
        mut self,
        handler: impl Fn(Vec<PathBuf>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_drop_paths = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Composer {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focused = self.input.focus_handle(cx).is_focused(window);
        let input = self.input.clone();
        let key = input.entity_id();
        let spelling = window.use_keyed_state(
            SharedString::from(format!("composer-spelling-{key}")),
            cx,
            |_, cx| super::spellcheck::Spellcheck::new(input, cx),
        );
        let folded = self.mode == ComposerMode::Folded && self.bar.is_some();
        let resting = self.mode == ComposerMode::Resting;
        let fold = if cx.reduce_motion() {
            if folded { 1. } else { 0. }
        } else {
            transition(
                format!("composer-fold-{key}"),
                if folded { 1. } else { 0. },
                Transition::new(FOLD_TRANSITION),
                window,
                cx,
            )
        };
        let rest_opacity = if cx.reduce_motion() {
            if resting { 0.7 } else { 1. }
        } else {
            transition(
                format!("composer-rest-{key}"),
                if resting { 0.7 } else { 1. },
                Transition::new(FOLD_TRANSITION),
                window,
                cx,
            )
        };
        let has_attachments = self.attachments.is_some();
        let scope = self.attach_scope;
        let last = window
            .use_keyed_state(format!("composer-attach-last-{key}"), cx, |_, _| {
                Rc::new(RefCell::new(None::<(u64, AttachBuilder)>))
            })
            .read(cx)
            .clone();
        if let Some(build) = &self.attachments {
            *last.borrow_mut() = Some((scope, build.clone()));
        } else if last.borrow().as_ref().is_some_and(|(s, _)| *s != scope) {
            *last.borrow_mut() = None;
        }
        let attach_cell = window
            .use_keyed_state(format!("composer-attach-height-{scope}"), cx, |_, _| {
                Rc::new(Cell::new(0f32))
            })
            .read(cx)
            .clone();
        let attach_target = if has_attachments { attach_cell.get() + ATTACH_ROW_TOP } else { 0. };
        let attach_h = transition(
            format!("composer-attach-{scope}"),
            attach_target,
            Transition::new(FOLD_TRANSITION),
            window,
            cx,
        );
        if !has_attachments && attach_h < 0.5 {
            *last.borrow_mut() = None;
        }
        let attach_build = last.borrow().as_ref().map(|(_, b)| b.clone());
        let attach_row = attach_build.map(|build| {
            let total = attach_cell.get() + ATTACH_ROW_TOP;
            let measure = attach_cell.clone();
            div()
                .w_full()
                .overflow_hidden()
                .h(px(attach_h))
                .opacity((attach_h / total).clamp(0., 1.))
                .child(
                    div().w_full().px(px(16.)).pt(px(ATTACH_ROW_TOP)).child(
                        div()
                            .w_full()
                            .relative()
                            .child(
                                canvas(
                                    move |b, window, _| {
                                        let height = f32::from(b.size.height);
                                        if (measure.get() - height).abs() > 0.5 {
                                            measure.set(height);
                                            window.refresh();
                                        }
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            )
                            .child(build(cx)),
                    ),
                )
        });
        let measurements = window
            .use_keyed_state(format!("composer-measurements-{key}"), cx, |_, _| {
                ComposerMeasurements::default()
            })
            .read(cx)
            .clone();
        let body_cell = measurements.body;
        let tray_cell = measurements.tray;
        let (body_cell_paint, tray_cell_paint) = (body_cell.clone(), tray_cell.clone());
        let measuring = fold == 0. && !folded;
        let theme = cx.theme();
        let border = if self.recording {
            crate::theme::danger_text(cx).opacity(0.5)
        } else if self.shell {
            crate::theme::success_text(cx).opacity(0.5)
        } else if self.attention {
            crate::theme::mango().opacity(0.4)
        } else if focused {
            theme.primary.opacity(0.5)
        } else {
            theme.border
        };
        let surface = if theme.is_dark() { theme.secondary } else { theme.background };
        let mono = theme.mono_font_family.clone();
        let (leading, trailing) = if self.shell {
            (vec![shell_hint(cx)], Vec::new())
        } else {
            (self.leading, self.trailing)
        };
        let bounds_cell = measurements.bounds;
        let input_focus = self.input.clone();
        let input_focus_inner = self.input.clone();
        // Restore the input immediately on expansion so keyboard focus survives
        // while the height transition finishes.
        let show_bar = folded && fold > 0.5;
        let phase = (fold * 2. - 1.).abs();

        let container = div().w_full().px_3().pb_4();

        let body = v_flex()
            .w_full()
            .children(attach_row)
            .child(
                div()
                    .relative()
                    .w_full()
                    .px(px(12.))
                    .pt(px(8.))
                    .pb(px(4.))
                    .when(self.shell, |d| d.font_family(mono))
                    .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| {
                        if !folded {
                            input_focus_inner.update(cx, |input, cx| {
                                input.focus(window, cx);
                            });
                        }
                    })
                    .child({
                        let mut textarea =
                            Textarea::new(&self.input).appearance(false).disabled(self.disabled);
                        if let Some(handler) = self.on_paste.filter(|_| !self.disabled) {
                            textarea = textarea
                                .on_paste(move |item, window, cx| handler(item, window, cx));
                        }
                        textarea
                    })
                    .children((!self.disabled && !self.shell).then_some(spelling)),
            )
            .child(
                h_flex()
                    .w_full()
                    .px(px(10.))
                    .pb(px(6.))
                    .min_h(px(36.))
                    .gap_1()
                    .items_center()
                    .opacity(rest_opacity)
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .gap_1()
                            .items_center()
                            .children(leading),
                    )
                    .child(
                        h_flex().gap_1p5().items_center().children(trailing).children(self.action),
                    ),
            );

        container.child(
            v_flex()
                .w_full()
                .when(!self.full_width, |c| c.max_w(crate::theme::CONTENT_WIDTH).mx_auto())
                .children(self.header.map(|header| div().w_full().mb_2().child(header)))
                .children(self.status.map(|status| div().w_full().pb(px(6.)).child(status)))
                .child(
                    div()
                        .w_full()
                        .relative()
                        .child(
                            canvas(
                                move |b, window, _| {
                                    if bounds_cell.get() != b {
                                        let was_default = bounds_cell.get() == Bounds::default();
                                        bounds_cell.set(b);
                                        if was_default {
                                            window.refresh();
                                        }
                                    }
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .child(
                            div()
                                .w_full()
                                .corner(px(24.))
                                .bg(surface)
                                .border_1()
                                .border_color(border)
                                .overflow_hidden()
                                .when_some(
                                    self.on_drop_paths.filter(|_| !self.disabled),
                                    |d, handler| {
                                        d.drag_over::<ExternalPaths>(|style, _, _, cx| {
                                            style.border_color(cx.theme().accent)
                                        })
                                        .on_drop::<ExternalPaths>(move |paths, window, cx| {
                                            handler(paths.paths().to_vec(), window, cx)
                                        })
                                    },
                                )
                                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| {
                                    if !folded {
                                        input_focus.update(cx, |input, cx| {
                                            input.focus(window, cx);
                                        });
                                    }
                                })
                                .child(
                                    div()
                                        .w_full()
                                        .relative()
                                        .overflow_hidden()
                                        .when(
                                            fold > 0. && fold < 1. && body_cell.get() > px(0.),
                                            |d| {
                                                let full = body_cell.get();
                                                d.h(full + (px(46.) - full) * fold)
                                            },
                                        )
                                        .child(
                                            canvas(
                                                move |b, _, _| {
                                                    if measuring {
                                                        body_cell_paint.set(b.size.height);
                                                    }
                                                },
                                                |_, _, _, _| {},
                                            )
                                            .absolute()
                                            .size_full(),
                                        )
                                        .child(div().w_full().opacity(phase).map(
                                            |d| match self.bar {
                                                Some(bar) if show_bar => d.child(bar),
                                                _ => d.child(body),
                                            },
                                        )),
                                ),
                        )
                        .children(self.popup.map(|popup| {
                            deferred(
                                div()
                                    .absolute()
                                    .bottom(relative(1.))
                                    .left_0()
                                    .w_full()
                                    .mb_2()
                                    .child(popup),
                            )
                        })),
                )
                .children(self.tray.map(|tray| {
                    let full = tray_cell.get();
                    let height = if fold >= 1. {
                        Some(px(0.))
                    } else if fold > 0. && full > px(0.) {
                        Some(full * (1. - fold))
                    } else if fold > 0.5 {
                        Some(px(0.))
                    } else {
                        None
                    };
                    div().px(px(2.)).overflow_hidden().when_some(height, |d, h| d.h(h)).child(
                        div()
                            .relative()
                            .opacity(1. - fold)
                            .child(
                                canvas(
                                    move |b, _, _| {
                                        if measuring {
                                            tray_cell_paint.set(b.size.height);
                                        }
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            )
                            .child(tray),
                    )
                })),
        )
    }
}

/// Round primary button that sends, or stops while a reply streams.
#[derive(IntoElement)]
pub struct SendButton {
    shell: bool,
    streaming: bool,
    loading: bool,
    disabled: bool,
    disabled_reason: Option<SharedString>,
    on_send: Option<ClickHandler>,
    on_stop: Option<ClickHandler>,
    stop_action: Option<Box<dyn Action>>,
}

impl SendButton {
    pub fn new() -> Self {
        Self {
            shell: false,
            streaming: false,
            loading: false,
            disabled: false,
            disabled_reason: None,
            on_send: None,
            on_stop: None,
            stop_action: None,
        }
    }

    /// Runs the draft in the terminal instead of sending it.
    pub fn shell(mut self, shell: bool) -> Self {
        self.shell = shell;
        self
    }

    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// Spinner in place of the arrow while the send waits on setup, e.g. a new worktree.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Tooltip shown instead of "Send" while disabled for a reason other than an empty draft.
    pub fn disabled_reason(mut self, reason: Option<impl Into<SharedString>>) -> Self {
        self.disabled_reason = reason.map(Into::into);
        self
    }

    pub fn on_send(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_send = Some(Box::new(handler));
        self
    }

    pub fn on_stop(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_stop = Some(Box::new(handler));
        self
    }

    /// Action whose shortcut the stop tooltip shows.
    pub fn stop_action(mut self, action: impl Action) -> Self {
        self.stop_action = Some(Box::new(action));
        self
    }
}

impl Default for SendButton {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for SendButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, bg, ghost) =
            (theme.foreground, theme.background.alpha(1.), theme.muted_foreground.opacity(0.6));
        let danger = theme.danger.opacity(0.18);
        let strong = overlay_strong(cx);
        // loading keeps the filled look: the send is accepted, just not started yet
        let dim = self.disabled && !self.loading;
        let arrow = if dim { ghost } else { bg };
        // variant foreground repaints the label on hover/press, so it must be the arrow colour
        let size = 36.;
        let base = |id: &'static str, label: Hsla, hover: Hsla| {
            crate::components::button(id)
                .custom(ButtonCustomVariant::new(cx).foreground(label).hover(hover).active(hover))
                .size(px(size))
                .rounded(px(size / 2.))
        };
        if self.streaming {
            base("stop", fg, danger)
                .child(div().size(px(10.)).corner(px(2.)).bg(fg))
                .bg(strong)
                .text_color(fg)
                .accessibility_label("Stop generating")
                .map(|b| match &self.stop_action {
                    Some(action) => b.tooltip_with_action("Stop generating", action.as_ref(), None),
                    None => b.tooltip("Stop generating"),
                })
                .when_some(self.on_stop, |b, handler| b.on_click(handler))
        } else {
            let (glyph, label) = if self.shell {
                (Icon::new(Lucide::Play), "Run in terminal")
            } else {
                (Icon::new(IconName::ArrowUp), "Send")
            };
            base("send", arrow, fg.opacity(0.9))
                .icon(glyph)
                .bg(if dim { strong } else { fg })
                .text_color(arrow)
                .accessibility_label(label)
                .tooltip(self.disabled_reason.unwrap_or_else(|| label.into()))
                .loading(self.loading)
                .disabled(self.disabled)
                .when_some(self.on_send, |b, handler| b.on_click(handler))
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/composer.rs"]
mod tests;
