# Appearance Customization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings > Appearance gains theme cards with 13 new color schemes, icon packs, corner styles, a UI font picker, window opacity and window blur.

**Architecture:** `Appearance` grows to 16 values, and one function, `apply_look(&Settings, window, cx)`, applies the scheme, title bar, font, corners and opacity whenever a setting changes. Schemes are compact palettes expanded into `gpui-component`'s `ThemeConfig` and installed in its light and dark slots. Icon packs route through the existing `AppAssets` seam and are fixed at launch.

**Tech Stack:** Rust, GPUI (`gpui-pre` 0.3.9), `gpui-kit`/`gpui-component` 0.7.3, serde.

**Spec:** `docs/superpowers/specs/2026-10-30-appearance-customization-design.md`

## Global Constraints

- A scheme never changes terminal cell colors or terminal background.
- Defaults (System, Lucide, Rounded, default font, 100% opacity) must render as today. Accepted exception: a saved Light or Dark on an OS in the other mode now gets a matching title bar.
- No new runtime dependencies. `serde_json` is already a workspace dependency. Vendored data files are fine.
- Existing settings files must keep parsing; `system`, `light`, `dark` round-trip unchanged.
- Opacity range 60% to 100%, default 100%. Blur window is macOS only. Popovers, dialogs and the terminal stay opaque.
- Everything must compile on Linux (CI runs `ubuntu-24.04`). Use `cfg!(target_os = "macos")`, not `#[cfg]`, around the blur row.
- Unit tests follow the repo's pattern: `#[cfg(test)] #[path = "../tests/unit/x.rs"] mod tests;`.
- Comments only where the why is genuinely hard. Match each file's style.
- Never stage, commit or push without the user's approval. Work on branch `appearance-customization`, never on `main`. Each phase ends by asking; there are no commit steps inside tasks. Commit messages carry no attribution lines.
- Set `CARGO_INCREMENTAL=0`. A worktree needs its own `CARGO_TARGET_DIR`.

## Review Focus

1. Switching from a scheme back to System: the window must follow the OS again, including later OS changes, so the forced appearance has to clear. Task 3.
2. Going from a dark scheme to a light one with the terminal open: the terminal chrome paints from the dark slot, which must return to Default Dark. Task 3.
3. A saved font that has since been uninstalled: UI falls back to the system font and the picker shows Default, with no crash. Task 5.
4. Moving the opacity slider repeatedly, changing scheme at low opacity, or a hand-edited `window_opacity` of 0 or 250: alpha must not compound, must survive the scheme change, and must clamp. Tasks 1 and 7.
5. Opening settings with Dracula active: the selected card starts off-screen and must scroll into view. Tasks 1 and 4.

---

## Phase 1: schemes and the card row

### Task 1: Settings model

**Files:**
- Modify: `crates/core/src/settings.rs`
- Test: `crates/core/tests/unit/settings.rs`

**Interfaces:**
- Produces, all re-exported from `aether_core` like `Appearance` is today:
  - `Appearance` variants in this order: `System, Dark, Light, CatppuccinLatte, CatppuccinFrappe, CatppuccinMacchiato, CatppuccinMocha, RosePineMain, RosePineMoon, RosePineDawn, GruvboxDark, GruvboxLight, SolarizedDark, SolarizedLight, Nord, Dracula`. `pub const ALL: [Self; 16]`, `label(self) -> &'static str` ("Catppuccin Frappé", "Rosé Pine Main"), `slug(self) -> &'static str` ("catppuccin-frappe", "rose-pine-main"), `is_dark(self) -> Option<bool>` (`None` for System).
  - `ThemeFamily { pub variants: &'static [Appearance], pub default: Appearance }`, `ThemeFamily::shown(&self, active: Appearance) -> Appearance`, `ThemeFamily::has_both_modes(&self) -> bool`, `pub const THEME_FAMILIES: [ThemeFamily; 9]`, `pub fn family_index(a: Appearance) -> usize`. Defaults: Frappé, Rosé Pine Main, Gruvbox Dark, Solarized Dark.
  - `CornerStyle { Square, Subtle, #[default] Rounded, Round }` with `ALL`, `label()`, `scale() -> f32` (0.0, 0.5, 1.0, 1.5).
  - `IconPack { #[default] Lucide, Iconoir, Remix, Solar }` with `ALL`, `label()`, `slug()`.
  - `Settings` fields `ui_font_family: String`, `corner_style: CornerStyle`, `icon_pack: IconPack`, `window_opacity: u8` (default 100), `window_blur: bool`, and `Settings::clamped_window_opacity(&self) -> u8`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn appearance_keeps_old_values_and_round_trips_every_scheme() {
    for (json, a) in [("\"system\"", Appearance::System), ("\"light\"", Appearance::Light), ("\"dark\"", Appearance::Dark)] {
        assert_eq!(serde_json::from_str::<Appearance>(json).unwrap(), a);
        assert_eq!(serde_json::to_string(&a).unwrap(), json);
    }
    for a in Appearance::ALL {
        assert_eq!(serde_json::from_str::<Appearance>(&serde_json::to_string(&a).unwrap()).unwrap(), a);
    }
    let slugs: std::collections::HashSet<_> = Appearance::ALL.iter().map(|a| a.slug()).collect();
    assert_eq!(slugs.len(), 16);
    assert_eq!(Appearance::System.is_dark(), None);
    assert_eq!(Appearance::RosePineDawn.is_dark(), Some(false));
    assert_eq!(Appearance::Nord.is_dark(), Some(true));
}

#[test]
fn theme_families_cover_every_appearance_once() {
    let listed: Vec<_> = THEME_FAMILIES.iter().flat_map(|f| f.variants.iter().copied()).collect();
    assert_eq!(listed, Appearance::ALL.to_vec());
    assert_eq!(family_index(Appearance::Dracula), 8);
    assert_eq!(family_index(Appearance::CatppuccinMocha), 3);
    assert_eq!(THEME_FAMILIES[3].shown(Appearance::Dark), Appearance::CatppuccinFrappe);
    assert_eq!(THEME_FAMILIES[3].shown(Appearance::CatppuccinMocha), Appearance::CatppuccinMocha);
    assert!(THEME_FAMILIES[3].has_both_modes());
    assert!(!THEME_FAMILIES[7].has_both_modes());
}

#[test]
fn settings_without_new_fields_load_with_defaults() {
    let s: Settings = serde_json::from_str(r#"{"appearance":"dark"}"#).unwrap();
    assert_eq!(s.appearance, Appearance::Dark);
    assert_eq!(s.corner_style, CornerStyle::Rounded);
    assert_eq!(s.icon_pack, IconPack::Lucide);
    assert_eq!(s.window_opacity, 100);
    assert!(!s.window_blur);
    assert!(s.ui_font_family.is_empty());
}

#[test]
fn window_opacity_clamps_hand_edited_values() {
    let at = |window_opacity| Settings { window_opacity, ..Settings::default() }.clamped_window_opacity();
    assert_eq!((at(0), at(80), at(250)), (60, 80, 100));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-core settings`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement the interfaces above**

Keep `#[serde(rename_all = "lowercase")]` on `Appearance`. The System, Dark and Light families have one variant each.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p aether-core settings`
Expected: PASS. `cargo check --workspace` may report non-exhaustive matches only if a new one appears; fix those in place.

### Task 2: Scheme palettes and legible semantic text

**Files:**
- Create: `crates/ui/src/schemes.rs`
- Modify: `crates/ui/src/lib.rs`, `crates/ui/src/theme.rs`, `crates/ui/Cargo.toml` (add `serde_json.workspace = true`)
- Test: `crates/ui/tests/unit/schemes.rs`, `crates/ui/tests/unit/theme.rs`

**Interfaces:**
- Consumes: `Appearance` from Task 1.
- Produces:
  - `aether_ui::schemes::SchemePalette` with public `u32` fields named as the table columns below.
  - `scheme_palette(a: Appearance) -> Option<&'static SchemePalette>`, `None` for System, Dark, Light.
  - `preview_palette(a: Appearance) -> &'static SchemePalette`, which also covers stock Dark and Light for thumbnails.
  - `scheme_config(a: Appearance) -> Option<Rc<ThemeConfig>>`, with `name == a.label()` and `mode` from `a.is_dark()`.
  - `aether_ui::theme::contrast(a: Hsla, b: Hsla) -> f32` (WCAG ratio, ignores alpha).
  - `aether_ui::theme::legible(color: Hsla, bg: Hsla) -> Hsla`: unchanged at 4.5:1 or above, otherwise moves lightness away from `bg` until it reaches 4.5:1.

Palette values, hex without `#`. They are the projects' published palettes, except that Nord and Dracula `muted` are mixes because neither publishes a readable mid-tone, and Rosé Pine Dawn's `primary` is Pine because Iris is too weak under text.

| Scheme | bg | panel | raised | border | fg | muted | primary | on_primary | red | green | yellow | orange | blue | magenta | cyan |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Latte | eff1f5 | e6e9ef | ccd0da | bcc0cc | 4c4f69 | 6c6f85 | 8839ef | eff1f5 | d20f39 | 40a02b | df8e1d | fe640b | 1e66f5 | ea76cb | 179299 |
| Frappé | 303446 | 292c3c | 414559 | 51576d | c6d0f5 | a5adce | ca9ee6 | 303446 | e78284 | a6d189 | e5c890 | ef9f76 | 8caaee | f4b8e4 | 81c8be |
| Macchiato | 24273a | 1e2030 | 363a4f | 494d64 | cad3f5 | a5adcb | c6a0f6 | 24273a | ed8796 | a6da95 | eed49f | f5a97f | 8aadf4 | f5bde6 | 8bd5ca |
| Mocha | 1e1e2e | 181825 | 313244 | 45475a | cdd6f4 | a6adc8 | cba6f7 | 1e1e2e | f38ba8 | a6e3a1 | f9e2af | fab387 | 89b4fa | f5c2e7 | 94e2d5 |
| Rosé Main | 191724 | 1f1d2e | 26233a | 403d52 | e0def4 | 908caa | c4a7e7 | 191724 | eb6f92 | 31748f | f6c177 | ebbcba | 9ccfd8 | c4a7e7 | ebbcba |
| Rosé Moon | 232136 | 2a273f | 393552 | 44415a | e0def4 | 908caa | c4a7e7 | 232136 | eb6f92 | 3e8fb0 | f6c177 | ea9a97 | 9ccfd8 | c4a7e7 | ea9a97 |
| Rosé Dawn | faf4ed | fffaf3 | f2e9e1 | dfdad9 | 575279 | 797593 | 286983 | faf4ed | b4637a | 286983 | ea9d34 | d7827e | 56949f | 907aa9 | d7827e |
| Gruvbox Dark | 282828 | 1d2021 | 3c3836 | 504945 | ebdbb2 | a89984 | fe8019 | 282828 | fb4934 | b8bb26 | fabd2f | fe8019 | 83a598 | d3869b | 8ec07c |
| Gruvbox Light | fbf1c7 | f2e5bc | ebdbb2 | d5c4a1 | 3c3836 | 7c6f64 | af3a03 | fbf1c7 | 9d0006 | 79740e | b57614 | af3a03 | 076678 | 8f3f71 | 427b58 |
| Solarized Dark | 002b36 | 073642 | 073642 | 586e75 | 93a1a1 | 839496 | 268bd2 | 002b36 | dc322f | 859900 | b58900 | cb4b16 | 268bd2 | d33682 | 2aa198 |
| Solarized Light | fdf6e3 | eee8d5 | eee8d5 | 93a1a1 | 586e75 | 657b83 | 268bd2 | fdf6e3 | dc322f | 859900 | b58900 | cb4b16 | 268bd2 | d33682 | 2aa198 |
| Nord | 2e3440 | 3b4252 | 434c5e | 4c566a | d8dee9 | 9ca2ae | 88c0d0 | 2e3440 | bf616a | a3be8c | ebcb8b | d08770 | 81a1c1 | b48ead | 8fbcbb |
| Dracula | 282a36 | 21222c | 44475a | 44475a | f8f8f2 | a5a6a7 | bd93f9 | 282a36 | ff5555 | 50fa7b | f1fa8c | ffb86c | 8be9fd | ff79c6 | 8be9fd |

Stock preview-only palettes (the rest of their fields can repeat these): Dark `bg 0a0a0a, panel 171717, raised 262626, muted a3a3a3, primary fafafa`; Light `bg ffffff, panel fafafa, raised f5f5f5, muted 737373, primary 171717`.

Roles onto `ThemeConfig` color keys, unlisted keys fall back to the mode's defaults:

| Role | Keys |
|---|---|
| bg | `background`, `list.background`, `table.background`, `accordion.background`, `group_box.background`, `tab.active.background`, `slider.thumb.background` |
| panel | `popover.background`, `sidebar.background`, `title_bar.background`, `status_bar.background`, `tab_bar.background`, `tab_bar.segmented.background` |
| raised | `muted.background`, `secondary.background`, `secondary.hover.background`, `secondary.active.background`, `accent.background`, `sidebar.accent.background`, `skeleton.background`, `switch.background` |
| border | `border`, `input.border`, `sidebar.border`, `title_bar.border`, `status_bar.border`, `window.border`, `table.row.border` |
| fg | `foreground`, `caret`, `popover.foreground`, `group_box.foreground`, `secondary.foreground`, `accent.foreground`, `sidebar.foreground`, `sidebar.accent.foreground`, `tab.active.foreground`, `tab.foreground` |
| muted | `muted.foreground`, `table.head.foreground`, `scrollbar.thumb.hover.background`; plus alpha `99` for `scrollbar.thumb.background` |
| primary | `primary.background`, `primary.hover.background`, `primary.active.background`, `sidebar.primary.background`, `ring`, `slider.bar.background`, `progress_bar.background`, `link.foreground`, `link.hover.foreground`, `link.active.foreground`, `list.active.border`, `drag_border`; plus alpha `4d` for `selection.background`, `33` for `list.active.background`, `19` for `drop_target.background` |
| on_primary | `primary.foreground`, `sidebar.primary.foreground` |
| red, green, yellow, blue | `danger`, `success`, `warning`, `info` `.background` and `.foreground`, in that order; `base.<color>` and `base.<color>.light` |
| magenta, cyan | `base.magenta`, `base.cyan` and their `.light` |

Highlight style: `editor.background` bg, `editor.foreground` fg, `editor.active_line.background` panel, `editor.line_number` muted, `editor.active_line_number` fg, `created.background`, `deleted.background`, `modified.background` from green, red, yellow with alpha `33`. Syntax: keyword magenta, string green, function blue, type yellow, constant, number and boolean orange, comment muted, attribute cyan, tag blue, property fg, title orange.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/ui/tests/unit/schemes.rs
fn themed(a: Appearance) -> Theme {
    let mut theme = Theme::default();
    theme.apply_config(&scheme_config(a).unwrap());
    theme
}

#[test]
fn every_scheme_applies_and_stays_legible() {
    for a in Appearance::ALL.into_iter().filter(|a| scheme_palette(*a).is_some()) {
        let t = themed(a);
        assert_eq!(t.is_dark(), a.is_dark().unwrap(), "{a:?}");
        assert_eq!(t.theme_name().as_ref(), a.label());
        assert!(contrast(t.foreground, t.background) >= 4.5, "{a:?}");
        assert!(contrast(t.muted_foreground, t.background) >= 3.0, "{a:?}");
        assert!(contrast(t.primary_foreground, t.primary) >= 3.0, "{a:?}");
    }
}

#[test]
fn stock_appearances_have_no_scheme() {
    for a in [Appearance::System, Appearance::Dark, Appearance::Light] {
        assert!(scheme_config(a).is_none());
    }
}

// crates/ui/tests/unit/theme.rs
#[test]
fn legible_only_moves_colors_that_fail() {
    let white: Hsla = rgb(0xffffff).into();
    let green: Hsla = rgb(0x40a02b).into();
    assert!(contrast(legible(green, white), white) >= 4.5);
    let dark_green: Hsla = rgb(0x15803d).into();
    assert_eq!(legible(dark_green, white), dark_green);
}

#[test]
fn semantic_text_keeps_stock_light_and_fixes_schemes() {
    let mut theme = Theme::default();
    theme.apply_config(&Rc::new(ThemeConfig { name: "Default Light".into(), ..Default::default() }));
    let stock = semantic_text(&theme, theme.success, 0x15803D);
    assert_eq!(stock, Hsla::from(rgb(0x15803D)));
    theme.apply_config(&crate::schemes::scheme_config(Appearance::CatppuccinLatte).unwrap());
    assert!(contrast(semantic_text(&theme, theme.success, 0x15803D), theme.background) >= 4.5);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui schemes && cargo test -p aether-ui theme`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement `schemes.rs` and the helpers**

The palette table was written from memory, so first check it against each project's published palette and correct any mismatch, keeping the noted exceptions. Build each config with `serde_json::json!` from the tables, then `serde_json::from_value::<ThemeConfig>`. In `theme.rs` add `fn semantic_text(theme: &Theme, token: Hsla, stock_light: u32) -> Hsla`: the hard-coded color when `theme.theme_name() == "Default Light"`, otherwise `legible(token, theme.background)`. Route `success_text`, `info_text`, `danger_text`, `warning_text` through it. Leave `mango`, `papaya`, `mango_text`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p aether-ui`
Expected: PASS.

### Task 3: Apply pipeline, title bar, terminal decoupling, command lists

**Files:**
- Modify: `crates/ui/src/schemes.rs`, `crates/app/src/app/mod.rs`, `crates/app/src/app/new.rs`, `crates/app/src/app/providers.rs`, `crates/app/src/palette.rs`, `crates/app/src/suggest/slash.rs`, `crates/terminal/src/view/mod.rs`, `crates/terminal/src/view/render.rs`, `crates/terminal/src/workspace/mod.rs`
- Test: `crates/ui/tests/unit/schemes.rs`, `crates/terminal/tests/unit/view/mod.rs`, `crates/app/tests/unit/palette.rs`

**Interfaces:**
- Consumes: `scheme_config`, `Appearance`.
- Produces:
  - `aether_ui::schemes::install_scheme(a: Appearance, cx: &mut App)`: through `Theme::update`, sets both slots to the registry's `default_light_theme()` and `default_dark_theme()`, then replaces the slot matching the scheme's mode.
  - `aether_ui::schemes::forced_appearance(a: Appearance) -> Option<WindowAppearance>`.
  - `pub(crate) fn apply_look(settings: &Settings, window: &mut Window, cx: &mut App)` in `crates/app/src/app/mod.rs`, replacing `apply_appearance`. Tasks 5 to 7 add lines to it.
  - `resolve_terminal_palette(terminal_theme: TerminalTheme) -> Palette`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/ui/tests/unit/schemes.rs
#[test]
fn forced_appearance_clears_for_system() {
    assert_eq!(forced_appearance(Appearance::System), None);
    assert_eq!(forced_appearance(Appearance::Nord), Some(WindowAppearance::Dark));
    assert_eq!(forced_appearance(Appearance::RosePineDawn), Some(WindowAppearance::Light));
}

#[gpui::test]
fn dark_to_light_scheme_resets_the_dark_slot(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        install_scheme(Appearance::Nord, cx);
        assert_eq!(Theme::global(cx).dark_theme.name.as_ref(), "Nord");
        install_scheme(Appearance::CatppuccinLatte, cx);
        assert_eq!(Theme::global(cx).dark_theme.name.as_ref(), "Default Dark");
        assert_eq!(Theme::global(cx).light_theme.name.as_ref(), "Catppuccin Latte");
    });
}

// crates/terminal/tests/unit/view/mod.rs: replaces the old app-color expectations
#[test]
fn aether_terminal_palette_ignores_the_app_theme() {
    let palette = resolve_terminal_palette(TerminalTheme::Aether);
    assert_eq!((palette.fg, palette.bg), (rgb(0xfafafa), rgb(0x0a0a0a)));
}

// crates/app/tests/unit/palette.rs: follow the file's helpers for building items
// assert the Theme group has 16 entries, including "Theme: Rosé Pine Moon"
```

Add a test in `crates/app/src/suggest/slash.rs`'s existing test module, or the file that covers it, asserting a `theme rose-pine-moon` command exists.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui schemes && cargo test -p aether-terminal palette`
Expected: compile errors.

- [ ] **Step 3: Implement `install_scheme` and `forced_appearance`**

- [ ] **Step 4: Replace `apply_appearance` with `apply_look`**

Order matters: `install_scheme`, then `cx.set_window_appearance(forced_appearance(a))`, then `Theme::sync_system_appearance(Some(window), cx)` for System or `Theme::change(mode, Some(window), cx)` otherwise. Clearing the override before the sync lets `window.appearance()` read the OS. Update the callers in `new.rs` (startup and the `observe_window_appearance` guard) and `set_appearance` in `providers.rs`.

- [ ] **Step 5: Pin the terminal**

Drop the `app_is_dark`, `app_fg` and `app_bg` parameters from `resolve_terminal_palette`. Aether Default always uses `#fafafa` on `#0a0a0a`, Default Dark's values, per the spec. Update both callers and the existing test expectations. A user on Light with the non-default Aether terminal theme moves from `#18181b` to `#0a0a0a`; mention this in the phase report.

- [ ] **Step 6: Generate the command lists**

In `palette.rs` and `slash.rs`, loop over `Appearance::ALL` with `a.label()` and `a.slug()`.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p aether-ui && cargo test -p aether-terminal && cargo test -p aether palette`
Expected: PASS.

### Task 4: Card components and the Theme section

**Files:**
- Create: `crates/ui/src/components/choice_cards.rs`
- Modify: `crates/ui/src/components/mod.rs`, `crates/ui/src/schemes.rs`, `crates/ui/src/settings.rs`, `crates/app/src/settings_view.rs`, `crates/app/src/settings_search.rs`
- Test: `crates/app/tests/unit/settings_view.rs`, `crates/app/tests/unit/settings_search.rs`

**Interfaces:**
- Consumes: `THEME_FAMILIES`, `family_index`, `preview_palette`, `set_appearance`.
- Produces, for Tasks 6 and 9:
  - `ChoiceCard::new(id, label).selected(bool).preview(impl IntoElement).label_element(AnyElement).on_click(handler)`: preview box, then label. `Role::Button`, `aria_label`, and `focus_visible` as in `crates/app/src/git_panel/list.rs:428`. Selected gets a 2px `foreground` ring and a bold label; others use `muted_foreground`.
  - `CardRow::new(cards)`: equal-width, no scrolling.
  - `CardCarousel::new(id, selected_ix).children(cards)`: `overflow_x_scroll`, keyed `ScrollHandle` via `use_keyed_state` as in `crates/ui/src/components/tabs.rs:427`, including its remembered index so `scroll_to_item(selected_ix)` runs only when the index changes. Arrows show when the handle can scroll that way; gradients from `background` to transparent fade the clipped edges.
  - `aether_ui::schemes::scheme_thumbnail(left: Appearance, right: Option<Appearance>) -> impl IntoElement`: sidebar with three bars, title bar, three tiles (first in `primary`), pill with a `primary` dot. `right` splits the card down the middle.
  - `SettingsSection::icon(Icon)` and `SettingsSection::bare()`: a header icon and no boxed surface, for card rows. Header icons not yet bundled go in `ExtraIcons` in `crates/app/src/assets.rs`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/app/tests/unit/settings_view.rs, in the existing window test
assert!(visual.debug_bounds("theme").is_some());
assert!(visual.debug_bounds("theme-card-nord").is_some());
assert!(visual.debug_bounds("theme-card-catppuccin-frappe").is_some());

// crates/app/tests/unit/settings_search.rs
#[test]
fn theme_search_finds_scheme_names() {
    for query in ["gruvbox", "dracula", "rose pine", "nord"] {
        assert!(search_settings(query).iter().any(|m| m.id == "theme"), "{query}");
    }
}
```

The card's `debug_selector` is `theme-card-{slug}`, where slug is that of the variant the card shows.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether settings`
Expected: FAIL on missing bounds and search hits.

- [ ] **Step 3: Implement the components and thumbnail**

Thumbnails split when `has_both_modes()`: the shown variant on its own side, the family's default or first variant of the other mode on the other. System splits stock Dark and Light. Only System follows the OS.

- [ ] **Step 4: Build the Theme section in `render_settings_category`**

Put a bare `SettingsSection` titled Theme at the top of the Appearance tab, keeping the `theme` selector and `show("theme")` guard. Card body click calls `set_appearance(family.shown(settings.appearance))`. A multi-variant card's `label_element` is a text button with a caret and `dropdown_menu_with_anchor`, reusing `settings_dropdown_menu`, listing the variants with `checked`. Selected index is `family_index(settings.appearance)`. Remove the old Theme dropdown row and `APPEARANCES`. Notifications and Watch-list stay in the Appearance section below.

- [ ] **Step 5: Update the search entry**

Description "System, dark, light, or a color scheme." Keywords add `catppuccin`, `rose pine`, `gruvbox`, `solarized`, `nord`, `dracula`. Check the terminal-theme entry still appears for "catppuccin".

- [ ] **Step 6: Run the tests**

Run: `cargo test -p aether settings`
Expected: PASS.

- [ ] **Step 7: Phase check**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS with no new warnings. Stop and ask the user whether to commit phase 1.

## Phase 2: font and corners

### Task 5: UI font

**Files:**
- Create: `crates/app/src/font_picker.rs`
- Modify: `crates/app/src/main.rs`, `crates/app/src/app/mod.rs`, `crates/app/src/app/new.rs`, `crates/app/src/settings_view.rs`, `crates/app/src/settings_search.rs`
- Test: `crates/app/tests/unit/font_picker.rs`, `crates/app/tests/unit/settings_view.rs`

**Interfaces:**
- Consumes: `Settings.ui_font_family`, `apply_look`, `aether_terminal::view::{installed_font_names_cached, resolve_terminal_family}`.
- Produces: `font_picker::FontItem { pub name: SharedString }` implementing `gpui_kit::component::select::SelectItem` (see `gpui-component-0.7.3/src/searchable_list/`), `font_picker::font_choices(installed: &[String]) -> Vec<FontItem>`, a `ChatApp.font_select: Entity<SelectState<SearchableVec<FontItem>>>` field.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/app/tests/unit/font_picker.rs
#[test]
fn font_choices_start_with_default_and_hide_private_families() {
    let installed = ["Zapfino", ".SFNS", "Arial", "Arial"].map(String::from);
    let names: Vec<_> = font_choices(&installed).into_iter().map(|f| f.name.to_string()).collect();
    assert_eq!(names, ["Default", "Arial", "Zapfino"]);
}

// crates/app/tests/unit/settings_view.rs
#[gpui::test]
fn missing_ui_font_falls_back_to_the_system_font(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    cx.update_window(*window, |_, window, cx| {
        let settings = aether_core::Settings { ui_font_family: "No Such Font".into(), ..Default::default() };
        crate::app::apply_look(&settings, window, cx);
        assert_eq!(cx.theme().font_family.as_ref(), ".SystemUIFont");
    }).unwrap();
}

#[gpui::test]
fn scheme_change_keeps_font_and_radius(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    cx.update_window(*window, |_, window, cx| {
        let family = aether_terminal::view::installed_font_names_cached(cx).first().cloned().unwrap_or_default();
        let settings = aether_core::Settings { ui_font_family: family.clone(), appearance: aether_core::Appearance::Nord, ..Default::default() };
        crate::app::apply_look(&settings, window, cx);
        if !family.is_empty() {
            assert_eq!(cx.theme().font_family.as_ref(), family);
        }
        assert_eq!(cx.theme().radius, px(6.));
    }).unwrap();
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether font`
Expected: compile errors.

- [ ] **Step 3: Implement `font_picker.rs`**

`font_choices` drops dot-prefixed families, sorts and dedups, and prepends "Default". `FontItem::value` is the family name, empty for Default. Its row renders with `.font_family(name)`, except Default.

- [ ] **Step 4: Apply the font in `apply_look`**

`Theme::update(cx, |t| t.font_family = resolved)`, where `resolved = resolve_terminal_family(&settings.ui_font_family, installed_font_names_cached(cx), ".SystemUIFont")`. Monospace is untouched.

- [ ] **Step 5: Add the Text section**

In `ChatApp::with_state`, build `font_select` with `SelectState::new(SearchableVec::new(font_choices(..)), selected, window, cx).searchable(true)`, where `selected` is the saved family's row or Default. On `SelectEvent::Confirm`, store the value and call `apply_look`. Add a `SettingsSection` titled Text with the icon `ALargeSmall`, rows Font (`Select::new(&state).search_placeholder("Search a font")`, selector `font`) and the existing Font size row moved unchanged. Add a `font` search entry.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p aether font && cargo test -p aether settings`
Expected: PASS.

### Task 6: Corners

**Files:**
- Modify: `crates/ui/src/theme.rs`, `crates/app/src/app/mod.rs`, `crates/app/src/settings_view.rs`, `crates/app/src/settings_search.rs`, and about 63 files under `crates/ui/src` and `crates/app/src` with literal radii
- Test: `crates/ui/tests/unit/theme.rs`, `crates/app/tests/unit/settings_view.rs`

**Interfaces:**
- Consumes: `CornerStyle::scale`, `CardRow`, `ChoiceCard`.
- Produces in `aether_ui::theme`: `set_corner_scale(scale: f32)`, `scale_length(r: AbsoluteLength, scale: f32) -> AbsoluteLength`, and `trait Corners: Styled + Sized { fn corner(self, r: impl Into<AbsoluteLength>) -> Self; fn corner_l(..); fn corner_r(..); }`, blanket-implemented for `Styled`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/ui/tests/unit/theme.rs
#[test]
fn scale_length_keeps_units() {
    assert_eq!(scale_length(px(10.).into(), 1.0), AbsoluteLength::Pixels(px(10.)));
    assert_eq!(scale_length(px(10.).into(), 0.0), AbsoluteLength::Pixels(px(0.)));
    assert_eq!(scale_length(rems(0.5).into(), 1.5), AbsoluteLength::Rems(rems(0.75)));
}

// crates/app/tests/unit/settings_view.rs, in the existing window test
assert!(visual.debug_bounds("corners").is_some());
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui theme`
Expected: compile errors.

- [ ] **Step 3: Implement the helpers**

Hold the scale in a process-wide `AtomicU32` of `f32` bits, default `1.0`, so call sites without `cx` work. Tests use `scale_length` directly to avoid racing on the global.

- [ ] **Step 4: Convert the call sites mechanically**

| From | To |
|---|---|
| `.rounded(px(N))` | `.corner(px(N))` |
| `.rounded_sm()` | `.corner(rems(0.25))` |
| `.rounded_md()` | `.corner(rems(0.375))` |
| `.rounded_lg()` | `.corner(rems(0.5))` |
| `.rounded_xl()` | `.corner(rems(0.75))` |
| `.rounded_l(px(N))` / `.rounded_r(px(N))` | `.corner_l(px(N))` / `.corner_r(px(N))` |

Leave `rounded_full`, `rounded_none`, `.rounded(size * k)` (brand mark, project avatar) and `.rounded(theme.radius…)`, which scale through the theme. Check `crates/app/src/git_panel/list.rs:434`'s `radius` before deciding. Then confirm:

Run: `grep -rnE '\.rounded(_sm|_md|_lg|_xl)\(\)|\.rounded(_l|_r)?\(px' crates/ui/src crates/app/src`
Expected: no output.

- [ ] **Step 5: Apply in `apply_look`, add the section**

`set_corner_scale(s)`, then `Theme::update(cx, |t| { t.radius = px(6. * s); t.radius_lg = px(8. * s); })`. Both defaults are the theme's own, so Rounded changes nothing. Add a bare section Corners, selector `corners`, with a `CardRow` of `CornerStyle::ALL`; each preview draws a top-left curve at `px(16. * style.scale())`. Add a `corners` search entry.

- [ ] **Step 6: Phase check**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS. Stop and ask the user whether to commit phase 2.

## Phase 3: opacity and window blur

### Task 7: Window translucency

**Files:**
- Modify: `crates/ui/src/theme.rs`, `crates/app/src/app/mod.rs`, `crates/app/src/app/new.rs`, `crates/app/src/session_workspace/windows.rs`, `crates/app/src/settings_view.rs`, `crates/app/src/settings_search.rs`
- Test: `crates/ui/tests/unit/theme.rs`, `crates/app/tests/unit/settings_view.rs`

**Interfaces:**
- Consumes: `Settings::clamped_window_opacity`, `Settings.window_blur`, `apply_look`.
- Produces in `aether_ui::theme`: `apply_surface_opacity(colors: &mut ThemeColor, opacity: u8)` and `window_background(opacity: u8, blur: bool) -> WindowBackgroundAppearance`. `ChatApp.opacity_slider: Entity<SliderState>`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/ui/tests/unit/theme.rs
#[test]
fn surface_opacity_does_not_compound_and_skips_popovers() {
    let mut colors = Theme::default().colors;
    apply_surface_opacity(&mut colors, 80);
    apply_surface_opacity(&mut colors, 70);
    assert!((colors.background.a - 0.7).abs() < 0.001);
    assert!((colors.sidebar.a - 0.7).abs() < 0.001);
    assert_eq!(colors.popover.a, 1.0);
}

#[test]
fn window_background_stays_opaque_at_full_opacity() {
    use WindowBackgroundAppearance as W;
    assert_eq!(window_background(100, true), W::Opaque);
    assert_eq!(window_background(80, false), W::Transparent);
    let blurred = if cfg!(target_os = "macos") { W::Blurred } else { W::Transparent };
    assert_eq!(window_background(80, true), blurred);
}

// crates/app/tests/unit/settings_view.rs
#[gpui::test]
fn scheme_change_keeps_translucency(cx: &mut gpui_kit::TestAppContext) {
    let window = settings_test_window(cx);
    cx.update_window(*window, |_, window, cx| {
        let settings = aether_core::Settings { window_opacity: 70, appearance: aether_core::Appearance::Nord, ..Default::default() };
        crate::app::apply_look(&settings, window, cx);
        assert!((cx.theme().background.a - 0.7).abs() < 0.001);
    }).unwrap();
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui theme`
Expected: compile errors.

- [ ] **Step 3: Implement the helpers**

`apply_surface_opacity` sets `.a` directly, never multiplies, on `background`, `sidebar`, `title_bar`, `status_bar`, `tab_bar`, `tab_bar_segmented`, `list`, `table`.

- [ ] **Step 4: Apply in `apply_look`**

After the theme change, `Theme::update(cx, |t| apply_surface_opacity(&mut t.colors, op))` with `op = settings.clamped_window_opacity()`. Then `window.set_background_appearance(window_background(op, settings.window_blur))`, and for each other window in `cx.windows()`, `handle.update(...)` ignoring `Err`, which only the window currently being updated returns. In `open_secondary`, call `set_background_appearance` from the `open_window` closure, reading `app.read(cx).settings`. `Opaque` at 100% also keeps today's subpixel text rendering, which GPUI disables for non-opaque windows.

- [ ] **Step 5: Add the Window section**

`SettingsSection` Window, rows Opacity (`Slider::new(&state)` with `SliderState::new().min(60.).max(100.).step(5.).default_value(..)`, plus a `{n}%` label, selector `window-opacity`) and, when `cfg!(target_os = "macos")`, Blur window (`Switch`, disabled at 100%, selector `window-blur`). Subscribe to `SliderEvent::Change` in `new.rs`, following the `cx.subscribe_in` calls there. Add both search entries.

- [ ] **Step 6: Phase check**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS. Stop and ask the user whether to commit phase 3.

## Phase 4: icon packs

### Task 8: Vendor the packs and route them

This task is independent of Tasks 1 to 7 except for `IconPack`, so it can run in parallel in its own worktree once Task 1 lands.

**Files:**
- Create: `scripts/vendor-icon-packs.py`, `scripts/icon-packs.json`, `crates/app/assets/icon-packs/{iconoir,remix,solar}/*.svg`, `crates/app/src/icon_packs.rs` (generated, sorted `pub(crate) static PACK_ICONS: &[(&str, &[u8])]`)
- Modify: `crates/app/src/assets.rs`, `crates/app/src/main.rs`, `crates/app/src/app/new.rs`, `crates/app/assets/icons/SOURCES.md`
- Test: `crates/app/tests/unit/assets.rs`

**Interfaces:**
- Consumes: `IconPack`.
- Produces in `crate::assets`: `set_active_icon_pack(pack: IconPack)` (backed by `OnceLock`), `pack_icon_path(pack: IconPack, name: &str) -> String` (`icons/pack/solar/house.svg`), `fn route(pack: IconPack, path: &str) -> Option<&'static [u8]>`, and `PREVIEW_ICONS: [&str; 5] = ["house", "message-square", "git-branch", "terminal", "search"]`.

- [ ] **Step 1: Write the mapping and the script**

`scripts/icon-packs.json` maps each Lucide name to `{ "iconoir": name|null, "remix": name|null, "solar": name|null }`. Cover every name in `gpui-kit-assets-0.7.3/default-icons.txt`, every `ExtraIcons` entry in `assets.rs`, and `PREVIEW_ICONS`. `null` means no honest equivalent, which falls back to Lucide. `PREVIEW_ICONS` must not be null, and any of them not already bundled get added to `ExtraIcons`, so the Lucide card can render them too.

The script runs `npm pack iconoir@7.12.1 remixicon@4.9.1 @iconify-json/solar@1.2.13` in a fresh temp directory and unpacks with `tar`, never `npm install`, so no package scripts run. Sources: Iconoir `icons/regular/<name>.svg`; Remix `icons/*/<name>.svg` using `-line` names; Solar `icons.json`, using `-linear` names, wrapped as `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">body</svg>`. It writes each file as `crates/app/assets/icon-packs/<pack>/<lucide-name>.svg`, fails on a mapped name that isn't found, and fails on any file containing `<script`, ` on*=` attributes, `foreignObject` or `href`. It regenerates `icon_packs.rs` with one `include_bytes!` line per file.

- [ ] **Step 2: Run it**

Run: `python3 -I scripts/vendor-icon-packs.py`
Expected: exits 0, prints per-pack counts.

- [ ] **Step 3: Write the failing tests**

```rust
#[test]
fn pack_routes_mapped_icons_and_falls_through_otherwise() {
    assert!(route(IconPack::Solar, "icons/house.svg").unwrap().starts_with(b"<svg"));
    assert!(route(IconPack::Lucide, "icons/house.svg").is_none());
    assert!(route(IconPack::Solar, "icons/definitely-not-an-icon.svg").is_none());
}

#[test]
fn provider_logos_never_swap() {
    for pack in IconPack::ALL {
        for provider in aether_core::Provider::ALL {
            assert!(route(pack, aether_ui::provider_icon_path(provider)).is_none());
        }
    }
}

#[test]
fn preview_icons_exist_under_their_own_paths_in_every_pack() {
    for pack in IconPack::ALL {
        for name in PREVIEW_ICONS {
            let bytes = AppAssets.load(&pack_icon_path(pack, name)).unwrap().expect(name);
            assert!(bytes.starts_with(b"<svg"), "{pack:?} {name}");
        }
    }
}

#[test]
fn vendored_icons_carry_no_active_content() {
    for (path, bytes) in crate::icon_packs::PACK_ICONS {
        let text = std::str::from_utf8(bytes).unwrap();
        for banned in ["<script", "foreignObject", "href"] {
            assert!(!text.contains(banned), "{path}: {banned}");
        }
    }
}
```

- [ ] **Step 4: Run them to verify they fail**

Run: `cargo test -p aether assets`
Expected: compile errors.

- [ ] **Step 5: Implement the routing**

`AppAssets::load` order: provider icons; `icons/pack/lucide/<name>` strips to `icons/<name>` and continues below with no routing; direct `PACK_ICONS` hit; `route(active, path)`; `ExtraIcons`; kit `Assets`. Look up with `binary_search_by_key`. `ChatApp::new`, not `with_state`, calls `set_active_icon_pack(settings.icon_pack)` before the first paint. Settings changes never call it again, which is what makes a change apply on next launch: GPUI's sprite atlas keys SVGs on path and size with no public way to clear it.

- [ ] **Step 6: Record the sources**

Add to `SOURCES.md`: Iconoir 7.12.1 MIT, Remix Icon 4.9.1 Apache-2.0, Solar via `@iconify-json/solar` 1.2.13 CC-BY-4.0 with credit to its authors, the renaming to Lucide names, and that the script rejects active content.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p aether assets`
Expected: PASS.

### Task 9: Icon pack section

**Files:**
- Modify: `crates/app/src/settings_view.rs`, `crates/app/src/settings_search.rs`
- Test: `crates/app/tests/unit/settings_view.rs`

**Interfaces:**
- Consumes: `pack_icon_path`, `PREVIEW_ICONS`, `CardRow`, `ChoiceCard`, `IconPack`.

- [ ] **Step 1: Write the failing test**

```rust
// in the existing window test
assert!(visual.debug_bounds("icon-pack").is_some());
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p aether settings`
Expected: FAIL.

- [ ] **Step 3: Add the section**

Bare section Icon pack, selector `icon-pack`, placed after Theme and before Corners, with the text "Applies on next launch." A `CardRow` of `IconPack::ALL`; each preview is the five `PREVIEW_ICONS` rendered from `pack_icon_path`, so cards show their pack whichever is active. Clicking only saves `settings.icon_pack`. Add an `icon-pack` search entry with keywords `lucide`, `iconoir`, `remix`, `solar`.

- [ ] **Step 4: Phase check**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS. Stop and ask the user whether to commit phase 4.

### Task 10: GUI verification

- [ ] **Step 1: Run the app and check each item, using the `run` skill**

- Defaults look identical to `main` side by side.
- Every scheme: sidebar, chat, composer, dialogs, a code block, a diff.
- Dark scheme to light scheme with the terminal open: terminal colors unchanged, chrome legible.
- Scheme back to System, then flip the OS mode: app and title bar follow.
- Opening settings on Dracula scrolls its card into view; arrows and edge fades behave.
- Font picker with several hundred fonts opens and filters without a visible stall.
- Square and Round across the app: nothing clips.
- Opacity 60% and Blur window on macOS across the main window, a session window and a pop-out. Terminal stays opaque. Nested panels read acceptably, since each layer's alpha stacks.
- Icon pack change across a restart, with provider logos unchanged.

- [ ] **Step 2: Report**

List each item as passed or failed. If `Blurred` has no effect on macOS, report it rather than leaving a dead toggle.
