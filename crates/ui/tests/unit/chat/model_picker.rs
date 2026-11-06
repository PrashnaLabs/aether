use super::*;

fn item(id: &str, name: &str, detail: &str) -> ModelItem {
    ModelItem {
        id: id.into(),
        name: name.into(),
        detail: detail.into(),
        default: false,
        description: "".into(),
    }
}

#[test]
fn matches_every_token_against_name_id_or_detail() {
    let opus = item("claude-opus-4-6", "Opus 4.6", "Anthropic · Claude Code");
    assert!(opus.matches(""));
    assert!(opus.matches("OPUS"));
    assert!(opus.matches("opus 4-6"));
    assert!(opus.matches("anthropic claude-opus"));
    assert!(!opus.matches("opus sonnet"));
}

#[test]
fn default_rows_say_so_in_their_title() {
    let mut m = item("default", "Default (recommended)", "Anthropic · Claude Code");
    assert_eq!(m.title(), "Default (recommended)");
    m.default = true;
    assert_eq!(m.title(), "Default (recommended) (default)");
    assert_eq!(m.value(), "default");
}
