use std::{cell::Cell, rc::Rc};

use gpui_kit::base::Popup;
use gpui_kit::{
    Anchor, Bounds, Decorations, ElementId, IntoElement, Pixels, Window, canvas, div, prelude::*,
    px,
};

/// Gap between a trigger and its popup.
const GAP: f32 = 6.;
/// `Popup` keeps this far from the window edge (gpui-base `WINDOW_MARGIN`).
const EDGE: f32 = 8.;
/// Below this the popup stops shrinking; only a tiny window lets it cover the trigger.
const MIN_HEIGHT: f32 = 120.;

/// Trigger's window bounds, written every frame the trigger paints.
pub type TriggerBounds = Rc<Cell<Bounds<Pixels>>>;

/// Height for a popup with `room` px between its trigger and the window edge, capped at `max`.
pub fn popup_height(room: f32, max: f32) -> f32 {
    (room - GAP - EDGE).clamp(MIN_HEIGHT.min(max), max)
}

/// Room between `trigger` and the window edge on the popup's side.
pub fn room(trigger: Bounds<Pixels>, below: bool, window: &Window) -> f32 {
    let inset = match window.window_decorations() {
        Decorations::Client { .. } => window.client_inset().unwrap_or_default(),
        Decorations::Server => px(0.),
    };
    let room = if below { window.viewport_size().height - trigger.bottom() } else { trigger.top() };
    f32::from(room - inset)
}

pub fn anchored_popup(
    id: impl Into<ElementId>,
    trigger: impl IntoElement,
    bounds: &TriggerBounds,
    below: bool,
) -> Popup {
    // corner anchor never shrinks, only clamps to window: too-tall popup slides over trigger.
    // content caps own height from these bounds instead
    let bounds = bounds.clone();
    let measured = div().relative().child(trigger).child(
        canvas(
            move |b, window, _| {
                // repaint once when size changed, else content height stays a step stale
                if bounds.get() != b {
                    bounds.set(b);
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
        .top_0()
        .left_0(),
    );
    Popup::new(id, measured)
        .anchor(if below { Anchor::TopLeft } else { Anchor::BottomLeft })
        .offset(px(GAP))
}

#[cfg(test)]
#[path = "../../tests/unit/components/anchored.rs"]
mod tests;
