use aether_core::Provider;
use gpui_kit::component::Icon;

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

pub fn provider_icon(provider: Provider) -> Icon {
    Icon::empty().path(provider_icon_path(provider))
}

#[cfg(test)]
#[path = "../../tests/unit/components/provider_icon.rs"]
mod tests;
