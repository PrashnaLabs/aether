use gpui_kit::{App, Font, FontFeatures, SharedString};

/// Symbol fonts covering prompt icons (Nerd Font private-use glyphs).
/// The primary family keeps its metrics; these only fill missing glyphs.
const TERMINAL_FONT_FALLBACKS: &[&str] = &[
    "Symbols Nerd Font Mono",
    "Symbols Nerd Font",
    "MesloLGS NF",
    "JetBrainsMono Nerd Font",
    "JetBrainsMono Nerd Font Mono",
    "Hack Nerd Font Mono",
    "FiraCode Nerd Font Mono",
    "Apple Symbols",
    "Apple Color Emoji",
];

/// Terminal font: the custom family from settings when set, else the theme mono,
/// with symbol fallbacks so prompt icons don't render as tofu.
pub(crate) fn terminal_font(custom: &str, theme_mono: SharedString) -> Font {
    let family = if custom.trim().is_empty() { theme_mono } else { custom.trim().into() };
    Font {
        family,
        features: FontFeatures::disable_ligatures(),
        fallbacks: Some(gpui_kit::FontFallbacks::from_fonts(
            TERMINAL_FONT_FALLBACKS.iter().map(|s| (*s).to_string()).collect(),
        )),
        ..gpui_kit::font("")
    }
}

/// Compares font names ignoring case and spaces: "JetBrains Mono Nerd Font"
/// matches the installed "JetBrainsMono Nerd Font".
fn normalize_font_name(name: &str) -> String {
    name.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect()
}

/// Effective terminal family: the requested family when installed (exactly or
/// space-insensitively), else the theme mono. Never passes a missing family to
/// GPUI: a missing primary makes `resolve_font` fall back to Helvetica and
/// drops our Nerd cascade, so unknown names use the theme mono + cascade.
pub fn resolve_terminal_family(
    requested: &str,
    installed: &[String],
    theme_mono: &str,
) -> SharedString {
    let req = requested.trim();
    if req.is_empty() {
        return theme_mono.into();
    }
    if installed.iter().any(|n| n == req) {
        return req.into();
    }
    let norm = normalize_font_name(req);
    if let Some(hit) = installed.iter().find(|n| normalize_font_name(n) == norm) {
        return hit.clone().into();
    }
    theme_mono.into()
}

/// Installed families carrying Nerd glyphs, for the settings hint.
pub fn installed_nerd_fonts(installed: &[String]) -> Vec<String> {
    let mut nerds: Vec<String> =
        installed.iter().filter(|n| n.to_lowercase().contains("nerd")).cloned().collect();
    nerds.sort();
    nerds.dedup();
    nerds
}

/// Share one system-font snapshot between terminal rendering and settings.
/// Restart after installing fonts, as indicated by the settings hint.
pub fn installed_font_names_cached(cx: &App) -> &'static [String] {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| cx.text_system().all_font_names())
}

#[cfg(test)]
#[path = "../../tests/unit/view/font.rs"]
mod tests;
