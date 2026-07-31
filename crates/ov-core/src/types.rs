//! Primitive value types shared across the domain.
//!
//! Everything here is plain data: no clocks, no IO, no platform types. Time enters
//! the domain only as [`Millis`] values supplied by the caller, which is what makes
//! the state machine deterministic and testable without sleeping.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A monotonic timestamp in milliseconds.
///
/// The domain never reads a clock itself. Adapters stamp inputs on the way in, so
/// tests can drive the machine through hours of simulated time instantly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Millis(pub u64);

impl Millis {
    /// Milliseconds elapsed since `earlier`, saturating at zero.
    #[must_use]
    pub fn since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

impl fmt::Display for Millis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ms", self.0)
    }
}

/// Identifies one dictation session from hotkey press to final disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s{}", self.0)
    }
}

/// How a session ended. Every session produces exactly one of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum Outcome {
    /// Text was transcribed, formatted, and delivered to the target application.
    Delivered,
    /// Transcription succeeded but injection failed; text is on the clipboard.
    ///
    /// This is a degraded success, not a failure: the user has not lost their words.
    ClipboardFallback(String),
    /// The user cancelled before delivery.
    Cancelled,
    /// The press was shorter than the configured minimum: a fat-finger tap.
    TooShort,
    /// Audio contained no detectable speech.
    Silent,
    /// The transcriber failed.
    AsrFailed(String),
}

impl Outcome {
    /// Whether the user ended up with their text somewhere usable.
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Delivered | Self::ClipboardFallback(_))
    }

    /// Stable identifier for persistence and metrics.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::ClipboardFallback(_) => "clipboard_fallback",
            Self::Cancelled => "cancelled",
            Self::TooShort => "too_short",
            Self::Silent => "silent",
            Self::AsrFailed(_) => "asr_failed",
        }
    }
}

/// Text produced by a [`crate::ports::Transcriber`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    /// Raw model output, before any formatting. Persisted so that history can be
    /// replayed through a newer formatter version and diffed.
    pub text: String,
    /// Detected language code, if the model reports one.
    pub language: Option<String>,
    /// Mean token log-probability, when available. Low values are a useful signal
    /// for surfacing "did you mean...?" rather than silently emitting garbage.
    pub confidence: Option<f32>,
}

/// The foreground application at the moment dictation started.
///
/// Captured on hotkey *press*, never on release: by the time text is injected the
/// user may have switched windows, and the profile must reflect where they were
/// speaking, not where they landed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundApp {
    /// Executable file name, e.g. `Code.exe`.
    pub exe: String,
    /// Window title. Useful as a decode hint (filenames, repo names).
    pub title: String,
}

/// How text should be delivered to the target application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectMode {
    /// Synthesize one Unicode key event per codepoint. No clipboard side effects.
    Keystrokes,
    /// Place on the clipboard and synthesize paste, restoring prior contents.
    ClipboardPaste,
    /// Copy to clipboard and stop; the user pastes manually.
    ClipboardOnly,
}

/// Confirmation that text reached the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectReceipt {
    /// The mode actually used, which may differ from the one requested if the
    /// adapter downgraded (for example, a secure input field rejecting paste).
    pub mode: InjectMode,
    /// Number of characters delivered.
    pub chars: usize,
}
