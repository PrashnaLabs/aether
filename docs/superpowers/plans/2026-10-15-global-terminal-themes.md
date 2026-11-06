# Global Terminal with Themes (tmux & Neovim Compatibility) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a persistent, workspace-level Global Terminal with a multi-session tabbed drawer alongside chat terminals, powered by a Catppuccin theme engine compatible with tmux and Neovim.

**Architecture:** Extend Aether's `Settings` with a `TerminalTheme` enum (Catppuccin suite + Aether defaults). Replace hardcoded ANSI arrays in `crates/app/src/terminal/grid.rs` with a dedicated palette engine that maps official Catppuccin colors to ANSI 0–15, foreground, background, cursor, and selection, wired to OSC 4/10/11 PTY responses. Decouple terminal drawer storage in `crates/app/src/terminal/` so a persistent global terminal lives at the workspace root across all pages, navigable via header tabs alongside contextual project terminals.

**Tech Stack:** Rust 2024 edition, GPUI (`gpui-kit`), `alacritty_terminal`, `serde`, `tokio`.

**Spec:** [docs/superpowers/specs/2026-10-15-global-terminal-themes-design.md](../specs/2026-10-15-global-terminal-themes-design.md)

## Global Constraints

- Preserve all existing keybindings (`ctrl-``` global toggle, `cmd-j` composer focus toggle, clipboard shortcuts).
- Thread safety in `session.rs`: Alacritty PTY event loop lock order must never be inverted (PTY thread lock first, UI never locks terminal while holding palette mutex).
- Compatibility environment variables: `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=aether`.
- Contrast compliance: Dim text (SGR 2) and fallback foregrounds must maintain readable WCAG contrast (>= 4.5:1) against theme backgrounds.

## Review Focus

1. `workspace_root` is empty or invalid when Global Terminal initializes: must fall back to `$HOME` without panicking or failing to spawn shell.
2. Shell exits or crashes in global terminal: drawer shows "Press Enter to restart" badge without losing tab state.
3. Theme switched dynamically while tmux/Neovim is active: PTY `set_palette` must update listener immediately so subsequent OSC 10/11 queries return updated colors, and terminal grid must redraw.
4. Switching pages (Chat -> Settings -> Skills -> Chat) with drawer open: Global terminal stays alive and maintains active process/scrollback.
5. Window resized to extreme small bounds: columns and rows must clamp to `MIN_COLS` (20) and `MIN_ROWS` (8) without alacritty division-by-zero panics.

---

### Task 1: `TerminalTheme` Setting in `aether-core`

**Files:**
- Modify: `crates/core/src/settings.rs:8-16, 215-228`
- Test: `crates/core/src/settings.rs:tests`

**Interfaces:**
- Produces: `TerminalTheme` enum (`AutoCatppuccin`, `CatppuccinMocha`, `CatppuccinMacchiato`, `CatppuccinFrappe`, `CatppuccinLatte`, `Aether`) with `ALL` array, `label(&self) -> &'static str`, and `is_dark(&self, system_dark: bool) -> bool`.
- Produces: `Settings.terminal_theme: TerminalTheme` defaulting to `TerminalTheme::AutoCatppuccin`.

- [ ] **Step 1: Write the failing test for `TerminalTheme` serialization and helper methods**

In `crates/core/src/settings.rs` under `mod tests`:
```rust
#[test]
fn terminal_theme_serializes_and_resolves_appearance() {
    assert_eq!(TerminalTheme::default(), TerminalTheme::AutoCatppuccin);
    assert_eq!(TerminalTheme::AutoCatppuccin.is_dark(true), true);
    assert_eq!(TerminalTheme::AutoCatppuccin.is_dark(false), false);
    assert_eq!(TerminalTheme::CatppuccinMocha.is_dark(false), true);
    assert_eq!(TerminalTheme::CatppuccinLatte.is_dark(true), false);

    let json = serde_json::to_string(&TerminalTheme::CatppuccinMocha).unwrap();
    assert_eq!(json, "\"catppuccin_mocha\"");
    let back: TerminalTheme = serde_json::from_str(&json).unwrap();
    assert_eq!(back, TerminalTheme::CatppuccinMocha);

    let default_settings = Settings::default();
    assert_eq!(default_settings.terminal_theme, TerminalTheme::AutoCatppuccin);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-core terminal_theme_serializes_and_resolves_appearance`
Expected: FAIL with "cannot find type `TerminalTheme`"

- [ ] **Step 3: Implement `TerminalTheme` in `crates/core/src/settings.rs`**

Add `TerminalTheme` enum with serde annotations, implement `label(&self)` and `is_dark(&self, system_dark: bool) -> bool`, and add `pub terminal_theme: TerminalTheme` to `Settings` with `#[serde(default)]`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-core terminal_theme_serializes_and_resolves_appearance`
Expected: PASS

- [ ] **Step 5: Verify all core tests pass**

Run: `cargo test -p aether-core`
Expected: All tests pass.

---

### Task 2: Catppuccin Palette Engine & ANSI 0–15 Calibration

**Files:**
- Create: `crates/app/src/terminal/palette.rs`
- Modify: `crates/app/src/terminal/grid.rs:15-100`
- Modify: `crates/app/src/terminal/mod.rs:3-8`
- Test: `crates/app/src/terminal/palette.rs:tests`

**Interfaces:**
- Consumes: `aether_core::TerminalTheme` from Task 1.
- Produces: `ThemePalette` struct containing `ansi: [Rgb; 16]`, `fg: Rgb`, `bg: Rgb`, `cursor: Rgb`, `selection: Rgb`.
- Produces: `ThemePalette::resolve(theme: TerminalTheme, is_dark: bool, fallback_fg: Rgb, fallback_bg: Rgb) -> Self`.
- Produces: Updated `Palette` in `grid.rs` constructed from `ThemePalette`.

- [ ] **Step 1: Write the failing tests for Catppuccin palette values and index lookups**

Create `crates/app/src/terminal/palette.rs` with test module:
```rust
#[cfg(test)]
mod tests {
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
    }

    #[test]
    fn auto_theme_switches_flavor_by_appearance() {
        let dark = ThemePalette::resolve(TerminalTheme::AutoCatppuccin, true, rgb(0), rgb(0));
        assert_eq!(dark.bg, rgb(0x1e1e2e)); // Mocha
        let light = ThemePalette::resolve(TerminalTheme::AutoCatppuccin, false, rgb(0), rgb(0));
        assert_eq!(light.bg, rgb(0xeff1f5)); // Latte
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether catppuccin_mocha_matches_spec`
Expected: FAIL with module/type missing.

- [ ] **Step 3: Implement `ThemePalette` with official Catppuccin hex codes**

In `crates/app/src/terminal/palette.rs`:
Define official Catppuccin color tables:
- **Mocha**: Base `0x1e1e2e`, Text `0xcdd6f4`, Surface2 `0x585b70`, Rosewater `0xf5e0dc`.
  ANSI: Black `0x45475a`, Red `0xf38ba8`, Green `0xa6e3a1`, Yellow `0xf9e2af`, Blue `0x89b4fa`, Magenta `0xf5c2e7`, Cyan `0x94e2d5`, White `0xbac2de`.
  Bright ANSI: Black `0x585b70`, Red `0xf38ba8`, Green `0xa6e3a1`, Yellow `0xf9e2af`, Blue `0x89b4fa`, Magenta `0xf5c2e7`, Cyan `0x94e2d5`, White `0xa6adc8`.
- **Macchiato**: Base `0x24273a`, Text `0 cad3f5`, Surface2 `0x5b6078`.
- **Frappé**: Base `0x303446`, Text `0xc6d0f5`, Surface2 `0x626880`.
- **Latte**: Base `0xeff1f5`, Text `0x4c4f69`, Surface2 `0xacb0be`.
  ANSI: Black `0x5c5f77`, Red `0xd20f39`, Green `0x40a02b`, Yellow `0xdf8e1d`, Blue `0x1e66f5`, Magenta `0xea76cb`, Cyan `0x179299`, White `0xacb0be`.
  Bright ANSI: Black `0x6c6f85`, Red `0xd20f39`, Green `0x40a02b`, Yellow `0xdf8e1d`, Blue `0x1e66f5`, Magenta `0xea76cb`, Cyan `0x179299`, White `0xbcc0cc`.
- Integrate `ThemePalette` into `Palette::from_theme` in `crates/app/src/terminal/grid.rs`. Ensure indices 256 (FG) and 257 (BG) resolve cleanly.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether catppuccin_mocha_matches_spec`
Expected: PASS

- [ ] **Step 5: Run existing terminal grid tests**

Run: `cargo test -p aether terminal::grid::tests`
Expected: PASS

---

### Task 3: PTY OSC Protocol Responses & Dynamic Palette Updates

**Files:**
- Modify: `crates/app/src/terminal/session.rs:60-95, 185-190`
- Modify: `crates/app/src/terminal/view.rs:880-920`
- Test: `crates/app/src/terminal/session.rs:tests`

**Interfaces:**
- Consumes: `Palette` from Task 2.
- Produces: `Session::update_theme(&self, palette: Palette)` that refreshes `s.palette` and marks terminal dirty.
- Produces: Correct `Event::ColorRequest` handling for OSC 4, OSC 10 (foreground, index 256), and OSC 11 (background, index 257).

- [ ] **Step 1: Write the failing test for OSC color requests in `session.rs`**

In `crates/app/src/terminal/session.rs` under `mod tests`:
```rust
#[test]
fn color_request_returns_active_theme_fg_and_bg() {
    let palette = Palette::new(true, rgb(0xcdd6f4), rgb(0x1e1e2e));
    assert_eq!(palette.index(256), rgb(0xcdd6f4)); // OSC 10 foreground
    assert_eq!(palette.index(257), rgb(0x1e1e2e)); // OSC 11 background
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether color_request_returns_active_theme_fg_and_bg`
Expected: FAIL if index 256 does not return `palette.fg`.

- [ ] **Step 3: Update `Palette.index` and `session.rs` color handling**

1. In `crates/app/src/terminal/grid.rs`, make `palette.index(256)` return `self.fg` and `palette.index(257)` return `self.bg`.
2. In `crates/app/src/terminal/session.rs`, ensure `Event::ColorRequest` correctly formats RGB response for standard 0–15 indices as well as 256/257.
3. In `crates/app/src/terminal/view.rs`, update `TerminalView::render` to resolve the current `TerminalTheme` from `Settings` and pass the resolved `Palette` to both `Session::set_palette` and grid paint runs.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether color_request_returns_active_theme_fg_and_bg`
Expected: PASS

---

### Task 4: Persistent Global Terminal Lifecycle

**Files:**
- Modify: `crates/app/src/terminal/mod.rs:75-175`
- Modify: `crates/app/src/app.rs:3745-3780`
- Test: `crates/app/src/terminal/mod.rs:tests`

**Interfaces:**
- Produces: `ChatApp.global_terminal: Option<Drawer>`.
- Produces: `ChatApp.global_folder(&self) -> PathBuf` (resolves `workspace_root` or `$HOME`).
- Produces: `ChatApp.ensure_global_terminal(&mut self, window, cx) -> Entity<TerminalView>`.
- Produces: App-level drawer rendering in `crates/app/src/app.rs` so the terminal drawer renders across all pages (`Chat`, `Skills`, `Agents`, `Settings`).

- [ ] **Step 1: Write unit tests for `global_folder` resolution and persistence**

In `crates/app/src/terminal/mod.rs` test module:
```rust
#[test]
fn global_folder_falls_back_to_home_when_workspace_empty() {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let folder = resolve_global_folder("");
    assert_eq!(folder, home);
}

#[test]
fn global_folder_uses_workspace_when_valid() {
    let tmp = std::env::temp_dir();
    let folder = resolve_global_folder(tmp.to_str().unwrap());
    assert_eq!(folder, tmp);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether global_folder_falls_back_to_home`
Expected: FAIL with `resolve_global_folder` missing.

- [ ] **Step 3: Implement global folder resolution and persistent drawer in `ChatApp`**

1. Implement `resolve_global_folder(workspace: &str) -> PathBuf`.
2. Add `pub(crate) global_terminal: Option<Drawer>` to `ChatApp`.
3. In `crates/app/src/terminal/mod.rs`, implement `ensure_global_terminal` which spawns the session with `resolve_global_folder(&self.settings.workspace_root)` on first request and retains it across page changes.
4. In `crates/app/src/app.rs`, adjust layout hierarchy so that the terminal drawer can render under the active page content regardless of `self.page` when opened.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether global_folder_falls_back_to_home`
Expected: PASS

---

### Task 5: Drawer Header Tab Switcher (Global vs Project)

**Files:**
- Modify: `crates/app/src/terminal/view.rs:650-730`
- Modify: `crates/app/src/terminal/mod.rs:90-170`
- Test: `crates/app/src/terminal/view.rs:tests`

**Interfaces:**
- Produces: `enum TerminalTab { Global, Project(i64) }`.
- Produces: `ChatApp.active_terminal_tab: TerminalTab`.
- Produces: Tab controls in terminal header: `[Global]` tab always visible, `[Project: <name>]` visible when current chat has a folder.
- Produces: Tab click events that swap the displayed `Entity<TerminalView>` in the drawer while preserving both sessions in the background.

- [ ] **Step 1: Write test for terminal tab state tracking**

In `crates/app/src/terminal/mod.rs` test module:
```rust
#[test]
fn terminal_tab_selection_swaps_active_view() {
    let mut state = TerminalTabState::new();
    assert_eq!(state.active, TerminalTab::Global);
    state.select(TerminalTab::Project(42));
    assert_eq!(state.active, TerminalTab::Project(42));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether terminal_tab_selection_swaps_active_view`
Expected: FAIL with missing types.

- [ ] **Step 3: Implement `TerminalTabState` and header tab rendering**

1. Define `TerminalTab` (`Global`, `Project(i64)`) and state tracking in `ChatApp`.
2. In `crates/app/src/terminal/view.rs` / `mod.rs`:
   - In the drawer header, render tab chips for `Global` and active `Project` (if any).
   - Styled using `gpui-kit` active tab styling (active tab highlighted, inactive tab muted).
   - On click, switch `active_terminal_tab` and focus the selected terminal view.
3. Update `toggle_terminal` and `focus_terminal` to act upon the active tab's view.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether terminal_tab_selection_swaps_active_view`
Expected: PASS

---

### Task 6: Terminal Theme Selector in Settings UI

**Files:**
- Modify: `crates/app/src/settings_view.rs:115-180`
- Modify: `crates/core/src/settings.rs`
- Test: `crates/app/src/settings_view.rs:tests`

**Interfaces:**
- Consumes: `TerminalTheme::ALL` and `Settings.terminal_theme`.
- Produces: Terminal Theme dropdown/picker in Settings view under the Terminal section.
- Produces: Instant theme update across open terminal views upon selection change via `crate::settings::save`.

- [ ] **Step 1: Write test for terminal theme options in settings**

In `crates/app/src/settings_view.rs`:
```rust
#[test]
fn settings_lists_all_terminal_themes() {
    assert_eq!(TerminalTheme::ALL.len(), 6);
    assert_eq!(TerminalTheme::ALL[0], TerminalTheme::AutoCatppuccin);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether settings_lists_all_terminal_themes`
Expected: FAIL with `TerminalTheme::ALL` missing or count mismatch.

- [ ] **Step 3: Add `TerminalTheme::ALL` and wire Settings UI picker**

1. In `crates/core/src/settings.rs`, add `pub const ALL: [Self; 6]` to `TerminalTheme`.
2. In `crates/app/src/settings_view.rs`, add a "Terminal Theme" row under the terminal settings section:
   - Dropdown offering all 6 options (`Auto (Catppuccin)`, `Catppuccin Mocha`, `Catppuccin Macchiato`, `Catppuccin Frappé`, `Catppuccin Latte`, `Aether Default`).
   - Changing selection updates `settings.terminal_theme`, saves settings, and notifies context so terminals redraw immediately.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether settings_lists_all_terminal_themes`
Expected: PASS

- [ ] **Step 5: Full verification build and test run**

Run: `cargo test`
Expected: All tests pass across all crates.
