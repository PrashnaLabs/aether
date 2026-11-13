use super::*;

#[test]
fn parses_typed_counts() {
    assert_eq!(parse_count(" 2"), Some(2));
    assert_eq!(parse_count("-2"), None);
}

#[test]
fn hire_models_list_the_cheap_tier_first() {
    let list = hire_models(&[Provider::Claude, Provider::Codex]);
    let (claude, codex) = (models(Provider::Claude), models(Provider::Codex));
    assert_eq!(
        list[..2],
        [(Provider::Claude, claude[claude.len() - 1]), (Provider::Codex, codex[2])]
    );
    assert_eq!(list.len(), claude.len() + codex.len());
    assert_eq!(list.last(), Some(&(Provider::Claude, claude[0])));
}
