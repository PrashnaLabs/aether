use aether_ui::theme::Corners;
use gpui_kit::rems;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use aether_core::{TileLayout, Watch, WatchCorner};
use aether_terminal::dark;
use aether_ui::theme::CONTENT_WIDTH;
use aether_ui::{ChatStatus, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Bounds, Context, CursorStyle, DragMoveEvent, Entity, Image, ImageFormat,
    MouseButton, MouseDownEvent, Pixels, Point, Role, SharedString, StyleRefinement, Window, div,
    img, point, prelude::*, px, size,
};

use super::{Chat, ChatApp, Page};
use crate::actions::{BackToChat, FocusComposer, NextSideTab, PreviousSideTab, StopGenerating};
use crate::app::status_bar::STATUS_BAR;
use crate::side_panel::SideTab;

const FAB: f32 = 36.;
const FAB_MARGIN: f32 = 12.;
const PEEK: f32 = 400.;
const PEEK_MIN: (f32, f32) = (320., 240.);
const GRIP: f32 = 6.;
// chat header and project bar height
const TOP_CHROME: f32 = 40.;
const SIDE_TAB_BAR: f32 = 38.;
const SESSION_TABS: f32 = 32.;
pub(crate) const WATCH_STRIP: Pixels = px(FAB + 2. * FAB_MARGIN);
// clears a one-line composer, whose send button would sit under a corner FAB
const ABOVE_COMPOSER: Pixels = px(104.);

impl ChatApp {
    pub(crate) fn render_watch(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.settle_peek();
        let covered = self.watch_open || self.watch_peek.is_some();
        let enabled = self.settings.watch_enabled;
        for browser in [&self.browser, &self.hn_browser] {
            browser.update(cx, |b, cx| {
                b.set_covered(covered, cx);
                b.watch_strip = enabled;
            });
        }
        if !enabled {
            return None;
        }
        let viewport = window.viewport_size();
        let corner = self.settings.watch_corner;
        let (x, y) = self.fab_inset(viewport.width, cx);
        let chrome = f32::from(self.top_chrome(cx));
        let beyond_fab = y + px(FAB + 8.);
        let peek = self.watch_peek.and_then(|item| {
            let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
            let area = [0., chrome, vw, vh - STATUS_BAR - chrome];
            let (left, top) = fab_edges(corner);
            let (x, y) = (f32::from(x), f32::from(beyond_fab));
            let w = (vw - x - FAB_MARGIN).clamp(0., PEEK);
            let h = (if top { vh - STATUS_BAR } else { vh - TOP_CHROME } - y - FAB_MARGIN)
                .clamp(0., PEEK);
            let beside_fab =
                [if left { x } else { vw - x - w }, if top { y } else { vh - y - h }, w, h];
            let bounds =
                self.watch_peek_live.or(self.settings.watch_peek_bounds).unwrap_or(beside_fab);
            self.render_peek(item, clamp_peek(bounds, area, PEEK_MIN), area, window, cx)
        });
        let count = self.settings.watchlist.len();
        let theme = cx.theme();
        let ring = theme.ring;
        let badge = (count > 0).then(|| {
            div()
                .absolute()
                .top(px(-4.))
                .right(px(-4.))
                .min_w(px(16.))
                .h(px(16.))
                .px(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(theme.danger)
                .text_color(theme.danger_foreground)
                .text_size(px(10.))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(count.to_string())
        });
        let fab = anchor(div().occlude().absolute().size(px(FAB)), corner, x, y)
            .child(
                div()
                    .id("watch-fab")
                    .debug_selector(|| "watch-fab".into())
                    .tab_index(0)
                    .role(Role::Button)
                    .aria_label("Watching")
                    .focus_visible(move |el| el.border_2().border_color(ring))
                    .size_full()
                    .cursor_pointer()
                    .corner(px(8.))
                    .overflow_hidden()
                    .shadow_lg()
                    .child(img(aether_icon()).size_full())
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Watching").build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_watch_list(cx))),
            )
            .children(badge);
        let (left, top) = fab_edges(corner);
        let fab_bounds = Bounds::new(
            point(
                if left { x } else { viewport.width - x - px(FAB) },
                if top { y } else { viewport.height - y - px(FAB) },
            ),
            size(px(FAB), px(FAB)),
        );
        let list = self
            .watch_open
            .then(|| self.render_watch_list(corner, (x, beyond_fab), fab_bounds, cx));
        Some(div().absolute().inset_0().child(fab).children(list).children(peek).into_any_element())
    }

    fn sidebar_width(&self) -> f32 {
        if self.settings.sidebar_collapsed {
            0.
        } else {
            self.settings.sidebar_width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH)
        }
    }

    /// FAB distance from its corner's horizontal and vertical edges.
    pub(crate) fn fab_inset(&self, viewport_width: Pixels, cx: &App) -> (Pixels, Pixels) {
        let corner = self.settings.watch_corner;
        let (left, top) = fab_edges(corner);
        let x = if left { self.sidebar_width() + FAB_MARGIN } else { FAB_MARGIN };
        let y = if top {
            px(f32::from(self.top_chrome(cx)) + FAB_MARGIN)
        } else {
            px(STATUS_BAR)
                + if self.fab_lifted(viewport_width) { ABOVE_COMPOSER } else { px(FAB_MARGIN) }
        };
        (px(x), y)
    }

    /// Height of the top bars currently shown over the page.
    pub(crate) fn top_chrome(&self, cx: &App) -> Pixels {
        if !self.side_open() || self.is_terminal_drawer_maximized(cx) {
            return px(TOP_CHROME);
        }
        let hoisted = self.side_full
            && self.settings.tile_layout == TileLayout::Monocle
            && self.workspace.model.windows.get(&0).is_some_and(|w| {
                w.maximized.is_none() && w.root.as_ref().is_some_and(|r| r.leaves().len() > 1)
            });
        px(SIDE_TAB_BAR + if hoisted { SESSION_TABS } else { 0. })
    }

    pub(crate) fn fab_lifted(&self, viewport_width: Pixels) -> bool {
        let composer_hidden =
            self.side_open() && !(self.side_full && self.side_tab == SideTab::Agent);
        if self.page != Page::Chat || composer_hidden {
            return false;
        }
        let narrow = f32::from(viewport_width) - self.sidebar_width()
            < f32::from(CONTENT_WIDTH) + 2. * (FAB + FAB_MARGIN);
        let split = self.settings.tile_layout != TileLayout::Monocle
            && self.workspace.model.windows.get(&0).is_some_and(|w| {
                w.maximized.is_none() && w.root.as_ref().is_some_and(|r| r.leaves().len() > 1)
            });
        narrow || split
    }

    fn render_watch_list(
        &self,
        corner: WatchCorner,
        (x, y): (Pixels, Pixels),
        fab: Bounds<Pixels>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let now = crate::app::now_ms();
        let theme = cx.theme();
        let (muted, hover) = (theme.muted_foreground, theme.accent);
        let rows = self
            .settings
            .watchlist
            .iter()
            .enumerate()
            .filter_map(|(ix, &item)| {
                let chat = self.chats.iter().find(|c| c.id == item.chat())?;
                let (icon, title) = watch_head(chat, item);
                let (status, dot) = match item {
                    Watch::Chat(_) => {
                        let status = chat.status(now);
                        let dot = match status {
                            ChatStatus::Idle => muted,
                            ChatStatus::Done => theme.success,
                            ChatStatus::Failed => theme.danger,
                            s if s.is_waiting() => theme.warning,
                            _ => theme.info,
                        };
                        let text = match status {
                            ChatStatus::Working { since_ms } => format!(
                                "{} · {}",
                                aether_ui::verb(chat.turn_at),
                                aether_ui::since(Some(since_ms), now).unwrap_or_default()
                            ),
                            s => s.text(now),
                        };
                        (text, dot)
                    }
                    Watch::Terminal(id) => {
                        let job = self.terminal_job(id, cx);
                        let dot = if job.is_some() { theme.info } else { muted };
                        (job.unwrap_or_default(), dot)
                    }
                };
                let status = if status.is_empty() { "Idle".into() } else { status };
                Some(
                    h_flex()
                        .id(("watch-row", ix))
                        .gap_2()
                        .px_2()
                        .py_1p5()
                        .corner(rems(0.375))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .child(icon.size(px(14.)).text_color(muted).flex_none())
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_sm().truncate().child(title))
                                .child(div().text_xs().text_color(muted).truncate().child(status)),
                        )
                        .child(div().flex_none().size(px(7.)).rounded_full().bg(dot))
                        .child(
                            aether_ui::button(("watch-remove", ix))
                                .debug_selector(|| "watch-remove".into())
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Unwatch")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.toggle_watch(item, cx)
                                })),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_watch_item(item, window, cx)
                        })),
                )
            })
            .collect::<Vec<_>>();
        let empty = rows.is_empty().then(|| {
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(muted)
                .child("Right-click a chat or terminal tab and choose Watch.")
        });
        anchor(v_flex().id("watch-list").occlude().absolute(), corner, x, y)
            .w(px(260.))
            .p(px(6.))
            .corner(rems(0.5))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_lg()
            // the FAB toggles the list itself; closing here too would reopen it
            .on_mouse_down_out(cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                if !fab.contains(&ev.position) {
                    this.watch_open = false;
                    cx.notify();
                }
            }))
            .child(div().px_2().py_1().text_xs().text_color(muted).child("Watching"))
            .children(rows)
            .children(empty)
            .into_any_element()
    }

    fn render_peek(
        &mut self,
        item: Watch,
        bounds: [f32; 4],
        area: [f32; 4],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (icon, title) = watch_head(self.chats.iter().find(|c| c.id == item.chat())?, item);
        let body = match item {
            Watch::Chat(id) => {
                self.ensure_session_ui(id, window, cx);
                if window.is_window_active() {
                    self.acknowledge_session(id, cx);
                }
                self.in_session(id, cx, |app, cx| {
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .on_action(
                            app.session_listener(cx, |app, _: &StopGenerating, window, cx| {
                                app.stop(window, cx)
                            }),
                        )
                        .on_action(app.session_listener(
                            cx,
                            |app, _: &FocusComposer, window, cx| {
                                app.expand_composer(cx);
                                app.composer.update(cx, |s, cx| s.focus(window, cx))
                            },
                        ))
                        .child(app.session_body(("watch-peek", 0usize).into(), window, cx))
                        .into_any_element()
                })?
            }
            Watch::Terminal(id) => {
                let view = self.terminals.get(&id)?.view.clone();
                if !view.read(cx).shown {
                    view.update(cx, |t, cx| t.set_shown(true, cx));
                }
                dark(view.cached(StyleRefinement::default().size_full()), cx).into_any_element()
            }
        };
        let theme = cx.theme();
        let header = h_flex()
            .id("watch-peek-header")
            .debug_selector(|| "watch-peek-header".into())
            .flex_none()
            .gap_2()
            .px_2p5()
            .py_1p5()
            .border_b_1()
            .border_color(theme.border)
            .child(icon.size(px(14.)).text_color(theme.muted_foreground).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                h_flex()
                    .gap_2()
                    // keeps a press on a button from starting a header drag
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        aether_ui::button("watch-peek-open")
                            .debug_selector(|| "watch-peek-open".into())
                            .outline()
                            .xsmall()
                            .label("Open ↗")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.jump_to_watched(item, window, cx)
                            })),
                    )
                    .child(
                        aether_ui::button("watch-peek-pop-out")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::ExternalLink)
                            .tooltip("Open in new window")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.pop_out_watched(item, window, cx)
                            })),
                    )
                    .child(
                        aether_ui::button("watch-peek-close")
                            .debug_selector(|| "watch-peek-close".into())
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip("Close")
                            .on_click(cx.listener(|this, _, _, cx| this.close_peek(cx))),
                    ),
            )
            .on_drag(
                PeekDrag { edges: [true; 4], start: bounds, origin: Default::default() },
                pick_up,
            );
        let [x, y, w, h] = bounds;
        let grips = GRIPS.into_iter().enumerate().map(|(ix, edges)| peek_grip(ix, edges, bounds));
        Some(
            v_flex()
                .id("watch-peek")
                .debug_selector(|| "watch-peek".into())
                .occlude()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .corner(rems(0.5))
                .overflow_hidden()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background.alpha(1.))
                .text_color(theme.foreground)
                .shadow_lg()
                // the root's handlers act on the page underneath (Esc falls through to BackToChat)
                .on_action(|_: &BackToChat, _, _| {})
                .on_action(|_: &PreviousSideTab, _, _| {})
                .on_action(|_: &NextSideTab, _, _| {})
                .on_drag_move(cx.listener(move |this, e: &DragMoveEvent<PeekDrag>, _, cx| {
                    let drag = e.drag(cx);
                    let [ox, oy] = drag.origin.get();
                    let by =
                        (f32::from(e.event.position.x) - ox, f32::from(e.event.position.y) - oy);
                    this.watch_peek_live = Some(drag_peek(drag.start, drag.edges, by, area));
                    cx.notify();
                }))
                .capture_any_mouse_up(cx.listener(|this, _, _, cx| this.end_peek_drag(cx)))
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.end_peek_drag(cx)),
                )
                .child(header)
                .child(v_flex().flex_1().min_h_0().child(body))
                .children(grips)
                .into_any_element(),
        )
    }

    fn end_peek_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(bounds) = self.watch_peek_live.take() {
            self.update_settings(cx, |s| s.watch_peek_bounds = Some(bounds));
        }
    }
}

#[derive(Clone)]
struct PeekDrag {
    edges: [bool; 4],
    start: [f32; 4],
    origin: Rc<Cell<[f32; 2]>>,
}
impl Render for PeekDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

fn pick_up(
    drag: &PeekDrag,
    _: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PeekDrag> {
    let at = window.mouse_position();
    drag.origin.set([f32::from(at.x), f32::from(at.y)]);
    cx.new(|_| drag.clone())
}

// the card edges each grip moves: left, top, right, bottom
const GRIPS: [[bool; 4]; 8] = [
    [true, false, false, false],
    [false, true, false, false],
    [false, false, true, false],
    [false, false, false, true],
    [true, true, false, false],
    [false, true, true, false],
    [false, false, true, true],
    [true, false, false, true],
];

fn peek_grip(ix: usize, edges: [bool; 4], start: [f32; 4]) -> impl IntoElement {
    let [left, top, right, bottom] = edges;
    let cursor = match (left || right, top || bottom) {
        (true, false) => CursorStyle::ResizeLeftRight,
        (false, true) => CursorStyle::ResizeUpDown,
        _ if left == top => CursorStyle::ResizeUpLeftDownRight,
        _ => CursorStyle::ResizeUpRightDownLeft,
    };
    let grip = div().id(("watch-peek-grip", ix)).absolute().occlude().cursor(cursor);
    let grip = match (left, right) {
        (true, _) => grip.left_0().w(px(GRIP)),
        (_, true) => grip.right_0().w(px(GRIP)),
        _ => grip.left(px(GRIP)).right(px(GRIP)),
    };
    let grip = match (top, bottom) {
        (true, _) => grip.top_0().h(px(GRIP)),
        (_, true) => grip.bottom_0().h(px(GRIP)),
        _ => grip.top(px(GRIP)).bottom(px(GRIP)),
    };
    grip.on_drag(PeekDrag { edges, start, origin: Default::default() }, pick_up)
}

pub(crate) fn clamp_peek(
    [x, y, w, h]: [f32; 4],
    [ax, ay, aw, ah]: [f32; 4],
    (mw, mh): (f32, f32),
) -> [f32; 4] {
    let (w, h) = (w.max(mw).min(aw), h.max(mh).min(ah));
    [x.max(ax).min(ax + aw - w), y.max(ay).min(ay + ah - h), w, h]
}

pub(crate) fn drag_peek(
    [x, y, w, h]: [f32; 4],
    edges: [bool; 4],
    (dx, dy): (f32, f32),
    area: [f32; 4],
) -> [f32; 4] {
    if edges == [true; 4] {
        return clamp_peek([x + dx, y + dy, w, h], area, PEEK_MIN);
    }
    let shift = |on: bool, v: f32, d: f32| if on { v + d } else { v };
    let ((mw, mh), [ax, ay, aw, ah]) = (PEEK_MIN, area);
    let l = shift(edges[0], x, dx).min(x + w - mw).max(ax);
    let t = shift(edges[1], y, dy).min(y + h - mh).max(ay);
    let r = shift(edges[2], x + w, dx).max(l + mw).min(ax + aw);
    let b = shift(edges[3], y + h, dy).max(t + mh).min(ay + ah);
    clamp_peek([l, t, r - l, b - t], area, PEEK_MIN)
}

fn watch_head(chat: &Chat, item: Watch) -> (Icon, SharedString) {
    match item {
        Watch::Chat(_) if chat.is_agent() => {
            (aether_ui::provider_icon(chat.provider), chat.title.clone())
        }
        Watch::Chat(_) => (Icon::new(Lucide::MessageCircle), chat.title.clone()),
        Watch::Terminal(_) => {
            (Icon::new(Lucide::Terminal), format!("{} · terminal", chat.title).into())
        }
    }
}

pub(crate) fn fab_edges(corner: WatchCorner) -> (bool, bool) {
    match corner {
        WatchCorner::BottomRight => (false, false),
        WatchCorner::BottomLeft => (true, false),
        WatchCorner::TopRight => (false, true),
        WatchCorner::TopLeft => (true, true),
    }
}

fn anchor<E: Styled>(el: E, corner: WatchCorner, x: Pixels, y: Pixels) -> E {
    let (left, top) = fab_edges(corner);
    let el = if left { el.left(x) } else { el.right(x) };
    if top { el.top(y) } else { el.bottom(y) }
}

fn aether_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../../assets/app-icons/128.png").to_vec(),
        ))
    })
    .clone()
}
