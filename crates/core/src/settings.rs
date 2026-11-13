use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{Skill, SkillSource};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
    #[serde(alias = "catppuccin-latte")]
    CatppuccinLatte,
    #[serde(alias = "catppuccin-frappe")]
    CatppuccinFrappe,
    #[serde(alias = "catppuccin-macchiato")]
    CatppuccinMacchiato,
    #[serde(alias = "catppuccin-mocha")]
    CatppuccinMocha,
    #[serde(alias = "rose-pine-main")]
    RosePineMain,
    #[serde(alias = "rose-pine-moon")]
    RosePineMoon,
    #[serde(alias = "rose-pine-dawn")]
    RosePineDawn,
    #[serde(alias = "gruvbox-dark")]
    GruvboxDark,
    #[serde(alias = "gruvbox-light")]
    GruvboxLight,
    #[serde(alias = "solarized-dark")]
    SolarizedDark,
    #[serde(alias = "solarized-light")]
    SolarizedLight,
    Nord,
    Dracula,
}

impl Appearance {
    pub const ALL: [Self; 16] = [
        Self::System,
        Self::Dark,
        Self::Light,
        Self::CatppuccinLatte,
        Self::CatppuccinFrappe,
        Self::CatppuccinMacchiato,
        Self::CatppuccinMocha,
        Self::RosePineMain,
        Self::RosePineMoon,
        Self::RosePineDawn,
        Self::GruvboxDark,
        Self::GruvboxLight,
        Self::SolarizedDark,
        Self::SolarizedLight,
        Self::Nord,
        Self::Dracula,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::CatppuccinLatte => "Catppuccin Latte",
            Self::CatppuccinFrappe => "Catppuccin Frappé",
            Self::CatppuccinMacchiato => "Catppuccin Macchiato",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::RosePineMain => "Rosé Pine Main",
            Self::RosePineMoon => "Rosé Pine Moon",
            Self::RosePineDawn => "Rosé Pine Dawn",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::GruvboxLight => "Gruvbox Light",
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::Nord => "Nord",
            Self::Dracula => "Dracula",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
            Self::CatppuccinLatte => "catppuccin-latte",
            Self::CatppuccinFrappe => "catppuccin-frappe",
            Self::CatppuccinMacchiato => "catppuccin-macchiato",
            Self::CatppuccinMocha => "catppuccin-mocha",
            Self::RosePineMain => "rose-pine-main",
            Self::RosePineMoon => "rose-pine-moon",
            Self::RosePineDawn => "rose-pine-dawn",
            Self::GruvboxDark => "gruvbox-dark",
            Self::GruvboxLight => "gruvbox-light",
            Self::SolarizedDark => "solarized-dark",
            Self::SolarizedLight => "solarized-light",
            Self::Nord => "nord",
            Self::Dracula => "dracula",
        }
    }

    /// `None` for System, which follows the OS.
    pub fn is_dark(self) -> Option<bool> {
        match self {
            Self::System => None,
            Self::Light
            | Self::CatppuccinLatte
            | Self::RosePineDawn
            | Self::GruvboxLight
            | Self::SolarizedLight => Some(false),
            _ => Some(true),
        }
    }
}

pub struct ThemeFamily {
    pub variants: &'static [Appearance],
    pub default: Appearance,
}

impl ThemeFamily {
    /// The active scheme when it belongs to this family, else the family default.
    pub fn shown(&self, active: Appearance) -> Appearance {
        if self.variants.contains(&active) { active } else { self.default }
    }

    pub fn has_both_modes(&self) -> bool {
        self.variants.iter().any(|a| a.is_dark() == Some(true))
            && self.variants.iter().any(|a| a.is_dark() == Some(false))
    }
}

pub const THEME_FAMILIES: [ThemeFamily; 9] = [
    ThemeFamily { variants: &[Appearance::System], default: Appearance::System },
    ThemeFamily { variants: &[Appearance::Dark], default: Appearance::Dark },
    ThemeFamily { variants: &[Appearance::Light], default: Appearance::Light },
    ThemeFamily {
        variants: &[
            Appearance::CatppuccinLatte,
            Appearance::CatppuccinFrappe,
            Appearance::CatppuccinMacchiato,
            Appearance::CatppuccinMocha,
        ],
        default: Appearance::CatppuccinFrappe,
    },
    ThemeFamily {
        variants: &[Appearance::RosePineMain, Appearance::RosePineMoon, Appearance::RosePineDawn],
        default: Appearance::RosePineMain,
    },
    ThemeFamily {
        variants: &[Appearance::GruvboxDark, Appearance::GruvboxLight],
        default: Appearance::GruvboxDark,
    },
    ThemeFamily {
        variants: &[Appearance::SolarizedDark, Appearance::SolarizedLight],
        default: Appearance::SolarizedDark,
    },
    ThemeFamily { variants: &[Appearance::Nord], default: Appearance::Nord },
    ThemeFamily { variants: &[Appearance::Dracula], default: Appearance::Dracula },
];

pub fn family_index(a: Appearance) -> usize {
    THEME_FAMILIES.iter().position(|f| f.variants.contains(&a)).unwrap_or(0)
}

/// Where a chat notification shows up; `Settings::notifications` off silences both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyDelivery {
    Toast,
    #[default]
    Os,
}

/// Which notifications also play a sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifySound {
    #[default]
    None,
    Done,
    Request,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerStyle {
    Square,
    Subtle,
    #[default]
    Rounded,
    Round,
}

impl CornerStyle {
    pub const ALL: [Self; 4] = [Self::Square, Self::Subtle, Self::Rounded, Self::Round];

    pub fn label(self) -> &'static str {
        match self {
            Self::Square => "Square",
            Self::Subtle => "Subtle",
            Self::Rounded => "Rounded",
            Self::Round => "Round",
        }
    }

    pub fn scale(self) -> f32 {
        match self {
            Self::Square => 0.0,
            Self::Subtle => 0.5,
            Self::Rounded => 1.0,
            Self::Round => 1.5,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconPack {
    #[default]
    Lucide,
    Iconoir,
    Remix,
    Solar,
}

impl IconPack {
    pub const ALL: [Self; 4] = [Self::Lucide, Self::Iconoir, Self::Remix, Self::Solar];

    pub fn label(self) -> &'static str {
        match self {
            Self::Lucide => "Lucide",
            Self::Iconoir => "Iconoir",
            Self::Remix => "Remix",
            Self::Solar => "Solar",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Lucide => "lucide",
            Self::Iconoir => "iconoir",
            Self::Remix => "remix",
            Self::Solar => "solar",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalTheme {
    #[default]
    AutoCatppuccin,
    CatppuccinMocha,
    CatppuccinMacchiato,
    CatppuccinFrappe,
    CatppuccinLatte,
    Aether,
}

impl TerminalTheme {
    /// Picker order. Latte stays loadable from old settings but renders dark, so it's not offered.
    pub const ALL: [Self; 5] = [
        Self::AutoCatppuccin,
        Self::CatppuccinMocha,
        Self::CatppuccinMacchiato,
        Self::CatppuccinFrappe,
        Self::Aether,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::AutoCatppuccin => "Auto (Catppuccin)",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::CatppuccinMacchiato => "Catppuccin Macchiato",
            Self::CatppuccinFrappe => "Catppuccin Frappé",
            Self::CatppuccinLatte => "Catppuccin Latte",
            Self::Aether => "Aether Default",
        }
    }

    pub fn is_dark(&self, system_dark: bool) -> bool {
        match self {
            Self::AutoCatppuccin => system_dark,
            Self::CatppuccinMocha | Self::CatppuccinMacchiato | Self::CatppuccinFrappe => true,
            Self::CatppuccinLatte => false,
            Self::Aether => system_dark,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalCursor {
    #[default]
    Block,
    Bar,
    Underline,
}

impl TerminalCursor {
    pub const ALL: [Self; 3] = [Self::Block, Self::Bar, Self::Underline];

    pub fn label(self) -> &'static str {
        match self {
            Self::Block => "Block",
            Self::Bar => "Bar",
            Self::Underline => "Underline",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reasoning {
    Off,
    #[default]
    Low,
    Medium,
    High,
}

impl Reasoning {
    pub const ALL: [Self; 4] = [Self::Off, Self::Low, Self::Medium, Self::High];

    pub fn effort(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    OpenRouter,
    Claude,
    Codex,
    Pi,
    OpenCode,
    #[serde(alias = "agy")]
    Antigravity,
}

impl Provider {
    pub const ALL: [Self; 6] =
        [Self::OpenRouter, Self::Claude, Self::Codex, Self::Pi, Self::OpenCode, Self::Antigravity];

    /// Saved name, also the `Settings.provider_models` key.
    pub fn key(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::OpenCode => "opencode",
            Self::Antigravity => "antigravity",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Pi => "Pi",
            Self::OpenCode => "OpenCode",
            Self::Antigravity => "Antigravity (AGY)",
        }
    }

    pub fn short_label(self) -> Option<&'static str> {
        match self {
            Self::OpenRouter => None,
            Self::Claude => Some("Claude"),
            Self::Codex => Some("Codex"),
            Self::OpenCode => Some("OpenCode"),
            Self::Pi => Some("Pi"),
            Self::Antigravity => Some("AGY"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    #[default]
    Ask,
    AutoEdit,
    Auto,
    ReadOnly,
    Full,
}

impl AccessMode {
    pub const ALL: [Self; 5] = [Self::Ask, Self::AutoEdit, Self::Auto, Self::ReadOnly, Self::Full];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Supervised",
            Self::AutoEdit => "Auto-accept edits",
            Self::Auto => "Auto",
            Self::ReadOnly => "Read only",
            Self::Full => "Full access",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Ask => "Ask before commands and file changes",
            Self::AutoEdit => "Auto-approve edits, ask before other actions",
            Self::Auto => "A reviewer model approves routine actions and may block risky ones",
            Self::ReadOnly => "Can read files and search; can't edit or run commands",
            Self::Full => "Allow commands and edits without prompts",
        }
    }

    /// Modes `provider` can enforce, safest first; the first is a new chat's default.
    pub fn for_provider(provider: Provider) -> &'static [Self] {
        match provider {
            Provider::OpenRouter => &[],
            Provider::Pi => &[Self::ReadOnly, Self::Full],
            Provider::Claude => &[Self::Ask, Self::AutoEdit, Self::Auto, Self::Full],
            Provider::OpenCode => &[Self::Ask, Self::AutoEdit, Self::Full],
            Provider::Codex => &[Self::Ask, Self::ReadOnly, Self::AutoEdit, Self::Auto, Self::Full],
            Provider::Antigravity => &[Self::AutoEdit, Self::Auto, Self::Full],
        }
    }
}

/// Effort values a CLI accepts; empty when aether offers no picker.
pub fn effort_levels(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["low", "medium", "high", "xhigh", "max"],
        Provider::Pi => &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        Provider::Antigravity => &["low", "medium", "high", "xhigh", "max"],
        Provider::Codex => &["low", "medium", "high", "xhigh", "max"],
        Provider::OpenRouter | Provider::OpenCode => &[],
    }
}

/// Title-cased effort for a chip; empty means the CLI default.
pub fn effort_label(effort: &str) -> String {
    match effort {
        "" => "Default".into(),
        "xhigh" => "Extra High".into(),
        _ => effort
            .split(['-', '_'])
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
            })
            .collect::<Vec<String>>()
            .join(" "),
    }
}

/// Which footer controls a provider's composer shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FooterChips {
    pub effort: bool,
    pub access: bool,
    pub folder: bool,
    pub toggles: bool,
}

impl FooterChips {
    pub fn for_provider(provider: Provider) -> Self {
        Self {
            effort: !effort_levels(provider).is_empty(),
            access: !AccessMode::for_provider(provider).is_empty(),
            folder: provider != Provider::OpenRouter,
            toggles: provider == Provider::OpenRouter,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPolicy {
    Ask,
    Allow,
}

fn default_bottom_bar_hidden_agents() -> Vec<Provider> {
    vec![Provider::OpenRouter, Provider::OpenCode]
}

fn default_font_size() -> f32 {
    16.0
}

const fn default_window_opacity() -> u8 {
    100
}

const fn default_max_children() -> usize {
    8
}

fn max_children_in_range<'de, D: Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
    Ok(match OrBad::<usize>::deserialize(deserializer)? {
        OrBad::Good(n) => n.clamp(1, 50),
        OrBad::Bad(_) => default_max_children(),
    })
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OrBad<T> {
    Good(T),
    Bad(serde::de::IgnoredAny),
}

fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(match OrBad::deserialize(deserializer)? {
        OrBad::Good(value) => value,
        OrBad::Bad(_) => T::default(),
    })
}

fn opacity_in_range<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    Ok(match OrBad::<f64>::deserialize(deserializer)? {
        OrBad::Good(percent) => percent.clamp(60., 100.).round() as u8,
        OrBad::Bad(_) => default_window_opacity(),
    })
}

const fn default_true() -> bool {
    true
}

fn default_background_panel_width() -> f32 {
    340.0
}

fn default_right_panel_share() -> f32 {
    0.6
}

fn default_terminal_font_size() -> f32 {
    13.0
}

/// How the sidebar lists agent chats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SidebarStyle {
    /// Project folders with branch and change details.
    #[default]
    Default,
    /// Two-line rows grouped by recency or project, with a filter menu.
    Minimal,
    /// One-line rows grouped by what each chat needs from you.
    Focus,
}

impl SidebarStyle {
    pub const ALL: [Self; 3] = [Self::Default, Self::Minimal, Self::Focus];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Minimal => "Minimal",
            Self::Focus => "Focus",
        }
    }
}

/// How much of a reply's thinking and tool calls the chat shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChatDetail {
    /// Hides thinking.
    Compact,
    #[default]
    Normal,
    /// Opens every step, tool output and thought, and unfolds finished replies.
    Verbose,
}

impl ChatDetail {
    pub const ALL: [Self; 3] = [Self::Compact, Self::Normal, Self::Verbose];

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Normal => "Normal",
            Self::Verbose => "Verbose",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Watch {
    Chat(i64),
    Terminal(i64),
}

impl Watch {
    pub fn chat(self) -> i64 {
        let (Self::Chat(id) | Self::Terminal(id)) = self;
        id
    }
}

/// Adds `item` or removes it if present; true when now watched.
pub fn toggle_watch(list: &mut Vec<Watch>, item: Watch) -> bool {
    let before = list.len();
    list.retain(|w| *w != item);
    if list.len() < before {
        return false;
    }
    list.push(item);
    true
}

/// How docked session panes share a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TileLayout {
    /// Panes stay where they were split, docked and dragged.
    #[default]
    Manual,
    /// The first pane on the left, the rest stacked on the right.
    Stack,
    Columns,
    Grid,
    /// One pane at a time, the rest behind tabs.
    Monocle,
}

impl TileLayout {
    pub const ALL: [Self; 5] =
        [Self::Manual, Self::Stack, Self::Columns, Self::Grid, Self::Monocle];

    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Stack => "Stack",
            Self::Columns => "Columns",
            Self::Grid => "Grid",
            Self::Monocle => "Monocle",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Manual => "Panes stay where you split, dock and drag them.",
            Self::Stack => "The first pane on the left, the rest stacked on the right.",
            Self::Columns => "Every pane gets an equal column.",
            Self::Grid => "Panes fill rows and columns evenly.",
            Self::Monocle => "One pane at a time, the rest behind tabs.",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatLayout {
    #[default]
    Tabbed,
    Sidebar,
    Split,
}

impl ChatLayout {
    pub const ALL: [Self; 3] = [Self::Tabbed, Self::Sidebar, Self::Split];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tabbed => "Tabbed",
            Self::Sidebar => "Sidebar",
            Self::Split => "Split",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Tabbed => "Agent chat, Terminal and Explorer as tabs.",
            Self::Sidebar => "Chat stays visible, tools live in a right panel.",
            Self::Split => {
                "Like Sidebar, but the right panel stays docked and chat width is capped."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrgLayout {
    #[default]
    Roster,
    Chart,
    Table,
}

impl OrgLayout {
    pub const ALL: [Self; 3] = [Self::Roster, Self::Chart, Self::Table];

    pub fn label(self) -> &'static str {
        match self {
            Self::Roster => "Roster console",
            Self::Chart => "Org chart",
            Self::Table => "Round table",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Roster => "A row per head with its workers, needs-you and messages on the right.",
            Self::Chart => "CEO above the heads, an inspector for the selected one.",
            Self::Table => "Heads seated around the goal, needs-you and messages beside.",
        }
    }
}

/// Who approves a head's request for a worker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrgHire {
    /// The rules decide: approve when they pass, deny when not.
    #[default]
    Rules,
    /// Approve when the rules pass, else ask the user.
    Auto,
    Ask,
}

impl OrgHire {
    pub const ALL: [Self; 3] = [Self::Rules, Self::Auto, Self::Ask];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rules => "CEO decides by rules",
            Self::Auto => "Auto-approve within rules",
            Self::Ask => "Always ask me",
        }
    }
}

/// Which window corner the watch-list button sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WatchCorner {
    #[default]
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

impl WatchCorner {
    pub const ALL: [Self; 4] = [Self::BottomRight, Self::BottomLeft, Self::TopRight, Self::TopLeft];

    pub fn label(self) -> &'static str {
        match self {
            Self::BottomRight => "Bottom right",
            Self::BottomLeft => "Bottom left",
            Self::TopRight => "Top right",
            Self::TopLeft => "Top left",
        }
    }
}

/// Minimal sidebar sections: by last activity or by project.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SidebarGrouping {
    #[default]
    Updated,
    Project,
}

/// Row order inside a Minimal sidebar section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SidebarOrdering {
    #[default]
    Newest,
    Oldest,
    Name,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TimeFormat {
    #[default]
    Hour12,
    Hour24,
}

impl TimeFormat {
    pub const ALL: [TimeFormat; 2] = [TimeFormat::Hour12, TimeFormat::Hour24];

    pub fn label(&self) -> &'static str {
        match self {
            TimeFormat::Hour12 => "12-hour (9:30 PM)",
            TimeFormat::Hour24 => "24-hour (21:30)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DateFormat {
    #[default]
    Short,
    Numeric,
    Full,
    Hidden,
}

impl DateFormat {
    pub const ALL: [DateFormat; 4] =
        [DateFormat::Short, DateFormat::Numeric, DateFormat::Full, DateFormat::Hidden];

    pub fn label(&self) -> &'static str {
        match self {
            DateFormat::Short => "Short (Fri, Oct 23)",
            DateFormat::Numeric => "Numeric (2026-10-23)",
            DateFormat::Full => "Full (Friday, Oct 23, 2026)",
            DateFormat::Hidden => "Hidden (Time only)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectScript {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub on_new_worktree: bool,
}

/// The editor used when opening notes without an explicit editor override.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotesEditor {
    #[default]
    Aether,
    Neovim,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotesSettings {
    pub directory: String,
    pub editor: NotesEditor,
    pub project_scope: bool,
    pub nvim: String,
    pub tmux: String,
    pub session: String,
    /// Empty uses $XDG_RUNTIME_DIR/notes-nvim.sock, or /tmp/notes-nvim.sock.
    pub socket: String,
}

impl Default for NotesSettings {
    fn default() -> Self {
        Self {
            directory: "~/notes".into(),
            editor: NotesEditor::Aether,
            project_scope: false,
            nvim: "nvim".into(),
            tmux: "tmux".into(),
            session: "notes".into(),
            socket: String::new(),
        }
    }
}

/// A named login for one provider's CLI, kept in its own config dir.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub provider: Provider,
    pub name: String,
    pub dir: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub notes: NotesSettings,
    #[serde(deserialize_with = "or_default")]
    pub appearance: Appearance,
    #[serde(default)]
    pub ui_font_family: String,
    #[serde(default, deserialize_with = "or_default")]
    pub corner_style: CornerStyle,
    #[serde(default, deserialize_with = "or_default")]
    pub icon_pack: IconPack,
    /// Window opacity in percent; read through `clamped_window_opacity`.
    #[serde(default = "default_window_opacity", deserialize_with = "opacity_in_range")]
    pub window_opacity: u8,
    #[serde(default, deserialize_with = "or_default")]
    pub window_blur: bool,
    /// Max running child agents per lead chat.
    #[serde(default = "default_max_children", deserialize_with = "max_children_in_range")]
    pub max_children: usize,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    pub model: String,
    #[serde(default)]
    pub model_windows: HashMap<String, u64>,
    pub reasoning: Reasoning,
    pub web_search: bool,
    pub instructions: String,
    /// Per-skill on/off choices by `Skill::id`. Unlisted skills use the default.
    pub skills: BTreeMap<String, bool>,
    /// Per-tool approval choices. Unlisted tools use `tool_policy`'s default.
    pub tool_approvals: BTreeMap<String, ToolPolicy>,
    /// Ask the model to propose a plan for approval before doing the work.
    pub plan_mode: bool,
    /// `None` until the user picks one; new chats then use a detected subscription.
    #[serde(default)]
    pub provider: Option<Provider>,
    #[serde(default)]
    pub provider_models: BTreeMap<String, String>,
    /// Default access mode for new chats, keyed by provider.
    pub provider_access: BTreeMap<String, AccessMode>,
    #[serde(default)]
    pub claude_binary: String,
    #[serde(default)]
    pub codex_binary: String,
    #[serde(default)]
    pub pi_binary: String,
    #[serde(default)]
    pub opencode_binary: String,
    #[serde(default)]
    pub antigravity_binary: String,
    #[serde(default)]
    pub workspace_root: String,
    /// `"<provider key>:<model id>"`, in the order they were starred.
    #[serde(default)]
    pub favorite_models: Vec<String>,
    /// `open_in` app id for projects without their own; the last one picked. Empty means Finder.
    #[serde(default)]
    pub open_in_app: String,
    /// Project folders folded in the sidebar, by path.
    #[serde(default)]
    pub collapsed_projects: Vec<String>,
    /// Organizations folded in the Organization sidebar, by project path.
    #[serde(default)]
    pub collapsed_orgs: Vec<String>,
    /// Whether the entire Projects section is folded in the sidebar.
    #[serde(default)]
    pub projects_section_collapsed: bool,
    /// Whether the Pinned section is folded in the sidebar.
    #[serde(default)]
    pub pinned_section_collapsed: bool,
    /// Whether the Agents section is folded in the sidebar.
    #[serde(default)]
    pub agents_section_collapsed: bool,
    /// Project folders removed from the sidebar; their chats are kept.
    #[serde(default)]
    pub hidden_projects: Vec<String>,
    /// Project folders the user added, listed even without chats.
    #[serde(default)]
    pub projects: Vec<String>,
    /// Project folders pinned to the top of the sidebar, in pin order.
    #[serde(default)]
    pub pinned_projects: Vec<String>,
    #[serde(default)]
    pub project_open_in: BTreeMap<String, String>,
    #[serde(default)]
    pub project_scripts: BTreeMap<String, Vec<ProjectScript>>,
    /// Terminal drawer height in px, shared by every chat.
    pub terminal_height: f32,
    /// Option+key in the terminal sends Esc+key instead of the Option character.
    pub terminal_option_as_meta: bool,
    /// Terminal font family; empty means the theme's monospace font.
    /// A Nerd Font (e.g. "MesloLGS NF") renders prompt icons.
    #[serde(default)]
    pub terminal_font_family: String,
    /// Terminal font size in pt/px, independent of the UI font size.
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: f32,
    /// Terminal theme preset.
    #[serde(default)]
    pub terminal_theme: TerminalTheme,
    /// Terminal cursor shape; a program can still set its own.
    #[serde(default, deserialize_with = "or_default")]
    pub terminal_cursor: TerminalCursor,
    /// Changes panel width in px, shared by every chat.
    pub git_panel_width: f32,
    /// Changes panel lists files as a folder tree instead of a flat list.
    pub git_tree_view: bool,
    /// Sidebar width in px.
    pub sidebar_width: f32,
    /// Whether the left navigation sidebar is collapsed.
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// Browser panel width in px.
    pub browser_panel_width: f32,
    /// Background panel width in px, beside the chat.
    #[serde(default = "default_background_panel_width")]
    pub background_panel_width: f32,
    /// Desktop notifications when a chat finishes, fails or needs you while you look elsewhere.
    pub notifications: bool,
    #[serde(default, deserialize_with = "or_default")]
    pub notify_delivery: NotifyDelivery,
    #[serde(default, deserialize_with = "or_default")]
    pub notify_sound: NotifySound,
    /// POST target for `{title, body, chat, state}` on each notification; empty is off.
    #[serde(default)]
    pub notify_webhook: String,
    /// Status bar clock 12-hour or 24-hour time format.
    #[serde(default)]
    pub status_bar_time_format: TimeFormat,
    /// Status bar date display format.
    #[serde(default)]
    pub status_bar_date_format: DateFormat,
    /// Whether to display seconds in the status bar clock.
    #[serde(default)]
    pub status_bar_show_seconds: bool,
    /// Bottom bar entries explicitly hidden by the user.
    #[serde(default)]
    pub bottom_bar_hidden: Vec<String>,
    /// Providers omitted from the compact usage strip, but still shown in its popover.
    #[serde(default = "default_bottom_bar_hidden_agents")]
    pub bottom_bar_hidden_agents: Vec<Provider>,
    /// Extra CLI logins, each a config dir the CLI keeps its own credentials in.
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// Account name in use per provider key; unlisted providers use the CLI's own login.
    #[serde(default)]
    pub default_accounts: BTreeMap<String, String>,
    /// Show hook runs in a reply's timeline. Failed hooks always show.
    #[serde(default)]
    pub show_hooks: bool,
    #[serde(default)]
    pub chat_detail: ChatDetail,
    /// Lists running chats in the sidebar's Agents section.
    #[serde(default)]
    pub show_running_agents: bool,
    /// Mic button and its shortcut in the composer.
    #[serde(default = "default_true")]
    pub voice_enabled: bool,
    #[serde(default)]
    pub sidebar_style: SidebarStyle,
    #[serde(default)]
    pub sidebar_grouping: SidebarGrouping,
    #[serde(default)]
    pub sidebar_ordering: SidebarOrdering,
    #[serde(default)]
    pub chat_layout: ChatLayout,
    /// Right panel's share of the sidebar layout width, 0.25–0.8.
    #[serde(default = "default_right_panel_share")]
    pub right_panel_share: f32,
    #[serde(default)]
    pub right_panel_collapsed: bool,
    /// BTW sidebar width in px (Tabbed layout).
    #[serde(default = "default_background_panel_width")]
    pub btw_width: f32,
    #[serde(default)]
    pub tile_layout: TileLayout,
    /// Share of the width the Stack layout's first pane takes, 0.3–0.8.
    pub tile_ratio: f32,
    /// Space between docked panes in px, 0–24.
    pub tile_gap: f32,
    /// Notes page, its shortcuts, and Save to note in menus. Off leaves the files alone.
    #[serde(default = "default_true")]
    pub notes_enabled: bool,
    /// Hacker News page and its shortcuts.
    #[serde(default = "default_true")]
    pub hackernews_enabled: bool,
    /// Watch-list button, its shortcut, and Watch in menus. Off keeps `watchlist` for later.
    #[serde(default = "default_true")]
    pub watch_enabled: bool,
    /// Watch-list items, in the order added.
    pub watchlist: Vec<Watch>,
    #[serde(default)]
    pub watch_corner: WatchCorner,
    /// Peek card x, y, width, height in window px; `None` opens it beside the FAB.
    pub watch_peek_bounds: Option<[f32; 4]>,
    /// Pinned chats stay listed under a collapsed project.
    pub sidebar_pins_when_collapsed: bool,
    /// Organization page, its shortcut, sidebar section and status item.
    pub org_enabled: bool,
    pub org_layout: OrgLayout,
    /// Org-wide worker cap; `None` sizes it from the machine.
    pub org_pool: Option<usize>,
    pub org_builds: usize,
    pub org_workers_per_head: usize,
    pub org_hire: OrgHire,
    /// Context percent at which a head's session should be rotated.
    pub org_rotate_at: u8,
    pub org_template: String,
}

impl Settings {
    /// The account chosen for `provider`, if it still exists and has a dir.
    pub fn active_account(&self, provider: Provider) -> Option<&Account> {
        let name = self.default_accounts.get(provider.key())?;
        self.accounts
            .iter()
            .find(|a| a.provider == provider && a.name == *name && !a.dir.trim().is_empty())
    }

    /// The active account's config dir, `~` expanded.
    pub fn account_dir(&self, provider: Provider) -> Option<PathBuf> {
        self.active_account(provider).map(|a| crate::paths::expand_tilde(a.dir.trim()))
    }

    /// Env that points the CLI at the active account, without logging out of the other.
    pub fn account_env(&self, provider: Provider) -> Vec<(String, String)> {
        let var = match provider {
            Provider::Claude => "CLAUDE_CONFIG_DIR",
            Provider::Codex => "CODEX_HOME",
            _ => return Vec::new(),
        };
        let dir = self.account_dir(provider);
        dir.map(|d| vec![(var.to_string(), d.to_string_lossy().into_owned())]).unwrap_or_default()
    }

    pub fn provider(&self) -> Provider {
        self.provider.unwrap_or_default()
    }

    pub fn default_access(&self, provider: Provider) -> AccessMode {
        let modes = AccessMode::for_provider(provider);
        self.provider_access
            .get(provider.key())
            .copied()
            .filter(|mode| modes.contains(mode))
            .unwrap_or_else(|| modes.first().copied().unwrap_or_default())
    }

    pub fn clamped_window_opacity(&self) -> u8 {
        self.window_opacity.clamp(60, 100)
    }

    pub const FONT_SIZES: [f32; 4] = [14.0, 16.0, 18.0, 20.0];
    pub const TERMINAL_FONT_SIZES: [f32; 6] = [11.0, 12.0, 13.0, 14.0, 16.0, 18.0];

    pub fn font_size_index(&self) -> usize {
        Self::FONT_SIZES
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (*a - self.font_size)
                    .abs()
                    .partial_cmp(&(*b - self.font_size).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map_or(1, |(ix, _)| ix)
    }

    pub fn terminal_font_size_index(&self) -> usize {
        Self::TERMINAL_FONT_SIZES
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (*a - self.terminal_font_size)
                    .abs()
                    .partial_cmp(&(*b - self.terminal_font_size).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map_or(2, |(ix, _)| ix)
    }

    /// Web search reaches an outside service, so it asks; local tools don't.
    pub fn tool_policy(&self, tool: &str) -> ToolPolicy {
        self.tool_approvals.get(tool).copied().unwrap_or(if tool == "web_search" {
            ToolPolicy::Ask
        } else {
            ToolPolicy::Allow
        })
    }

    /// Aether's own skills start on; skills shared with other tools start off.
    pub fn skill_enabled(&self, skill: &Skill) -> bool {
        self.skills.get(&skill.id()).copied().unwrap_or(skill.source == SkillSource::Aether)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notes: NotesSettings::default(),
            appearance: Appearance::default(),
            ui_font_family: String::new(),
            corner_style: CornerStyle::default(),
            icon_pack: IconPack::default(),
            window_opacity: default_window_opacity(),
            window_blur: false,
            max_children: default_max_children(),
            font_size: default_font_size(),
            model: "openrouter/auto".into(),
            model_windows: HashMap::new(),
            reasoning: Reasoning::default(),
            web_search: true,
            instructions: String::new(),
            skills: BTreeMap::new(),
            tool_approvals: BTreeMap::new(),
            plan_mode: false,
            provider: None,
            provider_models: BTreeMap::new(),
            provider_access: BTreeMap::new(),
            claude_binary: String::new(),
            codex_binary: String::new(),
            pi_binary: String::new(),
            opencode_binary: String::new(),
            antigravity_binary: String::new(),
            workspace_root: String::new(),
            favorite_models: Vec::new(),
            open_in_app: String::new(),
            collapsed_projects: Vec::new(),
            collapsed_orgs: Vec::new(),
            projects_section_collapsed: false,
            pinned_section_collapsed: false,
            agents_section_collapsed: false,
            hidden_projects: Vec::new(),
            projects: Vec::new(),
            pinned_projects: Vec::new(),
            project_open_in: BTreeMap::new(),
            project_scripts: BTreeMap::new(),
            terminal_height: 260.,
            terminal_option_as_meta: false,
            terminal_font_family: String::new(),
            terminal_font_size: default_terminal_font_size(),
            terminal_theme: TerminalTheme::default(),
            terminal_cursor: TerminalCursor::default(),
            git_panel_width: 360.,
            git_tree_view: false,
            sidebar_width: 255.,
            sidebar_collapsed: false,
            browser_panel_width: 480.,
            background_panel_width: default_background_panel_width(),
            notifications: true,
            notify_delivery: NotifyDelivery::default(),
            notify_sound: NotifySound::default(),
            notify_webhook: String::new(),
            status_bar_time_format: TimeFormat::default(),
            status_bar_date_format: DateFormat::default(),
            status_bar_show_seconds: false,
            bottom_bar_hidden: Vec::new(),
            bottom_bar_hidden_agents: default_bottom_bar_hidden_agents(),
            accounts: Vec::new(),
            default_accounts: BTreeMap::new(),
            show_hooks: false,
            chat_detail: ChatDetail::Normal,
            show_running_agents: false,
            voice_enabled: true,
            sidebar_style: SidebarStyle::Default,
            sidebar_grouping: SidebarGrouping::Updated,
            sidebar_ordering: SidebarOrdering::Newest,
            chat_layout: ChatLayout::Tabbed,
            right_panel_share: default_right_panel_share(),
            right_panel_collapsed: false,
            btw_width: default_background_panel_width(),
            tile_layout: TileLayout::Manual,
            tile_ratio: 0.55,
            tile_gap: 0.,
            notes_enabled: true,
            hackernews_enabled: true,
            watch_enabled: true,
            watchlist: Vec::new(),
            watch_corner: WatchCorner::BottomRight,
            watch_peek_bounds: None,
            sidebar_pins_when_collapsed: true,
            org_enabled: false,
            org_layout: OrgLayout::Roster,
            org_pool: None,
            org_builds: 1,
            org_workers_per_head: 2,
            org_hire: OrgHire::Rules,
            org_rotate_at: 75,
            org_template: "startup".into(),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
