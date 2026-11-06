# Appearance customization: design

Date: 2026-10-30
Status: approved 2026-10-30

## Intent and why

Aether's look is fixed today: a Theme dropdown with System, Light and Dark, and nothing else. The user wants the personal control shown in their reference screenshots of another app's Appearance settings: a row of visual theme cards, icon packs, corner styles, a font picker and window translucency.

The user gave no reason beyond the screenshots. The earlier terminal themes spec records a daily workflow built on Catppuccin in tmux and Neovim, so a guess is that the app should be able to sit comfortably next to that setup.

The screenshots are on the user's machine, outside the repo, in `~/.config/aether/attachments/1793026545808/`: `2-Pasted image 1.png` and `3-Pasted image 1.png` show the theme row, `4-Pasted image 1.png` shows the other sections.

## Goals

Settings > Appearance gains these sections, top of page, in this order. Everything takes effect immediately except icon packs. Everything persists.

### Theme

A horizontal row of preview cards. Arrows appear when the row overflows, and the clipped edges fade. Each thumbnail is a miniature app (sidebar, title bar, three tiles, input pill) painted from that scheme's real colors. The selected card gets a white ring and a bold label; other labels are dimmed.

Cards, in order:

- System: follows the OS between Default Light and Default Dark
- Dark: today's Default Dark
- Light: today's Default Light
- Catppuccin: Latte, Frappé, Macchiato, Mocha
- Rosé Pine: Main, Moon, Dawn
- Gruvbox: Dark, Light
- Solarized: Dark, Light
- Nord
- Dracula

Multi-variant cards show a chevron on the label that opens a menu of variants. Clicking the card body applies the variant named on the label.

A scheme recolors the whole app, including code highlighting. It does not touch the terminal's colors.

### Icon pack

Four cards: Lucide (today's icons, default), Iconoir, Remix, Solar. Each card previews five icons from its own pack. Changing the pack applies on next launch, and the row says so.

### Corners

Four cards: Square, Subtle, Rounded (default), Round. Each card draws a corner curve at that setting, as in the reference.

### Text

- Font: a dropdown that opens a searchable list of installed fonts, each row drawn in its own font. Default means today's system font. Applies to UI text only; monospace surfaces keep the monospace font.
- Font size: the existing control, moved here unchanged.

### Window

- Opacity: how see-through the app's surfaces are, from 60% to 100%. 100% is today's opaque window and the default.
- Blur window: blurs the desktop behind a translucent window. Does nothing visible at 100% opacity, so it is disabled there. macOS only; the row is not shown elsewhere.

## Non-goals

- Blur UI (blurring panels over app content). GPUI has no way to blur behind an element, and the user declined a renderer fork.
- Adaptive theme. In the reference, a music player, this is assumed to tint from album art. Aether has nothing equivalent to tint from.
- User-authored theme files, custom accent pickers, per-project themes.
- Any change to the terminal color picker or new terminal palettes.
- The reference's other tabs and any scheme past Dracula. The reference's arrow implies more cards, but they are unreadable in the screenshot.

## Anti-goals

- A scheme changing terminal cell colors or terminal background. The user was explicit that the terminal color scheme is a separate thing.
- A visible change for anyone who touches nothing. Defaults (System, Lucide, Rounded, default font, 100% opacity) must render as today. The one accepted exception: a saved Light or Dark on an OS in the other mode now gets a matching title bar.
- Illegible text in any scheme at full opacity. Below 100%, legibility depends on what is behind the window, which Aether can't control, so the slider's floor is the only guard.
- New runtime dependencies. Vendored data files are fine.

## Constraints

Facts found in the code and its dependencies. A builder should not need to rediscover these.

- `gpui-component` already has the theme machinery: `ThemeConfig` JSON (about 95 color keys plus a highlight style), a light slot and a dark slot on `Theme`, `Theme::apply_config`, `Theme::change`. Missing keys fall back to `ThemeColor::dark()` or `light()` by mode. The defaults live in its `src/theme/default-theme.json`.
- `crates/app/src/app/mod.rs` `apply_appearance` is the one place the app applies a theme, and `set_appearance` in `crates/app/src/app/providers.rs` is the one place it persists. Slash commands (`crates/app/src/suggest/slash.rs`) and the command palette (`crates/app/src/palette.rs`) each hardcode a list of the three appearances.
- The terminal is always rendered dark. `crates/terminal/src/dark.rs` paints its chrome from `Theme.dark_theme` while the app is light, so a light scheme must leave the dark slot on Default Dark, and switching from a dark scheme to a light one must reset that slot.
- `resolve_terminal_palette` in `crates/terminal/src/view/mod.rs` takes foreground and background from the app theme for the "Aether Default" terminal theme when the app is dark. With schemes, that would leak Nord's background into the terminal, so it must be pinned to Default Dark's values.
- `crates/ui/src/theme.rs` hardcodes Tailwind text colors for light mode (`success_text`, `info_text`, `danger_text`, `warning_text`). These will look foreign on Latte, Dawn and the other light schemes.
- Brand colors stay as they are: Hacker News orange, provider colors in the usage view, `mango`, `papaya`.
- `Theme.radius` and `radius_lg` only reach kit widgets and a few app sites. The app has about 160 hard-coded radii (`.rounded(px(..))` about 106, plus `rounded_md`, `rounded_lg`, `rounded_xl`, `rounded_sm`). `rounded_full` means pill or circle and should not scale.
- GPUI's `Root` applies `Theme.font_family`, so setting it reaches all UI text. `Select` has `.searchable(true)`. `aether_terminal::view::installed_font_names_cached` already lists installed fonts, and `resolve_terminal_family` shows the existing handling for a font that is no longer installed.
- Icons load through one seam, `AppAssets::load` in `crates/app/src/assets.rs`. GPUI's sprite atlas keys SVGs on path and size with no public way to clear it, so serving different bytes at the same path repaints nothing until restart. That is why icon packs apply on next launch. Pack files served at their own paths can still render live, which is what the preview cards need.
- The app uses about 74 icon names, plus `ExtraIcons`, plus whatever kit widgets use internally. Provider logos must never swap.
- `crates/app/assets/icons/SOURCES.md` records source, license and changes for each vendored icon, and that no file contains scripts, event attributes, foreignObject or hrefs. Vendored packs follow this.
- `window.set_background_appearance` accepts `Opaque`, `Transparent` and `Blurred`. GPUI documents `Blurred` as "not always supported", and it has not been run on macOS here yet. Windows are opened in `crates/app/src/main.rs` and `crates/app/src/session_workspace/windows.rs`, and the setting must reach every open and newly opened window.
- Both windows use the native title bar (`appears_transparent` is false), and the app never calls `cx.set_window_appearance`. AppKit draws that bar, so it follows the OS rather than the app theme. `appears_transparent` is a window-open option, so changing it affects every user.
- CI builds on `ubuntu-24.04` and the README documents Linux, so everything must compile there. On Linux, GPUI only handles `Blurred` on Wayland through KDE's protocol, with no public way to ask whether blur is supported.
- Settings load with `#[serde(default)]`, so new fields need defaults and existing settings files must keep parsing.
- Settings search needs entries in `crates/app/src/settings_search.rs` for each new row, following the existing `id` plus `debug_selector` pattern.

## Decided how

From the user:

- The scheme list and card treatment, from the screenshots.
- The terminal color scheme stays separate.
- Icon pack, corners, font, font size, opacity and window blur are in. Blur UI is out.
- The native title bar stays. `cx.set_window_appearance` matches it to the active scheme's light or dark mode (`None` for System), so it stays system grey and solid rather than taking the scheme's color or going translucent. No custom title bar.

The author's calls, accepted by the user:

- "Leave the Blur out" was read as Blur UI only. Blur window stays because GPUI supports it.
- Variant lists are the standard published ones; the screenshots show only the chevrons. Cards show Frappé, Main, Dark and Dark by default, as in the screenshots.
- Only the System card follows the OS. Picking a family's dark variant stays dark, even though some reference thumbnails are split light and dark.
- The last-used variant is not remembered per family. A card returns to its default variant when another card is selected.
- Extend the existing `Appearance` enum with the new schemes rather than adding a second setting. Slash commands, the palette and the settings row already speak `Appearance`, and `system`, `light` and `dark` keep serializing as they do.
- Slash commands and the command palette list every scheme.
- Rounded is today's look and the default.
- Scale factors for corners start at 0, 0.5, 1 and 1.5 of today's radii.
- Terminal stays fully opaque at any opacity, so Neovim stays legible. Popovers and dialogs stay opaque too. Opacity applies to the window background and panel surfaces.
- One opacity floor for all schemes, 60%. Holding 4.5:1 over a worst-case wallpaper would need floors from about 48% to 99% depending on the scheme, leaving Solarized almost no slider travel. The contrast test therefore runs at full opacity only, and low opacity over a bright wallpaper can read poorly by the user's own choice.
- Solarized Light uses base01 for body text, because its usual base00 is only 4.13:1 on its background and fails the contrast test.
- Blur window is macOS only. Opacity stays available on Linux without a GUI pass there.
- Terminal chrome text and borders may follow the app scheme, since chrome is app UI. Only cell colors and terminal background are locked.
- Font setting covers UI text only.
- Build order, each phase shippable alone: (1) schemes and the card row, (2) font and corners, (3) opacity and window blur, (4) icon packs.

## Verification

- Every scheme parses and applies. A data-driven test asserts foreground on background is at least 4.5:1 for every scheme.
- The Aether Default terminal palette is identical under every scheme.
- Old settings files load, and `system`, `light`, `dark` round-trip unchanged.
- Each icon the app uses resolves in each pack or falls back to Lucide.
- GUI checks by running the app: each scheme, a dark-to-light scheme switch with the terminal open, opacity and blur on macOS across all window types, and a pack change across a restart. If `Blurred` has no effect on macOS, report it rather than shipping a dead toggle.

## Left to the builder

- How scheme data is stored: JSON files, or a compact palette struct expanded into `ThemeConfig`. Colors come from each project's published palette; the mapping onto roles is the builder's.
- How to fix the light-mode text helpers in `crates/ui/src/theme.rs`, within the contrast test.
- Whether corners and font survive `Theme::change`, and re-applying them if not. Same for the semantic radius tokens.
- Whether opacity is applied by adjusting tokens after each theme change or another way.
- Names and locations of the card row component and the corner helper. Exact card sizes, fade and arrow treatment, matching the reference.
- Section header styling, and where the remaining Appearance rows (notifications, watch-list button) sit below the new sections.
- Which five icons the pack cards preview, and how to fetch the SVGs without running package scripts.
- Fallback when a saved font is no longer installed, following the terminal's handling.
- Keyboard access for cards, following existing accessibility labels.
