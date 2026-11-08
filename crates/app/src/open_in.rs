use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use aether_core::Settings;
use gpui_kit::assets::IconName as Lucide;

#[derive(Clone, Copy)]
pub(crate) struct Known {
    id: &'static str,
    name: &'static str,
    bundle: &'static str,
}

const FINDER_ID: &str = "finder";
const FINDER_PATH: &str = "/System/Library/CoreServices/Finder.app";
/// The app's own terminal page; has no bundle to open.
pub(crate) const AETHER_TERMINAL_ID: &str = "aether-terminal";

pub(crate) const KNOWN: [Known; 17] = [
    Known { id: FINDER_ID, name: "Finder", bundle: "Finder.app" },
    Known { id: "terminal", name: "Terminal", bundle: "Terminal.app" },
    Known { id: "iterm", name: "iTerm", bundle: "iTerm.app" },
    Known { id: "ghostty", name: "Ghostty", bundle: "Ghostty.app" },
    Known { id: "warp", name: "Warp", bundle: "Warp.app" },
    Known { id: "vscode", name: "Visual Studio Code", bundle: "Visual Studio Code.app" },
    Known {
        id: "vscode-insiders",
        name: "Visual Studio Code - Insiders",
        bundle: "Visual Studio Code - Insiders.app",
    },
    Known { id: "cursor", name: "Cursor", bundle: "Cursor.app" },
    Known { id: "windsurf", name: "Windsurf", bundle: "Windsurf.app" },
    Known { id: "zed", name: "Zed", bundle: "Zed.app" },
    Known { id: "sublime", name: "Sublime Text", bundle: "Sublime Text.app" },
    Known { id: "xcode", name: "Xcode", bundle: "Xcode.app" },
    Known { id: "android-studio", name: "Android Studio", bundle: "Android Studio.app" },
    Known { id: "idea", name: "IntelliJ IDEA", bundle: "IntelliJ IDEA.app" },
    Known { id: "idea-ce", name: "IntelliJ IDEA CE", bundle: "IntelliJ IDEA CE.app" },
    Known { id: "fleet", name: "Fleet", bundle: "Fleet.app" },
    Known { id: "nova", name: "Nova", bundle: "Nova.app" },
];

pub(crate) const BROWSERS: [Known; 13] = [
    Known { id: "safari", name: "Safari", bundle: "Safari.app" },
    Known { id: "chrome", name: "Google Chrome", bundle: "Google Chrome.app" },
    Known { id: "arc", name: "Arc", bundle: "Arc.app" },
    Known { id: "firefox", name: "Firefox", bundle: "Firefox.app" },
    Known { id: "brave", name: "Brave Browser", bundle: "Brave Browser.app" },
    Known { id: "edge", name: "Microsoft Edge", bundle: "Microsoft Edge.app" },
    Known { id: "orion", name: "Orion", bundle: "Orion.app" },
    Known { id: "zen", name: "Zen", bundle: "Zen.app" },
    Known { id: "vivaldi", name: "Vivaldi", bundle: "Vivaldi.app" },
    Known { id: "opera", name: "Opera", bundle: "Opera.app" },
    Known { id: "chromium", name: "Chromium", bundle: "Chromium.app" },
    Known { id: "dia", name: "Dia", bundle: "Dia.app" },
    Known { id: "helium", name: "Helium", bundle: "Helium.app" },
];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OpenApp {
    pub id: &'static str,
    pub name: &'static str,
    pub bundle: PathBuf,
    pub icon: Option<PathBuf>,
}

impl OpenApp {
    /// Shown when there's no extracted app icon.
    pub(crate) fn glyph(&self) -> Lucide {
        if self.id == AETHER_TERMINAL_ID { Lucide::Terminal } else { Lucide::Folder }
    }
}

/// Known apps whose bundle `exists`, in list order; the first directory holding one wins.
fn installed(known: &[Known], dirs: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Vec<OpenApp> {
    known
        .iter()
        .filter_map(|k| {
            let bundle = if k.id == FINDER_ID {
                PathBuf::from(FINDER_PATH)
            } else {
                dirs.iter().map(|d| d.join(k.bundle)).find(|p| exists(p))?
            };
            exists(&bundle).then_some(OpenApp { id: k.id, name: k.name, bundle, icon: None })
        })
        .collect()
}

/// `apps` plus the app's own terminal, right after Finder.
pub(crate) fn with_terminal(mut apps: Vec<OpenApp>) -> Vec<OpenApp> {
    let at = apps.iter().position(|a| a.id == FINDER_ID).map_or(0, |i| i + 1);
    let name = "Aether Terminal";
    apps.insert(at, OpenApp { id: AETHER_TERMINAL_ID, name, bundle: PathBuf::new(), icon: None });
    apps
}

/// Index of the saved app; an empty or no-longer-installed id means Finder.
pub(crate) fn preferred(apps: &[OpenApp], saved: &str) -> Option<usize> {
    apps.iter().position(|a| a.id == saved).or_else(|| apps.iter().position(|a| a.id == FINDER_ID))
}

pub(crate) fn project_key(project: &Path) -> String {
    project.display().to_string()
}

pub(crate) fn project_app(apps: &[OpenApp], settings: &Settings, project: &Path) -> Option<usize> {
    settings
        .project_open_in
        .get(&project_key(project))
        .and_then(|id| apps.iter().position(|a| a.id == id))
        .or_else(|| preferred(apps, &settings.open_in_app))
}

pub(crate) fn pick_app(settings: &mut Settings, project: &Path, id: &str) {
    settings.project_open_in.insert(project_key(project), id.to_string());
    settings.open_in_app = id.to_string();
}

/// Folder a chat opens: its own, else the workspace; none when both are unset.
pub(crate) fn chat_folder(cwd: &str, workspace_root: &str) -> Option<PathBuf> {
    [cwd, workspace_root].into_iter().find(|p| !p.is_empty()).map(PathBuf::from)
}

/// Icon file name from `plutil -extract ... raw` output; `.icns` is optional in the plist.
fn icon_file_name(plutil_out: &str) -> Option<String> {
    let name = plutil_out.trim();
    if name.is_empty() || name.contains('/') {
        return None;
    }
    Some(if name.ends_with(".icns") { name.into() } else { format!("{name}.icns") })
}

const TOOL_DEADLINE: Duration = Duration::from_secs(5);

/// Runs `cmd` and returns its stdout on success; kills it after `deadline`.
fn run_with_deadline(mut cmd: Command, deadline: Duration) -> Option<Vec<u8>> {
    let mut child =
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // drain on a thread: a full pipe would otherwise stall the child past the deadline
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).ok().map(|_| buf)
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = reader.join().ok().flatten();
    status.filter(|s| s.success()).and(out)
}

fn plist_string(plist: &Path, key: &str) -> Option<String> {
    let mut cmd = Command::new("/usr/bin/plutil");
    cmd.args(["-extract", key, "raw", "-o", "-"]).arg(plist);
    run_with_deadline(cmd, TOOL_DEADLINE).map(|out| String::from_utf8_lossy(&out).into_owned())
}

/// A cached PNG is reusable when non-empty and not older than its `.icns`.
fn cache_fresh(
    png_len: u64,
    png_mtime: Option<SystemTime>,
    icns_mtime: Option<SystemTime>,
) -> bool {
    png_len > 0
        && match (png_mtime, icns_mtime) {
            (Some(png), Some(icns)) => png >= icns,
            _ => true,
        }
}

fn extract_icon(bundle: &Path, id: &str, cache: &Path) -> Option<PathBuf> {
    if id == AETHER_TERMINAL_ID {
        return None;
    }
    let png = cache.join(format!("{id}.png"));
    let plist = bundle.join("Contents/Info.plist");
    let name = ["CFBundleIconFile", "CFBundleIconName"]
        .iter()
        .find_map(|key| plist_string(&plist, key).and_then(|s| icon_file_name(&s)))?;
    let icns = bundle.join("Contents/Resources").join(name);
    let mtime = |p: &Path| p.metadata().and_then(|m| m.modified()).ok();
    let png_len = png.metadata().map_or(0, |m| m.len());
    if cache_fresh(png_len, mtime(&png), mtime(&icns)) {
        return Some(png);
    }
    std::fs::create_dir_all(cache).ok()?;
    let mut cmd = Command::new("/usr/bin/sips");
    cmd.args(["-s", "format", "png", "-z", "64", "64"]).arg(&icns).arg("--out").arg(&png);
    run_with_deadline(cmd, TOOL_DEADLINE)?;
    Some(png)
}

pub(crate) fn icon_cache() -> PathBuf {
    aether_core::config_dir().unwrap_or_else(std::env::temp_dir).join("cache/open-in")
}

/// Install paths under `%LOCALAPPDATA%\Programs`, `%LOCALAPPDATA%` and the Program Files dirs.
#[cfg(windows)]
const WINDOWS_EXES: [(&str, &str); 10] = [
    ("vscode", r"Microsoft VS Code\Code.exe"),
    ("vscode-insiders", r"Microsoft VS Code Insiders\Code - Insiders.exe"),
    ("cursor", r"cursor\Cursor.exe"),
    ("windsurf", r"Windsurf\Windsurf.exe"),
    ("sublime", r"Sublime Text\sublime_text.exe"),
    ("chrome", r"Google\Chrome\Application\chrome.exe"),
    ("edge", r"Microsoft\Edge\Application\msedge.exe"),
    ("firefox", r"Mozilla Firefox\firefox.exe"),
    ("brave", r"BraveSoftware\Brave-Browser\Application\brave.exe"),
    ("vivaldi", r"Vivaldi\Application\vivaldi.exe"),
];

#[cfg(windows)]
fn discover_windows(known: &[Known]) -> Vec<OpenApp> {
    let env = |key: &str| std::env::var_os(key).map(PathBuf::from);
    let local = env("LOCALAPPDATA");
    let roots: Vec<PathBuf> = [
        local.as_ref().map(|d| d.join("Programs")),
        local,
        env("ProgramFiles"),
        env("ProgramFiles(x86)"),
    ]
    .into_iter()
    .flatten()
    .collect();
    known
        .iter()
        .filter_map(|k| {
            if k.id == FINDER_ID {
                let bundle = env("SystemRoot")?.join("explorer.exe");
                return Some(OpenApp { id: k.id, name: "File Explorer", bundle, icon: None });
            }
            let exe = WINDOWS_EXES.iter().find(|(id, _)| *id == k.id)?.1;
            let bundle = roots.iter().map(|r| r.join(exe)).find(|p| p.exists())?;
            Some(OpenApp { id: k.id, name: k.name, bundle, icon: None })
        })
        .collect()
}

/// Blocking: probes the filesystem only. Run off the UI thread.
pub(crate) fn discover(known: &[Known], home: &Path) -> Vec<OpenApp> {
    #[cfg(windows)]
    return discover_windows(known);
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let dirs = [
        PathBuf::from("/Applications"),
        home.join("Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    installed(known, &dirs, Path::exists)
}

/// Blocking: shells out to plutil/sips (each capped at 5 s). Icons in `apps` order. Run off the UI thread.
pub(crate) fn icons(apps: &[OpenApp], cache: &Path) -> Vec<Option<PathBuf>> {
    apps.iter().map(|a| extract_icon(&a.bundle, a.id, cache)).collect()
}

/// Only http(s) pages go to another browser; the scheme also keeps `open` from reading a flag.
pub(crate) fn web_url(url: &str) -> bool {
    url.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

/// Blocking until `open` returns. Run off the UI thread.
pub(crate) fn open(app: &Path, folder: &Path) -> Result<(), String> {
    // a leading "-" would be read by `open` as a flag
    if !folder.is_absolute() {
        return Err("Folder path isn't absolute".into());
    }
    open_with(app, folder.as_os_str())
}

/// Blocking until `open` returns. Run off the UI thread.
pub(crate) fn open_url(app: &Path, url: &str) -> Result<(), String> {
    if !web_url(url) {
        return Err("Only http and https pages can open in another browser".into());
    }
    open_with(app, url.as_ref())
}

// explorer.exe exits 1 even when it worked, so only the spawn is checked
#[cfg(windows)]
fn open_with(app: &Path, target: &OsStr) -> Result<(), String> {
    aether_core::command(app).arg(target).spawn().map(drop).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn open_with(app: &Path, target: &OsStr) -> Result<(), String> {
    let out = Command::new("/usr/bin/open")
        .arg("-a")
        .arg(app)
        .arg(target)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err.lines().next().unwrap_or("open failed").to_string())
}

#[cfg(test)]
#[path = "../tests/unit/open_in.rs"]
mod tests;
