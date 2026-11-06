use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use aether_core::{Attachment, create_private_dir, mime_for};
use image::{ImageDecoder, ImageReader};

const MAX_CONVERTED_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_CONVERTED_DIMENSION: u32 = 16_384;

pub(crate) fn pasted_name(n: usize, ext: &str) -> String {
    format!("Pasted image {n}.{ext}")
}

pub(crate) fn store_file(root: &Path, chat: i64, src: &Path) -> io::Result<Attachment> {
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    store_bytes(root, chat, name, &fs::read(src)?)
}

pub(crate) fn store_bytes(
    root: &Path,
    chat: i64,
    name: &str,
    bytes: &[u8],
) -> io::Result<Attachment> {
    let mut name = name.to_string();
    let mut mime = mime_for(Path::new(&name));
    let mut png = Vec::new();
    let mut bytes = bytes;
    if matches!(mime, "image/tiff" | "image/bmp") {
        let mut reader = ImageReader::new(io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(io::Error::other)?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_CONVERTED_DIMENSION);
        limits.max_image_height = Some(MAX_CONVERTED_DIMENSION);
        limits.max_alloc = Some(MAX_CONVERTED_PIXELS * 4);
        reader.limits(limits);
        let decoder = reader.into_decoder().map_err(io::Error::other)?;
        let (width, height) = decoder.dimensions();
        if width > MAX_CONVERTED_DIMENSION
            || height > MAX_CONVERTED_DIMENSION
            || width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > MAX_CONVERTED_PIXELS
            || decoder.total_bytes() > MAX_CONVERTED_PIXELS * 4
        {
            return Err(io::Error::other("image exceeds the conversion size limit"));
        }
        let img = image::DynamicImage::from_decoder(decoder).map_err(io::Error::other)?;
        img.write_to(&mut io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(io::Error::other)?;
        bytes = &png;
        name = Path::new(&name).with_extension("png").to_string_lossy().into_owned();
        mime = "image/png";
    }
    let dir = root.join(chat.to_string());
    create_private_dir(&dir)?;
    let mut n = fs::read_dir(&dir)?.count() + 1;
    let path = loop {
        let path = dir.join(format!("{n}-{name}"));
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                io::Write::write_all(&mut file, bytes)?;
                break path;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    };
    Ok(Attachment { name, path, mime: mime.into(), size: bytes.len() as u64 })
}

pub(crate) fn remove_chat(root: &Path, chat: i64) {
    let _ = fs::remove_dir_all(root.join(chat.to_string()));
}

/// Chats in `live` but not `loaded` failed to parse; their files stay untouched.
pub(crate) fn sweep(
    root: &Path,
    live: &HashSet<i64>,
    loaded: &HashSet<i64>,
    kept: &HashSet<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(id) = entry.file_name().to_str().and_then(|n| n.parse::<i64>().ok()) else {
            continue;
        };
        if !live.contains(&id) {
            let _ = fs::remove_dir_all(entry.path());
        } else if loaded.contains(&id) {
            for file in fs::read_dir(entry.path()).into_iter().flatten().flatten() {
                if !kept.contains(&file.path()) {
                    let _ = fs::remove_file(file.path());
                }
            }
        }
    }
}

pub(crate) fn copy_one(root: &Path, to_chat: i64, file: &Attachment) -> io::Result<Attachment> {
    store_bytes(root, to_chat, &file.name, &fs::read(&file.path)?)
}

pub(crate) fn copy_all(root: &Path, to_chat: i64, files: &[Attachment]) -> Vec<Attachment> {
    files.iter().filter_map(|f| copy_one(root, to_chat, f).ok()).collect()
}

#[cfg(test)]
#[path = "../tests/unit/attachments.rs"]
mod tests;
