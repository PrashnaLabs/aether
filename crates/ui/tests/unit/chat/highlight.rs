use super::*;
use gpui_kit::component::highlighter::{HighlightTheme, LanguageRegistry};

#[test]
fn languages_follow_the_extension_and_fence() {
    assert_eq!(lang_for_path("src/components/Hero.tsx"), "tsx");
    assert_eq!(lang_for_path("a/b.rs"), "rust");
    assert_eq!(lang_for_path("Makefile"), "text");
    assert_eq!(lang_for_fence("sh"), "bash");
    assert_eq!(lang_for_fence("ts title=a.ts"), "typescript");
}

#[test]
fn code_gets_coloured_ranges_and_plain_text_does_not() {
    let theme = HighlightTheme::default_dark();
    assert!(highlight_with("fn main() {}", "rust", &theme).iter().any(|(_, s)| s.color.is_some()));
    assert!(highlight_with("plain words", "text", &theme).iter().all(|(_, s)| s.color.is_none()));
}

#[test]
fn fence_aliases_resolve_in_the_registry() {
    register_aliases();
    let registry = LanguageRegistry::singleton();
    for alias in ["shell", "zsh", "console", "shellscript", "jsx", "mjs", "cjs", "h", "hpp", "cc"] {
        assert!(registry.language(alias).is_some(), "{alias}");
    }
    assert_eq!(lang_for_name("Rust"), "rust");
}

#[test]
fn tsx_gets_keyword_spans() {
    register_aliases();
    let theme = HighlightTheme::default_dark();
    let code = "import React from 'react';\nconst Hero = () => {\n  return <div>hi</div>;\n};";
    let spans = highlight_with(code, "tsx", &theme);
    for word in ["import", "const", "return"] {
        let at = code.find(word).unwrap();
        assert!(
            spans.iter().any(|(r, s)| r.start <= at && at < r.end && s.color.is_some()),
            "{word}"
        );
    }
}
