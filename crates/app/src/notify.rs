use aether_core::Provider;
use gpui_kit::{App, SystemNotification};

/// What a chat notification says; `tag` replaces an earlier one and routes the click.
pub(crate) struct Note {
    pub tag: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub provider: Provider,
}

/// Posts a desktop notification. macOS uses the notification center directly for the
/// subtitle and provider logo; outside a bundle it falls back to AppleScript.
#[cfg_attr(target_os = "macos", allow(unused_variables))]
pub(crate) fn post(note: Note, cx: &App) {
    #[cfg(target_os = "macos")]
    {
        if native::available() {
            return native::post(note);
        }
        let script = format!(
            "display notification {} with title {} subtitle {}",
            quoted(&note.body),
            quoted(&note.title),
            quoted(&note.subtitle)
        );
        std::thread::spawn(move || {
            std::process::Command::new("osascript").args(["-e", &script]).status()
        });
        return;
    }
    #[allow(unreachable_code)]
    cx.show_system_notification(SystemNotification {
        tag: note.tag.into(),
        title: format!("{} · {}", note.title, note.subtitle).into(),
        body: note.body.into(),
        actions: Vec::new(),
    });
}

/// Asks once for permission, so the first real notification isn't lost to the prompt.
pub(crate) fn authorize() {
    #[cfg(target_os = "macos")]
    if native::available() {
        native::authorize();
    }
}

/// First line of `text`, whitespace collapsed, cut at `max` chars.
pub(crate) fn preview(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", line[..cut].trim_end()),
        None => line,
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::Note;
    use aether_core::Provider;
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSArray, NSBundle, NSError, NSString, NSURL};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationAttachment,
        UNNotificationRequest, UNUserNotificationCenter,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The center aborts the process when there's no bundle identifier.
    pub(super) fn available() -> bool {
        NSBundle::mainBundle().bundleIdentifier().is_some()
    }

    pub(super) fn authorize() {
        static ASKED: AtomicBool = AtomicBool::new(false);
        if ASKED.swap(true, Ordering::Relaxed) {
            return;
        }
        let done = RcBlock::new(|_: Bool, _: *mut NSError| {});
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                &done,
            );
    }

    pub(super) fn post(note: Note) {
        authorize();
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&note.title));
        content.setSubtitle(&NSString::from_str(&note.subtitle));
        content.setBody(&NSString::from_str(&note.body));
        content.setThreadIdentifier(&NSString::from_str(&note.tag));
        let path = logo(note.provider, &note.tag);
        if let Some(path) = &path {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            // the center moves the file into its own store; no options, so nothing untyped
            #[allow(unsafe_code)]
            if let Ok(logo) = unsafe {
                UNNotificationAttachment::attachmentWithIdentifier_URL_options_error(
                    &NSString::from_str("provider"),
                    &url,
                    None,
                )
            } {
                content.setAttachments(&NSArray::from_retained_slice(&[logo]));
            }
        }
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&note.tag),
            &content,
            None,
        );
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, None);
        if let Some(path) = path {
            let _ = std::fs::remove_file(path);
        }
    }

    /// The provider's mark on a light tile, as a fresh PNG for one attachment.
    pub(super) fn logo(provider: Provider, tag: &str) -> Option<std::path::PathBuf> {
        use gpui_kit::AssetSource;
        use resvg::{tiny_skia, usvg};
        let svg = crate::assets::AppAssets
            .load(aether_ui::provider_icon_path(provider))
            .ok()
            .flatten()?;
        let tint = if provider == Provider::Claude { "#D97757" } else { "#1F1F1F" };
        let svg = String::from_utf8_lossy(&svg).replace("currentColor", tint);
        let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).ok()?;
        let size = 256.;
        let mut pixmap = tiny_skia::Pixmap::new(size as u32, size as u32)?;
        pixmap.fill(tiny_skia::Color::from_rgba8(0xF5, 0xF3, 0xEE, 0xFF));
        let scale = size * 0.6 / tree.size().width().max(tree.size().height());
        let pad = (size - tree.size().width() * scale) / 2.;
        let top = (size - tree.size().height() * scale) / 2.;
        let transform = tiny_skia::Transform::from_row(scale, 0., 0., scale, pad, top);
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        let png = pixmap.encode_png().ok()?;
        let path = std::env::temp_dir().join(format!(
            "aether-note-{tag}-{}.png",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).ok()?;
        use std::io::Write;
        if file.write_all(&png).is_err() {
            let _ = std::fs::remove_file(&path);
            return None;
        }
        Some(path)
    }
}

/// Debug builds relaunch from `Aether Dev.app` beside the binary: macOS only gives
/// notifications, with the app's name and icon, to bundled apps.
#[cfg(all(target_os = "macos", debug_assertions))]
pub(crate) fn relaunch_in_dev_bundle() {
    use std::os::unix::process::CommandExt;
    if bundled() || std::env::var_os("AETHER_NO_DEV_BUNDLE").is_some() {
        return;
    }
    let Some(exe) = std::env::current_exe().ok() else { return };
    let Some(dir) = exe.parent() else { return };
    let app = dir.join("Aether Dev.app");
    let bin = app.join("Contents/MacOS/aether");
    let modified = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    // codesign leaves the copy newer than the build it came from
    if modified(&bin) <= modified(&exe) && dev_bundle(&exe, &app).is_err() {
        return;
    }
    let error = std::process::Command::new(&bin).args(std::env::args_os().skip(1)).exec();
    eprintln!("aether: running outside the dev bundle: {error}");
}

#[cfg(all(target_os = "macos", debug_assertions))]
fn dev_bundle(exe: &std::path::Path, app: &std::path::Path) -> std::io::Result<()> {
    let contents = app.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS"))?;
    std::fs::create_dir_all(contents.join("Resources"))?;
    std::fs::write(contents.join("Info.plist"), DEV_PLIST)?;
    std::fs::write(
        contents.join("Resources/Aether.icns"),
        include_bytes!("../assets/app-icons/Aether.icns"),
    )?;
    std::fs::copy(exe, contents.join("MacOS/aether"))?;
    let sign = |identity: &str| {
        std::process::Command::new("codesign")
            .args(["--force", "--timestamp=none", "--sign", identity])
            .arg(app)
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    // ad-hoc signature is pinned to the build's hash: each rebuild loses TCC and keychain grants
    let listing = std::process::Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()?;
    let listing = String::from_utf8_lossy(&listing.stdout);
    if first_identity(&listing).is_some_and(sign) || sign("-") {
        Ok(())
    } else {
        Err(std::io::Error::other("codesign failed"))
    }
}

#[cfg(all(target_os = "macos", debug_assertions))]
fn first_identity(listing: &str) -> Option<&str> {
    listing.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        parts.next().filter(|index| index.ends_with(')'))?;
        parts.next().filter(|hash| hash.len() == 40)
    })
}

#[cfg(all(target_os = "macos", debug_assertions))]
const DEV_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key><string>aether</string>
	<key>CFBundleIdentifier</key><string>dev.aether.Aether.dev</string>
	<key>CFBundleName</key><string>Aether</string>
	<key>CFBundleDisplayName</key><string>Aether</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>0.0.0</string>
	<key>CFBundleVersion</key><string>0</string>
	<key>CFBundleIconFile</key><string>Aether.icns</string>
	<key>NSHighResolutionCapable</key><true/>
	<key>NSAppSleepDisabled</key><true/>
	<key>NSMicrophoneUsageDescription</key><string>Aether turns your speech into text in the composer. Audio stays on this device.</string>
</dict>
</plist>
"#;

/// Shows `waiting` on the dock icon, none at 0; AppKit only hears of changes.
#[cfg(target_os = "macos")]
pub(crate) fn dock_badge(waiting: usize) {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{MainThreadMarker, NSString};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SHOWN: AtomicUsize = AtomicUsize::new(0);
    let Some(mtm) = MainThreadMarker::new() else { return };
    if SHOWN.swap(waiting, Ordering::Relaxed) == waiting {
        return;
    }
    let label = (waiting > 0).then(|| NSString::from_str(&waiting.to_string()));
    NSApplication::sharedApplication(mtm).dockTile().setBadgeLabel(label.as_deref());
}

#[cfg(all(target_os = "macos", debug_assertions))]
fn bundled() -> bool {
    std::env::current_exe().is_ok_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/"))
}

/// `text` as one AppleScript string literal.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn quoted(text: &str) -> String {
    let text = text.replace('\\', "\\\\").replace('"', "\\\"").replace(['\n', '\r'], " ");
    format!("\"{text}\"")
}

#[cfg(test)]
#[path = "../tests/unit/notify.rs"]
mod tests;
