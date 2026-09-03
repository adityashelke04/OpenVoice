//! Recordings the user asked us to keep, and getting rid of them again.
//!
//! Kept audio is the one thing OpenVoice writes that grows without bound. At
//! 16 kHz mono it is roughly 32 kB a second, so a switch someone flipped once to
//! diagnose a problem and then forgot would quietly consume a disk.
//!
//! This module used to be `store`, which also managed the Hugging Face weight
//! cache: which models were present, how much room they took, and deleting one.
//! None of that survives a single model that ships inside the installer, but the
//! sweeper does -- retention is a user setting with real bytes behind it.

use std::path::Path;

use ov_core::error::{Error, Result};

/// Delete recordings older than `days`, returning how many went.
///
/// `days == 0` keeps everything, matching how history retention reads the same
/// value. A missing directory is success: there is nothing to clean, which is the
/// state the caller wanted.
///
/// Kept audio is the one thing OpenVoice writes that grows without bound — 16 kHz
/// mono is roughly 32 kB a second, so a switch someone flipped once to diagnose a
/// problem and then forgot would quietly consume a disk. Nothing else here needs
/// a sweeper; this does.
pub fn purge_recordings(dir: &Path, days: u32) -> Result<u64> {
    if days == 0 || !dir.is_dir() {
        return Ok(0);
    }

    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(u64::from(days) * 86_400))
        // Only reachable if the clock is set to somewhere near the epoch. Keeping
        // everything is the safe answer: deleting on a nonsense comparison would
        // destroy recordings the user still wanted.
        .ok_or_else(|| Error::Transcription("system clock is implausible".into()))?;

    let entries = std::fs::read_dir(dir)
        .map_err(|e| Error::Transcription(format!("reading {}: {e}", dir.display())))?;

    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "wav") {
            continue;
        }
        // Modified time, not the timestamp in the filename: the name is ours to
        // change and the filesystem's answer keeps working if it ever does.
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t < cutoff);
        if old {
            match std::fs::remove_file(&path) {
                Ok(()) => removed += 1,
                // One locked file must not abort the sweep; the rest still go.
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "could not delete recording")
                }
            }
        }
    }

    if removed > 0 {
        tracing::info!(removed, days, "purged old recordings");
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn recordings(tag: &str, ages_in_days: &[u64]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ov-rec-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        for age in ages_in_days {
            let path = dir.join(format!("{age}-day-old.wav"));
            std::fs::write(&path, b"RIFF").unwrap();
            let when = std::time::SystemTime::now() - std::time::Duration::from_secs(age * 86_400);
            filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(when)).unwrap();
        }
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn purge_removes_only_recordings_past_the_window() {
        let dir = recordings("window", &[1, 3, 10, 30]);

        assert_eq!(purge_recordings(&dir, 7).unwrap(), 2);
        assert_eq!(names(&dir), vec!["1-day-old.wav", "3-day-old.wav"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn purge_leaves_files_that_are_not_recordings_alone() {
        // The directory is the app's, but deleting something a user put there
        // because it happened to be old is not this function's business.
        let dir = recordings("mixed", &[30]);
        let note = dir.join("notes.txt");
        std::fs::write(&note, b"mine").unwrap();
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 86_400);
        filetime::set_file_mtime(&note, filetime::FileTime::from_system_time(when)).unwrap();

        assert_eq!(purge_recordings(&dir, 7).unwrap(), 1);
        assert_eq!(names(&dir), vec!["notes.txt"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_retention_of_zero_keeps_every_recording() {
        // Zero means "forever" here, the same as it does for history retention.
        // Reading it as "delete everything" would be a catastrophic inversion.
        let dir = recordings("forever", &[1, 400]);

        assert_eq!(purge_recordings(&dir, 0).unwrap(), 0);
        assert_eq!(names(&dir).len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn purging_a_directory_that_does_not_exist_succeeds() {
        // The normal case: recordings are off, so nothing was ever written.
        let dir = std::env::temp_dir().join(format!("ov-rec-absent-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(purge_recordings(&dir, 7).unwrap(), 0);
    }
}
