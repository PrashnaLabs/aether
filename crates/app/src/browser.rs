//! Browser panel: a native webview (WKWebView on macOS, WebKitGTK on X11
//! Linux, WebView2 on Windows) embedded as a child of the window beside the chat.
//!
//! The webview is native content gpui does not paint, so three rules keep it
//! honest:
//!
//! - Geometry: a canvas under the toolbar reports its laid-out bounds every
//!   frame and the native frame follows, deduplicated so a still frame is free.
//! - Visibility: [`ChatApp`](crate::app::ChatApp) says each frame whether the
//!   panel should be on screen; a hidden panel hides the native view too.
//! - Threading: native callbacks run on the main loop, possibly mid-update, so
//!   they only queue an event and schedule the entity update on the foreground
//!   executor.

use aether_ui::theme::Corners;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use aether_core::home_dir;
use aether_ui::{ChoiceMenu, ChoiceMenuEvent, ChoiceMenuState, ChoiceOption, field};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::ButtonVariants,
    h_flex,
    input::{InputEvent, InputState},
    menu::PopupMenu,
    v_flex,
};
use gpui_kit::{AnyElement, Focusable, prelude::*};
use gpui_kit::{
    App, Bounds, Context, DismissEvent, Entity, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, SharedString, Styled, Subscription, WeakEntity, Window, canvas, div, img, px,
};

use crate::app::{ChatApp, Page, WATCH_STRIP};
use crate::open_in::{self, OpenApp};
use crate::side_panel::SideTab;
use crate::workspace_popover::disabled_label;

/// What the address field resolves to when submitted.
#[derive(Debug, PartialEq, Eq)]
enum AddressTarget {
    Url(String),
    Search(String),
}

/// Browser-style omnibox: `http(s)` and `about:blank` pass through, host-like text
/// gets a scheme guessed for it, anything else (including `file:`, `data:`,
/// `mailto:`, `javascript:`) becomes a web search so the webview never loads it.
fn resolve_address(raw: &str) -> Option<AddressTarget> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let has_scheme = trimmed.split_once(':').is_some_and(|(scheme, rest)| {
        scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            && (scheme.eq_ignore_ascii_case("http")
                || scheme.eq_ignore_ascii_case("https")
                || scheme.eq_ignore_ascii_case("about"))
            && (rest.starts_with("//") || scheme.eq_ignore_ascii_case("about"))
    });
    if has_scheme {
        return Some(AddressTarget::Url(trimmed.to_owned()));
    }
    if trimmed.contains(char::is_whitespace) {
        return Some(AddressTarget::Search(trimmed.to_owned()));
    }

    let authority = trimmed.split(['/', '?', '#']).next().unwrap_or(trimmed);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, true)
        }
        Some(_) => return Some(AddressTarget::Search(trimmed.to_owned())),
        None => (authority, false),
    };
    let is_ip = !host.is_empty()
        && host.chars().all(|c| c.is_ascii_digit() || c == '.')
        && host.split('.').count() == 4;
    let is_local = host.eq_ignore_ascii_case("localhost") || is_ip;
    let host_like = is_local
        || (host.contains('.')
            && !host.starts_with('.')
            && !host.ends_with('.')
            && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')));
    if !host_like {
        return Some(AddressTarget::Search(trimmed.to_owned()));
    }
    // Dev servers rarely speak TLS; the public web rarely speaks anything else.
    let scheme =
        if is_local || (port && host.eq_ignore_ascii_case("localhost")) { "http" } else { "https" };
    Some(AddressTarget::Url(format!("{scheme}://{trimmed}")))
}

/// DevTools only in debug builds; wry enables them there without the
/// `devtools` feature, and the toolbar button is a no-op in release.
fn devtools_enabled() -> bool {
    cfg!(debug_assertions)
}

fn search_url(query: &str) -> String {
    let mut encoded = String::with_capacity(query.len() * 3);
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    format!("https://www.google.com/search?q={encoded}")
}

/// The address bar hides `https://`; everything else, `http://` included, stays visible.
fn display_url(url: &str) -> &str {
    url.strip_prefix("https://").unwrap_or(url)
}

/// Urls the webview may load: web pages and the blank document only.
pub(crate) fn navigable(url: &str) -> bool {
    open_in::web_url(url) || url.eq_ignore_ascii_case("about:blank")
}

/// What the page told us, queued by native callbacks.
enum Native {
    Started(String),
    Finished(String),
    Title(String),
    /// `window.open` and `target=_blank`: opened in this same view.
    NewWindow(String),
    CycleTab(bool),
}

pub(crate) struct CycleBrowserTab(pub bool);

fn tab_shortcut_message(message: &str) -> Option<bool> {
    match message {
        "aether:previous-tab" => Some(true),
        "aether:next-tab" => Some(false),
        _ => None,
    }
}

/// The platform webview behind a uniform surface.
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
mod host {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    use gpui_kit::{Bounds, Pixels, Window};
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::{NewWindowResponse, PageLoadEvent, WebViewBuilder};

    use super::Native;

    /// Mirror Safari's UA so sites serve the webview their real desktop build.
    #[cfg(not(windows))]
    const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
         AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15";

    pub(super) struct Host {
        webview: wry::WebView,
        #[cfg(target_os = "macos")]
        container: objc2::rc::Retained<objc2_app_kit::NSView>,
        last_bounds: Cell<Option<(i32, i32, i32, i32)>>,
        visible: Cell<bool>,
    }

    impl Host {
        /// `notify` is called after each event is queued, to schedule a drain.
        pub fn create(
            window: &Window,
            url: &str,
            queue: Rc<RefCell<VecDeque<Native>>>,
            notify: Rc<dyn Fn()>,
        ) -> Result<Self, String> {
            #[cfg(target_os = "linux")]
            gtk::init().map_err(|e| format!("GTK is unavailable: {e}"))?;

            let push = {
                let (queue, notify) = (queue.clone(), notify.clone());
                move |event: Native| {
                    queue.borrow_mut().push_back(event);
                    notify();
                }
            };
            let (on_load, on_title, on_window, on_shortcut) =
                (push.clone(), push.clone(), push.clone(), push);
            let modifier = if cfg!(target_os = "macos") {
                "e.metaKey && !e.ctrlKey"
            } else {
                "e.ctrlKey && !e.metaKey"
            };
            let shortcuts = format!(
                r#"
                document.addEventListener('keydown', e => {{
                    if (!e.isTrusted || !({modifier}) || !e.shiftKey || e.altKey) return;
                    const key = e.key.toLowerCase();
                    if (key !== 'h' && key !== 'l') return;
                    e.preventDefault();
                    e.stopImmediatePropagation();
                    window.ipc.postMessage(key === 'h' ? 'aether:previous-tab' : 'aether:next-tab');
                }}, true);
            "#
            );
            let builder = WebViewBuilder::new();
            // WebView2 already sends Edge's own UA
            #[cfg(not(windows))]
            let builder = builder.with_user_agent(USER_AGENT);
            let webview = builder
                .with_bounds(wry::Rect {
                    position: LogicalPosition::new(0.0, 0.0).into(),
                    size: LogicalSize::new(0.0, 0.0).into(),
                })
                .with_visible(false)
                .with_url(url)
                .with_devtools(super::devtools_enabled())
                .with_accept_first_mouse(true)
                // Native page focus bypasses GPUI's keymap; forward only tab cycling.
                .with_initialization_script_for_main_only(shortcuts, false)
                .with_ipc_handler(move |request| {
                    if let Some(previous) = super::tab_shortcut_message(request.body()) {
                        on_shortcut(Native::CycleTab(previous));
                    }
                })
                .with_on_page_load_handler(move |event, url| {
                    on_load(match event {
                        PageLoadEvent::Started => Native::Started(url),
                        PageLoadEvent::Finished => Native::Finished(url),
                    })
                })
                .with_document_title_changed_handler(move |title| on_title(Native::Title(title)))
                .with_new_window_req_handler(move |url, _| {
                    on_window(Native::NewWindow(url));
                    NewWindowResponse::Deny
                })
                .build_as_child(window)
                .map_err(|e| format!("Couldn't start the web view: {e}"))?;

            #[cfg(target_os = "macos")]
            #[allow(unsafe_code)]
            {
                use objc2::rc::Retained;
                use objc2::runtime::AnyObject;
                use wry::WebViewExtMacOS;

                let wk = webview.webview();
                unsafe {
                    let tool: Option<Retained<AnyObject>> = objc2::msg_send![&wk, _inspector];
                    if let Some(tool) = tool {
                        let () = objc2::msg_send![&tool, detach];
                    }
                }
            }

            #[cfg(target_os = "macos")]
            #[allow(unsafe_code)]
            let container = {
                use objc2::{MainThreadMarker, MainThreadOnly};
                use objc2_app_kit::NSView;
                use wry::WebViewExtMacOS;
                let wk = webview.webview();
                let container = NSView::initWithFrame(
                    NSView::alloc(MainThreadMarker::new().expect("browser on main thread")),
                    objc2_foundation::NSRect::ZERO,
                );
                unsafe {
                    let parent = wk.superview().expect("webview parent");
                    container.setClipsToBounds(true);
                    parent.addSubview(&container);
                    wk.removeFromSuperview();
                    container.addSubview(&wk);
                }
                container
            };
            Ok(Self {
                webview,
                #[cfg(target_os = "macos")]
                container,
                last_bounds: Cell::new(None),
                visible: Cell::new(false),
            })
        }

        /// gpui's window coordinates are top-left-origin logical points, which
        /// is wry's child-bounds convention. Each edge is rounded, not origin
        /// plus size, so every side stays within half a point of the layout
        /// rect and a panel drag can't expose a sliver of background.
        pub fn set_bounds(&self, bounds: Bounds<Pixels>) {
            let left = f32::from(bounds.origin.x).round() as i32;
            let top = f32::from(bounds.origin.y).round() as i32;
            let right = f32::from(bounds.origin.x + bounds.size.width).round() as i32;
            let bottom = f32::from(bounds.origin.y + bounds.size.height).round() as i32;
            let width = (right - left).max(0);
            let height = (bottom - top).max(0);
            if self.last_bounds.get() == Some((left, top, right, bottom)) {
                return;
            }
            self.last_bounds.set(Some((left, top, right, bottom)));
            #[cfg(target_os = "macos")]
            #[allow(unsafe_code)]
            unsafe {
                use objc2_foundation::{NSPoint, NSRect, NSSize};
                let parent = self.container.superview().expect("browser container parent");
                self.container.setFrame(NSRect::new(
                    NSPoint::new(left as f64, parent.bounds().size.height - bottom as f64),
                    NSSize::new(width as f64, height as f64),
                ));
            }
            let _ = self.webview.set_bounds(wry::Rect {
                position: {
                    #[cfg(target_os = "macos")]
                    {
                        LogicalPosition::new(0.0, 0.0).into()
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        LogicalPosition::new(f64::from(left), f64::from(top)).into()
                    }
                },
                size: LogicalSize::new(f64::from(width), f64::from(height)).into(),
            });
        }

        #[cfg(target_os = "linux")]
        pub fn visible(&self) -> bool {
            self.visible.get()
        }

        pub fn set_visible(&self, visible: bool) {
            if self.visible.replace(visible) != visible {
                if !visible {
                    // Hiding a WKWebView does not resign its native first responder.
                    self.focus_parent();
                }
                let _ = self.webview.set_visible(visible);
                #[cfg(target_os = "macos")]
                {
                    // The containing NSView must also stop intercepting input.
                    self.container.setHidden(!visible);
                }
            }
        }

        pub fn load(&self, url: &str) {
            let _ = self.webview.load_url(url);
        }

        pub fn reload(&self) {
            let _ = self.webview.reload();
        }

        pub fn eval(&self, js: &str) {
            let _ = self.webview.evaluate_script(js);
        }

        pub fn devtools(&self) {
            if !super::devtools_enabled() {
                return;
            }
            #[cfg(target_os = "macos")]
            #[allow(unsafe_code)]
            {
                use objc2::rc::Retained;
                use objc2::runtime::AnyObject;
                use wry::WebViewExtMacOS;

                let wk = self.webview.webview();
                unsafe {
                    let tool: Option<Retained<AnyObject>> = objc2::msg_send![&wk, _inspector];
                    if let Some(tool) = tool {
                        let is_visible: bool = objc2::msg_send![&tool, isVisible];
                        if is_visible {
                            let () = objc2::msg_send![&tool, close];
                        } else {
                            let () = objc2::msg_send![&tool, detach];
                            let () = objc2::msg_send![&tool, show];
                            let () = objc2::msg_send![&tool, detach];
                        }
                    }
                }
                self.last_bounds.set(None);
            }
            #[cfg(all(not(target_os = "macos"), debug_assertions))]
            {
                self.webview.open_devtools();
            }
        }

        pub fn focus(&self) {
            let _ = self.webview.focus();
        }

        pub fn focus_parent(&self) {
            let _ = self.webview.focus_parent();
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod host {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use gpui_kit::{Bounds, Pixels, Window};

    use super::Native;

    pub(super) struct Host;

    impl Host {
        pub fn create(
            _: &Window,
            _: &str,
            _: Rc<RefCell<VecDeque<Native>>>,
            _: Rc<dyn Fn()>,
        ) -> Result<Self, String> {
            Err("The browser panel isn't available on this platform yet.".into())
        }
        pub fn set_bounds(&self, _: Bounds<Pixels>) {}
        pub fn set_visible(&self, _: bool) {}
        pub fn load(&self, _: &str) {}
        pub fn reload(&self) {}
        pub fn eval(&self, _: &str) {}
        pub fn devtools(&self) {}
        pub fn focus(&self) {}
        pub fn focus_parent(&self) {}
    }
}

pub(crate) struct Browser {
    address: Entity<InputState>,
    host: Option<Rc<host::Host>>,
    queue: Rc<RefCell<VecDeque<Native>>>,
    /// Page to open once the view exists; it is made on first navigation.
    pending: Option<String>,
    url: String,
    title: String,
    loading: bool,
    /// Why the native view couldn't be made, shown in its place.
    error: Option<String>,
    /// Whether the owner wants the view on screen this frame.
    shown: bool,
    /// The watch list or peek is open over the window.
    pub(crate) covered: bool,
    /// Keep the bottom strip clear for the watch-list button, which a native view would cover.
    pub(crate) watch_strip: bool,
    /// Toolbar sits under the macOS traffic lights.
    pub(crate) lights: bool,
    /// Toolbar ends under the window controls.
    pub(crate) controls: bool,
    browsers: Vec<OpenApp>,
    browsers_found: bool,
    selected_browser: Option<usize>,
    open_menu: Entity<ChoiceMenuState>,
    covering_menu: Option<(WeakEntity<PopupMenu>, Subscription)>,
    open_error: Option<SharedString>,
    /// Leading toolbar button that leaves the page, e.g. back to Hacker News stories.
    home: Option<(Lucide, SharedString, Rc<dyn Fn(&mut Window, &mut App)>)>,
    _subscriptions: Vec<Subscription>,
}

impl gpui_kit::EventEmitter<CycleBrowserTab> for Browser {}

impl Browser {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let address =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or enter address"));
        let subscription = cx.subscribe_in(
            &address,
            window,
            |this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    let raw = input.read(cx).value().to_string();
                    this.go(&raw, cx);
                    // Native webview focus does not clear GPUI's focused input.
                    window.blur(cx);
                    if let Some(host) = &this.host {
                        host.focus();
                    }
                }
                InputEvent::Focus => {
                    if let Some(host) = &this.host {
                        host.focus_parent();
                    }
                }
                _ => {}
            },
        );
        let open_menu = cx.new(|cx| ChoiceMenuState::new(cx).below());
        let pick = cx.subscribe(&open_menu, |this, _, ev: &ChoiceMenuEvent, cx| {
            let ChoiceMenuEvent::Pick(ix) = ev;
            if this.browsers.get(*ix).is_some() {
                this.selected_browser = Some(*ix);
                this.open_error = None;
                cx.notify();
            }
        });
        let cache = open_in::icon_cache();
        cx.spawn(async move |this, cx| {
            let browsers = cx
                .background_executor()
                .spawn(async move {
                    let mut apps = open_in::discover(&open_in::BROWSERS, &home_dir());
                    let icons = open_in::icons(&apps, &cache);
                    for (app, icon) in apps.iter_mut().zip(icons) {
                        app.icon = icon;
                    }
                    apps
                })
                .await;
            this.update(cx, |this, cx| {
                this.browsers = browsers;
                this.selected_browser = (!this.browsers.is_empty()).then_some(0);
                this.browsers_found = true;
                cx.notify();
            })
            .ok();
        })
        .detach();
        Self {
            address,
            host: None,
            queue: Rc::default(),
            pending: None,
            url: String::new(),
            title: String::new(),
            loading: false,
            error: None,
            shown: false,
            covered: false,
            lights: false,
            controls: false,
            watch_strip: true,
            browsers: Vec::new(),
            browsers_found: false,
            selected_browser: None,
            open_menu,
            covering_menu: None,
            open_error: None,
            home: None,
            _subscriptions: vec![subscription, pick],
        }
    }

    /// Opens what the user typed: a URL, a host, or a search.
    pub(crate) fn go(&mut self, raw: &str, cx: &mut Context<Self>) {
        let url = match resolve_address(raw) {
            Some(AddressTarget::Url(url)) => url,
            Some(AddressTarget::Search(query)) => search_url(&query),
            None => return,
        };
        self.open_url(url, cx);
    }

    fn open_url(&mut self, url: String, cx: &mut Context<Self>) {
        if !navigable(&url) {
            self.open_error = Some("Only http and https pages can open here".into());
            cx.notify();
            return;
        }
        self.url = url.clone();
        self.loading = true;
        self.open_error = None;
        match &self.host {
            Some(host) => host.load(&url),
            None => self.pending = Some(url),
        }
        cx.notify();
    }

    /// Opens the current page in `self.browsers[ix]`.
    fn open_in(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(bundle) = self.browsers.get(ix).map(|b| b.bundle.clone()) else {
            return;
        };
        let url = self.url.clone();
        self.open_error = None;
        let task = cx.background_executor().spawn(async move { open_in::open_url(&bundle, &url) });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                this.update(cx, |this, cx| {
                    this.open_error = Some(format!("Couldn't open: {e}").into());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }

    /// Called by the owner every frame: whether the panel is on screen.
    pub(crate) fn set_shown(&mut self, shown: bool) {
        if self.shown != shown {
            self.shown = shown;
            // Showing is deferred to sync so a panel refresh cannot expose
            // the native view over a menu that is still open.
            if !shown && let Some(host) = &self.host {
                host.set_visible(false);
            }
        }
    }

    pub(crate) fn focus_address(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.address.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(crate) fn native_visible(&self, cx: &gpui_kit::App) -> bool {
        self.shown
            && !self.covered
            && !self.open_menu.read(cx).is_open()
            && !self.covering_menu.as_ref().is_some_and(|(menu, _)| menu.upgrade().is_some())
    }

    pub(crate) fn set_covered(&mut self, covered: bool, cx: &mut Context<Self>) {
        if self.covered == covered {
            return;
        }
        self.covered = covered;
        if let Some(host) = &self.host {
            host.set_visible(self.native_visible(cx));
        }
        // the root render calls this, and gpui drops a notify made mid-draw
        let view = cx.entity();
        cx.defer(move |cx| view.update(cx, |_, cx| cx.notify()));
    }

    /// Native child views paint above GPUI popups. Yield until this menu is
    /// dismissed; a weak reference also handles a trigger being removed.
    pub(crate) fn hide_for_menu(&mut self, menu: &Entity<PopupMenu>, cx: &mut Context<Self>) {
        let dismissed = cx.subscribe(menu, |this, _, _: &DismissEvent, cx| {
            this.covering_menu = None;
            cx.notify();
        });
        self.covering_menu = Some((menu.downgrade(), dismissed));
        if let Some(host) = &self.host {
            host.set_visible(false);
        }
        cx.notify();
    }

    /// Follows the content area's laid-out bounds, making the view on first use.
    fn sync(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        // Native content must yield to menus painted by GPUI.
        let visible = self.native_visible(cx);
        if self.host.is_none() && self.error.is_none() {
            let Some(url) = self.pending.take() else { return };
            let (this, async_cx, executor) =
                (cx.weak_entity(), cx.to_async(), cx.foreground_executor().clone());
            let notify: Rc<dyn Fn()> = Rc::new(move || {
                let (this, mut async_cx) = (this.clone(), async_cx.clone());
                executor
                    .spawn(async move {
                        let _ = this.update(&mut async_cx, |browser, cx| browser.drain(cx));
                    })
                    .detach();
            });
            match host::Host::create(window, &url, self.queue.clone(), notify) {
                Ok(host) => {
                    host.set_bounds(bounds);
                    host.set_visible(visible);
                    self.host = Some(Rc::new(host));
                    #[cfg(target_os = "linux")]
                    self.pump_gtk(cx);
                }
                Err(error) => {
                    self.error = Some(error);
                    self.loading = false;
                    cx.notify();
                }
            }
        }
        if let Some(host) = &self.host {
            host.set_bounds(bounds);
            host.set_visible(visible);
        }
    }

    /// GTK has no loop of its own here; turn it by hand so pages render, slower while hidden.
    #[cfg(target_os = "linux")]
    fn pump_gtk(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut shown = true;
            loop {
                let ms = if shown { 16 } else { 250 };
                cx.background_executor().timer(std::time::Duration::from_millis(ms)).await;
                let Ok(visible) =
                    this.update(cx, |b, _| b.host.as_ref().is_some_and(|h| h.visible()))
                else {
                    break;
                };
                shown = visible;
                while gtk::events_pending() {
                    gtk::main_iteration_do(false);
                }
            }
        })
        .detach();
    }

    fn drain(&mut self, cx: &mut Context<Self>) {
        let events: Vec<Native> = self.queue.borrow_mut().drain(..).collect();
        for event in events {
            match event {
                Native::Started(url) => {
                    self.loading = true;
                    self.set_url(url, cx);
                }
                Native::Finished(url) => {
                    self.loading = false;
                    self.set_url(url, cx);
                }
                Native::Title(title) => self.title = title,
                Native::NewWindow(url) => self.open_url(url, cx),
                Native::CycleTab(previous) => {
                    if self.native_visible(cx) {
                        cx.emit(CycleBrowserTab(previous));
                    }
                }
            }
        }
        cx.notify();
    }

    fn set_url(&mut self, url: String, cx: &mut Context<Self>) {
        if url == "about:blank" || url == self.url {
            return;
        }
        let _ = cx;
        self.url = url;
    }

    /// Mirrors the page's address into the field unless the user is typing.
    fn show_url(&self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = display_url(&self.url).to_string();
        let typing = self.address.read(cx).focus_handle(cx).is_focused(window);
        if !typing && self.address.read(cx).value().as_ref() != shown {
            self.address.update(cx, |input, cx| input.set_value(shown, window, cx));
        }
    }

    /// Drops the page and its native view, back to the empty start state.
    fn unload(&mut self, cx: &mut Context<Self>) {
        self.host = None;
        self.queue.borrow_mut().clear();
        self.pending = None;
        self.url.clear();
        self.title.clear();
        self.loading = false;
        self.error = None;
        self.open_error = None;
        cx.notify();
    }

    fn script(&self, js: &str) {
        if let Some(host) = &self.host {
            host.eval(js);
        }
    }
}

impl Browser {
    pub(crate) fn home(
        &mut self,
        icon: Lucide,
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) {
        self.home = Some((icon, label.into(), Rc::new(on_click)));
    }
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.show_url(window, cx);
        let lights = aether_ui::traffic_lights_gap(self.lights, window, cx);
        let theme = cx.theme();
        let (border, muted, bg, danger) = (
            theme.sidebar_border,
            theme.muted_foreground,
            theme.sidebar,
            aether_ui::theme::danger_text(cx),
        );
        let ready = self.host.is_some();
        let view = cx.entity();
        let placeholder = match (&self.error, ready || self.pending.is_some()) {
            (Some(error), _) => Some(error.clone()),
            (None, false) => Some("Enter an address above to start browsing.".to_string()),
            _ => None,
        };
        let options = self
            .browsers
            .iter()
            .enumerate()
            .map(|(ix, b)| ChoiceOption {
                mark: None,
                icon: Some(Lucide::Globe),
                image: b.icon.clone(),
                title: b.name.into(),
                description: None,
                checked: self.selected_browser == Some(ix),
                disabled: false,
            })
            .collect();
        self.open_menu.update(cx, |m, cx| m.sync(options, None, 220., cx));
        let selected = self.selected_browser.and_then(|ix| self.browsers.get(ix));
        let open_label: SharedString =
            selected.map_or("Open in browser".into(), |b| format!("Open in {}", b.name).into());
        let browsers_off = if !self.browsers_found {
            Some("Finding browsers\u{2026}")
        } else if self.browsers.is_empty() {
            Some("No browsers found")
        } else {
            None
        };
        let open_off =
            browsers_off.or_else(|| (!open_in::web_url(&self.url)).then_some("No web page loaded"));
        let mark = match selected.and_then(|b| b.icon.clone()) {
            Some(path) => img(path).size(px(16.)).flex_none().into_any_element(),
            None => Icon::new(Lucide::Globe).small().into_any_element(),
        };
        let open_in = aether_ui::button("browser-open-in")
            .ghost()
            .small()
            .px(px(7.))
            .corner_l(px(6.))
            .corner_r(px(0.))
            .child(mark)
            .accessibility_label(
                open_off.map_or(open_label.to_string(), |r| disabled_label(&open_label, r)),
            )
            .tooltip(open_off.map_or(open_label, SharedString::from))
            .disabled(open_off.is_some())
            .when_some(self.selected_browser, |b, ix| {
                b.on_click(cx.listener(move |this, _, _, cx| this.open_in(ix, cx)))
            });
        let choose_label = selected.map_or("Choose browser".to_string(), |b| {
            format!("Choose browser, current: {}", b.name)
        });
        let caret = aether_ui::button("browser-open-in-menu")
            .ghost()
            .small()
            .px(px(4.))
            .corner_l(px(0.))
            .corner_r(px(6.))
            .icon(IconName::ChevronDown)
            .accessibility_label(
                browsers_off.map_or(choose_label.clone(), |r| disabled_label(&choose_label, r)),
            )
            .tooltip(browsers_off.unwrap_or("Choose browser"))
            .disabled(browsers_off.is_some());
        let open_control = h_flex()
            .flex_none()
            .items_center()
            .corner(px(6.))
            .border_1()
            .border_color(border)
            .child(open_in)
            .child(div().w(px(1.)).h(px(16.)).flex_none().bg(border))
            .child(ChoiceMenu::new(&self.open_menu, caret));
        let toolbar = h_flex()
            .h(px(42.))
            .flex_none()
            .px_2()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(lights)
            .child(match self.home.clone() {
                Some((icon, label, on_click)) => aether_ui::button("browser-home")
                    .ghost()
                    .small()
                    .icon(icon)
                    .accessibility_label(label.clone())
                    .tooltip(label)
                    .on_click(move |_, window, cx| on_click(window, cx)),
                None => aether_ui::button("browser-home")
                    .ghost()
                    .small()
                    .icon(Lucide::House)
                    .accessibility_label("Home, closes the page")
                    .tooltip("Home")
                    .on_click(cx.listener(|this, _, _, cx| this.unload(cx))),
            })
            .child(
                aether_ui::button("browser-back")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowLeft)
                    .accessibility_label("Back")
                    .tooltip("Back")
                    .on_click(cx.listener(|this, _, _, _| this.script("history.back()"))),
            )
            .child(
                aether_ui::button("browser-forward")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowRight)
                    .accessibility_label("Forward")
                    .tooltip("Forward")
                    .on_click(cx.listener(|this, _, _, _| this.script("history.forward()"))),
            )
            .child(if self.loading {
                aether_ui::button("browser-stop")
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .accessibility_label("Stop loading")
                    .tooltip("Stop")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.script("window.stop()");
                        this.loading = false;
                        cx.notify();
                    }))
            } else {
                aether_ui::button("browser-reload")
                    .ghost()
                    .small()
                    .icon(IconName::RotateCw)
                    .accessibility_label("Reload")
                    .tooltip("Reload")
                    .on_click(cx.listener(|this, _, _, _| {
                        if let Some(host) = &this.host {
                            host.reload();
                        }
                    }))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(field(&self.address).small().aria_label("Search or enter address")),
            )
            .when(!self.url.is_empty(), |toolbar| {
                toolbar.child(
                    gpui_kit::component::clipboard::Clipboard::new("browser-copy")
                        .small()
                        .value(self.url.clone())
                        .accessibility_label("Copy page address")
                        .tooltip("Copy address"),
                )
            })
            .when(cfg!(any(target_os = "macos", windows)), |t| t.child(open_control))
            .child(
                aether_ui::button("browser-devtools")
                    .ghost()
                    .small()
                    .icon(IconName::Settings)
                    .accessibility_label("Open developer tools")
                    .tooltip("Developer tools")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(host) = &this.host {
                            host.devtools();
                            cx.notify();
                        }
                    })),
            )
            .when(self.controls, |t| t.child(aether_ui::window_controls_gap(window)));
        let host = self.host.clone();
        let open_error = self.open_error.clone().map(|e| {
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(danger)
                .border_b_1()
                .border_color(border)
                .child(e)
        });
        v_flex()
            .id("browser")
            .size_full()
            .bg(bg)
            .on_mouse_down_out(cx.listener(|this, _, _, _| {
                if let Some(host) = &this.host {
                    host.focus_parent();
                }
            }))
            .child(toolbar)
            .children(open_error)
            .child(
                div()
                    // clear of the panel's resize band, which a native view would cover
                    .ml(px(5.))
                    .mb(if self.watch_strip { WATCH_STRIP } else { px(0.) })
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                if let Some(host) = &host {
                                    host.set_bounds(bounds);
                                }
                                view.update(cx, |browser, cx| browser.sync(bounds, window, cx))
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .children(placeholder.map(|text| {
                        div()
                            .absolute()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .p_4()
                            .text_sm()
                            .text_color(muted)
                            .child(text)
                    })),
            )
    }
}

impl ChatApp {
    /// Cmd+Shift+B: opens the browser tab, or closes the panel when it is
    /// already the one showing.
    pub(crate) fn toggle_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != Page::Chat {
            return;
        }
        let id = self.chats[self.active].id;
        if self.browser_open
            && self.side_tab_in(window) == SideTab::Browser
            && self.git.open.contains(&id)
        {
            self.focus_composer(window, cx);
        } else {
            self.open_browser_tab(window, cx);
        }
        cx.notify();
    }

    /// Adds the browser tab if needed and shows it.
    pub(crate) fn open_browser_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.browser_open = true;
        let host = self.host_of(window);
        self.set_side_tab(host, SideTab::Browser);
        self.git.open.insert(self.chats[self.active].id);
        self.browser.update(cx, |browser, cx| browser.focus_address(window, cx));
        cx.notify();
    }

    /// Removes the browser tab, falling back to changes if it was showing.
    pub(crate) fn close_browser_tab(&mut self, cx: &mut Context<Self>) {
        self.browser.update(cx, |browser, _| browser.set_shown(false));
        self.browser_open = false;
        if self.side_tab == SideTab::Browser {
            self.side_tab = SideTab::Agent;
        }
        cx.notify();
    }

    /// Browser tab content. Also tells the webview whether it may be on
    /// screen: not under the command palette, not on another page, not behind
    /// another tab or a closed panel.
    pub(crate) fn browser_content(&mut self, shown: bool, cx: &mut Context<Self>) -> AnyElement {
        let shown = shown && self.draft_editor.is_none();
        self.browser.update(cx, |browser, _| browser.set_shown(shown));
        div().flex_1().min_h_0().w_full().child(self.browser.clone()).into_any_element()
    }
}

#[cfg(test)]
#[path = "../tests/unit/browser.rs"]
mod tests;
