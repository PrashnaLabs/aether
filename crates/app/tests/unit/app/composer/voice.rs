use super::*;

const MUTED: &str = if cfg!(target_os = "macos") {
    "Aether can't hear your microphone. Check System Settings > Privacy & Security > Microphone."
} else {
    "Aether can't hear your microphone. Check your sound settings."
};
const SILENCE: &str = "Didn't catch anything. Try again a little closer to the mic.";

#[test]
fn dictation_spaces_itself_against_its_neighbours() {
    assert_eq!(dictated("", 0, "hello"), "hello");
    assert_eq!(dictated("fix", 3, "the bug"), " the bug");
    assert_eq!(dictated("fix ", 4, "the bug"), "the bug");
    assert_eq!(dictated("ab", 1, "x"), " x ");
    assert_eq!(dictated("ok.", 2, "x"), " x");
    assert_eq!(dictated("héllo🙂", "héllo🙂".len(), "x"), " x");
}

#[test]
fn only_real_speech_goes_to_the_model() {
    assert_eq!(no_speech_copy(Heard::Muted), Some(MUTED));
    assert_eq!(no_speech_copy(Heard::Silence), Some(SILENCE));
    assert_eq!(no_speech_copy(Heard::Speech), None);
    assert_eq!(transcript_of(&[0.; 160]), Err(MUTED.to_string()));
    assert_eq!(transcript_of(&[0.0001; 160]), Err(SILENCE.to_string()));
}

#[test]
fn an_empty_transcript_reads_as_silence() {
    let nowhere = std::path::Path::new("/nonexistent/aether-voice-model");
    assert_eq!(transcript_outcome(Ok(String::new()), nowhere), Err(SILENCE.to_string()));
    assert_eq!(transcript_outcome(Ok("hi".into()), nowhere), Ok("hi".to_string()));
}

#[test]
fn only_a_model_that_will_not_load_is_removed() {
    let model = std::env::temp_dir().join(format!("aether-voice-{}.bin", std::process::id()));
    std::fs::write(&model, b"model").unwrap();

    let failed = VoiceError::Transcribe("boom".into());
    let note = failed.to_string();
    assert_eq!(transcript_outcome(Err(failed), &model), Err(note));
    assert!(model.exists());

    let failed = VoiceError::Model("bad magic".into());
    let note = failed.to_string();
    assert_eq!(transcript_outcome(Err(failed), &model), Err(note));
    assert!(!model.exists());
}

#[test]
fn the_banner_says_what_dictation_is_doing() {
    let chat = 1;
    assert_eq!(voice_copy(&VoiceState::Idle, 0), None);
    assert!(voice_copy(&VoiceState::NeedsModel { chat }, 0).unwrap().contains("141 MB"));
    let cancel = Arc::new(AtomicBool::new(false));
    let half = VoiceState::Downloading { chat, done: MODEL_BYTES / 2, cancel };
    assert!(voice_copy(&half, 0).unwrap().contains("50%"));
    assert_eq!(voice_copy(&VoiceState::Transcribing { chat }, 0).unwrap(), "Transcribing…");
    let opening = VoiceState::Opening { chat, started: Instant::now() };
    assert_eq!(voice_copy(&opening, 0).unwrap(), "Starting microphone…");
}

#[test]
fn the_banner_can_name_the_chat_the_text_goes_to() {
    assert_eq!(goes_to("Listening 0:03", "Fix login"), "Listening 0:03 · goes to \"Fix login\"");
}

#[test]
fn the_model_row_says_whether_it_is_installed() {
    assert!(model_status(true).starts_with("Installed"));
    assert!(model_status(false).starts_with("Not installed"));
    assert!(model_status(false).contains("141 MB"));
}
