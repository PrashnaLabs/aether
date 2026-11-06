use aether_ui::icon_chip;
use alacritty_terminal::event::WindowSize;
use alacritty_terminal::vte::ansi::CursorShape;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Icon, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Bounds, Context, DispatchPhase, ElementInputHandler, Entity, FocusHandle,
    Font, FontStyle, FontWeight, Hsla, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels,
    SharedString, StrikethroughStyle, TextAlign, TextRun, UnderlineStyle, Window, canvas, div,
    fill, outline, point, prelude::*, px, size,
};

use crate::grid::{
    Frame, Palette, SearchMatch, first_visible_row, grid_size, snapshot_search_matches,
};
use crate::keys::ScrollKey;
use crate::{
    ClearTerminal, TERMINAL_CONTEXT, TerminalCopy, TerminalPaste, TerminalSelectAll, TerminalTab,
    boxes, row_height,
};

use super::font::{installed_font_names_cached, resolve_terminal_family, terminal_font};
use super::{
    HEADER_HEIGHT, Layout, PAD_X, PANE_HEADER_HEIGHT, Status, TerminalPaneEvent, TerminalView,
    exit_label, exit_message, exit_parts, file_name, hsla, resolve_terminal_palette,
};

impl TerminalView {
    fn render_pane_header(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let pane_idx = self.pane_id.0 as usize;
        let folder = self.folder.display().to_string();
        let folder_tip = SharedString::from(folder.clone());
        let label = self.display_label();
        let label_tip = label.clone();
        let moved = (!self.hold && self.session.is_some() && self.session_folder != self.folder)
            .then(|| file_name(&folder));
        let tooltip = |tip: SharedString| {
            move |window: &mut Window, cx: &mut App| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            }
        };
        let title_color =
            if self.is_active_pane { theme.foreground } else { theme.muted_foreground };
        h_flex()
            .id(("terminal-pane-header", pane_idx))
            .flex_none()
            .h(px(PANE_HEADER_HEIGHT))
            .pl(px(10.))
            .pr(px(6.))
            .gap_1p5()
            .items_center()
            .text_xs()
            .border_b_1()
            .border_color(theme.border)
            .child(Icon::new(IconName::SquareTerminal).size(px(12.)).text_color(title_color))
            .child(
                div()
                    .id(("terminal-pane-title", pane_idx))
                    .min_w_0()
                    .max_w(px(240.))
                    .truncate()
                    .font_medium()
                    .text_color(title_color)
                    .tooltip(tooltip(label_tip))
                    .child(label),
            )
            .child(
                div()
                    .id(("terminal-pane-folder", pane_idx))
                    .min_w_0()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .tooltip(tooltip(folder_tip))
                    .child(file_name(&self.session_folder.display().to_string())),
            )
            .children(moved.map(|to| {
                let moved_label = match self.active_tab {
                    TerminalTab::Global => format!("· Workspace moved to {to}"),
                    TerminalTab::Project(_) => format!("· Chat moved to {to}"),
                };
                h_flex()
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .text_color(theme.muted_foreground)
                    .child(moved_label)
                    .child(
                        aether_ui::button(("terminal-restart-here", pane_idx))
                            .ghost()
                            .xsmall()
                            .label("Restart here")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.request_restart(window, cx)),
                            ),
                    )
            }))
            .child(div().flex_1())
            .child(
                icon_chip(("pane-split-right", pane_idx), false, cx)
                    .icon(IconName::PanelRight)
                    .accessibility_label("Split right")
                    .tooltip("Split right")
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.emit(TerminalPaneEvent::SplitRight(this.pane_id));
                    })),
            )
            .child(
                icon_chip(("pane-split-down", pane_idx), false, cx)
                    .icon(IconName::PanelBottom)
                    .accessibility_label("Split down")
                    .tooltip("Split down")
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.emit(TerminalPaneEvent::SplitDown(this.pane_id));
                    })),
            )
            .child(
                icon_chip(("pane-close", pane_idx), false, cx)
                    .icon(IconName::Close)
                    .accessibility_label("Close pane")
                    .tooltip("Close pane")
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.emit(TerminalPaneEvent::ClosePane(this.pane_id));
                    })),
            )
    }

    fn render_exit(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let Status::Exited(status) = &self.status else {
            return None;
        };
        let (code, signal) = exit_parts(*status);
        let failed = !self.stopped && (code.is_some_and(|c| c != 0) || signal.is_some());
        let theme = cx.theme();
        let icon =
            if failed || self.stopped { IconName::CircleAlert } else { IconName::CircleCheck };
        let color = if failed { aether_ui::theme::danger_text(cx) } else { theme.muted_foreground };
        let footer = h_flex()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .pl(px(PAD_X))
            .pr(px(8.))
            .gap_2()
            .items_center()
            .text_xs()
            .border_t_1()
            .border_color(theme.border)
            .child(Icon::new(icon).size(px(14.)).text_color(color));
        if self.hold {
            let label = exit_label(code, self.stopped);
            return Some(
                footer.child(div().text_color(theme.foreground).child(label)).into_any_element(),
            );
        }
        Some(
            footer
                .child(div().flex_none().text_color(theme.foreground).child(exit_message(
                    &self.shell,
                    code,
                    signal,
                )))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.muted_foreground)
                        .child("Press Enter to restart"),
                )
                .child(div().flex_1())
                .child(
                    aether_ui::button(("terminal-restart-exited", self.pane_id.0 as usize))
                        .ghost()
                        .small()
                        .label("Restart")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.request_restart(window, cx)),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Takes the drawer's focus, so Enter retries when the drawer opens on it.
    fn render_failed(&self, why: SharedString, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let pane_idx = self.pane_id.0 as usize;
        v_flex()
            .id(("terminal-failed", pane_idx))
            .flex_1()
            .px(px(PAD_X))
            .py_2()
            .gap_1()
            .text_xs()
            .track_focus(&self.focus)
            .key_context(TERMINAL_CONTEXT)
            .on_key_down(cx.listener(Self::key_down))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_color(theme.foreground)
                    .child(
                        Icon::new(IconName::CircleAlert)
                            .size(px(14.))
                            .text_color(aether_ui::theme::danger_text(cx)),
                    )
                    .child("Couldn't start the shell"),
            )
            .child(div().text_color(theme.muted_foreground).child(why))
            .when(!self.hold, |el| {
                el.child(
                    h_flex()
                        .pt_1()
                        .gap_2()
                        .items_center()
                        .child(
                            aether_ui::button(("terminal-retry", pane_idx))
                                .outline()
                                .small()
                                .label("Retry")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.restart(cx);
                                    window.focus(&this.focus, cx);
                                })),
                        )
                        .child(div().text_color(theme.muted_foreground).child("or press Enter")),
                )
            })
            .into_any_element()
    }
}

/// What the grid canvas needs from render.
struct Look {
    font: Font,
    font_size: Pixels,
    cell_w: Pixels,
    row_h: Pixels,
    palette: Palette,
    foreground: Hsla,
    background: Hsla,
    selection: Hsla,
    cursor: Hsla,
    focused: bool,
    blink_on: bool,
    marked: Option<String>,
    search_matches: Vec<SearchMatch>,
    active_match: Option<usize>,
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.shown = true;
        self.ensure_polling(window, cx);
        let theme = cx.theme();
        let palette = resolve_terminal_palette(self.terminal_theme);
        if self.palette != Some(palette) {
            self.palette = Some(palette);
            if let Some(session) = &self.session {
                session.update_theme(palette);
            }
        }
        let font_size = px(self.font_size);
        let row_h = row_height(font_size);
        let installed = installed_font_names_cached(cx);
        let effective =
            resolve_terminal_family(self.font_family.as_ref(), installed, &theme.mono_font_family);
        let font = terminal_font(&effective, theme.mono_font_family.clone());
        let cell_w = match &self.metrics {
            Some((size, family, w)) if *size == font_size && *family == font.family => *w,
            _ => {
                let text = window.text_system();
                let w = text
                    .advance(text.resolve_font(&font), font_size, 'm')
                    .map_or(font_size * 0.6, |a| a.width);
                self.metrics = Some((font_size, font.family.clone(), w));
                w
            }
        };
        let (search_matches, active_match) = if self.find_open && !self.find_matches.is_empty() {
            (self.find_matches.clone(), Some(self.find_index.min(self.find_matches.len() - 1)))
        } else {
            (Vec::new(), None)
        };
        let look = Look {
            font: font.clone(),
            font_size,
            cell_w,
            row_h,
            palette,
            foreground: hsla(palette.fg),
            background: hsla(palette.bg),
            selection: hsla(palette.selection),
            cursor: hsla(palette.cursor),
            focused: self.focus.is_focused(window) && window.is_window_active(),
            blink_on: self.blink_visible(window, cx),
            marked: self.marked.clone(),
            search_matches,
            active_match,
        };
        let pane_idx = self.pane_id.0 as usize;
        let scrolled =
            self.session.as_ref().is_some_and(|s| s.term.lock().grid().display_offset() > 0);
        let starting =
            matches!(self.status, Status::Starting).then(|| format!("Starting {}…", self.shell));
        let find_overlay = self.find_open.then(|| self.render_find_overlay(palette, cx));
        let grid = match &self.status {
            Status::Failed(why) => self.render_failed(why.clone(), cx),
            _ => {
                let view = cx.entity();
                let menu_view = view.clone();
                let menu_focus = self.focus.clone();
                let term = self.session.as_ref().map(|s| s.term.clone());
                div()
                    .id(("terminal-grid", pane_idx))
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .pl(px(PAD_X))
                    .pr(px(8.))
                    .pb_2()
                    .overflow_hidden()
                    .track_focus(&self.focus)
                    .key_context(TERMINAL_CONTEXT)
                    .cursor_text()
                    .on_key_down(cx.listener(Self::key_down))
                    .on_action(cx.listener(Self::copy))
                    .on_action(cx.listener(Self::paste))
                    .on_action(cx.listener(Self::select_all))
                    .on_action(cx.listener(Self::clear))
                    .capture_any_mouse_down(cx.listener(Self::report_mouse_down))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_scroll_wheel(cx.listener(Self::scroll))
                    .child(grid_canvas(view, term, self.focus.clone(), look))
                    .children(starting.map(|text| {
                        div()
                            .absolute()
                            .top_0()
                            .left(px(PAD_X))
                            .font_family(font.family.clone())
                            .text_size(font_size)
                            .line_height(row_h)
                            .text_color(theme.muted_foreground)
                            .child(text)
                    }))
                    .children(find_overlay)
                    .when(scrolled, |grid| {
                        grid.child(
                            div().absolute().bottom_2().right_3().child(
                                aether_ui::button(("terminal-jump", pane_idx))
                                    .secondary()
                                    .small()
                                    .icon(IconName::ArrowDown)
                                    .label("Jump to latest")
                                    .tooltip("Scroll to the newest output")
                                    .rounded(theme.radius_full())
                                    .border_1()
                                    .border_color(theme.border)
                                    .bg(hsla(palette.bg))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.scroll_to(ScrollKey::Bottom, cx);
                                        window.focus(&this.focus, cx);
                                    })),
                            ),
                        )
                    })
                    .context_menu({
                        let pane_id = self.pane_id;
                        move |menu, _, cx| {
                            let has_selection = menu_view.read(cx).has_selection();
                            let split_right_view = menu_view.clone();
                            let split_down_view = menu_view.clone();
                            let close_view = menu_view.clone();
                            menu.action_context(menu_focus.clone())
                                .menu_with_disabled("Copy", Box::new(TerminalCopy), !has_selection)
                                .menu("Paste", Box::new(TerminalPaste))
                                .separator()
                                .menu("Select all", Box::new(TerminalSelectAll))
                                .menu("Clear terminal", Box::new(ClearTerminal))
                                .separator()
                                .item(
                                    PopupMenuItem::new("Split right")
                                        .icon(IconName::PanelRight)
                                        .on_click(move |_, _, cx| {
                                            split_right_view.update(cx, |_, cx| {
                                                cx.emit(TerminalPaneEvent::SplitRight(pane_id));
                                            });
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("Split down")
                                        .icon(IconName::PanelBottom)
                                        .on_click(move |_, _, cx| {
                                            split_down_view.update(cx, |_, cx| {
                                                cx.emit(TerminalPaneEvent::SplitDown(pane_id));
                                            });
                                        }),
                                )
                                .separator()
                                .item(
                                    PopupMenuItem::new("Delete terminal")
                                        .icon(IconName::Trash)
                                        .on_click(move |_, _, cx| {
                                            close_view.update(cx, |_, cx| {
                                                cx.emit(TerminalPaneEvent::ClosePane(pane_id));
                                            });
                                        }),
                                )
                        }
                    })
                    .into_any_element()
            }
        };
        v_flex()
            .id(("terminal-pane", pane_idx))
            .size_full()
            .min_w_0()
            .min_h_0()
            .relative()
            .bg(hsla(palette.bg))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.emit(TerminalPaneEvent::Focused(this.pane_id));
                }),
            )
            .capture_any_mouse_up(cx.listener(|this, _, _, cx| this.mouse_up(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.mouse_up(cx)))
            .children(self.show_pane_header.then(|| self.render_pane_header(cx)))
            .child(grid)
            .children(self.render_exit(cx))
    }
}

fn grid_canvas(
    view: Entity<TerminalView>,
    term: Option<
        std::sync::Arc<
            alacritty_terminal::sync::FairMutex<alacritty_terminal::Term<crate::session::Listener>>,
        >,
    >,
    focus: FocusHandle,
    look: Look,
) -> impl IntoElement + use<> {
    canvas(
        {
            let view = view.clone();
            move |bounds, _, cx| {
                let (cols, rows) = grid_size(
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                    f32::from(look.cell_w),
                    f32::from(look.row_h),
                );
                let size = WindowSize {
                    num_cols: cols.min(u16::MAX as usize) as u16,
                    num_lines: rows.min(u16::MAX as usize) as u16,
                    cell_width: f32::from(look.cell_w) as u16,
                    cell_height: f32::from(look.row_h) as u16,
                };
                view.update(cx, |this, cx| this.fit(size, cx));
                let frame = term.map(|t| {
                    snapshot_search_matches(
                        &t.lock(),
                        &look.palette,
                        look.focused && look.blink_on,
                        &look.search_matches,
                        look.active_match,
                    )
                });
                let visible = ((bounds.size.height / look.row_h) as usize).max(1);
                let first = frame.as_ref().map_or(0, |f| {
                    first_visible_row(f.rows.len(), visible, f.cursor.map(|c| c.row))
                });
                let layout = Layout {
                    origin: bounds.origin,
                    cell_w: look.cell_w,
                    row_h: look.row_h,
                    first_row: first,
                    visible,
                    cols,
                    rows: frame.as_ref().map_or(rows, |f| f.rows.len()),
                };
                let cursor = frame
                    .as_ref()
                    .and_then(|f| f.cursor)
                    .filter(|c| c.row >= first)
                    .map(|c| cell_bounds(&layout, c.row, c.col, c.width));
                let blinking = frame.as_ref().is_some_and(|f| f.blinking);
                view.update(cx, |this, _| {
                    this.layout = Some(layout);
                    this.blinking = blinking;
                    this.cursor_bounds =
                        Some(cursor.unwrap_or_else(|| cell_bounds(&layout, first, 0, 1)));
                });
                (frame, layout, look)
            }
        },
        move |bounds, (frame, layout, look), window, cx| {
            window.handle_input(&focus, ElementInputHandler::new(bounds, view.clone()), cx);
            if let Some(frame) = frame {
                paint_grid(frame, layout, look, window, cx);
            }
            let up_view = view.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    view.update(cx, |this, cx| {
                        if !this.report_motion(e) && e.pressed_button == Some(MouseButton::Left) {
                            this.drag_select(e.position, cx);
                        }
                    });
                }
            });
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    up_view.update(cx, |this, _| this.report_mouse_up(e));
                }
            });
        },
    )
    .size_full()
}

fn cell_bounds(l: &Layout, row: usize, col: usize, width: usize) -> Bounds<Pixels> {
    Bounds::new(
        point(
            l.origin.x + l.cell_w * col as f32,
            l.origin.y + l.row_h * row.saturating_sub(l.first_row) as f32,
        ),
        size(l.cell_w * width as f32, l.row_h),
    )
}

fn paint_grid(frame: Frame, l: Layout, look: Look, window: &mut Window, cx: &mut App) {
    let cell = |row: usize, col: usize, width: usize| cell_bounds(&l, row, col, width);
    let shown = l.first_row..l.first_row + l.visible;
    let rows = || frame.rows.iter().enumerate().skip(l.first_row).take(l.visible);
    for (r, row) in rows() {
        for &(start, end, color) in &row.bgs {
            window.paint_quad(fill(cell(r, start, end - start), hsla(color)));
        }
        if let Some((start, end)) = row.selected {
            window.paint_quad(fill(cell(r, start, end - start), look.selection));
        }
    }
    let cursor = frame.cursor.filter(|c| shown.contains(&c.row));
    if let Some(c) = cursor {
        let rect = cell(c.row, c.col, c.width);
        let bar = px(2.);
        let color = look.cursor;
        match (look.focused, c.shape) {
            (false, _) | (_, CursorShape::HollowBlock) => {
                window.paint_quad(outline(rect, color, gpui_kit::BorderStyle::Solid))
            }
            _ if !look.blink_on => {}
            (_, CursorShape::Beam) => {
                window.paint_quad(fill(Bounds::new(rect.origin, size(bar, l.row_h)), color))
            }
            (_, CursorShape::Underline) => window.paint_quad(fill(
                Bounds::new(point(rect.origin.x, rect.bottom() - bar), size(rect.size.width, bar)),
                color,
            )),
            _ => window.paint_quad(fill(rect, color)),
        }
    }
    for (r, row) in rows() {
        for &(col, ch, color) in &row.boxes {
            let origin = cell(r, col, 1).origin;
            for ([x0, y0, x1, y1], alpha) in
                boxes::shape(ch, f32::from(l.cell_w), f32::from(l.row_h)).unwrap_or_default()
            {
                let bounds = Bounds::from_corners(
                    origin + point(px(x0), px(y0)),
                    origin + point(px(x1), px(y1)),
                );
                window.paint_quad(fill(bounds, hsla(color).opacity(alpha)));
            }
        }
    }
    // weight and slant variants, cloned per run as cheap handles
    let fonts = [
        look.font.clone(),
        Font { weight: FontWeight::BOLD, ..look.font.clone() },
        Font { style: FontStyle::Italic, ..look.font.clone() },
        Font { weight: FontWeight::BOLD, style: FontStyle::Italic, ..look.font.clone() },
    ];
    let mut runs: Vec<TextRun> = Vec::new();
    for (r, row) in frame.rows.into_iter().enumerate().skip(l.first_row).take(l.visible) {
        if row.text.is_empty() {
            continue;
        }
        runs.clear();
        runs.extend(row.runs.iter().map(|run| {
            let color = hsla(run.style.fg);
            TextRun {
                len: run.len,
                font: fonts[run.style.bold as usize | (run.style.italic as usize) << 1].clone(),
                color,
                background_color: None,
                underline: (run.style.underline || run.style.undercurl).then_some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(color),
                    wavy: run.style.undercurl,
                }),
                strikethrough: run
                    .style
                    .strike
                    .then_some(StrikethroughStyle { thickness: px(1.), color: Some(color) }),
            }
        }));
        // forcing the cell width pins glyph i to column i, whatever the fallback font's advance
        let line =
            window.text_system().shape_line(row.text.into(), look.font_size, &runs, Some(l.cell_w));
        let _ = line.paint(cell(r, 0, 1).origin, l.row_h, TextAlign::Left, None, window, cx);
    }
    if let (Some(text), Some(c)) = (look.marked, cursor) {
        let run = TextRun {
            len: text.len(),
            font: fonts[0].clone(),
            color: look.foreground,
            background_color: None,
            underline: Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(look.foreground),
                wavy: false,
            }),
            strikethrough: None,
        };
        let line = window.text_system().shape_line(text.into(), look.font_size, &[run], None);
        let origin = cell(c.row, c.col, 1).origin;
        window.paint_quad(fill(
            Bounds::new(origin, size(line.width.max(l.cell_w), l.row_h)),
            look.background,
        ));
        let _ = line.paint(origin, l.row_h, TextAlign::Left, None, window, cx);
    }
}
