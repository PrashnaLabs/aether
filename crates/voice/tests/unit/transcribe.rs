use super::*;

#[test]
fn transcripts_drop_whisper_silence_markers() {
    assert_eq!(clean_transcript([" fix the bug", " in login."]), "fix the bug in login.");
    assert_eq!(clean_transcript(["[BLANK_AUDIO]", " (silence)", " [Music]"]), "");
    assert_eq!(clean_transcript([]), "");
}

#[test]
fn a_bad_model_file_is_an_error_not_a_crash() {
    let file = std::env::temp_dir().join("aether-voice-bad-model.bin");
    std::fs::write(&file, b"not a model").unwrap();
    assert!(transcribe(&file, &[0.1; 16_000]).is_err());
    std::fs::remove_file(file).unwrap();
}

#[test]
#[ignore = "needs the model at AETHER_VOICE_MODEL; macOS for say and afconvert"]
#[cfg(target_os = "macos")]
fn real_speech_becomes_text() {
    use std::process::Command;

    let dir = std::env::temp_dir().join("aether-voice-real-speech");
    std::fs::create_dir_all(&dir).unwrap();
    let (aiff, wav) = (dir.join("a.aiff"), dir.join("a.wav"));
    let ok = Command::new("say")
        .arg("-o")
        .arg(&aiff)
        .arg("fix the login bug")
        .status()
        .unwrap()
        .success();
    assert!(ok, "say failed");
    let ok = Command::new("afconvert")
        .args(["-f", "WAVE", "-d", "LEF32@16000", "-c", "1"])
        .arg(&aiff)
        .arg(&wav)
        .status()
        .unwrap()
        .success();
    assert!(ok, "afconvert failed");
    let samples: Vec<f32> =
        hound::WavReader::open(&wav).unwrap().samples::<f32>().map(Result::unwrap).collect();
    let model = std::env::var("AETHER_VOICE_MODEL").expect("set AETHER_VOICE_MODEL");
    let text = transcribe(Path::new(&model), &samples).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    let text = text.to_lowercase();
    assert!(text.contains("fix"), "heard: {text}");
    assert!(text.contains("bug"), "heard: {text}");
}
