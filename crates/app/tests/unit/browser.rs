use super::*;

#[test]
fn browser_shortcuts_only_accept_tab_cycle_messages() {
    assert_eq!(tab_shortcut_message("aether:previous-tab"), Some(true));
    assert_eq!(tab_shortcut_message("aether:next-tab"), Some(false));
    assert_eq!(tab_shortcut_message("aether:quit"), None);
    assert_eq!(tab_shortcut_message(""), None);
}

fn url(raw: &str) -> Option<String> {
    match resolve_address(raw)? {
        AddressTarget::Url(url) => Some(url),
        AddressTarget::Search(_) => None,
    }
}

#[test]
fn devtools_are_gated_to_debug_builds() {
    // Test builds are debug builds: devtools on here, off in release.
    assert_eq!(devtools_enabled(), cfg!(debug_assertions));
    assert!(devtools_enabled());
}

#[test]
fn schemes_pass_through() {
    assert_eq!(url("https://example.com/a"), Some("https://example.com/a".into()));
    assert_eq!(url("file:///tmp/x.html"), Some("file:///tmp/x.html".into()));
    assert_eq!(url("about:blank"), Some("about:blank".into()));
}

#[test]
fn hosts_get_a_guessed_scheme() {
    assert_eq!(url("example.com"), Some("https://example.com".into()));
    assert_eq!(url("localhost:3000"), Some("http://localhost:3000".into()));
    assert_eq!(url("127.0.0.1:8080/x"), Some("http://127.0.0.1:8080/x".into()));
}

#[test]
fn everything_else_is_a_search() {
    assert_eq!(resolve_address("rust gpui"), Some(AddressTarget::Search("rust gpui".into())));
    assert_eq!(resolve_address("hello"), Some(AddressTarget::Search("hello".into())));
    assert_eq!(resolve_address("   "), None);
}

#[test]
fn search_urls_are_encoded() {
    assert_eq!(search_url("a b&c"), "https://www.google.com/search?q=a+b%26c");
}

#[test]
fn https_is_hidden_in_the_address_bar() {
    assert_eq!(display_url("https://example.com"), "example.com");
    assert_eq!(display_url("http://example.com"), "http://example.com");
}
