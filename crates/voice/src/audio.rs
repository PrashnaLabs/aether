pub const SAMPLE_RATE: u32 = 16_000;
const SILENCE_RMS: f32 = 0.002;
const RMS_WINDOW: usize = 1_600;

#[derive(Debug, PartialEq, Eq)]
pub enum Heard {
    Muted,
    Silence,
    Speech,
}

pub fn to_speech_rate(samples: &[f32], channels: u16, rate: u32) -> Vec<f32> {
    let mono: Vec<f32> = samples
        .chunks_exact(channels.max(1) as usize)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect();
    let out_len = (mono.len() as u64 * SAMPLE_RATE as u64 / rate.max(1) as u64) as usize;
    // ponytail: box average, no real low-pass; use rubato if accuracy suffers
    (0..out_len)
        .map(|i| {
            let start = i * mono.len() / out_len;
            let end = ((i + 1) * mono.len() / out_len).max(start + 1);
            let slot = &mono[start..end];
            slot.iter().sum::<f32>() / slot.len() as f32
        })
        .collect()
}

pub fn heard(samples: &[f32]) -> Heard {
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    if peak == 0. {
        return Heard::Muted;
    }
    let rms = samples
        .chunks(RMS_WINDOW)
        .map(|w| (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt())
        .fold(0f32, f32::max);
    if rms < SILENCE_RMS { Heard::Silence } else { Heard::Speech }
}

#[cfg(test)]
#[path = "../tests/unit/audio.rs"]
mod tests;
