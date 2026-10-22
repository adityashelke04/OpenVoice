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

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ov_core::config::Config;
use ov_format::dictionary::Entry;
use ov_format::profile::Profile;
use serde::{Deserialize, Serialize};

/// Everything the user can change, in one document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Monotonic persisted revision used to reject stale whole-document saves.
    pub revision: u64,
    /// Startup read failure: defaults may run, but cannot replace the original.
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub load_warning: Option<String>,
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
            revision: 0,
            load_warning: None,
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
    /// Retention must use authentic preferences, never read-only fallback defaults.
    pub fn retention_policy(&self) -> Option<&ov_core::config::PrivacyConfig> {
        self.load_warning.is_none().then_some(&self.config.privacy)
    }
    fn path() -> PathBuf {
        crate::history::data_dir().join("settings.toml")
    }

    /// Load usable settings without replacing an unreadable original. Defaults
    /// remain read-only after failure until the original can be reloaded.
    fn load_from(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => return Self::read_only(path, &e.to_string()),
        };

        match toml::from_str::<Self>(&text) {
            Ok(mut s) => {
                match s.config.clone().migrate() {
                    Ok(c) => s.config = c,
                    Err(e) => {
                        tracing::warn!(error = %e, "settings could not be migrated; using defaults");
                        s.config = Config::default();
                        s.load_warning = Self::read_only(path, &e.to_string()).load_warning;
                    }
                }
                // An empty profile list would silently disable per-app formatting.
                // Repair rather than refuse: the user cannot be expected to know
                // this file must be non-empty.
                if s.profiles.is_empty() {
                    s.profiles = Profile::builtins();
                }
                // A model name from a build that is no longer installed.
                // Upgrading from 0.4.x leaves `small.en` or `large-v3-turbo`
                // here, and every one of those is now unknown. The engine would
                // fall back and run correctly either way, but the Models screen
                // would show nothing selected, which reads as a broken install
                // rather than a migrated one. Repair it here, once.
                if ov_asr::catalog::find(&s.model).is_none() {
                    tracing::info!(
                        was = %s.model,
                        now = ov_asr::catalog::DEFAULT_MODEL,
                        "settings named a model this build does not have; migrating"
                    );
                    s.model = ov_asr::catalog::DEFAULT_MODEL.into();
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
                // Never replace an existing recovery copy. The original remains
                // read-only regardless of whether a backup can be created.
                let backup = path.with_extension("toml.broken");
                let backup_error = if backup.exists() {
                    None
                } else {
                    std::fs::copy(path, &backup).err()
                };
                let detail = match backup_error {
                    Some(error) => format!("{e}; recovery copy could not be created: {error}"),
                    None => e.to_string(),
                };
                Self::read_only(path, &detail)
            }
        }
    }

    fn read_only(path: &Path, error: &str) -> Self {
        let warning = format!("Settings could not be read from {}. The original file is preserved; changes are disabled to protect your dictionary and profiles. Fix the file, disk space or folder permissions, then retry. {error}", path.display());
        tracing::error!(error = %error, "settings unavailable; using read-only defaults");
        Self {
            load_warning: Some(warning),
            ..Self::default()
        }
    }

    /// Write atomically: full content to a temp file, then rename over the target.
    fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(warning) = &self.load_warning {
            return Err(warning.clone());
        }
        self.config.validate().map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }

        let text = toml::to_string_pretty(self).map_err(|e| format!("encoding settings: {e}"))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("replacing settings: {e}"))?;
        Ok(())
    }
}

/// Thread-safe holder, shared with the Tauri commands.
pub struct Store {
    path: PathBuf,
    inner: Mutex<Settings>,
}

impl Store {
    /// Persist and apply in the same order; application failures cannot undo disk.
    pub fn replace_and_apply(
        &self,
        settings: Settings,
        apply: impl FnOnce(&Settings, &Settings),
    ) -> Result<Settings, String> {
        self.update_then_apply(
            Some(settings.revision),
            |s| *s = settings,
            |s| s.save_to(&self.path),
            apply,
        )
    }

    fn update_then_apply<
        F: FnOnce(&mut Settings),
        P: FnOnce(&Settings) -> Result<(), String>,
        A: FnOnce(&Settings, &Settings),
    >(
        &self,
        expected: Option<u64>,
        f: F,
        persist: P,
        apply: A,
    ) -> Result<Settings, String> {
        let mut guard = self.inner.lock().expect("settings");
        if guard.load_warning.is_some() {
            let mut reloaded = Settings::load_from(&self.path);
            if reloaded.load_warning.is_none() {
                reloaded.revision = reloaded.revision.max(guard.revision).saturating_add(1);
            }
            let warning = reloaded.load_warning.clone().unwrap_or_else(|| "Settings were recovered from disk. Reloaded your dictionary and profiles; retry your edit.".into());
            *guard = reloaded;
            return Err(warning);
        }
        if expected.is_some_and(|v| v != guard.revision) {
            return Err("Settings changed in another window. Retry your edit.".into());
        }
        let before = guard.clone();
        f(&mut guard);
        guard.revision = before.revision.saturating_add(1);
        if let Err(error) = persist(&guard) {
            *guard = before;
            return Err(error);
        }
        apply(&before, &guard);
        Ok(guard.clone())
    }
    pub fn load() -> Self {
        Self::load_from(Settings::path())
    }

    fn load_from(path: PathBuf) -> Self {
        Self {
            inner: Mutex::new(Settings::load_from(&path)),
            path,
        }
    }

    pub fn get(&self) -> Settings {
        let mut guard = self.inner.lock().expect("settings");
        if guard.load_warning.is_some() {
            let mut reloaded = Settings::load_from(&self.path);
            if reloaded.load_warning.is_none() {
                reloaded.revision = reloaded.revision.max(guard.revision).saturating_add(1);
                *guard = reloaded;
            }
        }
        guard.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_settings_never_authorize_retention_from_fallback_defaults() {
        let mut settings = Settings {
            load_warning: Some("original settings are locked".into()),
            ..Default::default()
        };
        assert!(settings.retention_policy().is_none());
        settings.load_warning = None;
        assert!(settings.retention_policy().is_some());
    }

    fn temp_settings(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ov-settings-{}-{}-{}",
            std::process::id(),
            name,
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.toml")
    }

    #[test]
    fn unreadable_settings_never_overwrite_the_original() {
        let path = temp_settings("invalid-utf8");
        let original = [0xff, 0xfe, 0x80];
        std::fs::write(&path, original).unwrap();
        let store = Store::load_from(path.clone());
        let mut edited = store.get();
        edited.config.sound_enabled = false;
        assert!(
            store.replace_and_apply(edited, |_, _| {}).is_err(),
            "unreadable original must make defaults read-only"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn failed_malformed_backup_never_authorizes_overwriting_source() {
        let path = temp_settings("backup-failed");
        let original = "dictionary = [unterminated";
        std::fs::write(&path, original).unwrap();
        std::fs::create_dir(path.with_extension("toml.broken")).unwrap();
        let store = Store::load_from(path.clone());
        assert!(store.replace_and_apply(store.get(), |_, _| {}).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn locked_file_reload_preserves_dictionary_before_retrying_edit() {
        use std::os::windows::fs::OpenOptionsExt;
        let path = temp_settings("locked");
        let mut original = Settings::default();
        original
            .dictionary
            .push(Entry::new("Zyma", &["zima"], "code"));
        original.profiles[0].name = "personal profile".into();
        original.save_to(&path).unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let store = Store::load_from(path.clone());
        let mut stale_defaults = store.get();
        stale_defaults.config.sound_enabled = false;
        drop(lock);
        assert!(
            store.replace_and_apply(stale_defaults, |_, _| {}).is_err(),
            "must reload and ask for retry instead of applying stale defaults"
        );
        let mut recovered = store.get();
        assert_eq!(recovered.dictionary, original.dictionary);
        assert_eq!(recovered.profiles[0].name, "personal profile");
        recovered.config.sound_enabled = false;
        store.replace_and_apply(recovered, |_, _| {}).unwrap();
        assert_eq!(Settings::load_from(&path).dictionary, original.dictionary);
        assert!(!Settings::load_from(&path).config.sound_enabled);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn background_reload_rejects_a_read_only_defaults_snapshot() {
        use std::os::windows::fs::OpenOptionsExt;
        let path = temp_settings("reload-generation");
        let mut original = Settings::default();
        original
            .dictionary
            .push(Entry::new("Zyma", &["zima"], "code"));
        original.profiles[0].name = "personal profile".into();
        original.save_to(&path).unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let store = Store::load_from(path.clone());
        let mut stale = store.get();
        assert!(stale.load_warning.is_some());
        assert_eq!(stale.revision, 0);
        // IPC skips the warning during deserialization; it is not an edit token.
        stale.load_warning = None;
        stale.config.sound_enabled = false;
        drop(lock);
        let recovered = store.get();
        assert!(recovered.revision > original.revision);
        assert_eq!(
            Settings::load_from(&path).revision,
            original.revision,
            "recovery generation is not persisted before an actual edit"
        );
        assert_eq!(recovered.dictionary, original.dictionary);
        assert!(
            store.replace_and_apply(stale, |_, _| {}).is_err(),
            "a defaults snapshot must remain stale after background recovery"
        );
        assert_eq!(Settings::load_from(&path).dictionary, original.dictionary);
        assert_eq!(
            Settings::load_from(&path).profiles[0].name,
            "personal profile"
        );
        let mut legitimate = store.get();
        legitimate.config.sound_enabled = false;
        store.replace_and_apply(legitimate, |_, _| {}).unwrap();
        assert_eq!(Settings::load_from(&path).dictionary, original.dictionary);
        assert!(!Settings::load_from(&path).config.sound_enabled);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn persisted_preferences_survive_failed_live_apply_and_later_edit() {
        let path = temp_settings("live-apply");
        let store = Store::load_from(path.clone());
        let mut edited = store.get();
        edited.model = "whisper-tiny.en".into();
        let mut warning = None;
        let saved = store
            .replace_and_apply(edited, |_, saved| {
                assert_eq!(
                    Settings::load_from(&path).model,
                    saved.model,
                    "disk precedes live application"
                );
                let hotkey: Result<(), &str> = Err("hotkey rebind failed");
                warning = hotkey.err();
            })
            .unwrap();
        assert_eq!(warning, Some("hotkey rebind failed"));
        assert_eq!(saved.model, "whisper-tiny.en");
        let mut next = store.get();
        next.config.sound_enabled = false;
        store.replace_and_apply(next, |_, _| {}).unwrap();
        assert_eq!(Settings::load_from(&path).model, "whisper-tiny.en");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn newer_save_cannot_apply_before_an_older_save_finishes() {
        use std::sync::{mpsc, Arc};
        let store = Arc::new(Store {
            path: PathBuf::new(),
            inner: Mutex::new(Settings::default()),
        });
        let applied = Arc::new(Mutex::new(Vec::new()));
        let (started, entered) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let first = store.clone();
        let order = applied.clone();
        let a = std::thread::spawn(move || {
            first
                .update_then_apply(
                    None,
                    |s| s.model = "A".into(),
                    |_| Ok(()),
                    |_, saved| {
                        started.send(()).unwrap();
                        blocked.recv().unwrap();
                        order.lock().unwrap().push(saved.model.clone());
                    },
                )
                .unwrap()
        });
        entered.recv().unwrap();
        let second = store.clone();
        let order = applied.clone();
        let (finished, got) = mpsc::channel();
        let b = std::thread::spawn(move || {
            second
                .update_then_apply(
                    None,
                    |s| s.model = "B".into(),
                    |_| Ok(()),
                    |_, saved| {
                        order.lock().unwrap().push(saved.model.clone());
                        finished.send(()).unwrap();
                    },
                )
                .unwrap()
        });
        let raced = got
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_ok();
        release.send(()).unwrap();
        a.join().unwrap();
        b.join().unwrap();
        assert!(
            !raced,
            "a newer revision applied while an older application was still pending"
        );
        assert_eq!(*applied.lock().unwrap(), vec!["A", "B"]);
    }

    #[test]
    fn stale_settings_cannot_revert_a_newer_persisted_document() {
        let store = Store {
            path: PathBuf::new(),
            inner: Mutex::new(Settings::default()),
        };
        let mut stale = store.get();
        store
            .update_then_apply(None, |s| s.model = "tiny.en".into(), |_| Ok(()), |_, _| {})
            .unwrap();
        stale.model = "old-model".into();
        let result =
            store.update_then_apply(Some(stale.revision), |s| *s = stale, |_| Ok(()), |_, _| {});
        assert!(result.is_err());
        assert_eq!(store.get().model, "tiny.en");
    }

    #[test]
    fn failed_disk_write_preserves_values_and_revision() {
        let store = Store {
            path: PathBuf::new(),
            inner: Mutex::new(Settings::default()),
        };
        let before = store.get();
        assert!(store
            .update_then_apply(
                None,
                |s| s.model = "tiny.en".into(),
                |_| Err("disk full".into()),
                |_, _| {}
            )
            .is_err());
        assert_eq!(store.get().model, before.model);
        assert_eq!(store.get().revision, before.revision);
    }
}
