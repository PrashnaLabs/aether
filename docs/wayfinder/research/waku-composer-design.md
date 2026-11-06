# Waku composer: design description

Source: `reference/src/app/composer.rs`, `src/ui/mod.rs` (MenuChip), `src/ui/menu.rs`, `src/theme.rs`, `locales/app.yml`, `crates/waku-protocol/src/model.rs`. Prose only, for an original reimplementation. Px values are authored at a 14px UI font; text sizes scale with the UI font setting.

## 1. Composer layout

Stack order in the chat column, bottom to top: workspace footer (project / branch chips), composer card, queued-message tray (attached above the card, see section 5).

| Part | Spec |
|---|---|
| Outer wrapper | 20px horizontal margin, content capped at 720px wide, centred |
| Card | radius 13, 1px `border`, fill `composer` (white in light, near-black grey in dark), vertical padding 10, no horizontal padding on the card itself (rows and the text field own their insets so the field's scrollbar hugs the card edge) |
| Attachment row (only when files staged) | above the field, 14px side padding, 2 top / 8 bottom, wrapping, 8px gap |
| Text field | 2px top pad, 14px horizontal inset inside its scroll viewport, multi-line, auto-height, grows to 300px then scrolls. Enter sends. Placeholder "Do anything…" in `text_ghost`. Caret uses accent. |
| Footer row | 8px below field, 10px side padding, one flex row, 4px gap between items, items vertically centred, base text 12.5 |
| Drag-over files | card fill becomes composer blended with `overlay_strong`, border becomes accent at 70% |
| Autocomplete popup | anchors to the card bounds, takes arrows/enter/tab/escape as actions while it has rows |

Footer row left to right: provider/model chip, model-traits chip (only when the model has traits), agent-preset chip (DeepSeek only, before session starts), access chip, goal chip (only while a goal exists), flexible spacer, send/stop cluster on the right. There is no attach button, no context meter, no token counter in the composer; files arrive via drop, paste, or `@` mention. Attachments are handled in section 5.

### Chip look (shared MenuChip)

| Property | Value |
|---|---|
| Height / padding | 26 high, 7px horizontal |
| Radius | 6 |
| Border | none by default (an "outlined" variant exists: 30 high, 10px pad, radius 7, 1px `border_strong`, `raised` fill; not used in the composer footer) |
| Background | transparent; `overlay` on hover; `overlay` also while its menu is open (open state is a soft fill, not a border) |
| Content | optional 12px icon, 6px gap, label 13px (line 16) in `text_secondary`, truncating |
| Caret | none on every composer chip (`caret(false)`); the generic default is a 10.5px chevron-down in `text_ghost` |
| Focus ring | keyboard focus only: 1px border in `accent` |
| Disabled | 70% opacity, no hover fill |
| Cursor | default arrow, not pointer |

Non-interactive provider/model label (when the session cannot choose a model): plain 24-high, 7px pad, 6px gap, 10.5px provider mark at 90% provider colour plus model name in `text_secondary`, max 210 wide, no hover.

## 2. Provider / model picker

Trigger: chip with the provider mark (12px, provider colour at 90%) and the selected model's display name. No caret. Open uses `selected` fill. If no provider is installed or enabled: alert icon in `warning` plus the label "No providers", still clickable.

Popover: opens above the trigger, left edges aligned. Size 460 x 390, radius 13, 1px `border_strong`, fill `raised`, large shadow, clips content.

**Rail** (left column): 50 wide, `canvas` fill, 1px `border` on its right edge, 5px padding, 4px gap, items centred, only left corners rounded (12).
- Tab buttons 38 x 38, radius 7. Selected fill `overlay_strong`, hover `overlay`.
- First tab is Favorites: star outline icon 17px, `text` when selected, `text_tertiary` otherwise. Below it a 34 x 1 divider in `border`, 3px vertical margin.
- Then one tab per installed, enabled provider (provider mark 18px in provider colour: 100% when selected, 82% otherwise). Providers that are not installed or switched off in settings are omitted entirely, not dimmed.
- Locked tabs: when the session already has messages, only its own provider is usable; other tabs stay visible at 35% opacity, no hover, no click.
- No tooltips on rail tabs.
- Selection is hidden while a search query is non-empty (search spans all tabs).

**Right pane**: `surface` fill, right corners rounded 12, column layout.
- Search: 52 high container (padding 12 sides, 10 top, 8 bottom) holding a 34 high field, radius 9, `raised` fill, 10px side pad, 8 gap, 15px search icon in `text_secondary`, then the text input. Focused automatically on open.
- List: scrollable, 9px padding, overlay scrollbar. Rows are 58 high, 12px side pad, radius 9, 10 gap, each with a reserved 1px transparent border so highlight cannot shift layout.
  - Left: model name 13px semibold `text`, truncating; below (4px gap) a 10.5px provider mark at 85% plus subtitle 12.5px `text_tertiary`. Subtitle is "Sub-provider · Provider", or just the provider name when they match case-insensitively or no sub-provider exists.
  - Right: 28 x 28 star button (radius 6, hover `overlay_strong`): outline star 14px `text_ghost`, or filled star in `favorite` (amber) when favourited. Clicking it toggles favourite without selecting.
  - Current model row: fill `overlay_strong`. Hover: `overlay`. Press: 85% opacity. Keyboard-highlighted row: `overlay` fill plus 1px `accent` ring (a ring so it reads on top of the selected fill).
  - No check mark and no "default" tag in picker rows.
- Empty states, centred 12.5px `text_ghost`: "No models found" (search), "Star a model to keep it here" (favorites), "Loading models…" (discovery pending), "No models reported by this provider".
- No-provider state replaces the whole popover: 320 wide card, radius 13, centred column, 24/22 padding, 9 gap. 40px rounded-10 `overlay` tile with a bot icon, title "No providers available" (12.5 medium `text`), body "Install an agent CLI, or switch one back on in Provider settings." (12.5, line 17, `text_secondary`, centred), and a 28-high outlined button "Open Provider Settings" (radius 7, 1px `border_strong`).

**Behaviour**
- Open: tab resets to the session's provider (or first enabled provider if the draft's provider was switched off), highlight cleared, search cleared, that provider's model list re-discovered, list scrolled so the current model is in view (re-issued two frames later because the first paint has no real bounds). Search takes focus two frames after open.
- Close: picking a row, Enter, Escape, or click-away; focus returns to the composer field.
- Filtering: whitespace-separated tokens, all must appear in name, id, provider name, or sub-provider (case-insensitive). Favorites tab ordered by favouriting order.
- Keys (all while focus stays in the search field): Down/Up move the highlight and wrap, scrolling it into view; Enter picks the highlighted row (first row if none highlighted yet) and closes; Tab/Shift-Tab cycle rail tabs (Favorites included, locked tabs skipped, wraps); tab cycling is a no-op while a query is typed; Escape closes.

## 3. Model-traits chip

Shown only when the current model reports at least one of: reasoning efforts, service tiers, context windows. Otherwise absent.

- Trigger: standard chip, no caret. Optional 12px lightning-bolt icon in `text_secondary`, shown only when the service tier is "fast". Label is the effort label (falls back to the tier label if the model has no efforts), and when the context window is non-default it appends " · " plus the window label, e.g. "High · 1M".
- Effective effort resolution: session choice, else provider-suffix hint (Cursor), else model default, else first option. Tier defaults to "default" shown as "Standard".
- Menu: standard dropdown card (see below), opens above-left. Sections in order, each present only if the model has options, separated by a hairline: header "Reasoning" with one row per effort; header "Service Tier" with "Standard" first then the model's tiers (e.g. "Fast"); header "Context Window" with options (e.g. "200K", "1M").
- Effort labels: None, Minimal, Low, Medium, High, Extra High, Max, Ultra, Ultracode; unknown ids are shown with dashes/underscores turned into spaces. Rows carry no descriptions in the menu (tier "Fast" has a description string in the catalogue, "Faster responses at a higher usage rate", but the menu does not render it).
- Row: 190 wide, 2px vertical pad, 10 gap, label in `text_secondary`, then optional "Default" tag (18 high, 5px pad, radius 4, 1px `border_strong`, `overlay` fill, 12.5 semibold `text_tertiary`) on the model default, then a 11px check in `text_tertiary` on the selected one. Header text 12.5 medium `text_tertiary`.
- Keyboard: standard menu keys (below).

### Dropdown menu card (shared)

Min 176 / max 320 wide, 4px vertical pad, radius 9, 1px `border_strong`, `raised` fill, large shadow. Rows: 4px side margin, 8px pad, min 26 high, radius 6, 8 gap, 12.5 text; hover `overlay`, keyboard highlight `overlay_strong`. Separator 1px `border`, 4px vertical / 6px horizontal margin. Header: 10px side pad, 6 top / 2 bottom. Keys: Up/Down move with wrap, Home/End jump, Enter/Space activate, Right/Enter opens a submenu, Left closes it, Escape dismisses. Menu closes then runs the action on mouse-down.

## 4. Access chip

Always shown. Trigger: mode icon 12px in `text_tertiary` plus mode label, no caret. Menu opens above-left; each row is a custom 288-wide layout with 4px vertical pad, 10 gap: 14px icon in `text_tertiary`, a text column, and an 11px check in `text_tertiary` when selected. Text column: label 12.5 (semibold if selected, medium otherwise) in `text`, then description 12.5 / line 14, 2px above, wrapping, `text_tertiary`. No warning tint anywhere, Full access included; the icon change (closed lock to open lock) is the only signal.

| Mode | Label | Description | Icon file |
|---|---|---|---|
| Ask (default) | Supervised | Ask before commands and file changes | lock |
| AutoAcceptEdits | Auto-accept edits | Auto-approve edits, ask before other actions | pencil |
| Auto | Auto | An AI reviewer approves routine actions; risky ones still ask | sparkle |
| FullAccess | Full access | Allow commands and edits without prompts | lock-open |

## 5. Other composer controls

- **Send** (idle): 26px circle, fill `inverse` (light disc in dark theme, dark disc in light) with 16px arrow-up in `on_inverse`; hover 90% opacity, press 80%. Without a draft (or no providers) it is `overlay_strong` with `text_ghost` arrow, no hover, not clickable. When no providers: tooltip "Install or switch on a provider to send".
- **Preparing**: same circle, `overlay_strong`, spinning loader-circle 15px `text_secondary`, tooltip "Preparing task…".
- **Stop** (turn running): 26px circle, `overlay_strong`, 18px stop icon in `text`; hover fill `danger_soft`. After the first Escape press the icon is replaced by the text "Esc" (12.5 semibold) to show the second press will stop ("Press Escape again to stop").
- **Queue follow-up** (while running and a draft exists): second 26px `inverse` circle with arrow-up, 6px gap right of the stop button, tooltip "Queue as follow-up (⏎)".
- **Queue tray** (above card): attached to the card's top edge, same width as the card inset by 14px each side, top corners 12, borders on top/left/right only, `composer` fill. Rows 30 high, 12 left / 6 right pad, 9 gap, hover `overlay`; queue icon 12px `text_tertiary`, message text 12.5 `text` truncating (falls back to attachment names). Right controls: optional "Steer" button (24 high, 7px pad, radius 6, corner-down-right icon 11px, hover `overlay_strong`, tooltip "Steer current turn"; shown only when the provider can steer), trash button 24 square (tooltip "Remove from queue"), ellipsis button 24 square opening a menu: "Edit in composer" (pencil), "Remove from queue" (trash). Clicking the row loads it back into the composer.
- **Attachment tiles**: 64 x 64, radius 8, 1px `border`, `inset` fill; images fill the tile; files show 16px type icon plus truncated name (12.5 `text_tertiary`). 16px remove button top-right (3px inset, radius 6, `canvas` at 80%, 9px x icon). Focus ring accent. Tooltip shows `@mention`.
- **Agent-preset chip** (DeepSeek, before first message only, hidden if no presets): bot icon 12px `text_tertiary` plus preset name; menu rows 340 wide, name 12.5 semibold/medium plus wrapped description, check on selected; custom presets are suffixed " · Custom".
- **Goal chip**: 24 high, 7px pad, radius 6, target icon 10.5px plus status phrase in a status colour, max 220 wide, hover `overlay`, tooltip is the goal text.
- **Context meter**: none in the composer; Waku keeps usage meters in a separate panel (`gauge` blue token).

## 6. Theme tokens

Names used by the composer: `canvas` (page and rail), `surface` (picker right pane), `raised` (menus, search field, popover), `composer` (card), `inset` (attachment tile), `overlay` and `overlay_strong` (hover and selected fills, 5% and 9% neutral wash), `border` (card, hairlines, 7-8% neutral), `border_strong` (popover and menu outline, 14-15%), `text`, `text_secondary` (chip labels, menu rows), `text_tertiary` (icons, subtitles), `text_ghost` (placeholder, empty states, idle star), `accent` (focus ring, keyboard highlight ring, caret), `inverse` / `on_inverse` (send button), `favorite` (filled star), `warning` (no-providers alert), `danger_soft` (stop hover), `gauge` (usage panel only).

Light vs dark: palette is neutral graphite either way, colour reserved for meaning. Dark: canvas ~#1A1A1A, composer ~#212121, raised ~#232323, accent coral ~#E2795B. Light: canvas ~#F6F5F6, composer white, raised ~#ECECEC, accent ~#C85F44. Washes and borders flip from light-on-dark to dark-on-light at the same alphas. Inverse send disc is near-white in dark, near-black in light. Provider colours: Claude orange, Amp red, DeepSeek blue, all others a neutral (near-white in dark, charcoal in light).

## Icon file names used

zap, lock, lock-open, pencil, sparkle, bot, star, star-filled, search, check, alert, arrow-up, stop, loader-circle, queue, corner-down-right, trash, ellipsis, x, target, chevron-down, folder, file-types/image, and `provider-*` marks (claude, openai, amp, cursor, deepseek, fx, grok, kimi, ohmypi, opencode, pi).
