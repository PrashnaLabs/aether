use std::ops::Range;

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use gpui_kit::{
    Bounds, ClipboardItem, Context, EntityInputHandler, KeyDownEvent, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent, UTF16Selection,
    Window,
};

use crate::keys::{
    Mods, ScrollKey, key_bytes, leave_to_ime, mouse_report, paste_bytes, scroll_key, wheel_report,
};
use crate::{ClearTerminal, TerminalCopy, TerminalPaste, TerminalSelectAll};

use super::{Status, TerminalPaneEvent, TerminalView};

impl TerminalView {
    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &event.keystroke;
        let m = ks.modifiers;
        if self.find_open {
            match ks.key.as_str() {
                "escape" => {
                    self.close_find(window, cx);
                    return cx.stop_propagation();
                }
                "enter" => {
                    if m.shift {
                        self.find_prev(cx);
                    } else {
                        self.find_next(cx);
                    }
                    return cx.stop_propagation();
                }
                "up" => {
                    self.find_prev(cx);
                    return cx.stop_propagation();
                }
                "down" => {
                    self.find_next(cx);
                    return cx.stop_propagation();
                }
                "backspace" => {
                    if m.platform {
                        self.find_query.clear();
                    } else {
                        self.find_query.pop();
                    }
                    self.find_index = 0;
                    self.refresh_find_matches(true);
                    cx.notify();
                    return cx.stop_propagation();
                }
                _ => {
                    if !m.control
                        && !m.platform
                        && let Some(text) = ks.key_char.as_deref()
                        && !text.is_empty()
                        && !text.chars().any(char::is_control)
                    {
                        self.find_query.push_str(text);
                        self.find_index = 0;
                        self.refresh_find_matches(true);
                        cx.notify();
                        return cx.stop_propagation();
                    }
                    if !m.platform {
                        return cx.stop_propagation();
                    }
                    return;
                }
            }
        }
        if matches!(self.status, Status::Exited(_) | Status::Failed(_)) {
            // a held command re-runs from the app, which tracks its run state
            if ks.key == "enter" && !m.modified() && !self.hold {
                self.restart(cx);
            }
            return cx.stop_propagation();
        }
        let mods = Mods { shift: m.shift, alt: m.alt, ctrl: m.control, cmd: m.platform };
        if leave_to_ime(ks.key_char.as_deref(), mods, self.option_as_meta) {
            // held keys repeat here; the input method would open the accent menu instead
            if event.is_held
                && self.marked.is_none()
                && let Some(text) = &ks.key_char
            {
                self.send(text.clone().into_bytes(), cx);
                cx.stop_propagation();
            }
            return;
        }
        let mode = self.mode();
        if !mode.contains(TermMode::ALT_SCREEN)
            && let Some(key) = scroll_key(&ks.key, mods, cfg!(target_os = "macos"))
        {
            self.scroll_to(key, cx);
            return cx.stop_propagation();
        }
        if let Some(bytes) =
            key_bytes(&ks.key, ks.key_char.as_deref(), mods, mode.contains(TermMode::APP_CURSOR))
        {
            self.send(bytes, cx);
            cx.stop_propagation();
        }
    }

    pub(super) fn scroll_to(&mut self, key: ScrollKey, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let scroll = match key {
            ScrollKey::PageUp => Scroll::PageUp,
            ScrollKey::PageDown => Scroll::PageDown,
            ScrollKey::LineUp => Scroll::Delta(1),
            ScrollKey::LineDown => Scroll::Delta(-1),
            ScrollKey::Top => Scroll::Top,
            ScrollKey::Bottom => Scroll::Bottom,
        };
        session.term.lock().scroll_display(scroll);
        cx.notify();
    }

    pub(super) fn has_selection(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| s.term.lock().selection.as_ref().is_some_and(|sel| !sel.is_empty()))
    }

    pub(super) fn copy(&mut self, _: &TerminalCopy, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.session.as_ref().and_then(|s| s.term.lock().selection_to_string());
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub(super) fn paste(&mut self, _: &TerminalPaste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            if self.find_open {
                let clean: String = text.chars().filter(|&c| c != '\n' && c != '\r').collect();
                self.find_query.push_str(&clean);
                self.find_index = 0;
                self.refresh_find_matches(true);
                cx.notify();
                return;
            }
            let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
            self.send(paste_bytes(&text, bracketed), cx);
        }
    }

    pub(super) fn select_all(
        &mut self,
        _: &TerminalSelectAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &self.session else {
            return;
        };
        let mut term = session.term.lock();
        let (top, bottom, last) = (term.topmost_line(), term.bottommost_line(), term.last_column());
        let mut selection =
            Selection::new(SelectionType::Simple, GridPoint::new(top, Column(0)), Side::Left);
        selection.update(GridPoint::new(bottom, last), Side::Right);
        term.selection = Some(selection);
        cx.notify();
    }

    pub(crate) fn clear_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        {
            let mut term = session.term.lock();
            term.grid_mut().clear_history();
            term.selection = None;
        }
        // Ctrl+L redraws the prompt at the top of the cleared screen
        self.send(vec![0x0c], cx);
    }

    pub(super) fn clear(&mut self, _: &ClearTerminal, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_terminal(cx);
    }

    /// The cell under `position`, clamped to the grid, as a viewport row and column.
    fn cell_at(&self, position: Point<Pixels>) -> Option<(usize, usize, Side)> {
        let l = self.layout?;
        let x = (position.x - l.origin.x) / l.cell_w;
        let y = (position.y - l.origin.y) / l.row_h;
        let col = (x.max(0.) as usize).min(l.cols.saturating_sub(1));
        let row = (y.max(0.) as usize + l.first_row).min(l.rows.saturating_sub(1));
        let side = if x >= l.cols as f32 || x.fract() >= 0.5 { Side::Right } else { Side::Left };
        Some((row, col, side))
    }

    fn grid_point(
        &self,
        position: Point<Pixels>,
        display_offset: usize,
    ) -> Option<(GridPoint, Side)> {
        let (row, col, side) = self.cell_at(position)?;
        Some((GridPoint::new(Line(row as i32 - display_offset as i32), Column(col)), side))
    }

    pub(super) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(TerminalPaneEvent::Focused(self.pane_id));
        window.focus(&self.focus, cx);
        let Some(session) = &self.session else {
            return;
        };
        let mut term = session.term.lock();
        let Some((point, side)) = self.grid_point(event.position, term.grid().display_offset())
        else {
            return;
        };
        match term.selection.as_mut() {
            Some(selection) if event.modifiers.shift => selection.update(point, side),
            _ => {
                let kind = match event.click_count {
                    2 => SelectionType::Semantic,
                    3.. => SelectionType::Lines,
                    _ => SelectionType::Simple,
                };
                term.selection = Some(Selection::new(kind, point, side));
            }
        }
        self.selecting = true;
        cx.notify();
    }

    fn in_grid(&self, p: Point<Pixels>) -> bool {
        self.layout.is_some_and(|l| {
            p.x >= l.origin.x
                && p.y >= l.origin.y
                && p.x < l.origin.x + l.cell_w * l.cols as f32
                && p.y < l.origin.y + l.row_h * l.visible as f32
        })
    }

    /// Sends a press to a program tracking the mouse (tmux, vim, htop); shift keeps it for selection.
    pub(super) fn report_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &self.session else {
            return;
        };
        let (mode, scrolled) = {
            let term = session.term.lock();
            (*term.mode(), term.grid().display_offset() > 0)
        };
        if !mode.intersects(TermMode::MOUSE_MODE)
            || event.modifiers.shift
            || self.find_open
            || scrolled
            || !self.in_grid(event.position)
        {
            return;
        }
        let (Some(code), Some((row, col, _))) =
            (report_code(event.button, event.modifiers), self.cell_at(event.position))
        else {
            return;
        };
        session.write(mouse_report(code, col, row, true, mode.contains(TermMode::SGR_MOUSE)));
        self.held = Some(code);
        self.reported_at = Some((row, col));
        // capture phase: keeps selection and the context menu out of it
        cx.stop_propagation();
        cx.emit(TerminalPaneEvent::Focused(self.pane_id));
        window.focus(&self.focus, cx);
    }

    pub(super) fn report_mouse_up(&mut self, event: &MouseUpEvent) {
        let (Some(code), Some(session)) = (self.held.take(), &self.session) else {
            return;
        };
        let mode = *session.term.lock().mode();
        if !mode.intersects(TermMode::MOUSE_MODE) {
            return;
        }
        let (row, col) = self
            .cell_at(event.position)
            .map_or(self.reported_at.unwrap_or_default(), |(row, col, _)| (row, col));
        session.write(mouse_report(code, col, row, false, mode.contains(TermMode::SGR_MOUSE)));
    }

    /// Drag (1002) and hover (1003) reports; true when the move belongs to the program.
    pub(super) fn report_motion(&mut self, event: &MouseMoveEvent) -> bool {
        if self.held.is_none() && !self.in_grid(event.position) {
            return false;
        }
        let Some(session) = &self.session else {
            return false;
        };
        let mode = *session.term.lock().mode();
        let code = match self.held {
            Some(code) if mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) => {
                code + 32
            }
            None if mode.contains(TermMode::MOUSE_MOTION) && event.pressed_button.is_none() => {
                report_code(MouseButton::Left, event.modifiers).unwrap_or(0) + 3 + 32
            }
            _ => return self.held.is_some(),
        };
        let Some((row, col, _)) = self.cell_at(event.position) else {
            return true;
        };
        if self.reported_at != Some((row, col)) {
            self.reported_at = Some((row, col));
            session.write(mouse_report(code, col, row, true, mode.contains(TermMode::SGR_MOUSE)));
        }
        true
    }

    /// Extends the selection; past the top or bottom edge it scrolls history too.
    pub(super) fn drag_select(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let (true, Some(session), Some(l)) = (self.selecting, &self.session, self.layout) else {
            return;
        };
        let mut term = session.term.lock();
        if position.y < l.origin.y {
            term.scroll_display(Scroll::Delta(1));
        } else if position.y >= l.origin.y + l.row_h * l.visible as f32 {
            term.scroll_display(Scroll::Delta(-1));
        }
        let Some((point, side)) = self.grid_point(position, term.grid().display_offset()) else {
            return;
        };
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
        cx.notify();
    }

    pub(super) fn mouse_up(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.selecting)
            && let Some(session) = &self.session
        {
            let mut term = session.term.lock();
            if term.selection.as_ref().is_some_and(|s| s.is_empty()) {
                term.selection = None;
                cx.notify();
            }
        }
    }

    pub(super) fn scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(session), Some(l)) = (&self.session, self.layout) else {
            return;
        };
        cx.stop_propagation();
        self.scroll_px += f32::from(event.delta.pixel_delta(l.row_h).y);
        let lines = (self.scroll_px / f32::from(l.row_h)) as i32;
        if lines == 0 {
            return;
        }
        self.scroll_px -= lines as f32 * f32::from(l.row_h);
        let cell = self.cell_at(event.position).map_or((0, 0), |(row, col, _)| (row, col));
        let mut term = session.term.lock();
        let mode = *term.mode();
        let notches = lines.unsigned_abs() as usize;
        if mode.intersects(TermMode::MOUSE_MODE) {
            drop(term);
            let (row, col) = cell;
            let report = wheel_report(lines > 0, col, row, mode.contains(TermMode::SGR_MOUSE));
            session.write(report.repeat(notches));
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            // full-screen programs (less, man) have no scrollback; scroll them with arrow keys
            drop(term);
            let arrow = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => "\x1bOA",
                (true, false) => "\x1b[A",
                (false, true) => "\x1bOB",
                (false, false) => "\x1b[B",
            };
            session.write(arrow.repeat(notches).into_bytes());
        } else {
            term.scroll_display(Scroll::Delta(lines));
            cx.notify();
        }
    }
}

/// Button plus alt/ctrl bits of a mouse report; shift never reaches a report.
fn report_code(button: MouseButton, m: Modifiers) -> Option<u8> {
    let button = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        _ => return None,
    };
    Some(button | if m.alt { 8 } else { 0 } | if m.control { 16 } else { 0 })
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let utf16: Vec<u16> = self.marked.as_deref()?.encode_utf16().collect();
        let range = range.start.min(utf16.len())..range.end.min(utf16.len());
        *adjusted = Some(range.clone());
        Some(String::from_utf16_lossy(&utf16[range]))
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: 0..0, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|m| 0..m.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = None;
        if self.find_open {
            self.find_query.push_str(text);
            self.find_index = 0;
            self.refresh_find_matches(true);
            cx.notify();
            return;
        }
        if matches!(self.status, Status::Starting | Status::Running) {
            self.send(text.as_bytes().to_vec(), cx);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.cursor_bounds
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}
