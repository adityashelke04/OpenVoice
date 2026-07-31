//! The complete contract surface between the domain and the outside world.
//!
//! These six traits are the *only* way `ov-core` reaches hardware, the operating
//! system, the network, or the disk. Adding a seventh port requires an ADR
//! (see `docs/adr/0001-hexagonal-architecture.md`).
//!
//! Note the deliberate absence of channel types in these signatures. Inbound ports
//! push through callbacks rather than returning a `Receiver`, which keeps this crate
//! free of any specific async runtime and lets it compile to `wasm32` as the
//! purity check requires. Adapters use whatever channel they like internally.

use crate::config::Chord;
use crate::error::Result;
use crate::types::{ForegroundApp, InjectMode, InjectReceipt, Transcript};
use std::sync::Arc;

/// A press or release of the configured dictation chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// Chord went down. Capture should begin.
    Pressed,
    /// Chord came up. Capture should end and transcription begin.
    Released,
    /// The cancel key (Escape) was pressed while a session was active.
    Cancelled,
}

/// Callback invoked from the adapter's own thread.
///
/// **Realtime constraint:** for the Windows low-level keyboard hook this runs inside
/// the hook procedure, which must return in well under 10 ms or Windows silently
/// evicts the hook and every hotkey in the app stops working. Implementations must
/// do nothing but hand the event off. No locks, no allocation, no logging.
pub type HotkeySink = Arc<dyn Fn(HotkeyEvent) + Send + Sync>;

/// Observes the dictation chord globally, without stealing focus from other apps.
pub trait HotkeyListener: Send + Sync {
    /// Begin listening, delivering events to `sink`.
    fn start(&self, sink: HotkeySink) -> Result<()>;
    /// Change the observed chord while running.
    fn rebind(&self, chord: &Chord) -> Result<()>;
    /// Stop listening and release the OS hook.
    fn stop(&self) -> Result<()>;
}

/// A block of captured audio: mono `f32` samples at 16 kHz, the rate Whisper expects.
///
/// Resampling and downmixing are the adapter's problem, so the domain and the
/// transcriber never have to care what the hardware actually produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm16k {
    /// Mono samples in `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
}

impl Pcm16k {
    /// Sample rate of every [`Pcm16k`] buffer, in hertz.
    pub const RATE: u32 = 16_000;

    /// Duration of the buffer in milliseconds.
    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        (self.samples.len() as u64 * 1000) / u64::from(Self::RATE)
    }

    /// Root-mean-square amplitude, used to detect a muted or dead microphone.
    #[must_use]
    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.samples.iter().map(|s| s * s).sum();
        (sum / self.samples.len() as f32).sqrt()
    }
}

/// A short slice of live audio plus its level, emitted continuously while capturing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelFrame {
    /// RMS amplitude of this frame, for the overlay waveform.
    pub rms: f32,
    /// Peak amplitude, for clipping indication.
    pub peak: f32,
}

/// Captures microphone audio and normalizes it to 16 kHz mono.
pub trait AudioSource: Send + Sync {
    /// Start capturing. `levels` is called roughly every 20 ms for UI metering.
    fn start(&self, levels: Arc<dyn Fn(LevelFrame) + Send + Sync>) -> Result<()>;

    /// Stop capturing and return everything recorded since [`AudioSource::start`],
    /// including the pre-roll retained before the hotkey registered.
    fn stop(&self) -> Result<Pcm16k>;

    /// Discard the current capture without returning it.
    fn abort(&self) -> Result<()>;

    /// Human-readable names of available input devices.
    fn devices(&self) -> Result<Vec<String>>;
}

/// Hints that bias decoding toward the vocabulary the user actually uses.
///
/// Correcting `kubectl` or `useMemo` *during* decoding is strictly better than
/// repairing it afterwards with fuzzy string replacement, because the decoder can
/// use acoustic evidence that post-processing has already thrown away.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DecodeHint {
    /// Vocabulary terms, packed into the model's initial prompt budget.
    pub vocabulary: Vec<String>,
    /// Forced language code, or `None` to auto-detect.
    pub language: Option<String>,
}

/// Turns captured audio into text.
///
/// Implementations: `FasterWhisperSidecar` (v0.1), `WhisperCpp` (planned), `Mock`.
pub trait Transcriber: Send + Sync {
    /// Preload weights so the first real utterance is not penalized by a cold start.
    fn warm(&self) -> Result<()>;

    /// Transcribe one utterance. Blocking; callers run it off the UI thread.
    fn transcribe(&self, audio: &Pcm16k, hint: &DecodeHint) -> Result<Transcript>;

    /// Identifier of the loaded model, recorded in history so that a transcript can
    /// always be attributed to the model that produced it.
    fn model_id(&self) -> String;
}

/// Delivers text to whatever application owns the caret.
pub trait TextSink: Send + Sync {
    /// Insert `text` at the caret using `mode`.
    ///
    /// Implementations may downgrade the mode (reporting what they used in the
    /// receipt) but must never silently drop text: if delivery is impossible, leave
    /// the text on the clipboard and return an error so the caller can tell the user.
    fn inject(&self, text: &str, mode: InjectMode) -> Result<InjectReceipt>;
}

/// Identifies the foreground application so a formatting profile can be selected.
pub trait AppContext: Send + Sync {
    /// The currently focused application.
    fn foreground(&self) -> Result<ForegroundApp>;
}

/// One completed dictation session, as persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    /// Wall-clock creation time, unix milliseconds.
    pub created_at: u64,
    /// Length of the captured audio.
    pub duration_ms: u64,
    /// Unformatted model output. Kept so history can be replayed through a new
    /// formatter and diffed before the change ships.
    pub raw_text: String,
    /// Text as actually delivered.
    pub final_text: String,
    /// Name of the formatting profile applied.
    pub profile: String,
    /// Target application, if known.
    pub target_app: Option<String>,
    /// Model identifier.
    pub model: String,
    /// Outcome code from [`crate::types::Outcome::code`].
    pub status: String,
    /// End-to-end latency from hotkey release to delivery.
    pub latency_ms: u64,
}

/// Durable local storage for dictation history.
pub trait HistoryStore: Send + Sync {
    /// Append a completed session.
    fn append(&self, entry: &Utterance) -> Result<i64>;
    /// Full-text search over delivered text, newest first.
    fn search(&self, query: &str, limit: usize) -> Result<Vec<Utterance>>;
    /// Delete entries older than `days`, returning the number removed.
    fn purge_older_than(&self, days: u32) -> Result<u64>;
}
