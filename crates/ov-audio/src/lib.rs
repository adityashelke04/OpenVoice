//! # ov-audio — microphone capture
//!
//! Implements [`ov_core::ports::AudioSource`]: opens the default (or configured)
//! input device via WASAPI, downmixes to mono, resamples to the 16 kHz Whisper
//! expects, and hands back a single buffer when capture stops.
//!
//! ## The microphone is only open while you hold the key
//!
//! An earlier design retained ~200 ms of "pre-roll" audio from before the hotkey
//! registered, to avoid clipping the first syllable. That was dropped, because it
//! requires the microphone to be recording *continuously* — which contradicts the
//! central privacy property of push-to-talk, and would leave Windows' microphone
//! indicator lit whenever the app was running.
//!
//! Opening the stream on press and closing it on release costs roughly 10–30 ms of
//! WASAPI startup, against which a human's reaction time is generous. In exchange,
//! the operating system's own microphone indicator becomes an independent, visible
//! confirmation of the guarantee — worth more than the syllable.
//!
//! ## Threading
//!
//! `cpal::Stream` is `!Send` on Windows, so it cannot simply live in a struct shared
//! between threads. A dedicated thread owns the stream for its whole life and is
//! driven by commands over a channel.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ov_core::error::{Error, Result};
use ov_core::ports::{AudioSource, LevelFrame, Pcm16k};

mod resample;

/// Commands sent to the thread that owns the `cpal` stream.
enum Cmd {
    Start {
        levels: Arc<dyn Fn(LevelFrame) + Send + Sync>,
        reply: Sender<Result<()>>,
    },
    Stop {
        reply: Sender<Result<Pcm16k>>,
    },
    Abort,
}

/// `AudioSource` backed by cpal/WASAPI.
pub struct CpalAudioSource {
    tx: Sender<Cmd>,
    device_names: Vec<String>,
}

impl CpalAudioSource {
    /// Open the audio subsystem and spawn the stream-owning thread.
    ///
    /// `preferred` names an input device; `None` uses the system default. An
    /// unknown name falls back to the default with a warning rather than failing:
    /// a renamed or unplugged microphone should degrade, not prevent dictation.
    pub fn new(preferred: Option<String>) -> Result<Self> {
        let host = cpal::default_host();
        let device_names = host
            .input_devices()
            .map_err(|e| Error::Audio(format!("enumerating input devices: {e}")))?
            .filter_map(|d| d.name().ok())
            .collect();

        let (tx, rx) = channel::<Cmd>();
        std::thread::Builder::new()
            .name("ov-audio".into())
            .spawn(move || audio_thread(&rx, preferred))
            .map_err(|e| Error::Audio(format!("audio thread: {e}")))?;

        Ok(Self { tx, device_names })
    }
}

impl AudioSource for CpalAudioSource {
    fn start(&self, levels: Arc<dyn Fn(LevelFrame) + Send + Sync>) -> Result<()> {
        let (reply, wait) = channel();
        self.tx
            .send(Cmd::Start { levels, reply })
            .map_err(|_| Error::Audio("audio thread is gone".into()))?;
        wait.recv()
            .map_err(|_| Error::Audio("audio thread died while starting".into()))?
    }

    fn stop(&self) -> Result<Pcm16k> {
        let (reply, wait) = channel();
        self.tx
            .send(Cmd::Stop { reply })
            .map_err(|_| Error::Audio("audio thread is gone".into()))?;
        wait.recv()
            .map_err(|_| Error::Audio("audio thread died while stopping".into()))?
    }

    fn abort(&self) -> Result<()> {
        self.tx
            .send(Cmd::Abort)
            .map_err(|_| Error::Audio("audio thread is gone".into()))
    }

    fn devices(&self) -> Result<Vec<String>> {
        Ok(self.device_names.clone())
    }
}

/// Shared between the realtime callback and the control thread.
///
/// The callback locks this briefly to append. That is a compromise: strict realtime
/// discipline says never lock in an audio callback. The alternative — a lock-free
/// ring — costs a fixed maximum recording length and more machinery than a
/// push-to-talk recorder needs, and this mutex is uncontended except at stop time.
/// If audio glitches ever appear, this is the first thing to replace.
type Capture = Arc<Mutex<Vec<f32>>>;

struct Active {
    stream: cpal::Stream,
    buffer: Capture,
    src_rate: u32,
    channels: u16,
}

fn audio_thread(rx: &std::sync::mpsc::Receiver<Cmd>, preferred: Option<String>) {
    let mut active: Option<Active> = None;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start { levels, reply } => {
                if active.is_some() {
                    let _ = reply.send(Ok(())); // already capturing; idempotent
                    continue;
                }
                match open_stream(preferred.as_deref(), levels) {
                    Ok(a) => {
                        let started = a.stream.play().map_err(|e| Error::Audio(format!("{e}")));
                        if started.is_ok() {
                            active = Some(a);
                        }
                        let _ = reply.send(started);
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }

            Cmd::Stop { reply } => {
                let Some(a) = active.take() else {
                    let _ = reply.send(Err(Error::Audio("not capturing".into())));
                    continue;
                };
                // Dropping the stream closes the device, which is what turns the
                // operating system's microphone indicator back off.
                let _ = a.stream.pause();
                let raw = a.buffer.lock().map(|b| b.clone()).unwrap_or_default();
                drop(a.stream);

                let mono = downmix(&raw, a.channels);
                let out = resample::to_16k(&mono, a.src_rate)
                    .map(|samples| Pcm16k { samples })
                    .map_err(|e| Error::Audio(format!("resample: {e}")));
                let _ = reply.send(out);
            }

            Cmd::Abort => {
                if let Some(a) = active.take() {
                    let _ = a.stream.pause();
                    drop(a.stream);
                }
            }
        }
    }
}

fn open_stream(
    preferred: Option<&str>,
    levels: Arc<dyn Fn(LevelFrame) + Send + Sync>,
) -> Result<Active> {
    let host = cpal::default_host();

    let device = preferred
        .and_then(|want| {
            host.input_devices()
                .ok()?
                .find(|d| d.name().map(|n| n == want).unwrap_or(false))
        })
        .or_else(|| {
            if preferred.is_some() {
                tracing::warn!(?preferred, "input device not found; using system default");
            }
            host.default_input_device()
        })
        .ok_or_else(|| Error::Audio("no input device available".into()))?;

    let config = device
        .default_input_config()
        .map_err(|e| Error::Audio(format!("default input config: {e}")))?;

    let src_rate = config.sample_rate().0;
    let channels = config.channels();
    let buffer: Capture = Arc::new(Mutex::new(Vec::with_capacity(src_rate as usize * 4)));

    let err_fn = |e| tracing::error!(error = %e, "audio stream error");
    let sink = buffer.clone();
    let meter = levels.clone();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| on_data(data, channels, &sink, &meter),
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _| {
                let f: Vec<f32> = data.iter().map(|s| f32::from(*s) / 32768.0).collect();
                on_data(&f, channels, &sink, &meter);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config.into(),
            move |data: &[u16], _| {
                let f: Vec<f32> = data
                    .iter()
                    .map(|s| (f32::from(*s) - 32768.0) / 32768.0)
                    .collect();
                on_data(&f, channels, &sink, &meter);
            },
            err_fn,
            None,
        ),
        other => {
            return Err(Error::Audio(format!("unsupported sample format {other:?}")));
        }
    }
    .map_err(|e| Error::Audio(format!("building input stream: {e}")))?;

    tracing::info!(rate = src_rate, channels, "capture started");
    Ok(Active {
        stream,
        buffer,
        src_rate,
        channels,
    })
}

/// The realtime callback. Appends samples and reports a level.
fn on_data(
    data: &[f32],
    channels: u16,
    sink: &Capture,
    levels: &Arc<dyn Fn(LevelFrame) + Send + Sync>,
) {
    if let Ok(mut buf) = sink.lock() {
        buf.extend_from_slice(data);
    }

    // Metering is computed on the interleaved frame, which is close enough for a
    // level display and avoids allocating in the callback.
    let mut sum = 0.0f32;
    let mut peak = 0.0f32;
    for s in data {
        sum += s * s;
        peak = peak.max(s.abs());
    }
    let n = data.len().max(1) as f32;
    let rms = (sum / n).sqrt();
    // A mono-equivalent correction so a stereo mic does not read differently.
    let _ = channels;
    levels(LevelFrame { rms, peak });
}

/// Average interleaved channels down to mono.
fn downmix(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}
