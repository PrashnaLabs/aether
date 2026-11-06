//! The terminal is always dark, so its chrome is laid out and painted with the
//! app theme's dark variant even while the app is light.

use gpui_kit::component::{ActiveTheme, Theme};
use gpui_kit::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window, base,
};

/// `theme` switched to its dark config.
pub(crate) fn dark_theme(theme: &Theme) -> Theme {
    let mut dark = theme.clone();
    dark.apply_config(&theme.dark_theme.clone());
    dark
}

/// `child` with every `cx.theme()` read inside it seeing the dark theme.
pub fn dark(child: impl IntoElement, cx: &App) -> impl IntoElement {
    let themes = (!cx.theme().is_dark()).then(|| {
        let theme = dark_theme(cx.theme());
        let mut base = base::Theme::global(cx);
        base.appearance = base::ThemeAppearance::Dark;
        base.tokens = theme.semantic_tokens();
        (theme, base)
    });
    Dark { themes, child: child.into_any_element() }
}

struct Dark {
    themes: Option<(Theme, base::Theme)>,
    child: AnyElement,
}

impl Dark {
    // themes are globals: swap in for each pass, swap back before siblings run
    fn scoped<R>(&mut self, cx: &mut App, f: impl FnOnce(&mut AnyElement, &mut App) -> R) -> R {
        let Some((theme, base)) = &mut self.themes else {
            return f(&mut self.child, cx);
        };
        std::mem::swap(Theme::global_mut(cx), theme);
        std::mem::swap(base::Theme::global_mut(cx), base);
        let out = f(&mut self.child, cx);
        std::mem::swap(Theme::global_mut(cx), theme);
        std::mem::swap(base::Theme::global_mut(cx), base);
        out
    }
}

impl IntoElement for Dark {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Dark {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.scoped(cx, |child, cx| child.request_layout(window, cx)), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.scoped(cx, |child, cx| {
            child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.scoped(cx, |child, cx| child.paint(window, cx));
    }
}

#[cfg(test)]
#[path = "../tests/unit/dark.rs"]
mod tests;
