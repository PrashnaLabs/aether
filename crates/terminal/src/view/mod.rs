//! The drawer's view: shell lifecycle, keyboard and IME input, mouse, scrolling, painting.

mod find;
mod font;
mod input;
mod render;

use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::Rgb;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, EventEmitter, FocusHandle, Hsla, Pixels, Point,
    SharedString, Subscription, Task, Window, px,
};

use aether_core::{TerminalCursor, TerminalTheme};

use super::grid::{Palette, SearchMatch};
use super::palette::ThemePalette;
use super::session::{Notice, Session};
use super::workspace::PaneId;
use super::{TerminalTab, row_height};

pub use font::{installed_font_names_cached, installed_nerd_fonts, resolve_terminal_family};

const POLL: Duration = Duration::from_millis(24);
const BLINK: Duration = Duration::from_millis(500);
/// Shell resizes during a drawer drag are spaced at least this far apart.
const RESIZE_EVERY: Duration = Duration::from_millis(50);
/// Input kept while the shell starts.
const PENDING_LIMIT: usize = 64 * 1024;
pub(super) const HEADER_HEIGHT: f32 = 32.;
pub(crate) const PANE_HEADER_HEIGHT: f32 = 24.;
/// Grid padding above and below the rows.
pub(super) const GRID_PAD_Y: f32 = 8.;
const PAD_X: f32 = 16.;

pub enum TerminalEvent {
    Resize(f32),
    ResizeEnd,
    ResetHeight,
    Hide,
    SelectTab(TerminalTab),
    ToggleWatch,
    CommandEnded { key: SharedString, code: Option<i32>, stopped: bool, failed_to_start: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalPaneEvent {
    Focused(PaneId),
    SplitRight(PaneId),
    SplitDown(PaneId),
    ClosePane(PaneId),
    Exited(PaneId),
    TitleChanged(PaneId),
}

enum Status {
    Starting,
    Running,
    Exited(Option<ExitStatus>),
    Failed(SharedString),
}

/// Where the grid was last painted, for mapping the mouse to cells.
#[derive(Clone, Copy)]
struct Layout {
    origin: Point<Pixels>,
    cell_w: Pixels,
    row_h: Pixels,
    first_row: usize,
    visible: usize,
    cols: usize,
    rows: usize,
}

pub(crate) fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

pub(crate) fn resolve_pane_label(
    custom_title: Option<&str>,
    _osc_title: Option<&str>,
    running_job: Option<&str>,
    shell: &str,
) -> String {
    if let Some(custom) = custom_title.map(str::trim).filter(|s| !s.is_empty()) {
        return custom.to_string();
    }
    if let Some(job) = running_job.map(str::trim).filter(|s| !s.is_empty()) {
        return job.to_string();
    }
    shell.to_string()
}

pub(crate) fn hsla(c: Rgb) -> Hsla {
    gpui_kit::rgb(u32::from_be_bytes([0, c.r, c.g, c.b])).into()
}

/// Exit line for the shell `shell`: by exit code, or by signal when it was killed.
pub(super) fn exit_message(shell: &str, code: Option<i32>, signal: Option<i32>) -> String {
    match (code, signal) {
        (Some(0), _) | (None, None) => format!("{shell} exited"),
        (Some(code), _) => format!("Process exited (code {code})"),
        (None, Some(signal)) => format!("{shell} was stopped (signal {signal})"),
    }
}

pub(crate) fn exit_label(code: Option<i32>, stopped: bool) -> String {
    match (stopped, code) {
        (true, _) => "Stopped".into(),
        (false, Some(code)) => format!("Exited with code {code}"),
        (false, None) => "Exited".into(),
    }
}

pub(crate) fn start_size(font_size: f32) -> WindowSize {
    WindowSize {
        num_cols: 80,
        num_lines: 24,
        cell_width: (font_size * 0.6) as u16,
        cell_height: f32::from(row_height(px(font_size))) as u16,
    }
}

fn exit_parts(status: Option<ExitStatus>) -> (Option<i32>, Option<i32>) {
    let code = status.and_then(|s| s.code());
    #[cfg(unix)]
    let signal = status.and_then(|s| std::os::unix::process::ExitStatusExt::signal(&s));
    #[cfg(not(unix))]
    let signal = None;
    (code, signal)
}

pub(crate) struct TerminalView {
    pub(crate) pane_id: PaneId,
    pub(crate) custom_title: Option<SharedString>,
    pub(crate) show_pane_header: bool,
    pub(crate) is_active_pane: bool,
    pub(crate) find_open: bool,
    pub(crate) find_query: String,
    pub(crate) find_matches: Vec<SearchMatch>,
    pub(crate) find_index: usize,
    /// The chat's folder now; a restart starts there.
    pub(crate) folder: PathBuf,
    /// Where the running shell started.
    pub(crate) session_folder: PathBuf,
    /// `folder` was set for this pane alone; workspace syncs leave it.
    pub(crate) own_folder: bool,
    pub(crate) shell: SharedString,
    pub(crate) title: Option<SharedString>,
    pub(crate) command: Option<String>,
    pub(crate) hold: bool,
    pub(crate) key: Option<SharedString>,
    pub(crate) stopped: bool,
    session: Option<Session>,
    status: Status,
    spawning: Option<Task<()>>,
    pending: Vec<u8>,
    pub(crate) open: bool,
    /// Drawn by the chat view; polling stops once it isn't.
    pub(crate) shown: bool,
    pub(crate) option_as_meta: bool,
    /// Custom terminal font family from settings; empty means the theme mono font.
    pub(crate) font_family: SharedString,
    pub(crate) font_size: f32,
    pub(crate) terminal_theme: TerminalTheme,
    pub(crate) terminal_cursor: TerminalCursor,
    pub(crate) active_tab: TerminalTab,
    pub(crate) full_page: bool,
    polling: bool,
    pub(crate) focus: FocusHandle,
    palette: Option<Palette>,
    /// Cell width per font size and family, measured once.
    metrics: Option<(Pixels, SharedString, Pixels)>,
    layout: Option<Layout>,
    /// Where the IME candidate window goes.
    cursor_bounds: Option<Bounds<Pixels>>,
    /// IME composition not yet committed.
    marked: Option<String>,
    scroll_px: f32,
    selecting: bool,
    /// Report code of the button a mouse-tracking program saw go down.
    held: Option<u8>,
    /// Cell of the last mouse report, so motion reports only on cell changes.
    reported_at: Option<(usize, usize)>,
    last_input: Instant,
    blinking: bool,
    blink_on: bool,
    resized_at: Instant,
    pending_size: Option<WindowSize>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TerminalEvent> for TerminalView {}
impl EventEmitter<TerminalPaneEvent> for TerminalView {}

impl TerminalView {
    pub(crate) fn new_pane(
        pane_id: PaneId,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let shell = std::env::var("SHELL").map_or_else(|_| "sh".into(), |s| file_name(&s));
        let subscriptions = vec![
            cx.on_focus_in(&focus, window, |this, _, cx| {
                cx.emit(TerminalPaneEvent::Focused(this.pane_id));
                cx.notify();
            }),
            cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
            cx.observe_window_activation(window, |_, _, cx| cx.notify()),
        ];
        Self {
            pane_id,
            custom_title: None,
            show_pane_header: false,
            is_active_pane: true,
            find_open: false,
            find_query: String::new(),
            find_matches: Vec::new(),
            find_index: 0,
            session_folder: folder.clone(),
            folder,
            own_folder: false,
            shell: shell.into(),
            title: None,
            command: None,
            hold: false,
            key: None,
            stopped: false,
            session: None,
            status: Status::Starting,
            spawning: None,
            pending: Vec::new(),
            open: false,
            shown: false,
            option_as_meta: false,
            font_family: "".into(),
            font_size: 13.0,
            terminal_theme: TerminalTheme::default(),
            terminal_cursor: TerminalCursor::default(),
            active_tab: TerminalTab::Global,
            full_page: false,
            polling: false,
            focus,
            palette: None,
            metrics: None,
            layout: None,
            cursor_bounds: None,
            marked: None,
            scroll_px: 0.,
            selecting: false,
            held: None,
            reported_at: None,
            last_input: Instant::now(),
            blinking: false,
            blink_on: true,
            resized_at: Instant::now(),
            pending_size: None,
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn display_label(&self) -> SharedString {
        let job = self.running_job();
        resolve_pane_label(
            self.custom_title.as_deref(),
            self.title.as_deref(),
            job.as_deref(),
            self.shell.as_ref(),
        )
        .into()
    }

    pub(crate) fn set_custom_title(&mut self, title: Option<SharedString>, cx: &mut Context<Self>) {
        let normalized = title.and_then(|t| {
            let trimmed = t.trim();
            (!trimmed.is_empty()).then(|| SharedString::from(trimmed.to_string()))
        });
        if self.custom_title != normalized {
            self.custom_title = normalized;
            cx.emit(TerminalPaneEvent::TitleChanged(self.pane_id));
            cx.notify();
        }
    }

    /// Ends the shell; the next paint starts a new one.
    pub(crate) fn shutdown(&mut self) {
        if self.hold
            && let Some(session) = &self.session
        {
            session.terminate();
        }
        self.spawning = None;
        self.session = None;
        self.status = Status::Starting;
        self.pending.clear();
        self.marked = None;
        self.title = None;
        self.stopped = false;
    }

    pub fn set_font_size(&mut self, font_size: f32, cx: &mut Context<Self>) {
        if self.font_size != font_size {
            self.font_size = font_size;
            self.metrics = None;
            cx.notify();
        }
    }

    pub fn set_terminal_theme(&mut self, theme: TerminalTheme, cx: &mut Context<Self>) {
        if self.terminal_theme != theme {
            self.terminal_theme = theme;
            cx.notify();
        }
    }

    pub fn set_terminal_cursor(&mut self, cursor: TerminalCursor, cx: &mut Context<Self>) {
        if self.terminal_cursor != cursor {
            self.terminal_cursor = cursor;
            if let Some(session) = &self.session {
                session.set_cursor(cursor);
            }
            cx.notify();
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        self.session.is_some() && matches!(self.status, Status::Running)
    }

    pub(crate) fn command_live(&self) -> bool {
        self.hold && matches!(self.status, Status::Starting | Status::Running)
    }

    pub(crate) fn unstarted(&self) -> bool {
        self.spawning.is_none() && matches!(self.status, Status::Starting)
    }

    pub(crate) fn failed_to_start(&self) -> bool {
        matches!(self.status, Status::Failed(_))
    }

    pub(crate) fn exit_code(&self) -> Option<i32> {
        match self.status {
            Status::Exited(status) => exit_parts(status).0,
            _ => None,
        }
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        let stopped = match (&self.session, &self.status) {
            (Some(session), Status::Running) => session.terminate(),
            (_, Status::Starting | Status::Running) => {
                self.spawning = None;
                self.session = None;
                self.status = Status::Exited(None);
                cx.emit(TerminalPaneEvent::Exited(self.pane_id));
                true
            }
            _ => false,
        };
        if stopped {
            self.stopped = true;
            cx.notify();
        }
    }

    /// Starts a held command now, so it runs and reports while its drawer is hidden.
    pub(crate) fn start_command(
        &mut self,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette = Some(palette);
        self.start(start_size(self.font_size), palette, cx);
        self.ensure_polling(window, cx);
    }

    /// Name of the program running in front of the shell; empty when unknown.
    pub(crate) fn running_job(&self) -> Option<String> {
        self.session.as_ref().filter(|_| self.is_running()).and_then(Session::running_job)
    }

    pub(crate) fn restart(&mut self, cx: &mut Context<Self>) {
        self.shutdown();
        self.scroll_px = 0.;
        cx.emit(TerminalPaneEvent::TitleChanged(self.pane_id));
        cx.notify();
    }

    /// Restarts, after a confirm when a program is still running.
    pub(crate) fn request_restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = self.running_job() else {
            self.restart(cx);
            return window.focus(&self.focus, cx);
        };
        let what = if job.is_empty() { "A program".to_string() } else { job };
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .confirm()
                .title("Restart terminal?")
                .description(SharedString::from(format!(
                    "{what} is still running and will be stopped."
                )))
                .ok_text("Restart")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.restart(cx);
                        window.focus(&this.focus, cx);
                    });
                    true
                })
        });
    }

    /// Called while painting: resizes the shell to the grid, or starts it at that size.
    fn fit(&mut self, size: WindowSize, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            if session.same_size(size) {
                self.pending_size = None;
            } else if self.resized_at.elapsed() >= RESIZE_EVERY {
                session.resize(size);
                self.resized_at = Instant::now();
                self.pending_size = None;
            } else {
                self.pending_size = Some(size);
            }
            return;
        }
        let (Status::Starting, None, Some(palette)) = (&self.status, &self.spawning, self.palette)
        else {
            return;
        };
        self.start(size, palette, cx);
    }

    fn start(&mut self, size: WindowSize, palette: Palette, cx: &mut Context<Self>) {
        let folder = self.folder.clone();
        self.session_folder = folder.clone();
        let command = self.command.clone();
        let task = cx.background_executor().spawn(async move {
            match command {
                Some(command) => Session::spawn_command(&folder, &command, size, palette),
                None => Session::spawn(&folder, size, palette),
            }
        });
        self.spawning = Some(cx.spawn(async move |this, cx| {
            let started = task.await;
            this.update(cx, |this, cx| {
                this.spawning = None;
                match started {
                    Ok(session) => {
                        this.shell = file_name(&session.shell).into();
                        session.write(std::mem::take(&mut this.pending));
                        session.set_cursor(this.terminal_cursor);
                        this.session = Some(session);
                        this.status = Status::Running;
                        cx.emit(TerminalPaneEvent::TitleChanged(this.pane_id));
                    }
                    Err(e) => {
                        this.pending.clear();
                        this.status = Status::Failed(e.into());
                        if this.hold {
                            cx.emit(TerminalPaneEvent::Exited(this.pane_id));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn ensure_polling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                match this.update_in(cx, |this, window, cx| this.tick(window, cx)) {
                    Ok(true) => {}
                    _ => break,
                }
            }
        })
        .detach();
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // a hidden command still has to report its exit
        if !self.shown && !self.command_live() {
            self.polling = false;
            return false;
        }
        let Some(session) = &self.session else {
            return true;
        };
        let mut changed = session.take_dirty();
        if let Some(size) = self.pending_size
            && self.resized_at.elapsed() >= RESIZE_EVERY
        {
            session.resize(size);
            self.resized_at = Instant::now();
            self.pending_size = None;
            changed = true;
        }
        let notices: Vec<Notice> = session.notices().collect();
        for notice in notices {
            changed = true;
            match notice {
                Notice::Exited(status) => {
                    let (ended, known) = match self.status {
                        Status::Exited(prev) => (true, prev),
                        _ => (false, None),
                    };
                    self.status = Status::Exited(known.or(status));
                    // alacritty reports ChildExit then Exit; one event per run
                    if !ended {
                        cx.emit(TerminalPaneEvent::Exited(self.pane_id));
                    }
                }
                Notice::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                Notice::Title(title) => {
                    let next = title.filter(|t| !t.trim().is_empty()).map(Into::into);
                    if self.title != next {
                        self.title = next;
                        cx.emit(TerminalPaneEvent::TitleChanged(self.pane_id));
                    }
                }
            }
        }
        if changed && self.find_open && !self.find_query.trim().is_empty() {
            self.refresh_find_matches(false);
        }
        let blink = self.blink_visible(window, cx);
        if blink != self.blink_on {
            self.blink_on = blink;
            changed = true;
        }
        if changed {
            cx.notify();
        }
        true
    }

    /// Steady unless the program asked for a blinking cursor; reduce motion keeps it steady.
    fn blink_visible(&self, window: &Window, cx: &App) -> bool {
        if !self.blinking
            || cx.reduce_motion()
            || !self.focus.is_focused(window)
            || !window.is_window_active()
        {
            return true;
        }
        (self.last_input.elapsed().as_millis() / BLINK.as_millis()).is_multiple_of(2)
    }

    fn send(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        match (&self.session, &self.status) {
            (Some(session), Status::Running) => {
                {
                    let mut term = session.term.lock();
                    term.selection = None;
                    term.scroll_display(Scroll::Bottom);
                }
                session.write(bytes);
                self.last_input = Instant::now();
                self.scroll_px = 0.;
                cx.notify();
            }
            (None, Status::Starting) if self.pending.len() + bytes.len() <= PENDING_LIMIT => {
                self.pending.extend(bytes);
            }
            _ => {}
        }
    }

    fn mode(&self) -> TermMode {
        self.session.as_ref().map(|s| *s.term.lock().mode()).unwrap_or_default()
    }
}

pub(crate) fn resolve_terminal_palette(terminal_theme: TerminalTheme) -> Palette {
    let dark_fg = Rgb { r: 250, g: 250, b: 250 };
    let dark_bg = Rgb { r: 10, g: 10, b: 10 };
    let theme = match terminal_theme {
        TerminalTheme::AutoCatppuccin | TerminalTheme::CatppuccinLatte => {
            TerminalTheme::CatppuccinMocha
        }
        t => t,
    };
    let resolved = ThemePalette::resolve(
        theme, true, // Always dark mode for terminal
        dark_fg, dark_bg,
    );
    Palette::from_theme(resolved)
}

#[cfg(test)]
#[path = "../../tests/unit/view/mod.rs"]
mod tests;
