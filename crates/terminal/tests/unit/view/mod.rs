use super::*;
use crate::palette::rgb;

#[test]
fn exit_message_names_the_shell_code_or_signal() {
    assert_eq!(exit_message("zsh", Some(0), None), "zsh exited");
    assert_eq!(exit_message("zsh", Some(3), None), "Process exited (code 3)");
    assert_eq!(exit_message("zsh", None, Some(9)), "zsh was stopped (signal 9)");
    assert_eq!(exit_message("zsh", None, None), "zsh exited");
}

#[test]
fn terminal_view_resolves_palette_from_terminal_theme() {
    let mocha_resolved = ThemePalette::resolve(
        TerminalTheme::CatppuccinMocha,
        true,
        Rgb { r: 0, g: 0, b: 0 },
        Rgb { r: 0, g: 0, b: 0 },
    );
    let palette = Palette::from_theme(mocha_resolved);
    assert_eq!(palette.bg, rgb(0x1e1e2e));
    assert_eq!(palette.fg, rgb(0xcdd6f4));
    assert_eq!(palette.cursor, rgb(0xf5e0dc));
    assert_eq!(palette.selection, rgb(0x585b70));
}

#[test]
fn terminal_view_tabs_and_events() {
    assert_eq!(TerminalTab::Global, TerminalTab::Global);
    assert_ne!(TerminalTab::Global, TerminalTab::Project(42));
    assert_eq!(TerminalTab::Project(42), TerminalTab::Project(42));
}

#[test]
fn moved_banner_label_distinguishes_global_and_project() {
    let to = "/new/path";
    let global_label = match TerminalTab::Global {
        TerminalTab::Global => format!("· Workspace moved to {to}"),
        TerminalTab::Project(_) => format!("· Chat moved to {to}"),
    };
    assert_eq!(global_label, "· Workspace moved to /new/path");

    let project_label = match TerminalTab::Project(42) {
        TerminalTab::Global => format!("· Workspace moved to {to}"),
        TerminalTab::Project(_) => format!("· Chat moved to {to}"),
    };
    assert_eq!(project_label, "· Chat moved to /new/path");
}

#[test]
fn aether_terminal_palette_ignores_the_app_theme() {
    let palette = resolve_terminal_palette(TerminalTheme::Aether);
    assert_eq!((palette.fg, palette.bg), (rgb(0xfafafa), rgb(0x0a0a0a)));
    let auto = resolve_terminal_palette(TerminalTheme::AutoCatppuccin);
    assert_eq!((auto.fg, auto.bg), (rgb(0xcdd6f4), rgb(0x1e1e2e)));
}

#[test]
fn pane_display_label_prefers_custom_title_then_running_job_then_shell() {
    assert_eq!(
        resolve_pane_label(Some("api-server"), Some("iris@192:~/aether"), Some("cargo"), "zsh"),
        "api-server"
    );
    assert_eq!(resolve_pane_label(None, Some("iris@192:~/aether"), Some("cargo"), "zsh"), "cargo");
    assert_eq!(resolve_pane_label(None, Some("iris@192:~/aether"), None, "zsh"), "zsh");
    assert_eq!(resolve_pane_label(None, None, None, "zsh"), "zsh");
}

#[test]
fn exit_label_words() {
    assert_eq!(exit_label(Some(0), false), "Exited with code 0");
    assert_eq!(exit_label(Some(1), true), "Stopped");
    assert_eq!(exit_label(None, false), "Exited");
    assert_eq!(exit_label(None, true), "Stopped");
    assert_eq!(exit_label(Some(127), false), "Exited with code 127");
}

#[test]
fn commands_start_at_80_by_24_in_the_font_cell() {
    let size = start_size(13.);
    assert_eq!((size.num_cols, size.num_lines), (80, 24));
    assert_eq!((size.cell_width, size.cell_height), (7, 19));
}
