use super::*;

#[test]
fn terminal_font_uses_theme_mono_unless_custom() {
    let font = terminal_font("", "Menlo".into());
    assert_eq!(font.family.as_ref(), "Menlo");
    let fallbacks = font.fallbacks.expect("symbol fallbacks").fallback_list().to_vec();
    assert!(fallbacks.iter().any(|f| f == "Symbols Nerd Font Mono"));
    assert!(fallbacks.iter().any(|f| f == "Apple Symbols"));

    let font = terminal_font("  MesloLGS NF  ", "Menlo".into());
    assert_eq!(font.family.as_ref(), "MesloLGS NF");
}

#[test]
fn resolve_matches_installed_exactly_or_space_insensitively() {
    let installed = ["Menlo".to_string(), "JetBrainsMono Nerd Font".to_string()];
    assert_eq!(resolve_terminal_family("", &installed, "Menlo").as_ref(), "Menlo");
    assert_eq!(
        resolve_terminal_family("JetBrainsMono Nerd Font", &installed, "Menlo").as_ref(),
        "JetBrainsMono Nerd Font"
    );
    assert_eq!(
        resolve_terminal_family("JetBrains Mono Nerd Font", &installed, "Menlo").as_ref(),
        "JetBrainsMono Nerd Font"
    );
}

#[test]
fn resolve_falls_back_to_theme_mono_for_unknown_families() {
    let installed = ["Menlo".to_string()];
    assert_eq!(
        resolve_terminal_family("JetBrains Mono Nerd Font", &installed, "Menlo").as_ref(),
        "Menlo"
    );
    assert!(installed_nerd_fonts(&installed).is_empty());
    let installed = ["Menlo".to_string(), "JetBrainsMono Nerd Font".to_string()];
    assert_eq!(installed_nerd_fonts(&installed), ["JetBrainsMono Nerd Font"]);
}
