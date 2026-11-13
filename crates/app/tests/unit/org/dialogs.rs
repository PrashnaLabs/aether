use super::*;

#[test]
fn parses_typed_amounts() {
    assert_eq!(parse_usd("$12.5"), Some(12.5));
    assert_eq!(parse_usd(" 3 "), Some(3.));
    assert_eq!(parse_usd("$ 4"), Some(4.));
    assert_eq!(parse_usd("-1"), None);
    assert_eq!(parse_usd("ten"), None);
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
