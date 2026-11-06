use std::{fmt, path::Path, sync::Once};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

#[derive(Debug)]
pub enum VoiceError {
    Model(String),
    Download(String),
    Transcribe(String),
    NoMicrophone,
    Capture(String),
}

impl fmt::Display for VoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(e) => write!(f, "Could not load the voice model: {e}"),
            Self::Download(e) => write!(f, "Could not download the speech model: {e}"),
            Self::Transcribe(e) => write!(f, "Could not transcribe the recording: {e}"),
            Self::NoMicrophone => write!(f, "No microphone found"),
            Self::Capture(e) => write!(f, "Could not record: {e}"),
        }
    }
}

impl std::error::Error for VoiceError {}

pub fn clean_transcript<'a>(segments: impl IntoIterator<Item = &'a str>) -> String {
    let kept: Vec<&str> = segments
        .into_iter()
        .map(str::trim)
        .filter(|s| {
            !s.is_empty()
                && !(s.starts_with('[') && s.ends_with(']'))
                && !(s.starts_with('(') && s.ends_with(')'))
        })
        .collect();
    kept.join(" ")
}

pub fn transcribe(model: &Path, samples: &[f32]) -> Result<String, VoiceError> {
    if crate::model::model_path().is_some_and(|p| p == model) && !crate::model::verify_model(model)
    {
        return Err(VoiceError::Model(
            "speech model failed verification; please re-download it".into(),
        ));
    }
    static HOOKS: Once = Once::new();
    HOOKS.call_once(whisper_rs::install_logging_hooks);
    // ponytail: model loaded per call so memory is held only during use; cache if load time shows
    let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default())
        .map_err(|e| VoiceError::Model(e.to_string()))?;
    let mut state = ctx.create_state().map_err(|e| VoiceError::Transcribe(e.to_string()))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    params.set_n_threads(threads as i32);
    state.full(params, samples).map_err(|e| VoiceError::Transcribe(e.to_string()))?;
    let texts: Vec<String> = (0..state.full_n_segments())
        .filter_map(|i| state.get_segment(i))
        .map(|s| s.to_str_lossy().map(|t| t.into_owned()))
        .collect::<Result<_, _>>()
        .map_err(|e| VoiceError::Transcribe(e.to_string()))?;
    Ok(clean_transcript(texts.iter().map(String::as_str)))
}

#[cfg(test)]
#[path = "../tests/unit/transcribe.rs"]
mod tests;
