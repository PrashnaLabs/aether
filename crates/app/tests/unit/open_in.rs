use super::*;

fn ids(apps: &[OpenApp]) -> Vec<&str> {
    apps.iter().map(|a| a.id).collect()
}

#[test]
fn only_installed_apps_in_brief_order() {
    let dirs = [PathBuf::from("/A"), PathBuf::from("/B")];
    let have = ["/A/Zed.app", "/B/Cursor.app", "/B/Nova.app", "/A/Terminal.app", FINDER_PATH];
    let apps = installed(&KNOWN, &dirs, |p| have.iter().any(|h| Path::new(*h) == p));
    assert_eq!(ids(&apps), ["finder", "terminal", "cursor", "zed", "nova"]);
    assert_eq!(apps[2].bundle, PathBuf::from("/B/Cursor.app"));
}

#[test]
fn first_directory_wins_and_missing_finder_is_dropped() {
    let dirs = [PathBuf::from("/A"), PathBuf::from("/B")];
    let apps = installed(&KNOWN, &dirs, |p| p.ends_with("Warp.app"));
    assert_eq!(ids(&apps), ["warp"]);
    assert_eq!(apps[0].bundle, PathBuf::from("/A/Warp.app"));
}

#[test]
fn preferred_falls_back_to_finder() {
    let dirs = [PathBuf::from("/A")];
    let apps = installed(&KNOWN, &dirs, |p| p.ends_with("Zed.app") || p.ends_with("Finder.app"));
    assert_eq!(preferred(&apps, "zed"), Some(1));
    assert_eq!(preferred(&apps, ""), Some(0));
    assert_eq!(preferred(&apps, "gone"), Some(0));
    assert_eq!(preferred(&[], "zed"), None);
}

#[test]
fn chat_folder_prefers_own_then_workspace() {
    assert_eq!(chat_folder("/p", "/w"), Some("/p".into()));
    assert_eq!(chat_folder("", "/w"), Some("/w".into()));
    assert_eq!(chat_folder("", ""), None);
}

#[test]
fn icon_file_name_from_plutil_output() {
    assert_eq!(icon_file_name("Code.icns\n").as_deref(), Some("Code.icns"));
    assert_eq!(icon_file_name("Terminal\n").as_deref(), Some("Terminal.icns"));
    assert_eq!(icon_file_name("  \n"), None);
    assert_eq!(icon_file_name("../x"), None);
}

#[test]
fn cache_goes_stale_when_icns_is_newer_or_png_empty() {
    let t = |s| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(s));
    assert!(cache_fresh(10, t(5), t(5)));
    assert!(cache_fresh(10, t(6), t(5)));
    assert!(!cache_fresh(10, t(4), t(5)));
    assert!(!cache_fresh(0, t(6), t(5)));
    assert!(!cache_fresh(0, None, None));
    assert!(cache_fresh(10, t(4), None));
}

#[test]
fn hung_command_is_killed_at_the_deadline() {
    #[cfg(windows)]
    let cmd = {
        let mut cmd = Command::new("ping");
        cmd.args(["-n", "30", "127.0.0.1"]);
        cmd
    };
    #[cfg(not(windows))]
    let cmd = {
        let mut cmd = Command::new("/bin/sleep");
        cmd.arg("30");
        cmd
    };
    let start = Instant::now();
    assert!(run_with_deadline(cmd, Duration::from_millis(100)).is_none());
    assert!(start.elapsed() < Duration::from_secs(5));
    #[cfg(windows)]
    let expected = b"hi\r\n";
    #[cfg(not(windows))]
    let expected = b"hi\n";
    #[cfg(windows)]
    let cmd = {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "echo", "hi"]);
        cmd
    };
    #[cfg(not(windows))]
    let cmd = {
        let mut cmd = Command::new("/bin/echo");
        cmd.arg("hi");
        cmd
    };
    assert_eq!(run_with_deadline(cmd, Duration::from_secs(5)).unwrap(), expected);
}

#[test]
fn relative_folder_is_refused_before_spawning() {
    assert!(open(Path::new("/Applications/Zed.app"), Path::new("-a")).is_err());
}

#[test]
fn project_app_prefers_project_entry_then_global() {
    let dirs = [PathBuf::from("/A")];
    let apps = installed(&KNOWN, &dirs, |p| {
        p.ends_with("Cursor.app") || p.ends_with("Zed.app") || p.ends_with("Finder.app")
    });
    let mut settings = Settings { open_in_app: "zed".into(), ..Settings::default() };
    settings.project_open_in.insert(project_key(Path::new("/p")), "cursor".into());
    assert_eq!(project_app(&apps, &settings, Path::new("/p")), Some(1));
    assert_eq!(project_app(&apps, &settings, Path::new("/q")), Some(2));
    settings.project_open_in.insert(project_key(Path::new("/p")), "gone".into());
    assert_eq!(project_app(&apps, &settings, Path::new("/p")), Some(2));
}

#[test]
fn picked_app_is_the_projects_and_the_new_default() {
    let mut settings = Settings { open_in_app: "zed".into(), ..Settings::default() };
    pick_app(&mut settings, Path::new("/p"), "cursor");
    assert_eq!(settings.project_open_in.get(&project_key(Path::new("/p"))).unwrap(), "cursor");
    assert_eq!(settings.open_in_app, "cursor");
}

#[test]
fn aether_terminal_follows_finder_and_can_be_picked() {
    let dirs = [PathBuf::from("/A")];
    let apps = with_terminal(installed(&KNOWN, &dirs, |p| {
        p.ends_with("Zed.app") || p.ends_with("Finder.app")
    }));
    assert_eq!(ids(&apps), ["finder", AETHER_TERMINAL_ID, "zed"]);
    assert_eq!(preferred(&apps, AETHER_TERMINAL_ID), Some(1));
    assert_eq!(ids(&with_terminal(Vec::new())), [AETHER_TERMINAL_ID]);
}

#[test]
fn browsers_in_list_order_with_ids_apart_from_apps() {
    let dirs = [PathBuf::from("/A"), PathBuf::from("/B")];
    let have = ["/B/Zen.app", "/A/Safari.app", "/A/Arc.app", "/B/Google Chrome.app", FINDER_PATH];
    let apps = installed(&BROWSERS, &dirs, |p| have.iter().any(|h| Path::new(*h) == p));
    assert_eq!(ids(&apps), ["safari", "chrome", "arc", "zen"]);
    // ids name the shared icon cache files
    assert!(BROWSERS.iter().all(|b| KNOWN.iter().all(|k| k.id != b.id)));
}

#[test]
fn only_http_urls_go_to_another_browser() {
    assert!(web_url("https://example.com"));
    assert!(web_url("HTTP://localhost:3000/x"));
    for url in
        ["", "-a", "about:blank", "file:///etc/hosts", "javascript:alert('http://x')", "ftp://x"]
    {
        assert!(!web_url(url), "{url}");
    }
    assert!(open_url(Path::new("/Applications/Safari.app"), "-a").is_err());
}

#[cfg(windows)]
#[test]
fn explorer_is_always_listed_first_on_windows() {
    let apps = discover(&KNOWN, Path::new("C:\\"));
    assert_eq!((apps[0].id, apps[0].name), (FINDER_ID, "File Explorer"));
    assert!(apps[0].bundle.exists(), "{:?}", apps[0].bundle);
}
