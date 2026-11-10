use aether_core::Provider;
use gpui_kit::component::Icon;
use gpui_kit::{Hsla, Styled, rgb};

pub fn provider_icon_path(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenRouter => "icons/provider-openrouter.svg",
        Provider::Claude => "icons/provider-claude.svg",
        Provider::Codex => "icons/provider-codex.svg",
        Provider::Pi => "icons/provider-pi.svg",
        Provider::OpenCode => "icons/provider-opencode.svg",
        Provider::Antigravity => "icons/provider-antigravity.svg",
    }
}

pub fn provider_tint(provider: Provider) -> Hsla {
    rgb(match provider {
        Provider::Claude => 0xe08a5f,
        Provider::Pi => 0xa78bfa,
        Provider::Antigravity => 0x5eead4,
        Provider::OpenCode => 0x93a9ff,
        Provider::Codex | Provider::OpenRouter => 0xc9ccd6,
    })
    .into()
}

/// Brand-coloured provider glyph; monochrome brands keep the surrounding text colour.
pub fn provider_icon(provider: Provider) -> Icon {
    let icon = Icon::empty().path(provider_icon_path(provider));
    match provider {
        Provider::Codex | Provider::OpenRouter => icon,
        _ => icon.text_color(provider_tint(provider)),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/components/provider_icon.rs"]
mod tests;
