use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{App, Hsla};

use crate::theme;

/// Returns a semantic Lucide [`IconName`] for a file path based on its filename or extension.
pub fn file_icon(path: &Path) -> IconName {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_ascii_lowercase();

    match name.as_str() {
        "cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "bun.lockb"
        | "gemfile.lock" | "poetry.lock" | "uv.lock" => return IconName::Lock,
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitkeep" => {
            return IconName::GitBranch;
        }
        "dockerfile" | "makefile" | "justfile" => return IconName::SquareTerminal,
        "readme" | "readme.md" | "readme.txt" | "license" | "license.md" | "changelog.md" => {
            return IconName::BookOpen;
        }
        _ => {}
    }

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();

    match ext.as_str() {
        "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "go" | "c" | "cc" | "cpp"
        | "cxx" | "h" | "hh" | "hpp" | "java" | "kt" | "swift" | "rb" | "php" | "zig" | "lua"
        | "sql" | "html" | "htm" | "css" | "scss" | "sass" | "less" | "vue" | "svelte" | "wasm"
        | "proto" | "graphql" | "gql" | "nix" | "dart" | "ex" | "exs" | "erl" | "hs" | "ml"
        | "clj" | "scala" | "r" | "cs" | "fs" => IconName::FileCode,
        "json" | "jsonc" | "json5" | "toml" | "yaml" | "yml" | "xml" | "ini" | "cfg" | "conf"
        | "env" | "properties" => IconName::Braces,
        "sh" | "bash" | "zsh" | "fish" | "nu" | "ps1" | "bat" | "cmd" => IconName::SquareTerminal,
        "md" | "mdx" | "rst" | "adoc" => IconName::BookOpen,
        "lock" => IconName::Lock,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "bmp" | "tiff" | "avif" => {
            IconName::FileImage
        }
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" => IconName::FileArchive,
        "csv" | "tsv" | "xls" | "xlsx" | "parquet" => IconName::FileSpreadsheet,
        _ => IconName::FileText,
    }
}

/// Returns a subtle accent color for a file icon based on its type, falling back to `muted_foreground`.
pub fn file_icon_color(path: &Path, cx: &App) -> Hsla {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();

    match ext.as_str() {
        "rs" | "toml" => theme::mango_text(cx),
        "js" | "jsx" | "ts" | "tsx" | "py" | "go" | "c" | "cpp" | "h" | "java" | "swift"
        | "zig" | "html" | "css" | "scss" => theme::info_text(cx),
        "json" | "jsonc" | "yaml" | "yml" => theme::warning_text(cx),
        "sh" | "bash" | "zsh" | "fish" => theme::success_text(cx),
        _ => cx.theme().muted_foreground,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/components/file_icon.rs"]
mod tests;
