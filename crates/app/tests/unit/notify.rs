use super::*;

#[test]
fn notification_text_cannot_break_out_of_its_applescript_string() {
    assert_eq!(quoted("Fix the \"login\" bug"), r#""Fix the \"login\" bug""#);
    assert_eq!(quoted("a\\\" & do shell script \"x"), r#""a\\\" & do shell script \"x""#);
    assert_eq!(quoted("two\nlines"), r#""two lines""#);
}

#[test]
fn preview_is_one_trimmed_line() {
    assert_eq!(preview("  Done.\n\nAll tests   pass ", 80), "Done. All tests pass");
    assert_eq!(preview("abcdef ghij", 7), "abcdef…");
}

#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn signing_identity_is_the_first_valid_certificate() {
    let hash = "A".repeat(40);
    let listing =
        format!("  1) {hash} \"Apple Development: a (B)\"\n     1 valid identities found\n");
    assert_eq!(first_identity(&listing), Some(hash.as_str()));
    assert_eq!(first_identity("     0 valid identities found\n"), None);
}

#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn dev_bundle_is_not_pinned_to_the_build_when_a_certificate_exists() {
    use std::process::Command;
    let listing =
        Command::new("security").args(["find-identity", "-v", "-p", "codesigning"]).output();
    let listing = String::from_utf8_lossy(&listing.unwrap().stdout).into_owned();
    let dir = std::env::temp_dir().join(format!("aether-dev-bundle-{}", std::process::id()));
    let app = dir.join("Probe.app");
    dev_bundle(std::path::Path::new("/usr/bin/true"), &app).unwrap();
    let requirement = Command::new("codesign").args(["-dr", "-"]).arg(&app).output().unwrap();
    let requirement = String::from_utf8_lossy(&requirement.stdout).into_owned();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(requirement.contains("cdhash"), first_identity(&listing).is_none(), "{requirement}");
}

#[cfg(target_os = "macos")]
#[test]
fn provider_logo_is_a_png() {
    let path = native::logo(Provider::Claude, "test").expect("logo renders");
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(bytes.starts_with(b"\x89PNG"));
}

#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn dev_bundle_can_ask_for_the_microphone() {
    assert!(DEV_PLIST.contains("NSMicrophoneUsageDescription"));
}
