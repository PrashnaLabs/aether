# Settings UI Revamp: Two-Pane Master-Detail Design

## Intent & The Why

Aether's current settings interface constrains all content within a narrow single column (`Column::new()`) and squeezes 6 navigation categories into a cramped horizontal segmented pill bar (`Segmented::new("settings-tab")`) at the top of the screen.

This creates several usability and aesthetic issues:
1. **Constrained Width:** Wide settings (such as CLI agent binary paths, model override fields, custom instruction textareas, and git worktree paths) are truncated or awkwardly wrapped within a narrow ~560px column.
2. **Cramped Navigation:** 6 categories forced into a horizontal segmented control create visual clutter and an unappealing, crowded header.
3. **Desktop UX & Accessibility Mismatch:** Standard desktop applications (macOS System Settings, VS Code, Zed) use a two-pane master-detail pattern with vertical sidebar navigation, distinct focus rings, and keyboard accessibility ($\uparrow$/$\downarrow$ navigation).

This revamp replaces the single-column layout with a spacious, accessible **Two-Pane Master-Detail** layout.

---

## Goals, Non-Goals, and Anti-Goals

### Goals
- **Two-Pane Master-Detail Layout:**
  - Left sidebar (fixed 240px) with navigation header ("← Back to Chat"), optional filter/search input, and grouped vertical category tabs with Lucide icons.
  - Right canvas (`flex-1`, scrollable, comfortable max-width of ~860px with generous padding) eliminating awkward stretching on ultrawide monitors while providing ample room for multi-column inputs and descriptions.
- **Visual Polish & Component Modernization:**
  - Clean card containers (`SettingsSection` / `SettingCard`) with subtle borders and clear separation.
  - Settings rows (`SettingsRow`) with primary label, secondary contextual description, and right-aligned or stacked controls.
- **Accessibility & Keyboard Navigation:**
  - Full keyboard accessibility: $\uparrow$/$\downarrow$ arrow keys navigate sidebar items; `Tab` moves focus into active content controls; `Escape` returns to chat.
  - Explicit ARIA attributes (`role="tablist"`, `role="tab"`, `role="tabpanel"`, `aria-selected`, `aria-checked`).
  - High-contrast, WCAG 2.2 compliant `:focus-visible` rings across all interactive controls.
- **Preserve All Existing Settings & Functionality:**
  - Appearance (theme, font scaling, notifications, terminal color theme, terminal font size, terminal font family with Nerd Font detection, Option as Meta key).
  - Model (default model selector, reasoning budget, web search switch, custom instructions textarea).
  - Providers (workspace root picker, CLI agent probes, binary overrides, default models, default access modes).
  - Worktrees (worktree branch listing, open/delete/prune actions).
  - Tools (approval policies for web search and skills, plan-first switch).
  - Account (secure Keychain API key input with validation status).

### Non-Goals
- Adding new setting capabilities or backend settings data fields that do not currently exist in `aether_core::Settings`.
- Changing the storage backend (settings remain stored in `Settings` JSON and keychain via `keyring`).
- Restructuring the global application pages (`Page::Chat`, `Page::Skills`, `Page::Agents` remain accessible via the app sidebar).

### Anti-Goals
- Sticking with horizontal segmented tabs at the top of the settings page.
- Constraining the canvas to the narrow `Column` container.
- Introducing modal dialogs or multi-level popup wizards for simple settings.

---

## Technical Constraints & Decisions

### 1. Framework & Platform
- **Framework:** GPUI (`gpui-kit` / GPUI components).
- **Styling:** GPUI styles (`ActiveTheme`, `h_flex`, `v_flex`, `rounded_xl`, `border_1`, `border_color`).
- **Icons:** GPUI kit `IconName` and Lucide icons (`gpui_kit::assets::IconName`).

### 2. Component Architecture
- Update `crates/ui/src/settings.rs` to support the master-detail pattern:
  - Add `SettingsSplit` or update `SettingsPage` to support a sidebar slot and content header:
    ```rust
    pub struct SettingsPage {
        title: SharedString,
        on_back: Option<ClickHandler>,
        sidebar: Option<AnyElement>,
        content_header: Option<SharedString>,
        actions: Vec<AnyElement>,
        children: Vec<AnyElement>,
    }
    ```
  - **Layout Split:**
    - Left Sidebar (240px fixed): Hosts the "← Back to Chat" button, optional search input, and vertical category tab list.
    - Right Content Canvas (`flex-1`, scrollable, max-width ~860px): Features a dedicated content header displaying the active tab's title, descriptive subtitle, and tab-level action buttons (such as the "Refresh" buttons for `Providers` and `Worktrees`), followed by the category's setting cards.
  - Existing uses of `SettingsPage` without a sidebar (e.g., `agents_view.rs` or `skills_view.rs`) continue to render as single-column layouts seamlessly.

### 3. Navigation & State
- `SettingsTab` in `crates/app/src/settings_view.rs`:
  - Tab variants remain: `Appearance`, `Model`, `Providers`, `Worktrees`, `Tools`, `Account`.
  - Sidebar renders a vertical list of navigation items:
    - Group 1: General (`Appearance`, `Model`, `Providers`)
    - Group 2: Developer (`Worktrees`, `Tools`)
    - Group 3: Account & Security (`Account`)
  - Arrow key navigation (`KeyDownEvent`) allows moving up and down the tab list with wrap-around.
  - Active tab is highlighted with subtle accent background and accent icon color.

### 4. Search Filter Architecture & Behavior
- **Sidebar Search Input:** Positioned at the top of the sidebar beneath the "Back to Chat" header.
  - Features a search icon, placeholder `"Search settings…"`, and clear button or `Escape` to clear.
  - Keyboard accessible: can be focused with `⌘K` (or `Ctrl+K`).
- **Search Index & Catalog:**
  - Fast in-memory matching over a catalog of settings entries across all categories:
    - Setting title (e.g., "Color scheme", "Nerd font", "Default model", "Reasoning effort", "Web search", "Coding agents", "Worktrees", "Tool approvals", "API key").
    - Setting description and keywords (e.g., "terminal", "mocha", "claude", "opencode", "keychain", "duckduckgo", "permissions").
    - Associated category (`SettingsTab`).
- **Results Presentation:**
  - **Query empty:** Normal category view is shown based on `self.settings_tab`.
  - **Query active:**
    - Sidebar displays badge indicators on categories containing matches.
    - The content canvas switches to a filtered view rendering matching `SettingsRow` entries grouped under their category headers, with full interactive controls intact.
    - Pressing `Escape` clears the query and returns to normal tab browsing.

---

## What's Left to the Builder
- Precise pixel tuning of sidebar padding, item spacing, and border colors to match the active GPUI theme (`theme.border`, `theme.secondary`, `theme.sidebar_border`).
- Key binding registration for sidebar arrow-key navigation in GPUI context.
- Unit test coverage for `SettingsTab` switching, keyboard cycling, and layout rendering.
