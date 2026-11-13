use super::*;

#[test]
fn only_a_matching_file_gets_installed() {
    let dir = std::env::temp_dir().join(format!("aether-voice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (part, dest) = (dir.join("m.part"), dir.join("m.bin"));
    std::fs::write(&part, b"hello").unwrap();
    let hello = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
    assert!(install_verified(&part, &dest, "00").is_err());
    assert!(!dest.exists() && !part.exists(), "bad download is removed");
    std::fs::write(&part, b"hello").unwrap();
    install_verified(&part, &dest, hello).unwrap();
    assert!(dest.exists() && !part.exists());
}

#[test]
fn a_same_size_file_with_the_wrong_hash_is_not_verified() {
    let dir = std::env::temp_dir().join(format!("aether-voice-verify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("m.bin");
    let hello = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
    std::fs::write(&file, b"hello").unwrap();
    assert!(file_verified(&file, hello, 5));
    assert!(!file_verified(&file, "00", 5), "wrong hash rejected");
    assert!(!file_verified(&file, hello, 6), "wrong size rejected");
    std::fs::write(&file, b"hellx").unwrap();
    assert!(!file_verified(&file, hello, 5), "same-size corrupt rejected");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failed_download_is_not_a_corrupt_model() {
    assert!(matches!(err("offline"), VoiceError::Download(_)));
    assert_eq!(err("offline").to_string(), "Could not download the speech model: offline");
}

#[test]
fn the_model_url_is_pinned_to_a_commit() {
    assert!(URL.contains("/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/"));
}

#[test]
fn verify_model_rejects_wrong_size_immediately() {
    let dir = std::env::temp_dir().join(format!("aether-voice-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("m.bin");
    std::fs::write(&file, b"too small").unwrap();
    assert!(!verify_model(&file));
    let _ = std::fs::remove_dir_all(&dir);
}
