use std::sync::atomic::{AtomicU32, Ordering};

use gpui_kit::component::dialog::AlertDialog;
use gpui_kit::component::{ActiveTheme, Theme, ThemeColor, WindowExt};
use gpui_kit::{
    AbsoluteLength, App, Hsla, Pixels, Rgba, Styled, Window, WindowBackgroundAppearance, px, rems,
    rgb,
};

pub const CONTENT_WIDTH: Pixels = px(760.);

static CORNER_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);

pub fn set_corner_scale(scale: f32) {
    CORNER_SCALE.store(scale.to_bits(), Ordering::Relaxed);
}

pub fn scale_length(r: AbsoluteLength, scale: f32) -> AbsoluteLength {
    match r {
        AbsoluteLength::Pixels(p) => AbsoluteLength::Pixels(p * scale),
        AbsoluteLength::Rems(r) => AbsoluteLength::Rems(rems(r.0 * scale)),
    }
}

pub trait Corners: Styled + Sized {
    fn corner(self, r: impl Into<AbsoluteLength>) -> Self {
        self.rounded(scale_length(r.into(), f32::from_bits(CORNER_SCALE.load(Ordering::Relaxed))))
    }

    fn corner_l(self, r: impl Into<AbsoluteLength>) -> Self {
        self.rounded_l(scale_length(r.into(), f32::from_bits(CORNER_SCALE.load(Ordering::Relaxed))))
    }

    fn corner_r(self, r: impl Into<AbsoluteLength>) -> Self {
        self.rounded_r(scale_length(r.into(), f32::from_bits(CORNER_SCALE.load(Ordering::Relaxed))))
    }
}

impl<T: Styled> Corners for T {}

pub fn apply_surface_opacity(colors: &mut ThemeColor, opacity: u8) {
    let a = f32::from(opacity) / 100.;
    for c in [
        &mut colors.background,
        &mut colors.sidebar,
        &mut colors.title_bar,
        &mut colors.status_bar,
        &mut colors.tab_bar,
        &mut colors.tab_bar_segmented,
        &mut colors.list,
        &mut colors.table,
    ] {
        c.a = a;
    }
}

pub fn window_background(opacity: u8, blur: bool) -> WindowBackgroundAppearance {
    match (opacity >= 100, blur && cfg!(target_os = "macos")) {
        (true, _) => WindowBackgroundAppearance::Opaque,
        (false, true) => WindowBackgroundAppearance::Blurred,
        (false, false) => WindowBackgroundAppearance::Transparent,
    }
}

pub fn open_alert(
    window: &mut Window,
    cx: &mut App,
    build: impl Fn(AlertDialog, &mut Window, &mut App) -> AlertDialog + 'static,
) {
    // kit paints dialogs with tokens.background, translucent below 100% opacity
    window.open_alert_dialog(cx, move |alert, window, cx| {
        let opaque = cx.theme().background.alpha(1.);
        build(alert, window, cx).bg(opaque)
    });
}

pub fn mango() -> Hsla {
    rgb(0xF59E0B).into()
}

pub fn papaya() -> Hsla {
    rgb(0xFB7185).into()
}

pub fn mango_text(cx: &App) -> Hsla {
    if cx.theme().is_dark() { rgb(0xFBBF24).into() } else { rgb(0xB45309).into() }
}

fn luminance(c: Hsla) -> f32 {
    let c = Rgba::from(c);
    let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio, ignoring alpha.
pub fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// `color`, or its lightness moved away from `bg` until it reaches 4.5:1.
pub fn legible(color: Hsla, bg: Hsla) -> Hsla {
    let step = if bg.l > 0.5 { -0.01 } else { 0.01 };
    let mut out = color;
    while contrast(out, bg) < 4.5 && (0. ..1.).contains(&(out.l + step)) {
        out.l += step;
    }
    out
}

fn semantic_text(theme: &Theme, token: Hsla, stock_light: u32) -> Hsla {
    if theme.theme_name() == "Default Light" {
        rgb(stock_light).into()
    } else {
        legible(token, theme.background)
    }
}

/// Success as small text: tokens can be too light on the background (stock
/// light green is 2.3:1 on white), so low-contrast tokens are darkened or
/// lightened. Tints keep the token.
pub fn success_text(cx: &App) -> Hsla {
    semantic_text(cx.theme(), cx.theme().success, 0x15803D)
}

/// Info (renames, copies) as small text; see [`success_text`].
pub fn info_text(cx: &App) -> Hsla {
    semantic_text(cx.theme(), cx.theme().info, 0x1D4ED8)
}

/// Danger as small text; see [`success_text`].
pub fn danger_text(cx: &App) -> Hsla {
    semantic_text(cx.theme(), cx.theme().danger, 0xDC2626)
}

/// Warning as text or a small icon. Keep the brighter token for background tints.
pub fn warning_text(cx: &App) -> Hsla {
    semantic_text(cx.theme(), cx.theme().warning, 0xA16207)
}

#[cfg(test)]
#[path = "../tests/unit/theme.rs"]
mod tests;
