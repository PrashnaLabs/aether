use crate::VoiceError;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

const URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.en.bin";
const SHA256: &str = "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002";
pub const MODEL_BYTES: u64 = 147_964_211;

pub fn model_path() -> Option<PathBuf> {
    aether_core::config_dir().map(|dir| dir.join("models/ggml-base.en.bin"))
}

static CHECKED: Mutex<Option<(PathBuf, u64, Option<SystemTime>, bool)>> = Mutex::new(None);

pub fn installed() -> bool {
    let Some(path) = model_path() else { return false };
    verify_model(&path)
}

pub(crate) fn verify_model(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else { return false };
    if meta.len() != MODEL_BYTES {
        return false;
    }
    let stamp = (path.to_path_buf(), meta.len(), meta.modified().ok());
    if let Ok(guard) = CHECKED.lock()
        && let Some((p, len, modified, ok)) = &*guard
        && (p, *len, *modified) == (&stamp.0, stamp.1, stamp.2)
    {
        return *ok;
    }
    let ok = file_verified(path, SHA256, MODEL_BYTES);
    if let Ok(mut guard) = CHECKED.lock() {
        *guard = Some((stamp.0, stamp.1, stamp.2, ok));
    }
    ok
}

fn file_verified(path: &Path, sha256: &str, bytes: u64) -> bool {
    fs::metadata(path).is_ok_and(|m| m.len() == bytes) && hash(path).is_ok_and(|got| got == sha256)
}

pub fn remove_model() -> std::io::Result<()> {
    model_path().map_or(Ok(()), fs::remove_file)
}

pub fn download(cancel: &AtomicBool, progress: impl FnMut(u64)) -> Result<PathBuf, VoiceError> {
    let dest = model_path().ok_or_else(|| err("no config directory"))?;
    let part = dest.with_extension("bin.part");
    fs::create_dir_all(dest.parent().unwrap()).map_err(err)?;
    let result =
        fetch(&part, cancel, progress).and_then(|()| install_verified(&part, &dest, SHA256));
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result.map(|()| dest)
}

fn fetch(
    part: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<(), VoiceError> {
    let runtime = tokio::runtime::Runtime::new().map_err(err)?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()
            .map_err(net_err)?;
        let mut response =
            client.get(URL).send().await.and_then(|r| r.error_for_status()).map_err(net_err)?;
        let mut file = File::create(part).map_err(err)?;
        let mut done = 0;
        while let Some(chunk) = response.chunk().await.map_err(net_err)? {
            if cancel.load(Ordering::Relaxed) {
                return Err(err("download cancelled"));
            }
            file.write_all(&chunk).map_err(err)?;
            done += chunk.len() as u64;
            progress(done);
        }
        Ok(())
    })
}

fn install_verified(part: &Path, dest: &Path, sha256: &str) -> Result<(), VoiceError> {
    let result = hash(part).and_then(|got| {
        if got == sha256 {
            let _ = fs::remove_file(dest);
            fs::rename(part, dest).map_err(err)
        } else {
            Err(err("downloaded file failed verification"))
        }
    });
    if result.is_err() {
        let _ = fs::remove_file(part);
    }
    result
}

fn hash(path: &Path) -> Result<String, VoiceError> {
    let mut file = File::open(path).map_err(err)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buf).map_err(err)? {
            0 => break,
            n => hasher.update(&buf[..n]),
        }
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn net_err(e: reqwest::Error) -> VoiceError {
    err(e.without_url())
}

fn err(e: impl ToString) -> VoiceError {
    VoiceError::Download(e.to_string())
}

#[cfg(test)]
#[path = "../tests/unit/model.rs"]
mod tests;
