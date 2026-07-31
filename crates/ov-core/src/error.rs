//! Domain error type.

use std::fmt;

/// Errors an adapter may report back into the domain.
///
/// Deliberately coarse. The domain only needs to decide *what to do next*, and the
/// possible responses are the same for most failures: fall back, tell the user, keep
/// the audio. Detailed diagnostics belong in `tracing` spans, not in match arms.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// No audio device, device disappeared, or capture could not start.
    #[error("audio: {0}")]
    Audio(String),

    /// The transcriber failed, timed out, or the sidecar died.
    #[error("transcription: {0}")]
    Transcription(String),

    /// Text could not be delivered to the target application.
    #[error("injection: {0}")]
    Injection(String),

    /// The global hotkey could not be registered or was evicted by the OS.
    #[error("hotkey: {0}")]
    Hotkey(String),

    /// Persistence failure.
    #[error("storage: {0}")]
    Storage(String),

    /// Configuration was invalid or could not be migrated.
    #[error("config: {0}")]
    Config(String),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Whether retrying the same operation could plausibly succeed.
    ///
    /// Drives the sidecar supervisor's restart policy and the UI's choice between
    /// "try again" and "here's what to fix".
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Transcription(_) | Self::Injection(_) | Self::Audio(_)
        )
    }
}

/// A short, human-readable reason suitable for a toast notification.
///
/// Toasts are read in under a second while the user is mid-task, so these are
/// phrased as *what to do*, not as *what went wrong*.
impl Error {
    /// User-facing one-liner.
    #[must_use]
    pub fn user_message(&self) -> impl fmt::Display + '_ {
        struct Msg<'a>(&'a Error);
        impl fmt::Display for Msg<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.0 {
                    Error::Audio(_) => f.write_str("No microphone input — check your input device"),
                    Error::Transcription(_) => {
                        f.write_str("Transcription failed — audio kept, try again")
                    }
                    Error::Injection(_) => f.write_str("Copied to clipboard — press Ctrl+V"),
                    Error::Hotkey(_) => f.write_str("Hotkey stopped working — reactivating"),
                    Error::Storage(_) => f.write_str("Could not save to history"),
                    Error::Config(_) => {
                        f.write_str("Settings could not be loaded — using defaults")
                    }
                }
            }
        }
        Msg(self)
    }
}
