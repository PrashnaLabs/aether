use super::*;

#[test]
fn missing_override_falls_back_to_path_search() {
    assert!(find_binary("definitely-not-a-binary-xyz", "").is_none());
}
#[test]
fn explicit_override_missing_returns_none() {
    assert!(find_binary("sh", "/definitely/not/here-xyz").is_none());
}

#[cfg(unix)]
#[test]
fn login_shell_path_is_cached_and_repaired_path_has_extras() {
    assert_eq!(login_shell_path(), login_shell_path());
    let path = repaired_path();
    assert_eq!(path, repaired_path());
    let env_path = repaired_env_path();
    if !env_path.is_empty() {
        assert!(path.split(':').any(|p| p == env_path.split(':').next().unwrap_or("")));
    }
    assert!(path.contains(".local/bin"));
}

#[cfg(unix)]
#[test]
fn private_dir_is_owner_only_and_existing_dir_keeps_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("aether-private-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let nested = dir.join("aether/attachments");
    create_private_dir(&nested).unwrap();
    assert_eq!(std::fs::metadata(&nested).unwrap().permissions().mode() & 0o777, 0o700);
    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o755)).unwrap();
    create_private_dir(&nested).unwrap();
    assert_eq!(std::fs::metadata(&nested).unwrap().permissions().mode() & 0o777, 0o755);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(target_os = "macos")]
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aether-paths-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("aether")).unwrap();
    dir
}

#[cfg(target_os = "macos")]
#[test]
fn xdg_wins_only_when_library_has_no_settings() {
    let library = scratch("lib");
    let xdg = scratch("xdg");
    assert_eq!(pick_macos_base(library.clone(), xdg.clone()), library);
    std::fs::write(xdg.join("aether/settings.json"), "{}").unwrap();
    assert_eq!(pick_macos_base(library.clone(), xdg.clone()), xdg);
    std::fs::write(library.join("aether/settings.json"), "{}").unwrap();
    assert_eq!(pick_macos_base(library.clone(), xdg), library);
}
