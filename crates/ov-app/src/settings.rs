//! Persisted user settings.
//!
//! `ov_core::config::Config` defines the *schema* — versioned, validated, with a
//! migration path — but nothing has ever written it to disk. This is the store:
//! load at startup, save on every change, and keep the user's dictionary alongside
//! it.
//!
//! Writes are atomic (temp file, then rename). A settings file half-written by a
//! crash or a power cut would be worse than one that never changed: the dictionary
//! represents real accumulated effort, and losing it silently is unacceptable.

use std::path::PathBuf;
use std::sync::Mutex;

use ov_core::config::Config;
use ov_format::dictionary::Entry;
use ov_format::profile::Profile;
use serde::{Deserialize, Serialize};

/// Everything the user can change, in one document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Engine configuration. Shares `ov_core`'s schema so validation and migration
    /// are defined in one place.
    pub config: Config,
    /// ASR model preset name.
    pub model: String,
    /// User dictionary. Merged ahead of the builtins, so a user entry always wins.
    pub dictionary: Vec<Entry>,
    /// Per-application writing rules. Seeded from the builtins on first run so the
    /// user edits real, working profiles rather than starting from nothing.
    pub profiles: Vec<Profile>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            config: Config::default(),
            // Vestigial, and deliberately kept.
            //
            // There is one model and it is not selectable, so nothing reads this
            // to decide what to load -- the catalogue is the
            // answer to that. The field stays because `settings.toml` on every
            // existing install contains it, and a struct that no longer accepts
            // the key would make those files fail to parse on upgrade. It is
            // written out with the current engine's id so a curious reader of
            // the file is told the truth rather than "base.en".
            model: ov_asr::catalog::DEFAULT_MODEL.into(),
            dictionary: Vec::new(),
            profiles: Profile::builtins(),
        }
    }
}

impl Settings {
    fn path() -> PathBuf {
        crate::history::data_dir().join("settings.toml")
    }

    /// Load, migrate, and validate. Any failure falls back to defaults rather than
    /// refusing to start: a dictation tool that will not launch because one field
    /// is malformed is worse than one that launches with a stale setting.
    pub fn load() -> Self {
        let path = Self::path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };

        match toml::from_str::<Self>(&text) {
            Ok(mut s) => {
                match s.config.clone().migrate() {
                    Ok(c) => s.config = c,
                    Err(e) => {
                        tracing::warn!(error = %e, "settings could not be migrated; using defaults");
                        s.config = Config::default();
                    }
                }
                // An empty profile list would silently disable per-app formatting.
                // Repair rather than refuse: the user cannot be expected to know
                // this file must be non-empty.
                if s.profiles.is_empty() {
                    s.profiles = Profile::builtins();
                }
                tracing::info!(
                    model = %s.model,
                    terms = s.dictionary.len(),
                    profiles = s.profiles.len(),
                    "settings loaded"
                );
                s
            }
            Err(e) => {
                // Keep the unreadable file rather than overwriting it. It is the
                // only copy of whatever the user had.
                let backup = path.with_extension("toml.broken");
                let _ = std::fs::copy(&path, &backup);
                tracing::error!(
                    error = %e,
                    backup = %backup.display(),
                    "settings file is unreadable; kept a copy and using defaults"
                );
                Self::default()
            }
        }
    }

    /// Write atomically: full content to a temp file, then rename over the target.
    pub fn save(&self) -> Result<(), String> {
        self.config.validate().map_err(|e| e.to_string())?;

        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }

        let text = toml::to_string_pretty(self).map_err(|e| format!("encoding settings: {e}"))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("replacing settings: {e}"))?;
        Ok(())
    }
}

/// Thread-safe holder, shared with the Tauri commands.
pub struct Store {
    inner: Mutex<Settings>,
}

impl Store {
    pub fn load() -> Self {
        Self {
            inner: Mutex::new(Settings::load()),
        }
    }

    pub fn get(&self) -> Settings {
        self.inner.lock().expect("settings").clone()
    }

    /// Apply a change and persist it. The closure sees the live settings so callers
    /// cannot accidentally write back a stale copy.
    pub fn update<F: FnOnce(&mut Settings)>(&self, f: F) -> Result<Settings, String> {
        let mut guard = self.inner.lock().expect("settings");
        let before = guard.clone();
        f(&mut guard);

        if let Err(e) = guard.save() {
            // Roll back in memory so what is on screen matches what is on disk.
            *guard = before;
            return Err(e);
        }
        Ok(guard.clone())
    }
}
