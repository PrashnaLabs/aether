use super::*;
use crate::app::ChatApp;
use gpui_kit::{
    AppContext, Bounds, Context, Entity, Render, TitlebarOptions, Window, WindowBounds,
    WindowOptions, prelude::*, px, size,
};

struct DetachedWorkspace {
    app: Entity<ChatApp>,
    host: WindowId,
    _subscription: gpui_kit::Subscription,
    _bounds_subscription: gpui_kit::Subscription,
}
impl Render for DetachedWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.app.update(cx, |app, cx| {
            crate::app::apply_font_size(app.settings.font_size, window);
            #[cfg(target_os = "macos")]
            app.sync_touch_bar(self.host, window, cx);
            let body = app.render_session_workspace(self.host, window, cx);
            let body = match app.fullscreen_terminal(window, cx) {
                Some(view) => aether_terminal::dark(
                    view.cached(gpui_kit::StyleRefinement::default().size_full()),
                    cx,
                )
                .into_any_element(),
                None => body,
            };
            let viewer = app.image_viewer_overlay(window, cx);
            gpui_kit::div().relative().size_full().child(body).children(viewer).into_any_element()
        })
    }
}
impl ChatApp {
    pub(crate) fn restore_session_windows(&mut self, cx: &mut Context<Self>) {
        let hosts: Vec<_> = self
            .workspace
            .model
            .windows
            .iter()
            .filter(|(id, w)| **id != 0 && !w.panes().is_empty())
            .map(|(id, _)| *id)
            .collect();
        let app = cx.entity();
        cx.defer(move |cx| {
            for host in hosts {
                match open_secondary(app.clone(), host, cx) {
                    Ok(handle) => {
                        let chats = app.update(cx, |app, _| {
                            app.workspace.windows.insert(host, handle);
                            app.workspace
                                .model
                                .windows
                                .get(&host)
                                .map(|w| w.panes())
                                .unwrap_or_default()
                                .iter()
                                .filter_map(|p| app.workspace.model.panes.get(p).copied())
                                .collect::<Vec<_>>()
                        });
                        let _ = handle.update(cx, |_, window, cx| {
                            app.update(cx, |app, cx| {
                                for chat in chats {
                                    app.rebind_session_ui(chat, window, cx);
                                }
                                cx.notify();
                            })
                        });
                    }
                    Err(error) => app.update(cx, |app, cx| {
                        app.workspace.error =
                            Some(format!("Couldn't restore session window: {error}"));
                        app.return_session_window(host, cx);
                        cx.notify();
                    }),
                }
            }
        });
    }
    pub(crate) fn detach_session(
        &mut self,
        pane: PaneId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.workspace.model.panes.get(&pane).copied() else { return };
        let Some(source) = self.workspace.model.window_of(pane) else { return };
        let app = cx.entity();
        let reusable = self
            .workspace
            .windows
            .iter()
            .find(|(id, _)| {
                **id != 0
                    && self.workspace.model.windows.get(id).is_none_or(|w| w.panes().is_empty())
            })
            .map(|(id, w)| (*id, *w));
        let host = reusable.map(|(id, _)| id).unwrap_or_else(|| self.workspace.model.alloc());
        let Ok(ticket) = self.workspace.model.prepare_transfer(pane, host) else { return };
        cx.defer(move |cx| {
            if app.read(cx).workspace.model.panes.get(&pane) != Some(&chat)
                || app.read(cx).workspace.model.window_of(pane) != Some(source)
            {
                return;
            }
            let existing = reusable.map(|(_, w)| w);
            let opened = if let Some(handle) = existing {
                Ok(handle)
            } else {
                open_secondary(app.clone(), host, cx)
            };
            match opened {
                Ok(handle) => {
                    let committed = app.update(cx, |app, cx| {
                        app.workspace.windows.insert(host, handle);
                        app.side_tabs.remove(&host);
                        if app.workspace.model.commit_transfer(ticket).is_err() {
                            return false;
                        }
                        app.workspace.views.remove(&pane);
                        cx.notify();
                        true
                    });
                    let _ = handle.update(cx, |_, window, cx| {
                        if committed {
                            app.update(cx, |app, cx| app.rebind_session_ui(chat, window, cx));
                            window.activate_window();
                        } else {
                            retain_closed_window(window);
                        }
                    });
                }
                Err(error) => app.update(cx, |app, cx| {
                    app.workspace.error = Some(format!("Couldn't open session window: {error}"));
                    cx.notify();
                }),
            }
        });
    }
    pub(crate) fn return_session(
        &mut self,
        pane: PaneId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.workspace.model.panes.get(&pane).copied() else { return };
        let Some(source) = self.workspace.model.window_of(pane).filter(|host| *host != 0) else {
            return;
        };
        let Ok(ticket) = self.workspace.model.prepare_transfer(pane, 0) else { return };
        let app = cx.entity();
        let main = self.workspace.windows.get(&0).copied();
        let secondary = self.workspace.windows.get(&source).copied();
        cx.defer(move |cx| {
            if let Some(main) = main {
                let _ = main.update(cx, |_, window, cx| {
                    let empty = app.update(cx, |app, cx| {
                        if app.workspace.model.commit_transfer(ticket).is_err() {
                            return false;
                        }
                        app.workspace.views.remove(&pane);
                        app.rebind_session_ui(chat, window, cx);
                        cx.notify();
                        app.workspace
                            .model
                            .windows
                            .get(&source)
                            .is_none_or(|w| w.panes().is_empty())
                    });
                    if empty {
                        app.update(cx, |app, _| app.side_tabs.remove(&source));
                    }
                    if empty && let Some(secondary) = secondary {
                        let _ = secondary.update(cx, |_, window, _| retain_closed_window(window));
                    }
                    window.activate_window();
                });
            }
        });
    }
    pub(crate) fn return_session_window(&mut self, host: WindowId, cx: &mut Context<Self>) {
        let panes = self.workspace.model.windows.get(&host).map(|w| w.panes()).unwrap_or_default();
        let app = cx.entity();
        let main = self.workspace.windows.get(&0).copied();
        cx.defer(move |cx| {
            if let Some(main) = main {
                let _ = main.update(cx, |_, window, cx| {
                    app.update(cx, |app, cx| {
                        for pane in panes {
                            if app.workspace.model.window_of(pane) != Some(host) {
                                continue;
                            }
                            let Some(chat) = app.workspace.model.panes.get(&pane).copied() else {
                                continue;
                            };
                            let _ = app.workspace.model.transfer(pane, 0);
                            app.workspace.views.remove(&pane);
                            app.rebind_session_ui(chat, window, cx);
                        }
                        app.side_tabs.remove(&host);
                        cx.notify();
                    });
                    window.activate_window();
                });
            }
        });
    }
}

fn open_secondary(
    app: Entity<ChatApp>,
    host: WindowId,
    cx: &mut gpui_kit::App,
) -> Result<gpui_kit::AnyWindowHandle, String> {
    let root_app = app.clone();
    let bounds = app
        .read(cx)
        .workspace
        .model
        .windows
        .get(&host)
        .and_then(|w| w.bounds)
        .map(|saved| saved_window_bounds(saved, cx))
        .unwrap_or_else(|| Bounds::centered(None, size(px(900.), px(700.)), cx));
    gpui_kit::open_window(
        WindowOptions {
            app_id: Some("dev.aether.Aether".into()),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            icon: Some(std::sync::Arc::new(
                image::load_from_memory(include_bytes!("../../assets/app-icons/256.png"))
                    .expect("decode app icon")
                    .into_rgba8(),
            )),
            titlebar: Some(TitlebarOptions {
                title: Some("Aether · Sessions".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        cx,
        move |window, cx| {
            let settings = &app.read(cx).settings;
            window.set_background_appearance(aether_ui::theme::window_background(
                settings.clamped_window_opacity(),
                settings.window_blur,
            ));
            window.on_window_should_close(cx, move |window, cx| {
                app.update(cx, |app, cx| app.return_session_window(host, cx));
                retain_closed_window(window);
                false
            });
            cx.new(|cx| {
                let subscription = cx.observe(&root_app, |_, _, cx| cx.notify());
                let observed = root_app.clone();
                let bounds_subscription = cx.observe_window_bounds(window, move |_, window, cx| {
                    let bounds = window.window_bounds().get_bounds();
                    observed.update(cx, |app, cx| {
                        if let Some(layout) = app.workspace.model.windows.get_mut(&host) {
                            layout.bounds = Some(Rect::new(
                                f32::from(bounds.origin.x),
                                f32::from(bounds.origin.y),
                                f32::from(bounds.size.width),
                                f32::from(bounds.size.height),
                            ));
                        }
                        app.schedule_workspace_save(cx);
                    });
                });
                DetachedWorkspace {
                    app: root_app,
                    host,
                    _subscription: subscription,
                    _bounds_subscription: bounds_subscription,
                }
            })
        },
    )
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

pub(crate) fn saved_window_bounds(saved: Rect, cx: &gpui_kit::App) -> Bounds<gpui_kit::Pixels> {
    let displays: Vec<_> = cx
        .displays()
        .iter()
        .map(|d| {
            let b = d.bounds();
            Rect::new(
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            )
        })
        .collect();
    let rect = restore_window_rect(saved, &displays);
    Bounds { origin: gpui_kit::point(px(rect.x), px(rect.y)), size: size(px(rect.w), px(rect.h)) }
}

pub(crate) fn initial_window_bounds(cx: &gpui_kit::App) -> Option<Bounds<gpui_kit::Pixels>> {
    let dir = aether_core::config_dir()?;
    let json = std::fs::read_to_string(dir.join("session-layout.json")).ok()?;
    let saved: super::persistence::SavedWorkspace = serde_json::from_str(&json).ok()?;
    if saved.version != 1 {
        return None;
    }
    saved.model.windows.get(&0)?.bounds.map(|bounds| saved_window_bounds(bounds, cx))
}

// GPUI's native window cannot be dropped on affected Touch Bar Macs. Keep it
// alive and reusable. On Linux GPUI exposes minimization as the retention API.
#[cfg_attr(test, allow(unused_variables))]
fn retain_closed_window(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        hide_macos_window(window);
    }
    // gpui test windows leave minimize unimplemented.
    #[cfg(all(not(target_os = "macos"), not(test)))]
    window.minimize_window();
}
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn hide_macos_window(window: &Window) {
    use objc2::runtime::AnyObject;
    use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(handle) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::AppKit(handle) = handle.as_raw()
    {
        // The NSView belongs to this live GPUI window, on the UI thread.
        unsafe {
            let view = &*(handle.ns_view.as_ptr() as *const AnyObject);
            let native: *mut AnyObject = objc2::msg_send![view, window];
            if !native.is_null() {
                let () = objc2::msg_send![native,orderOut: std::ptr::null::<AnyObject>()];
            }
        }
    }
}
