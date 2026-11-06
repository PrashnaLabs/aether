use aether_core::TerminalTheme;
use alacritty_terminal::vte::ansi::Rgb;

pub const fn rgb(hex: u32) -> Rgb {
    Rgb { r: (hex >> 16) as u8, g: (hex >> 8) as u8, b: hex as u8 }
}

/// ANSI 0-15 for the standard Aether dark theme.
pub const DARK: [Rgb; 16] = [
    rgb(0x404040),
    rgb(0xf87171),
    rgb(0x4ade80),
    rgb(0xfacc15),
    rgb(0x60a5fa),
    rgb(0xc084fc),
    rgb(0x22d3ee),
    rgb(0xd4d4d4),
    rgb(0x8a8a8a),
    rgb(0xfca5a5),
    rgb(0x86efac),
    rgb(0xfde047),
    rgb(0x93c5fd),
    rgb(0xd8b4fe),
    rgb(0x67e8f9),
    rgb(0xfafafa),
];

/// ANSI 0-15 for the standard Aether light theme.
pub const LIGHT: [Rgb; 16] = [
    rgb(0x262626),
    rgb(0xdc2626),
    rgb(0x15803d),
    rgb(0xa16207),
    rgb(0x2563eb),
    rgb(0x9333ea),
    rgb(0x0e7490),
    rgb(0x6e6e6e),
    rgb(0x575757),
    rgb(0xb91c1c),
    rgb(0x166534),
    rgb(0x854d0e),
    rgb(0x1d4ed8),
    rgb(0x7e22ce),
    rgb(0x155e75),
    rgb(0x8c8c8c),
];

/// ANSI 0-15 for Catppuccin Mocha.
pub const MOCHA_ANSI: [Rgb; 16] = [
    rgb(0x45475a), // Black (Surface1)
    rgb(0xf38ba8), // Red
    rgb(0xa6e3a1), // Green
    rgb(0xf9e2af), // Yellow
    rgb(0x89b4fa), // Blue
    rgb(0xf5c2e7), // Magenta (Pink)
    rgb(0x94e2d5), // Cyan (Teal)
    rgb(0xbac2de), // White (Subtext1)
    rgb(0x585b70), // Bright Black (Surface2)
    rgb(0xf38ba8), // Bright Red
    rgb(0xa6e3a1), // Bright Green
    rgb(0xf9e2af), // Bright Yellow
    rgb(0x89b4fa), // Bright Blue
    rgb(0xf5c2e7), // Bright Magenta
    rgb(0x94e2d5), // Bright Cyan
    rgb(0xa6adc8), // Bright White (Subtext0)
];

/// ANSI 0-15 for Catppuccin Macchiato.
pub const MACCHIATO_ANSI: [Rgb; 16] = [
    rgb(0x494d64), // Black (Surface1)
    rgb(0xed8796), // Red
    rgb(0xa6da95), // Green
    rgb(0xeed49f), // Yellow
    rgb(0x8aadf4), // Blue
    rgb(0xf5bde6), // Magenta (Pink)
    rgb(0x8bd5ca), // Cyan (Teal)
    rgb(0xb8c0e0), // White (Subtext1)
    rgb(0x5b6078), // Bright Black (Surface2)
    rgb(0xed8796), // Bright Red
    rgb(0xa6da95), // Bright Green
    rgb(0xeed49f), // Bright Yellow
    rgb(0x8aadf4), // Bright Blue
    rgb(0xf5bde6), // Bright Magenta
    rgb(0x8bd5ca), // Bright Cyan
    rgb(0xa5adcb), // Bright White (Subtext0)
];

/// ANSI 0-15 for Catppuccin Frappé.
pub const FRAPPE_ANSI: [Rgb; 16] = [
    rgb(0x51576d), // Black (Surface1)
    rgb(0xe78284), // Red
    rgb(0xa6d189), // Green
    rgb(0xe5c890), // Yellow
    rgb(0x8caaee), // Blue
    rgb(0xf4b8e4), // Magenta (Pink)
    rgb(0x81c8be), // Cyan (Teal)
    rgb(0xb5bfe2), // White (Subtext1)
    rgb(0x626880), // Bright Black (Surface2)
    rgb(0xe78284), // Bright Red
    rgb(0xa6d189), // Bright Green
    rgb(0xe5c890), // Bright Yellow
    rgb(0x8caaee), // Bright Blue
    rgb(0xf4b8e4), // Bright Magenta
    rgb(0x81c8be), // Bright Cyan
    rgb(0xa5adce), // Bright White (Subtext0)
];

/// ANSI 0-15 for Catppuccin Latte.
pub const LATTE_ANSI: [Rgb; 16] = [
    rgb(0x5c5f77), // Black (Subtext1)
    rgb(0xd20f39), // Red
    rgb(0x40a02b), // Green
    rgb(0xdf8e1d), // Yellow
    rgb(0x1e66f5), // Blue
    rgb(0xea76cb), // Magenta (Pink)
    rgb(0x179299), // Cyan (Teal)
    rgb(0xacb0be), // White (Surface2)
    rgb(0x6c6f85), // Bright Black (Subtext0)
    rgb(0xd20f39), // Bright Red
    rgb(0x40a02b), // Bright Green
    rgb(0xdf8e1d), // Bright Yellow
    rgb(0x1e66f5), // Bright Blue
    rgb(0xea76cb), // Bright Magenta
    rgb(0x179299), // Bright Cyan
    rgb(0xbcc0cc), // Bright White (Surface1)
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemePalette {
    pub ansi: [Rgb; 16],
    pub fg: Rgb,
    pub bg: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
}

impl ThemePalette {
    pub const fn mocha() -> Self {
        Self {
            ansi: MOCHA_ANSI,
            fg: rgb(0xcdd6f4),        // Text
            bg: rgb(0x1e1e2e),        // Base
            cursor: rgb(0xf5e0dc),    // Rosewater
            selection: rgb(0x585b70), // Surface2
        }
    }

    pub const fn macchiato() -> Self {
        Self {
            ansi: MACCHIATO_ANSI,
            fg: rgb(0xcad3f5),        // Text
            bg: rgb(0x24273a),        // Base
            cursor: rgb(0xf4dbd6),    // Rosewater
            selection: rgb(0x5b6078), // Surface2
        }
    }

    pub const fn frappe() -> Self {
        Self {
            ansi: FRAPPE_ANSI,
            fg: rgb(0xc6d0f5),        // Text
            bg: rgb(0x303446),        // Base
            cursor: rgb(0xf2d5cf),    // Rosewater
            selection: rgb(0x626880), // Surface2
        }
    }

    pub const fn latte() -> Self {
        Self {
            ansi: LATTE_ANSI,
            fg: rgb(0x4c4f69),        // Text
            bg: rgb(0xeff1f5),        // Base
            cursor: rgb(0xdc8a78),    // Rosewater
            selection: rgb(0xacb0be), // Surface2
        }
    }

    pub const fn aether(is_dark: bool, fallback_fg: Rgb, fallback_bg: Rgb) -> Self {
        Self {
            ansi: if is_dark { DARK } else { LIGHT },
            fg: fallback_fg,
            bg: fallback_bg,
            cursor: fallback_fg,
            selection: if is_dark { rgb(0x404040) } else { rgb(0xd4d4d4) },
        }
    }

    pub fn resolve(
        theme: TerminalTheme,
        is_dark: bool,
        fallback_fg: Rgb,
        fallback_bg: Rgb,
    ) -> Self {
        match theme {
            TerminalTheme::AutoCatppuccin => {
                if is_dark {
                    Self::mocha()
                } else {
                    Self::latte()
                }
            }
            TerminalTheme::CatppuccinMocha => Self::mocha(),
            TerminalTheme::CatppuccinMacchiato => Self::macchiato(),
            TerminalTheme::CatppuccinFrappe => Self::frappe(),
            TerminalTheme::CatppuccinLatte => Self::latte(),
            TerminalTheme::Aether => Self::aether(is_dark, fallback_fg, fallback_bg),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/palette.rs"]
mod tests;
