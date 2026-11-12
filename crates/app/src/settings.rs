use std::path::PathBuf;
use std::{env, fs};

use aether_core::{Settings, create_private_dir};

const SERVICE: &str = "aether";
const ACCOUNT: &str = "openrouter-api-key";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    Keychain,
    Env,
}

fn path() -> Option<PathBuf> {
    aether_core::config_dir().map(|dir| dir.join("settings.json"))
}

pub fn load() -> Settings {
    let mut settings = load_saved();
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")));
    if let Some(home) = home {
        let before = settings.model_windows.len();
        seed_model_windows(&mut settings, &home.join("models_cache.json"));
        if settings.model_windows.len() != before {
            save(&settings);
        }
    }
    settings
}

fn seed_model_windows(settings: &mut Settings, path: &std::path::Path) {
    #[derive(serde::Deserialize)]
    struct Cache {
        models: Vec<Model>,
    }
    #[derive(serde::Deserialize)]
    struct Model {
        slug: String,
        context_window: u64,
    }
    let Some(cache) =
        fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<Cache>(&bytes).ok())
    else {
        return;
    };
    for model in cache.models {
        settings.model_windows.entry(model.slug).or_insert(model.context_window);
    }
}

fn load_saved() -> Settings {
    let Some(path) = path() else {
        return Settings::default();
    };
    match fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
            eprintln!("ignoring unreadable {}: {e}", path.display());
            Settings::default()
        }),
        Err(_) => Settings {
            model: env::var("OPENROUTER_MODEL").unwrap_or_else(|_| Settings::default().model),
            ..Settings::default()
        },
    }
}

pub fn save(settings: &Settings) {
    if cfg!(test) {
        return;
    }
    let Some(path) = path() else { return };
    let tmp = path.with_extension("json.tmp");
    let result =
        serde_json::to_vec_pretty(settings).map_err(std::io::Error::other).and_then(|bytes| {
            path.parent().map_or(Ok(()), create_private_dir)?;
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
            opts.open(&tmp).and_then(|mut f| std::io::Write::write_all(&mut f, &bytes))?;
            fs::rename(&tmp, &path)
        });
    if let Err(e) = result {
        eprintln!("couldn't save {}: {e}", path.display());
    }
}

pub fn api_key() -> Option<(String, KeySource)> {
    let stored = keyring::Entry::new(SERVICE, ACCOUNT).and_then(|entry| entry.get_password());
    match stored {
        Ok(key) if !key.is_empty() => Some((key, KeySource::Keychain)),
        _ => env::var("OPENROUTER_API_KEY")
            .ok()
            .filter(|key| !key.is_empty())
            .map(|key| (key, KeySource::Env)),
    }
}

pub fn store_api_key(key: &str) -> keyring::Result<()> {
    keyring::Entry::new(SERVICE, ACCOUNT)?.set_password(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_cache_seeds_missing_windows() {
        let path = env::temp_dir().join(format!(
            "aether-model-windows-{}.json",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let mut settings = Settings::default();
        settings.model_windows.insert("learned".into(), 123);
        seed_model_windows(&mut settings, &path);
        fs::write(&path, r#"{"models":[{"slug":"learned","context_window":999},{"slug":"fresh","context_window":272000}]}"#).unwrap();
        seed_model_windows(&mut settings, &path);
        assert_eq!(settings.model_windows.get("learned"), Some(&123));
        assert_eq!(settings.model_windows.get("fresh"), Some(&272_000));
        fs::write(&path, "invalid").unwrap();
        seed_model_windows(&mut settings, &path);
        assert_eq!(settings.model_windows.len(), 2);
        fs::remove_file(path).unwrap();
    }
}
