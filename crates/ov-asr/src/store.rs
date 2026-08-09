//! What is actually on disk, and getting rid of it.
//!
//! The Models screen could offer a 1.6 GB download and then had nothing to say
//! about it afterwards: no indication of which models were already present, how
//! much room they took, or any way to remove one. A user who tried the accurate
//! model, found it too slow on a CPU, and switched back had paid for the download
//! twice over — once in time, once in disk they could not reclaim without finding
//! `%APPDATA%\OpenVoice\models` by hand.
//!
//! # Why this reads the cache directly
//!
//! Asking the sidecar would be the obvious route and is the wrong one here. The
//! sidecar is a child process that exists to run inference; it is busy during a
//! decode, it is not running at all before the engine has started, and the Models
//! screen has to render during the very first-run download when there is no
//! healthy sidecar to ask. Directory layout is a stable, documented
//! `huggingface_hub` convention — a repository `org/name` is stored under
//! `<cache>/models--org--name` — so reading it needs no cooperation from anyone.
//!
//! The one thing this cannot know is whether a snapshot is *complete*. A transfer
//! interrupted halfway leaves real bytes in a real directory. Reporting those
//! bytes is still the honest answer to "what is this using", and the sidecar
//! re-verifies the snapshot before every load anyway.

use std::path::{Path, PathBuf};

use crate::catalog::ModelSpec;
use ov_core::error::{Error, Result};

/// Directory name `huggingface_hub` gives a repository.
///
/// `Systran/faster-whisper-base.en` becomes
/// `models--Systran--faster-whisper-base.en`.
#[must_use]
pub fn cache_dir_name(repo: &str) -> String {
    format!("models--{}", repo.replace('/', "--"))
}

/// Where a model's files live under the app's model root.
///
/// `root` is what the app passes as `HF_HOME`; `huggingface_hub` puts its blobs
/// under a `hub` subdirectory of that, which is why this is not simply
/// `root.join(name)`.
#[must_use]
pub fn cache_path(root: &Path, spec: &ModelSpec) -> PathBuf {
    root.join("hub").join(cache_dir_name(spec.repo))
}

/// Bytes a model occupies, or `None` when it is not present at all.
///
/// `Some(0)` and `None` mean different things and both are reachable: an empty
/// directory left behind by a failed transfer is present-but-empty, and the UI
/// should offer to remove it rather than claim there is nothing there.
#[must_use]
pub fn installed_bytes(root: &Path, spec: &ModelSpec) -> Option<u64> {
    let path = cache_path(root, spec);
    if !path.is_dir() {
        return None;
    }
    Some(dir_size(&path))
}

/// Total size of a directory tree, skipping anything unreadable.
///
/// Errors are skipped rather than propagated on purpose: this figure is shown
/// beside a delete button, and one locked file should not turn the whole screen
/// into an error. An undercount is the right failure here.
fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            // `symlink_metadata`, not `metadata`: huggingface_hub stores a
            // snapshot as links into a shared blob directory that lives under
            // this same tree, so following them would count every byte twice —
            // once as the blob, once as the link pointing at it.
            Ok(_) => e.path().symlink_metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

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

/// Delete a model's files.
///
/// Removing a model that is not there succeeds. The caller is asking for it to be
/// gone, and it is.
pub fn remove(root: &Path, spec: &ModelSpec) -> Result<()> {
    let path = cache_path(root, spec);
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&path)
        .map_err(|e| Error::Transcription(format!("could not delete {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    fn spec() -> &'static ModelSpec {
        catalog::find("base.en").unwrap()
    }

    #[test]
    fn cache_dir_name_follows_the_huggingface_convention() {
        assert_eq!(
            cache_dir_name("Systran/faster-whisper-base.en"),
            "models--Systran--faster-whisper-base.en"
        );
        assert_eq!(
            cache_dir_name("deepdml/faster-whisper-large-v3-turbo-ct2"),
            "models--deepdml--faster-whisper-large-v3-turbo-ct2"
        );
    }

    #[test]
    fn a_missing_model_reports_none_not_zero() {
        // The difference the UI renders as "not downloaded" versus "downloaded,
        // and somehow empty" -- which is a broken transfer worth offering to
        // clear rather than something to hide.
        let dir = std::env::temp_dir().join(format!("ov-store-{}", std::process::id()));
        assert_eq!(installed_bytes(&dir, spec()), None);
    }

    #[test]
    fn counts_the_bytes_in_a_present_model() {
        let root = std::env::temp_dir().join(format!("ov-store-present-{}", std::process::id()));
        let path = cache_path(&root, spec());
        std::fs::create_dir_all(path.join("snapshots/abc")).unwrap();
        std::fs::write(path.join("snapshots/abc/model.bin"), vec![0u8; 2048]).unwrap();
        std::fs::write(path.join("refs"), vec![0u8; 16]).unwrap();

        assert_eq!(installed_bytes(&root, spec()), Some(2064));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_empty_directory_is_present_with_zero_bytes() {
        let root = std::env::temp_dir().join(format!("ov-store-empty-{}", std::process::id()));
        std::fs::create_dir_all(cache_path(&root, spec())).unwrap();

        assert_eq!(installed_bytes(&root, spec()), Some(0));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remove_deletes_the_tree_and_is_idempotent() {
        let root = std::env::temp_dir().join(format!("ov-store-rm-{}", std::process::id()));
        let path = cache_path(&root, spec());
        std::fs::create_dir_all(path.join("snapshots/abc")).unwrap();
        std::fs::write(path.join("snapshots/abc/model.bin"), vec![0u8; 32]).unwrap();

        remove(&root, spec()).unwrap();
        assert_eq!(installed_bytes(&root, spec()), None);

        // Asking again is not an error: the caller wants it gone, and it is.
        remove(&root, spec()).unwrap();

        std::fs::remove_dir_all(&root).ok();
    }

    /// A recordings directory holding one file per given age in days.
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
    fn a_retention_of_zero_keeps_every_recording() {
        // Zero means "forever" here, the same as it does for history retention.
        // Reading it as "delete everything" would be a catastrophic inversion.
        let dir = recordings("forever", &[1, 400]);

        assert_eq!(purge_recordings(&dir, 0).unwrap(), 0);
        assert_eq!(names(&dir).len(), 2);

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
    fn purging_a_directory_that_does_not_exist_succeeds() {
        // The normal case: recordings are off, so nothing was ever written.
        let dir = std::env::temp_dir().join(format!("ov-rec-absent-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(purge_recordings(&dir, 7).unwrap(), 0);
    }

    #[test]
    fn models_do_not_share_a_directory() {
        // Two models resolving to one path would make deleting either destroy
        // both, which is the kind of bug that only shows up as a re-download.
        let mut seen = std::collections::HashSet::new();
        for m in catalog::CATALOG {
            assert!(
                seen.insert(cache_dir_name(m.repo)),
                "{} shares a cache directory with another model",
                m.id
            );
        }
    }
}
