//! Where OpenVoice keeps user data.
//!
//! History itself now lives in SQLite (`ov-store`). What remains here is the one
//! thing several modules need and none of them should each decide for themselves:
//! the directory holding the database, the settings file, the overlay placement,
//! and the log.
//!
//! The JSONL writer this module used to contain is gone. `ov_store::import_jsonl`
//! reads the old file once on startup and renames it aside, so nothing a user
//! recorded under the previous format is lost.

use std::path::PathBuf;

/// Directory holding OpenVoice's user data.
///
/// Falls back to the temp directory if `APPDATA` is somehow unset. Losing history
/// to a temp folder is a bad outcome; refusing to start is a worse one.
#[must_use]
pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenVoice")
}
