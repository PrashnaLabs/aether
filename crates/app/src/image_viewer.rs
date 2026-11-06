//! A read-only image overlay with independent ownership and focus per native window.
mod data;
mod view;

use crate::app::ChatApp;
use data::LoadedImage;
pub(crate) use data::is_image;
use gpui_kit::{
    AnyElement, AppContext, Context, FocusHandle, Subscription, Task, Window, prelude::*,
};
use std::path::{Path, PathBuf};

pub(crate) struct ImageViewer {
    path: PathBuf,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    visible: bool,
    loaded: Option<LoadedImage>,
    error: Option<String>,
    task: Option<Task<()>>,
    keys: Option<Subscription>,
    generation: u64,
    zoom: Option<f32>,
    pan: [f32; 2],
    drag: Option<([f32; 2], [f32; 2])>,
}

impl ImageViewer {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            path: PathBuf::new(),
            focus: cx.focus_handle(),
            restore: None,
            visible: false,
            loaded: None,
            error: None,
            task: None,
            keys: None,
            generation: 0,
            zoom: None,
            pan: [0.; 2],
            drag: None,
        }
    }

    fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible {
            self.restore = window.focused(cx);
        }
        if self.keys.is_none() {
            let owner = window.window_handle().window_id();
            let viewer = cx.weak_entity();
            // Key bindings dispatch before element key-down handlers. Intercept
            // first so chat shortcuts cannot steal focus behind this modal.
            self.keys = Some(cx.intercept_keystrokes(move |event, window, cx| {
                if window.window_handle().window_id() != owner {
                    return;
                }
                let _ = viewer.update(cx, |viewer, cx| {
                    if viewer.visible {
                        viewer.handle_key(&event.keystroke, window, cx);
                    }
                });
            }));
        }
        self.path = path.clone();
        self.visible = true;
        self.loaded = None;
        self.error = None;
        self.zoom = None;
        self.pan = [0.; 2];
        self.drag = None;
        self.generation += 1;
        let generation = self.generation;
        self.focus.focus(window, cx);
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { data::load(&path) }).await;
            let _ = this.update(cx, |this, cx| {
                if !this.visible || this.generation != generation {
                    return;
                }
                match result {
                    Ok(image) => this.loaded = Some(image),
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = false;
        self.generation += 1;
        self.task = None;
        self.loaded = None;
        self.error = None;
        self.drag = None;
        if let Some(focus) = self.restore.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }
}

impl ChatApp {
    pub(crate) fn preview_image(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !is_image(path) || path.is_dir() {
            return false;
        }
        let id = window.window_handle().window_id();
        let viewer = if let Some((viewer, _)) = self.image_viewers.get(&id) {
            viewer.clone()
        } else {
            let viewer = cx.new(ImageViewer::new);
            let subscription = cx.observe(&viewer, |_, _, cx| cx.notify());
            self.image_viewers.insert(id, (viewer.clone(), subscription));
            viewer
        };
        viewer.update(cx, |viewer, cx| viewer.open(path.to_path_buf(), window, cx));
        true
    }

    pub(crate) fn image_viewer_overlay(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (viewer, _) = self.image_viewers.get(&window.window_handle().window_id())?;
        let viewer = viewer.clone();
        if !viewer.read(cx).visible {
            return None;
        }
        // The native browser lives in the main window. A detached preview
        // must not hide native content in another window.
        if self
            .workspace
            .windows
            .get(&0)
            .is_some_and(|main| main.window_id() == window.window_handle().window_id())
        {
            self.browser.update(cx, |browser, _| browser.set_shown(false));
        }
        Some(viewer.into_any_element())
    }
}

#[cfg(test)]
#[path = "../tests/unit/image_viewer/data.rs"]
mod tests;
