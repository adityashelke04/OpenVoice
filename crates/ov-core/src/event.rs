//! Events published by the domain for the UI to render.
//!
//! The UI is a pure projection of this stream. It holds no business logic and makes
//! no decisions — it renders whatever the last event said. That constraint is what
//! keeps the app testable headlessly and lets a future CLI reuse the same core.

use crate::types::{Millis, Outcome, SessionId};
use serde::{Deserialize, Serialize};

/// A user-visible change in the engine's state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Event {
    /// Engine is idle and ready. Overlay hides.
    Idle,

    /// Capture began. Overlay appears.
    Listening {
        /// Session that started.
        session: SessionId,
        /// Profile selected from the foreground application.
        profile: String,
    },

    /// Live microphone level, emitted at roughly 30 Hz while capturing.
    ///
    /// Deliberately separate from [`Event::Listening`] so the UI can throttle or
    /// drop these without missing a state change.
    Level {
        /// RMS amplitude in `[0.0, 1.0]`.
        rms: f32,
        /// Peak amplitude in `[0.0, 1.0]`.
        peak: f32,
        /// Milliseconds captured so far.
        elapsed_ms: u64,
    },

    /// Audio is being transcribed. Overlay shows a working indicator.
    Transcribing {
        /// Session being transcribed.
        session: SessionId,
        /// Length of audio handed to the model.
        audio_ms: u64,
    },

    /// Text is being delivered to the target application.
    Injecting {
        /// Session being delivered.
        session: SessionId,
        /// Character count, so the UI can warn before a very large paste.
        chars: usize,
    },

    /// A session reached a terminal state.
    Finished {
        /// Session that ended.
        session: SessionId,
        /// How it ended.
        outcome: Outcome,
        /// Delivered text, empty for non-success outcomes.
        text: String,
        /// End-to-end latency from hotkey release.
        latency_ms: u64,
    },

    /// Something the user should know about, shown as a toast.
    Notice {
        /// Severity, driving toast styling.
        level: NoticeLevel,
        /// Message phrased as what to do, not what went wrong.
        message: String,
    },

    /// Per-stage timings for the debug panel's latency waterfall.
    ///
    /// Emitted for every session, not only slow ones: a latency regression is only
    /// visible if the baseline was recorded too.
    Timing {
        /// Session measured.
        session: SessionId,
        /// Stage name.
        stage: Stage,
        /// Duration of the stage.
        took_ms: u64,
        /// When the stage completed.
        at: Millis,
    },
}

/// Toast severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoticeLevel {
    /// Informational.
    Info,
    /// Degraded but recoverable, e.g. clipboard fallback.
    Warn,
    /// The session failed.
    Error,
}

/// A measurable stage of the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Hotkey release to capture stopped.
    Finalize,
    /// Silence trimming and voice-activity detection.
    Vad,
    /// Model inference.
    Decode,
    /// Formatting pipeline.
    Format,
    /// Text injection.
    Inject,
    /// Whole session, hotkey release to delivery.
    Total,
}

impl Stage {
    /// Target budget in milliseconds for a ten-second utterance on the reference
    /// machine (RTX 3050 Laptop). Exceeding it is a regression worth investigating.
    #[must_use]
    pub fn budget_ms(self) -> u64 {
        match self {
            Self::Finalize => 10,
            Self::Vad => 25,
            Self::Decode => 600,
            Self::Format => 5,
            Self::Inject => 120,
            Self::Total => 800,
        }
    }
}
