use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Aether's per-user config folder: Application Support on macOS, %APPDATA% on
/// Windows, XDG config elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = {
        let library = env::home_dir().map(|home| home.join("Library/Application Support"));
        let xdg = env::var_os("XDG_CONFIG_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| env::home_dir().map(|home| home.join(".config")));
        match (library, xdg) {
            (Some(library), Some(xdg)) => Some(pick_macos_base(library, xdg)),
            (library, xdg) => library.or(xdg),
        }
    };
    #[cfg(windows)]
    let base = env::var_os("APPDATA").filter(|dir| !dir.is_empty()).map(PathBuf::from);
    #[cfg(not(any(target_os = "macos", windows)))]
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(".config")));
    base.map(|dir| dir.join("aether"))
}

/// Where Aether keeps attachment copies; each CLI must be able to read it.
pub fn attachments_dir() -> Option<PathBuf> {
    Some(config_dir()?.join("attachments"))
}

/// Creates `path` and parents owner-only on unix; existing dirs keep their mode.
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(path)
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path)
}

// Library wins when it already holds settings, so existing installs never
// migrate by surprise; otherwise an existing XDG file is honored.
#[cfg(target_os = "macos")]
fn pick_macos_base(library: PathBuf, xdg: PathBuf) -> PathBuf {
    if !library.join("aether/settings.json").exists() && xdg.join("aether/settings.json").exists() {
        xdg
    } else {
        library
    }
}

pub fn find_binary(name: &str, override_path: &str) -> Option<PathBuf> {
    if !override_path.is_empty() {
        let p = expand_tilde(override_path);
        return p.is_file().then_some(p);
    }
    let exts = executable_exts();
    search_dirs()
        .into_iter()
        .flat_map(|d| exts.iter().map(move |e| d.join(format!("{name}{e}"))).collect::<Vec<_>>())
        .find(|p| p.is_file())
}

// Windows: bare name last, an extensionless npm shim next to a .cmd is not runnable.
fn executable_exts() -> Vec<String> {
    #[cfg(windows)]
    {
        let pathext = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let mut exts: Vec<String> =
            pathext.split(';').filter(|e| !e.is_empty()).map(str::to_lowercase).collect();
        exts.push(String::new());
        exts
    }
    #[cfg(not(windows))]
    vec![String::new()]
}

fn expand_tilde(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home_dir().join(rest),
        None if p == "~" => home_dir(),
        None => PathBuf::from(p),
    }
}

pub fn home_dir() -> PathBuf {
    env::home_dir().unwrap_or_default()
}

fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        env::split_paths(&login_path()).filter(|p| !p.as_os_str().is_empty()).collect();
    for extra in [
        home_dir().join(".local/bin"),
        home_dir().join(".cargo/bin"),
        home_dir().join(".opencode/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ] {
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    dirs
}

fn login_path() -> String {
    login_shell_path().unwrap_or_else(repaired_env_path)
}

fn login_shell_path() -> Option<String> {
    static CACHED: OnceLock<Option<String>> = OnceLock::new();
    CACHED.get_or_init(run_login_shell).clone()
}

fn run_login_shell() -> Option<String> {
    if cfg!(windows) {
        return None;
    }
    let mut child = Command::new("sh")
        .args(["-l", "-c", "printf %s \"$PATH\""])
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait().ok()? {
            Some(status) => {
                if !status.success() {
                    return None;
                }
                let mut s = String::new();
                child.stdout.take()?.read_to_string(&mut s).ok()?;
                return if s.is_empty() { None } else { Some(s) };
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn repaired_env_path() -> String {
    std::env::var("PATH").unwrap_or_default()
}

/// Login-shell PATH plus common tool folders. Blocking: runs `sh -l`.
pub fn repaired_path() -> String {
    let path = login_shell_path().unwrap_or_else(repaired_env_path);
    let home = home_dir();
    let extras = [
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        home.join(".opencode/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    let mut dirs: Vec<PathBuf> =
        env::split_paths(&path).filter(|p| !p.as_os_str().is_empty()).collect();
    for e in extras {
        if !dirs.contains(&e) {
            dirs.push(e);
        }
    }
    env::join_paths(dirs).map(|p| p.to_string_lossy().into_owned()).unwrap_or(path)
}

#[cfg(test)]
#[path = "../tests/unit/paths.rs"]
mod tests;
