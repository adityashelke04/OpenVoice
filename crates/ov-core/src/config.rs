//! Versioned configuration.
//!
//! Config is `serde`-shaped plain data with a `version` field and an explicit
//! migration step. A dictation tool accumulates a dictionary and per-app profiles
//! that represent real user effort; silently discarding them on an upgrade because
//! a field was renamed is not acceptable. Migration is therefore designed in from
//! v0.1 rather than retrofitted after the first breaking change.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Current schema version.
///
/// **v2** (2026-08-09) added [`UpdateConfig`] and removed a vestigial `model`
/// field. The model has always been read from `ov_app::settings::Settings::model`;
/// the copy on this struct was written, migrated and validated, and then never
/// read by anything — so editing it in `settings.toml` silently did nothing.
pub const SCHEMA_VERSION: u32 = 2;

/// A physical key usable as the dictation trigger.
///
/// Deliberately a closed enum rather than a raw virtual-key code: it keeps the
/// config file readable, keeps the TOML validated at parse time, and stops users
/// binding dictation to something catastrophic like `A`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    /// Right Control. The default: present on nearly every keyboard, rarely used,
    /// and reachable without a chord.
    RightCtrl,
    /// Right Alt.
    RightAlt,
    /// Right Shift.
    RightShift,
    /// Caps Lock, remapped. Popular with users who have already disabled it.
    CapsLock,
    /// F13 through F24 — unreachable on normal keyboards but emitted by macro keys.
    F13,
    /// See [`Key::F13`].
    F14,
    /// Scroll Lock.
    ScrollLock,
}

/// The key combination that triggers dictation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chord {
    /// The trigger key.
    pub key: Key,
    /// When true, the key is swallowed and never reaches other applications.
    ///
    /// Only safe for keys with no other purpose. Defaults to false so that binding
    /// to Right Ctrl cannot break the user's normal shortcuts.
    #[serde(default)]
    pub exclusive: bool,
}

impl Default for Chord {
    fn default() -> Self {
        Self {
            key: Key::RightCtrl,
            exclusive: false,
        }
    }
}

/// How dictation is activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationMode {
    /// Hold to speak, release to transcribe.
    ///
    /// The default, because it makes it structurally impossible to leave the
    /// microphone open — a guarantee the user can verify with their own fingers
    /// rather than trust a settings toggle.
    PushToTalk,
    /// Press to start, press again to stop, with silence auto-stop as a backstop.
    Toggle,
}

/// Timing limits for the session state machine.
///
/// Deliberately not `Eq`: `silence_rms` is an amplitude threshold, and float
/// equality is not a meaningful operation on it. `PartialEq` is enough for tests
/// and config diffing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionLimits {
    /// Presses shorter than this are discarded silently as accidental taps.
    pub min_duration_ms: u64,
    /// Capture stops unconditionally after this long, so a stuck key cannot record
    /// indefinitely.
    pub max_duration_ms: u64,
    /// Audio retained from before the hotkey registered, which prevents the clipped
    /// first syllable that is the most common complaint about dictation tools.
    pub preroll_ms: u64,
    /// Below this RMS the capture is treated as silence — usually a muted mic.
    pub silence_rms: f32,
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            min_duration_ms: 300,
            max_duration_ms: 120_000,
            preroll_ms: 200,
            silence_rms: 0.002,
        }
    }
}

/// Privacy controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyConfig {
    /// Write captured audio to disk. Off by default; for debugging only.
    pub retain_audio: bool,
    /// Days of history to keep. Zero disables history entirely.
    pub history_days: u32,
    /// Regular expressions whose matches are redacted before anything is written to
    /// history or logs. Defaults cover the most common accidental secret leaks.
    pub redact_patterns: Vec<String>,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            retain_audio: false,
            history_days: 90,
            redact_patterns: vec![
                r"sk-[A-Za-z0-9]{20,}".into(),
                r"gh[pousr]_[A-Za-z0-9]{20,}".into(),
                r"AKIA[0-9A-Z]{16}".into(),
            ],
        }
    }
}

/// How OpenVoice learns that a new version exists.
///
/// # Why this is allowed to touch the network at all
///
/// Every other outbound request in this application is one the user explicitly
/// asked for — a model download they chose from a picker. An update check is not
/// that: nobody asks to be told about a release. It is therefore the one place the
/// local-first guarantee is deliberately traded, and the trade is written down in
/// ADR 0005 rather than assumed.
///
/// Three properties keep it honest, and all three are enforced in code rather than
/// promised here:
///
/// 1. **Check, never install.** The check reports a version; applying it is a
///    button the user presses. An update can never arrive unannounced.
/// 2. **One request, to one host, carrying nothing.** A signed manifest is
///    fetched. No identifier, no version histogram, no telemetry rides along.
/// 3. **Switchable off, and it stays off.** Setting `check_on_launch` to false
///    means no request is ever made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// Ask, once per launch, whether a newer release exists.
    ///
    /// Defaults **on**. A project whose entire value is incremental improvements
    /// to formatting rules cannot deliver one to a user who never learns it
    /// shipped, and every alternative — a README line, a Twitter post — reaches
    /// strictly fewer people than a prompt in the app they already opened.
    ///
    /// Defaulting off would have been the more cautious choice, and it was
    /// rejected: a privacy setting nobody finds protects nobody, while a check
    /// that is announced on first run and disableable in one click is a decision
    /// the user actually gets to make.
    pub check_on_launch: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_on_launch: true,
        }
    }
}

/// Root configuration document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Schema version, used to drive [`Config::migrate`].
    pub version: u32,
    /// Trigger chord.
    pub chord: Chord,
    /// Activation style.
    pub activation: ActivationMode,
    /// Session timing limits.
    pub limits: SessionLimits,
    /// Privacy controls.
    pub privacy: PrivacyConfig,
    /// How new versions are discovered.
    pub updates: UpdateConfig,
    /// Forced transcription language as an ISO 639-1 code (`"en"`, `"es"`, ...), or
    /// `None` to let the model auto-detect it from the audio.
    ///
    /// Defaults to `"en"` rather than auto-detect. Whisper's auto-detection looks
    /// at only the first ~30 seconds and is measurably less reliable than being
    /// told the language outright, which matters most for exactly the case this
    /// app is built around: a short, single push-to-talk utterance with no extra
    /// audio to spare. Auto-detect is there for anyone who dictates in more than
    /// one language and wants it, not the default for everyone else.
    pub language: Option<String>,
    /// Preferred input device name, or `None` for the system default.
    pub input_device: Option<String>,
    /// Above this many characters, injection switches from synthesized keystrokes
    /// to clipboard paste.
    ///
    /// Two failure modes pull this in opposite directions, so the value sits
    /// between them.
    ///
    /// *Too high* and synthesized keystrokes corrupt: Windows coalesces adjacent
    /// `VK_PACKET` events when the target cannot drain its input queue, turning
    /// `Are you the best?` into `Are ?????????????`. That is now mitigated by
    /// sending in small paced chunks rather than one batch.
    ///
    /// *Too low* and everything routes through the clipboard, which is atomic but
    /// depends on the target honouring `Ctrl+V` and reading the clipboard promptly.
    /// An Electron editor did neither, and text vanished while the engine recorded
    /// "delivered".
    ///
    /// 60 characters covers most single utterances by typing them directly —
    /// borrowing no clipboard and working in terminals that do not accept `Ctrl+V`
    /// — while longer passages still paste atomically.
    pub paste_threshold_chars: usize,
    /// Whether the UI plays a short tone when a dictation starts and another when
    /// it finishes. Defaults on; purely a frontend concern — the Rust engine
    /// never plays audio itself, this just persists the user's preference the
    /// same way every other setting does.
    pub sound_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            chord: Chord::default(),
            activation: ActivationMode::PushToTalk,
            limits: SessionLimits::default(),
            privacy: PrivacyConfig::default(),
            updates: UpdateConfig::default(),
            language: Some("en".into()),
            input_device: None,
            paste_threshold_chars: 60,
            sound_enabled: true,
        }
    }
}

impl Config {
    /// Upgrade a config document to [`SCHEMA_VERSION`].
    ///
    /// Each version gets its own arm. Migrations are additive and never drop unknown
    /// fields, so downgrading after an upgrade loses settings but not data.
    pub fn migrate(mut self) -> Result<Self> {
        // v1 -> v2. Both changes are absorbed by `#[serde(default)]` on the way
        // in: `updates` arrives as its default, and the removed `model` key is
        // ignored rather than rejected. So there is no field to transform here —
        // only the version to advance, which is what marks the document as
        // understood rather than merely parsed.
        //
        // The `model` key is deliberately *not* stripped from the user's file. It
        // costs nothing to leave, and a downgrade to a build that still reads it
        // then finds it intact. Migrations here are additive; they never destroy
        // a key they no longer understand.
        if self.version == 1 {
            self.version = 2;
        }

        if self.version < SCHEMA_VERSION {
            return Err(Error::Config(format!(
                "no migration path from schema version {}",
                self.version
            )));
        }
        if self.version > SCHEMA_VERSION {
            return Err(Error::Config(format!(
                "config was written by a newer version of OpenVoice (schema {}, this build supports {SCHEMA_VERSION})",
                self.version
            )));
        }
        self.validate()?;
        Ok(self)
    }

    /// Reject configurations that would produce confusing runtime behaviour.
    ///
    /// Failing loudly at load time is far kinder than a mysterious dead hotkey.
    pub fn validate(&self) -> Result<()> {
        if self.limits.min_duration_ms >= self.limits.max_duration_ms {
            return Err(Error::Config(
                "limits.min_duration_ms must be less than limits.max_duration_ms".into(),
            ));
        }
        if self.limits.max_duration_ms > 600_000 {
            return Err(Error::Config(
                "limits.max_duration_ms above 10 minutes risks exhausting memory".into(),
            ));
        }
        // TOML happily parses `nan`/`-nan`/`inf` float literals, and a NaN here is
        // not merely wrong, it is silent: `rms < silence_rms` is `false` for every
        // possible `rms` when `silence_rms` is NaN (all comparisons with NaN are
        // false), and the same holds for any negative threshold since RMS is never
        // negative. Either one completely and quietly disables the muted-microphone
        // check with no error anywhere -- exactly the "mysterious dead hotkey" this
        // function exists to prevent, just for a different symptom.
        if !self.limits.silence_rms.is_finite() || self.limits.silence_rms < 0.0 {
            return Err(Error::Config(
                "limits.silence_rms must be a finite, non-negative number".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        Config::default()
            .validate()
            .expect("shipped defaults must validate");
    }

    #[test]
    fn default_is_push_to_talk_on_right_ctrl() {
        let c = Config::default();
        assert_eq!(c.activation, ActivationMode::PushToTalk);
        assert_eq!(c.chord.key, Key::RightCtrl);
        // Must not swallow the key: users rely on Right Ctrl for normal shortcuts.
        assert!(!c.chord.exclusive);
    }

    #[test]
    fn sound_feedback_defaults_on() {
        // Pinned so a future refactor can't silently ship it off by default --
        // the whole point is that most users never have to find the toggle.
        assert!(Config::default().sound_enabled);
    }

    #[test]
    fn default_language_is_forced_english_not_auto_detect() {
        // A deliberate choice, not an oversight: auto-detection looks at only the
        // first ~30s of audio and is measurably less reliable than being told the
        // language, which matters most for a short single utterance. Pinned so a
        // future refactor can't silently drift it back to `None`.
        assert_eq!(Config::default().language.as_deref(), Some("en"));
    }

    #[test]
    fn auto_detect_language_is_a_valid_config() {
        let c = Config {
            language: None,
            ..Config::default()
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn rejects_inverted_duration_limits() {
        let c = Config {
            limits: SessionLimits {
                min_duration_ms: 5_000,
                max_duration_ms: 1_000,
                ..SessionLimits::default()
            },
            ..Config::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_nan_silence_threshold() {
        // A NaN threshold makes `rms < silence_rms` false for every rms, silently
        // and completely disabling muted-microphone detection with no error.
        let c = Config {
            limits: SessionLimits {
                silence_rms: f32::NAN,
                ..SessionLimits::default()
            },
            ..Config::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_negative_silence_threshold() {
        let c = Config {
            limits: SessionLimits {
                silence_rms: -0.01,
                ..SessionLimits::default()
            },
            ..Config::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_config_from_a_newer_build() {
        let c = Config {
            version: SCHEMA_VERSION + 1,
            ..Config::default()
        };
        let err = c.migrate().unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn migrate_is_identity_at_current_version() {
        let c = Config::default();
        assert_eq!(c.clone().migrate().unwrap(), c);
    }

    #[test]
    fn a_v1_document_migrates_to_the_current_schema() {
        // The shape a v0.1.1 install actually has on disk, including the `model`
        // key that v2 removed. It must load rather than be rejected: a user who
        // updates and finds their dictionary gone would be right to be angry.
        let v1 = r#"
            version = 1
            model = "small.en"
            language = "en"
            paste_threshold_chars = 60
            sound_enabled = true

            [chord]
            key = "right_ctrl"
            exclusive = false
        "#;
        let c: Config = toml::from_str(v1).expect("a v1 document must still parse");
        let c = c.migrate().expect("v1 must have a migration path");

        assert_eq!(c.version, SCHEMA_VERSION);
        // Settings the user actually chose survive untouched.
        assert_eq!(c.chord.key, Key::RightCtrl);
        assert_eq!(c.language.as_deref(), Some("en"));
        // The new section arrives at its default rather than missing.
        assert!(c.updates.check_on_launch);
    }

    #[test]
    fn update_checks_default_on() {
        // Pinned deliberately. This is a values decision documented in ADR 0005,
        // not an incidental default, and flipping it should require editing a
        // test that says so.
        assert!(Config::default().updates.check_on_launch);
    }

    #[test]
    fn update_checks_can_be_turned_off_and_still_validate() {
        let c = Config {
            updates: UpdateConfig {
                check_on_launch: false,
            },
            ..Config::default()
        };
        assert!(c.validate().is_ok());
    }
}
