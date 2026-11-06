use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Once;

use gpui_kit::HighlightStyle;
use gpui_kit::component::{
    Rope,
    highlighter::{HighlightTheme, LanguageRegistry, SyntaxHighlighter},
};

pub fn register_aliases() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let registry = LanguageRegistry::singleton();
        let aliases = [
            ("bash", &["shell", "zsh", "console", "shellscript"][..]),
            ("javascript", &["jsx", "mjs", "cjs"]),
            ("c", &["h"]),
            ("cpp", &["hpp", "cc"]),
            // upstream tsx lacks the javascript base query and renders nearly plain
            ("typescript", &["tsx"]),
        ];
        for (name, alias) in aliases {
            if let Some(config) = registry.language(name) {
                for alias in alias {
                    registry.register(alias, &config);
                }
            }
        }
    });
}

pub fn highlight_with(
    code: &str,
    lang: &str,
    theme: &HighlightTheme,
) -> Vec<(Range<usize>, HighlightStyle)> {
    thread_local! {
        static HIGHLIGHTERS: RefCell<HashMap<String, SyntaxHighlighter>> = RefCell::default();
    }
    register_aliases();
    HIGHLIGHTERS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let highlighter =
            cache.entry(lang.to_string()).or_insert_with(|| SyntaxHighlighter::new(lang));
        highlighter.update(None, &Rope::from_str(code), None);
        highlighter.styles(&(0..code.len()), theme)
    })
}

pub fn lang_for_path(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    lang_for_name(ext)
}

pub fn lang_for_fence(info: &str) -> &'static str {
    lang_for_name(info.split_whitespace().next().unwrap_or(""))
}

fn lang_for_name(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "rs" | "rust" => "rust",
        "ts" | "typescript" => "typescript",
        "tsx" => "tsx",
        "js" | "jsx" | "mjs" | "cjs" | "javascript" => "javascript",
        "json" => "json",
        "css" => "css",
        "html" => "html",
        "py" | "python" => "python",
        "go" => "go",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "md" | "markdown" => "markdown",
        "sh" | "bash" | "zsh" | "shell" | "console" => "bash",
        "sql" => "sql",
        "swift" => "swift",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "hpp" => "cpp",
        "rb" | "ruby" => "ruby",
        _ => "text",
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/highlight.rs"]
mod tests;
