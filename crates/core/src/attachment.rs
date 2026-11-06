use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Provider;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub path: PathBuf,
    pub mime: String,
    pub size: u64,
}

pub const MAX_ATTACHMENTS: usize = 10;
pub const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;
pub const DIRECT_INLINE_BYTES: u64 = 10 * 1024 * 1024;
/// Base64 grows 4/3; Claude caps each encoded image near 5 MiB.
pub const CLAUDE_IMAGE_BYTES: u64 = 3_932_160;
/// Base64 of more would push a Claude request past its 32 MB limit.
pub const CLAUDE_MESSAGE_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
pub const INLINE_TEXT_BYTES: u64 = 100 * 1024;

pub fn mime_for(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "tif" | "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "html" => "text/html",
        "txt" | "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "toml" | "yaml" | "yml" | "sh"
        | "go" | "swift" | "java" | "c" | "h" | "cpp" | "css" | "sql" | "log" => "text/plain",
        _ => "application/octet-stream",
    }
}

pub fn is_image(mime: &str) -> bool {
    mime.starts_with("image/")
}

pub fn is_text(mime: &str) -> bool {
    mime.starts_with("text/") || mime == "application/json"
}

pub fn check_attachment(
    count_before: usize,
    name: &str,
    size: u64,
    mime: &str,
    direct: bool,
) -> Result<(), String> {
    if count_before >= MAX_ATTACHMENTS {
        return Err(format!("Up to {MAX_ATTACHMENTS} attachments"));
    }
    if size > MAX_ATTACHMENT_BYTES {
        return Err(format!("{name} is over 20 MB"));
    }
    if direct && !(is_image(mime) || is_text(mime) || mime == "application/pdf") {
        return Err(format!("{name}: this chat can only take images, PDFs and text files"));
    }
    if direct && size > DIRECT_INLINE_BYTES {
        return Err(format!("{name} is over 10 MB"));
    }
    Ok(())
}

const ATTACHED_FILES: &str = "\n\nAttached files:";

pub fn attached_files_block(files: &[Attachment]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut out = String::from(ATTACHED_FILES);
    for f in files {
        out.push_str(&format!("\n- {} ({})", f.path.display(), f.mime));
    }
    out
}

/// `text` without a trailing `attached_files_block`.
pub fn without_attached_files(text: &str) -> &str {
    text.rsplit_once(ATTACHED_FILES).map_or(text, |(own, _)| own)
}

pub fn native_image_split(
    provider: Provider,
    files: &[Attachment],
) -> (Vec<Attachment>, Vec<Attachment>) {
    let mut room = CLAUDE_MESSAGE_IMAGE_BYTES;
    let native = |f: &Attachment| {
        let web =
            matches!(f.mime.as_str(), "image/png" | "image/jpeg" | "image/gif" | "image/webp");
        match provider {
            Provider::Claude if web && f.size <= CLAUDE_IMAGE_BYTES => {
                let fits = f.size <= room;
                room = if fits { room - f.size } else { 0 };
                fits
            }
            Provider::Codex | Provider::Pi | Provider::OpenCode => web,
            _ => false,
        }
    };
    files.iter().cloned().partition(native)
}

#[cfg(test)]
#[path = "../tests/unit/attachment.rs"]
mod tests;
