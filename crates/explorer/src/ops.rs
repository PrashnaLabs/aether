use std::io::{Error, ErrorKind, Result};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// `dir/name`, when `name` is a single file or folder name.
fn child(dir: &Path, name: &str) -> Result<PathBuf> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(_)), None) => Ok(dir.join(name)),
        _ => Err(Error::new(ErrorKind::InvalidInput, format!("\"{name}\" isn't a valid name"))),
    }
}

/// Makes an empty file `name` in `dir`; an existing file is left alone.
pub fn create_file(dir: &Path, name: &str) -> Result<PathBuf> {
    let path = child(dir, name)?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
    Ok(path)
}

/// Makes folder `name` in `dir`; fails if the name is taken.
pub fn create_folder(dir: &Path, name: &str) -> Result<PathBuf> {
    let path = child(dir, name)?;
    std::fs::create_dir(&path)?;
    Ok(path)
}

/// Renames `path` to `name` in its folder; fails if the name is taken by another file.
pub fn rename(path: &Path, name: &str) -> Result<PathBuf> {
    let to = child(path.parent().unwrap_or(Path::new("/")), name)?;
    let case_only = path.to_string_lossy().to_lowercase() == to.to_string_lossy().to_lowercase();
    if to.symlink_metadata().is_ok() && !(case_only && same_file(path, &to)) {
        return Err(Error::new(ErrorKind::AlreadyExists, format!("{name} already exists")));
    }
    std::fs::rename(path, &to)?;
    Ok(to)
}

/// Whether `a` and `b` are one file, like `Foo.rs` and `foo.rs` on a case-insensitive disk.
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let id = |path: &Path| path.symlink_metadata().ok().map(|m| (m.dev(), m.ino()));
    id(a).is_some_and(|a| id(b) == Some(a))
}

#[cfg(not(unix))]
fn same_file(_: &Path, _: &Path) -> bool {
    false
}

/// Moves `path` to the Trash with `/usr/bin/trash` on macOS or `gio trash`
/// elsewhere; without either tool nothing is deleted.
pub fn trash(path: &Path) -> std::result::Result<(), String> {
    let (tool, args): (Option<PathBuf>, &[&str]) = if cfg!(target_os = "macos") {
        (Some(PathBuf::from("/usr/bin/trash")).filter(|p| p.is_file()), &[])
    } else {
        (aether_core::find_binary("gio", ""), &["trash"])
    };
    let tool =
        tool.ok_or("No trash command found (/usr/bin/trash or gio), so nothing was deleted.")?;
    let out = Command::new(&tool)
        .args(args)
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Couldn't run {}: {e}", tool.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
#[path = "../tests/unit/ops.rs"]
mod tests;
