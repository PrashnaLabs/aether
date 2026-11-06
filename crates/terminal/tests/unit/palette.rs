use super::*;
use aether_core::TerminalTheme;

#[test]
fn catppuccin_mocha_matches_spec() {
    let palette = ThemePalette::resolve(TerminalTheme::CatppuccinMocha, true, rgb(0), rgb(0));
    // Catppuccin Mocha Base: #1e1e2e, Text: #cdd6f4
    assert_eq!(palette.bg, rgb(0x1e1e2e));
    assert_eq!(palette.fg, rgb(0xcdd6f4));
    // Red: #f38ba8, Green: #a6e3a1
    assert_eq!(palette.ansi[1], rgb(0xf38ba8));
    assert_eq!(palette.ansi[2], rgb(0xa6e3a1));
    // Selection Surface2: #585b70
    assert_eq!(palette.selection, rgb(0x585b70));
    // Cursor Rosewater: #f5e0dc
    assert_eq!(palette.cursor, rgb(0xf5e0dc));
}

#[test]
fn catppuccin_macchiato_matches_spec() {
    let palette = ThemePalette::resolve(TerminalTheme::CatppuccinMacchiato, true, rgb(0), rgb(0));
    assert_eq!(palette.bg, rgb(0x24273a));
    assert_eq!(palette.fg, rgb(0xcad3f5));
    assert_eq!(palette.ansi[1], rgb(0xed8796));
    assert_eq!(palette.ansi[2], rgb(0xa6da95));
    assert_eq!(palette.selection, rgb(0x5b6078));
    assert_eq!(palette.cursor, rgb(0xf4dbd6));
}

#[test]
fn catppuccin_frappe_matches_spec() {
    let palette = ThemePalette::resolve(TerminalTheme::CatppuccinFrappe, true, rgb(0), rgb(0));
    assert_eq!(palette.bg, rgb(0x303446));
    assert_eq!(palette.fg, rgb(0xc6d0f5));
    assert_eq!(palette.ansi[1], rgb(0xe78284));
    assert_eq!(palette.ansi[2], rgb(0xa6d189));
    assert_eq!(palette.selection, rgb(0x626880));
    assert_eq!(palette.cursor, rgb(0xf2d5cf));
}

#[test]
fn catppuccin_latte_matches_spec() {
    let palette = ThemePalette::resolve(TerminalTheme::CatppuccinLatte, false, rgb(0), rgb(0));
    // Catppuccin Latte Base: #eff1f5, Text: #4c4f69
    assert_eq!(palette.bg, rgb(0xeff1f5));
    assert_eq!(palette.fg, rgb(0x4c4f69));
    // Red: #d20f39, Green: #40a02b
    assert_eq!(palette.ansi[1], rgb(0xd20f39));
    assert_eq!(palette.ansi[2], rgb(0x40a02b));
    // Selection Surface2: #acb0be
    assert_eq!(palette.selection, rgb(0xacb0be));
    // Cursor Rosewater: #dc8a78
    assert_eq!(palette.cursor, rgb(0xdc8a78));
}

#[test]
fn auto_theme_switches_flavor_by_appearance() {
    let dark = ThemePalette::resolve(TerminalTheme::AutoCatppuccin, true, rgb(0), rgb(0));
    assert_eq!(dark.bg, rgb(0x1e1e2e)); // Mocha
    let light = ThemePalette::resolve(TerminalTheme::AutoCatppuccin, false, rgb(0), rgb(0));
    assert_eq!(light.bg, rgb(0xeff1f5)); // Latte
}

#[test]
fn aether_theme_uses_fallbacks() {
    let fg = rgb(0x112233);
    let bg = rgb(0x445566);
    let dark = ThemePalette::resolve(TerminalTheme::Aether, true, fg, bg);
    assert_eq!(dark.fg, fg);
    assert_eq!(dark.bg, bg);
    assert_eq!(dark.ansi[1], DARK[1]);

    let light = ThemePalette::resolve(TerminalTheme::Aether, false, fg, bg);
    assert_eq!(light.fg, fg);
    assert_eq!(light.bg, bg);
    assert_eq!(light.ansi[1], LIGHT[1]);
}
