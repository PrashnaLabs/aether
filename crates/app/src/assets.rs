use std::sync::OnceLock;

use aether_core::IconPack;
use gpui_kit::AssetSource;

use crate::icon_packs::PACK_ICONS;

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Timer,
        Trash,
        Lock,
        LockKeyhole,
        KeyRound,
        LockOpen,
        Pencil,
        Eye,
        Pin,
        PinOff,
        Square,
        Asterisk,
        MessageCircle,
        GitBranch,
        GitFork,
        GitCommitHorizontal,
        FilePen,
        ListChecks,
        ListTree,
        List,
        Wrench,
        Circle,
        CircleDot,
        CircleSlash,
        Laptop,
        FolderX,
        FolderPlus,
        GitCompareArrows,
        ArrowUpFromLine,
        Terminal,
        FileDiff,
        ChartColumn,
        Gauge,
        FileCode,
        Braces,
        FileImage,
        FileArchive,
        FileSpreadsheet,
        FileSearch,
        CornerDownRight,
        TrendingDown,
        TrendingUp,
        WholeWord,
        Regex,
        Save,
        RotateCcw,
        CircleQuestionMark,
        PanelRight,
        PanelBottom,
        PanelTop,
        PanelRightOpen,
        PanelRightClose,
        PictureInPicture2,
        Minimize2,
        SquarePen,
        ListFilter,
        Minimize,
        Maximize,
        Archive,
        ArchiveRestore,
        ArrowDownToLine,
        ArrowDownUp,
        Cloud,
        CloudDownload,
        Eraser,
        FileOutput,
        GitBranchPlus,
        GitCommitVertical,
        House,
        MessageSquarePlus,
        PackageOpen,
        Undo2,
        ChevronsDownUp,
        LayoutList,
        CloudUpload,
        GitPullRequest,
        Paperclip,
        Image,
        X,
        Lightbulb,
        Shield,
        ShieldX,
        Layers,
        AppWindow,
        Zap,
        ArrowLeftRight,
        Plug,
        Files,
        FilePlus,
        Clock,
        UnfoldVertical,
        Rows2,
        Columns2,
        LogIn,
        Flame,
        Newspaper,
        BotMessageSquare,
        MessageSquare,
        RefreshCw,
        TextQuote,
        SquareRoundCorner,
        Grid2x2,
        Users,
        SquareCheck,
        ClockAlert,
        Hash,
        Forward
    ]
);

const PROVIDER_ICONS: [(&str, &[u8]); 6] = [
    ("icons/provider-claude.svg", include_bytes!("../assets/icons/provider-claude.svg")),
    ("icons/provider-codex.svg", include_bytes!("../assets/icons/provider-codex.svg")),
    ("icons/provider-pi.svg", include_bytes!("../assets/icons/provider-pi.svg")),
    ("icons/provider-opencode.svg", include_bytes!("../assets/icons/provider-opencode.svg")),
    ("icons/provider-antigravity.svg", include_bytes!("../assets/icons/provider-antigravity.svg")),
    ("icons/provider-openrouter.svg", include_bytes!("../assets/icons/provider-openrouter.svg")),
];

static ACTIVE_PACK: OnceLock<IconPack> = OnceLock::new();

pub(crate) const PREVIEW_ICONS: [&str; 5] =
    ["house", "message-square", "git-branch", "terminal", "search"];

pub(crate) fn set_active_icon_pack(pack: IconPack) {
    let _ = ACTIVE_PACK.set(pack);
}

pub(crate) fn active_icon_pack() -> IconPack {
    ACTIVE_PACK.get().copied().unwrap_or_default()
}

pub(crate) fn pack_icon_path(pack: IconPack, name: &str) -> String {
    format!("icons/pack/{}/{name}.svg", pack.slug())
}

fn pack_bytes(path: &str) -> Option<&'static [u8]> {
    PACK_ICONS.binary_search_by_key(&path, |(p, _)| p).ok().map(|i| PACK_ICONS[i].1)
}

fn route(pack: IconPack, path: &str) -> Option<&'static [u8]> {
    if pack == IconPack::Lucide {
        return None;
    }
    pack_bytes(&pack_icon_path(pack, path.strip_prefix("icons/")?.strip_suffix(".svg")?))
}

/// Default component icons plus the extras named above.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = PROVIDER_ICONS.iter().find(|(p, _)| *p == path) {
            return Ok(Some(std::borrow::Cow::Borrowed(*bytes)));
        }
        let lucide = path.strip_prefix("icons/pack/lucide/").map(|name| format!("icons/{name}"));
        if let Some(bytes) = pack_bytes(path) {
            return Ok(Some(std::borrow::Cow::Borrowed(bytes)));
        }
        if lucide.is_none()
            && let Some(bytes) = route(active_icon_pack(), path)
        {
            return Ok(Some(std::borrow::Cow::Borrowed(bytes)));
        }
        let path = lucide.as_deref().unwrap_or(path);
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<gpui_kit::SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        if path.trim_end_matches('/') == "icons" {
            paths.extend(PROVIDER_ICONS.iter().map(|(p, _)| (*p).into()));
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
#[path = "../tests/unit/assets.rs"]
mod tests;
