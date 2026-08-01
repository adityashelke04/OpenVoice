//! Events published by the domain for the UI to render.
//!
//! The UI is a pure projection of this stream. It holds no business logic and makes
//! no decisions — it renders whatever the last event said. That constraint is what
//! keeps the app testable headlessly and lets a future CLI reuse the same core.

use crate::types::{Millis, Outcome, SessionId};
use serde::{Deserialize, Serialize};

/// A user-visible change in the engine's state.
///
/// # Wire format
///
/// `rename_all_fields`, **not** `rename_all`. On an enum, `rename_all` renames the
/// *variants* — so `Level` became `"level"` while its fields stayed `snake_case`.
/// The TypeScript consumer matched on `"Level"` and read `elapsedMs`, so every
/// event fell through its switch and the UI froze while the engine ran perfectly.
///
/// Variants stay PascalCase (matching the Rust names, so the two sides are
/// greppable) and fields are camelCase (idiomatic for the JavaScript reading them).
/// The round-trip is pinned by a test below; do not change this without updating
/// `apps/ui/src/engine/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Outcome, SessionId};

    /// Pins the exact JSON the UI consumes.
    ///
    /// Regression: the enum previously carried `rename_all = "camelCase"`, which
    /// renames *variants* rather than fields. Rust emitted `{"type":"level",
    /// "elapsed_ms":…}` while the TypeScript matched `"Level"` and read
    /// `elapsedMs`. Nothing matched, so the interface sat frozen while the engine
    /// worked perfectly — a failure with no error anywhere to find.
    #[test]
    fn wire_format_matches_the_typescript_contract() {
        let json = serde_json::to_value(Event::Level {
            rms: 0.5,
            peak: 0.8,
            elapsed_ms: 1200,
        })
        .unwrap();
        assert_eq!(json["type"], "Level", "variant names stay PascalCase");
        assert_eq!(json["elapsedMs"], 1200, "fields are camelCase");
        assert!(
            json.get("elapsed_ms").is_none(),
            "no snake_case field survives"
        );

        let json = serde_json::to_value(Event::Finished {
            session: SessionId(1),
            outcome: Outcome::Delivered,
            text: "hi".into(),
            latency_ms: 640,
        })
        .unwrap();
        assert_eq!(json["type"], "Finished");
        assert_eq!(json["latencyMs"], 640);
        assert_eq!(json["outcome"]["kind"], "delivered");

        let json = serde_json::to_value(Event::Transcribing {
            session: SessionId(2),
            audio_ms: 3000,
        })
        .unwrap();
        assert_eq!(json["audioMs"], 3000);

        assert_eq!(serde_json::to_value(Event::Idle).unwrap()["type"], "Idle");
    }
}
