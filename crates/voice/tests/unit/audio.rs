use super::*;

#[test]
fn stereo_48k_becomes_mono_16k() {
    let stereo: Vec<f32> = [0.5, -0.5].repeat(48_000);
    let out = to_speech_rate(&stereo, 2, 48_000);
    assert_eq!(out.len(), 16_000);
    assert!(out.iter().all(|s| s.abs() < 1e-6), "channels average out");
    assert_eq!(to_speech_rate(&[0.25; 16_000], 1, 16_000), vec![0.25; 16_000]);
    assert!(to_speech_rate(&[], 2, 44_100).is_empty());
}

#[test]
fn heard_tells_a_dead_mic_from_a_quiet_room_from_speech() {
    assert_eq!(heard(&[0.; 16_000]), Heard::Muted);
    assert_eq!(heard(&[]), Heard::Muted);
    assert_eq!(heard(&[0.0005; 16_000]), Heard::Silence);
    assert_eq!(heard(&[0.1; 16_000]), Heard::Speech);
}

#[test]
fn a_moment_of_speech_in_a_long_recording_is_speech() {
    let burst = |level: f32| {
        let mut clip = vec![0.; 160_000];
        clip[78_400..81_600].fill(level);
        clip
    };
    assert_eq!(heard(&burst(0.1)), Heard::Speech);
    assert_eq!(heard(&burst(0.01)), Heard::Speech);
    assert_eq!(heard(&[vec![0.; 16_000], vec![0.005; 800]].concat()), Heard::Speech);
    assert_eq!(heard(&burst(0.001)), Heard::Silence);
}
