use super::*;

#[test]
fn serves_chat_diff_layout_icons() {
    use gpui_kit::assets::IconName as I;
    let listed = AppAssets.list("icons").unwrap();
    for icon in [I::Rows2, I::Columns2] {
        let path = icon.path();
        let bytes = AppAssets.load(&path).unwrap().expect("diff layout icon must be bundled");
        assert!(bytes.starts_with(b"<svg"), "{path}");
        assert!(listed.contains(&path), "{path}");
    }
}

#[test]
fn serves_provider_icons() {
    let listed = AppAssets.list("icons").unwrap();
    for provider in aether_core::Provider::ALL {
        let path = aether_ui::provider_icon_path(provider);
        let bytes = AppAssets.load(path).unwrap().unwrap();
        assert!(bytes.starts_with(b"<svg"), "{path}");
        assert!(listed.iter().any(|p| p.as_ref() == path), "{path}");
    }
    for provider in aether_agent::usage_history::UsageProvider::ALL {
        let path = crate::usage_view::usage_provider_icon_path(provider);
        let bytes = AppAssets.load(path).unwrap().unwrap();
        assert!(bytes.starts_with(b"<svg"), "{path}");
        assert!(listed.iter().any(|p| p.as_ref() == path), "{path}");
    }
}

#[test]
fn serves_sidebar_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [
        I::MessageCircle,
        I::GitBranch,
        I::GitFork,
        I::GitCommitHorizontal,
        I::Folder,
        I::FolderOpen,
        I::FolderClosed,
        I::EyeOff,
        I::Ellipsis,
        I::CircleAlert,
        I::Laptop,
        I::FolderX,
        I::ChartColumn,
        I::Flame,
        I::LayoutDashboard,
    ] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_terminal_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [
        I::Terminal,
        I::SquareTerminal,
        I::RotateCw,
        I::Close,
        I::CircleAlert,
        I::CircleCheck,
        I::CornerDownRight,
        I::Minimize,
        I::Maximize,
        I::PanelBottom,
        I::PanelRightOpen,
        I::PanelRightClose,
        I::CircleQuestionMark,
    ] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_explorer_and_file_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [
        I::Pin,
        I::PinOff,
        I::Square,
        I::FolderPlus,
        I::FilePlus,
        I::FileCode,
        I::Braces,
        I::FileImage,
        I::FileArchive,
        I::FileSpreadsheet,
        I::FileSearch,
        I::BookOpen,
        I::Lock,
        I::GitBranch,
        I::SquareTerminal,
        I::FileText,
        I::CaseSensitive,
        I::WholeWord,
        I::Regex,
        I::Save,
        I::RotateCcw,
        I::TrendingDown,
        I::TrendingUp,
    ] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_git_panel_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [
        I::GitBranch,
        I::FileDiff,
        I::GitCompareArrows,
        I::ArrowUpFromLine,
        I::ArrowUp,
        I::ArrowDown,
        I::ArrowLeft,
        I::Plus,
        I::Minus,
        I::RotateCw,
        I::FileText,
        I::File,
        I::Info,
        I::Copy,
        I::ListTree,
        I::List,
        I::Folder,
        I::FolderOpen,
        I::Archive,
        I::ArchiveRestore,
        I::ArrowDownToLine,
        I::ArrowDownUp,
        I::Cloud,
        I::CloudDownload,
        I::Eraser,
        I::FileOutput,
        I::GitBranchPlus,
        I::GitCommitVertical,
        I::House,
        I::MessageSquarePlus,
        I::PackageOpen,
        I::Undo2,
    ] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_tool_row_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [I::FilePen, I::ListChecks, I::Wrench, I::Circle, I::CircleDot, I::CircleSlash] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_changes_card_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [
        I::ChevronsUpDown,
        I::ChevronsDownUp,
        I::FileDiff,
        I::FileOutput,
        I::Folder,
        I::FolderOpen,
        I::ChevronRight,
        I::ChevronDown,
        I::FileCode,
    ] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_top_bar_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [
        I::LayoutList,
        I::PanelBottom,
        I::PanelRight,
        I::Laptop,
        I::Folder,
        I::FileDiff,
        I::Copy,
        I::Check,
        I::ChevronRight,
        I::ChevronDown,
        I::ArrowLeft,
        I::House,
        I::SquareTerminal,
        I::GitBranch,
        I::GitBranchPlus,
        I::Cloud,
        I::Plus,
        I::ExternalLink,
        I::Search,
    ] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_remote_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [
        I::Cloud,
        I::CloudUpload,
        I::ArrowUp,
        I::ArrowDown,
        I::RotateCw,
        I::GitPullRequest,
        I::ExternalLink,
    ] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_script_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [I::Play, I::Square, I::Ellipsis, I::Pencil, I::Trash] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_project_page_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [
        I::LayoutDashboard,
        I::House,
        I::GitBranch,
        I::Plus,
        I::CircleAlert,
        I::Folder,
        I::FileDiff,
        I::Cloud,
        I::ChevronDown,
    ] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_attachment_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [I::Paperclip, I::Image, I::X, I::FileText] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_transcript_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [
        I::Lightbulb,
        I::Shield,
        I::ShieldX,
        I::Layers,
        I::Zap,
        I::ArrowLeftRight,
        I::Plug,
        I::Files,
        I::FilePlus,
        I::Clock,
        I::UnfoldVertical,
        I::LogIn,
    ] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_hackernews_icons() {
    use gpui_kit::assets::IconName as I;
    let listed = AppAssets.list("icons").unwrap();
    for icon in [I::Flame, I::Newspaper, I::BotMessageSquare, I::MessageSquare, I::RefreshCw] {
        let path = icon.path();
        let bytes = AppAssets.load(&path).unwrap().expect("news icon must be bundled");
        assert!(bytes.starts_with(b"<svg"), "{path}");
        assert!(listed.contains(&path), "{path}");
    }
}

#[test]
fn serves_markdown_preview_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [I::Eye, I::Pencil] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}

#[test]
fn serves_session_float_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [I::PictureInPicture2, I::Minimize2] {
        let path = icon.path();
        let bytes = AppAssets.load(&path).unwrap().expect("float menu icon must be bundled");
        assert!(bytes.starts_with(b"<svg"), "{path}");
    }
}

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

#[test]
fn pack_icons_are_sorted_for_binary_search() {
    assert!(crate::icon_packs::PACK_ICONS.windows(2).all(|w| w[0].0 < w[1].0));
}

fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn serves_every_icon_the_sources_use() {
    use gpui_kit::assets::IconName;
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let mut files = Vec::new();
    for krate in std::fs::read_dir(&crates).unwrap() {
        let src = krate.unwrap().path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    let mut used = std::collections::BTreeSet::new();
    for file in files {
        let text = std::fs::read_to_string(file).unwrap();
        let cut = text
            .match_indices("#[cfg(test)]")
            .find(|(at, attr)| {
                let rest = text[at + attr.len()..].trim_start();
                let rest = if rest.starts_with("#[path") {
                    rest.split_once('\n').map_or("", |(_, r)| r.trim_start())
                } else {
                    rest
                };
                rest.starts_with("mod ")
            })
            .map_or(text.len(), |(at, _)| at);
        let text = &text[..cut];
        for prefix in ["IconName::", "Lucide::"] {
            for (at, _) in text.match_indices(prefix) {
                let name: String = text[at + prefix.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                    used.insert(name);
                }
            }
        }
    }
    assert!(used.len() > 100, "scan found only {}", used.len());
    let missing: Vec<_> = used
        .iter()
        .filter(|name| {
            let icon = IconName::ALL.iter().find(|i| format!("{i:?}") == **name);
            icon.is_some_and(|i| !matches!(AppAssets.load(&i.path()), Ok(Some(_))))
        })
        .collect();
    assert!(missing.is_empty(), "icons used but not bundled ({} scanned): {missing:?}", used.len());
}
