use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use aether_core::MutexExt;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

use crate::audio::to_speech_rate;
use crate::transcribe::VoiceError;

const MAX_SECONDS: usize = 600;

fn fatal(kind: ErrorKind) -> bool {
    !matches!(kind, ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied)
}

pub struct Recording {
    samples: Arc<Mutex<Vec<f32>>>,
    failure: Arc<Mutex<Option<String>>>,
    channels: u16,
    rate: u32,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
    failure: Arc<Mutex<Option<String>>>,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let cap = MAX_SECONDS * config.sample_rate as usize * config.channels as usize;
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            let mut buf = samples.guard();
            let room = cap.saturating_sub(buf.len());
            buf.extend(data.iter().take(room).map(|s| s.to_sample::<f32>()));
        },
        move |e| {
            if fatal(e.kind()) {
                *failure.guard() = Some(e.to_string());
            }
        },
        None,
    )
}

fn open(
    samples: Arc<Mutex<Vec<f32>>>,
    failure: Arc<Mutex<Option<String>>>,
) -> Result<(Stream, u16, u32), VoiceError> {
    let capture = |e: cpal::Error| VoiceError::Capture(e.to_string());
    let device = cpal::default_host().default_input_device().ok_or(VoiceError::NoMicrophone)?;
    let supported = device.default_input_config().map_err(capture)?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, samples, failure),
        SampleFormat::I16 => build::<i16>(&device, config, samples, failure),
        SampleFormat::U16 => build::<u16>(&device, config, samples, failure),
        other => return Err(VoiceError::Capture(format!("unsupported sample format {other}"))),
    }
    .map_err(capture)?;
    stream.play().map_err(capture)?;
    Ok((stream, config.channels, config.sample_rate))
}

impl Recording {
    pub fn start() -> Result<Self, VoiceError> {
        let samples = Arc::new(Mutex::new(Vec::new()));
        let failure = Arc::new(Mutex::new(None));
        let (opened_tx, opened_rx) = mpsc::channel();
        let (stop, stop_rx) = mpsc::channel::<()>();
        let (s, f) = (samples.clone(), failure.clone());
        let thread = std::thread::spawn(move || {
            // stream is not Send everywhere: it lives and dies on this thread
            let stream = match open(s, f) {
                Ok((stream, channels, rate)) => {
                    opened_tx.send(Ok((channels, rate))).ok();
                    stream
                }
                Err(e) => {
                    opened_tx.send(Err(e)).ok();
                    return;
                }
            };
            stop_rx.recv().ok();
            drop(stream);
        });
        let (channels, rate) = match opened_rx.recv() {
            Ok(Ok(format)) => format,
            Ok(Err(e)) => {
                thread.join().ok();
                return Err(e);
            }
            Err(_) => return Err(VoiceError::Capture("recording thread failed".into())),
        };
        Ok(Self { samples, failure, channels, rate, stop: Some(stop), thread: Some(thread) })
    }

    pub fn failure(&self) -> Option<String> {
        self.failure.guard().clone()
    }

    pub fn finish(mut self) -> Vec<f32> {
        self.halt();
        let raw = std::mem::take(&mut *self.samples.guard());
        to_speech_rate(&raw, self.channels, self.rate)
    }

    fn halt(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.halt();
    }
}

#[cfg(test)]
#[path = "../tests/unit/capture.rs"]
mod tests;
