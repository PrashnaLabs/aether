use std::rc::Rc;

use alacritty_terminal::vte::ansi::Rgb;
use gpui_kit::component::{ThemeConfig, ThemeMode};
use gpui_kit::{Hsla, Rgba};

use super::*;
use crate::grid::contrast;
use crate::palette::ThemePalette;

fn rgb(c: Hsla) -> Rgb {
    let c = Rgba::from(c);
    let byte = |v: f32| (v * 255.).round() as u8;
    Rgb { r: byte(c.r), g: byte(c.g), b: byte(c.b) }
}

#[test]
fn light_app_theme_gives_chrome_legible_on_every_terminal_palette() {
    let light = Theme {
        dark_theme: Rc::new(ThemeConfig { mode: ThemeMode::Dark, ..Default::default() }),
        ..Theme::default()
    };
    assert!(!light.is_dark());
    let dark = dark_theme(&light);
    assert!(dark.is_dark());
    for palette in [ThemePalette::mocha(), ThemePalette::macchiato(), ThemePalette::frappe()] {
        for text in [dark.foreground, dark.muted_foreground] {
            assert!(contrast(rgb(text), palette.bg) >= 4.5, "{text:?} on {:?}", palette.bg);
        }
    }
}
