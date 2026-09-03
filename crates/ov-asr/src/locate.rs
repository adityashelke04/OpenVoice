//! Where the speech model lives on this machine.
//!
//! Deliberately probes both installed layouts. Whether the weights sit in
//! `resources/` as a Tauri resource or in `models/` placed by an NSIS hook is a
//! packaging decision, and engine code should not have to care which way it went
//! — two `stat` calls at startup is a cheap way to not care, and it means a
//! build packaged either way still runs.

use std::path::PathBuf;

use ov_core::error::{Error, Result};

/// Directory name for the model, in every layout: checkout, installed, and the
/// NSIS hook that puts it there.
pub const MODEL_DIR_NAME: &str = "parakeet-tdt-0.6b-v2";

/// Locate the model, or explain everywhere it was looked for.
///
/// The error is deliberately long. Reaching it means the app cannot transcribe
/// at all, and the only useful thing it can do is say precisely where it
/// expected the weights so someone can put them there.
pub fn model_dir() -> Result<PathBuf> {
    let candidates = candidates();
    resolve_from(std::env::var_os("OPENVOICE_MODEL_DIR").map(PathBuf::from), &candidates).ok_or_else(
        || {
            Error::Transcription(format!(
                "no speech model found. Looked in: {}. Set OPENVOICE_MODEL_DIR, or run \
                 scripts/fetch-model.ps1 from a checkout.",
                candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            ))
        },
    )
}

/// The places a model may be, most specific first.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Installed, NSIS hook layout — the shipping one.
            out.push(dir.join("models").join(MODEL_DIR_NAME));
            // Installed, Tauri resource layout. Kept so that a build packaged
            // the other way still finds its weights instead of failing at the
            // user's first sentence.
            out.push(dir.join("resources").join("models").join(MODEL_DIR_NAME));
        }
    }
    // A checkout, for `cargo run` and the tests.
    out.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("models")
            .join(MODEL_DIR_NAME),
    );
    out
}

/// Pure core of [`model_dir`], so the ordering is testable without a filesystem
/// arranged to look like an install.
fn resolve_from(override_dir: Option<PathBuf>, candidates: &[PathBuf]) -> Option<PathBuf> {
    if let Some(d) = override_dir {
        return Some(d);
    }
    candidates.iter().find(|p| p.is_dir()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_override_wins_and_is_returned_verbatim() {
        // Returned without an existence check: an override that does not exist
        // must produce ParakeetTranscriber's specific "model is incomplete"
        // error naming the file, not a vague "nothing found anywhere".
        let dir = PathBuf::from("Z:/some/override");
        assert_eq!(
            resolve_from(Some(dir.clone()), &[std::env::temp_dir()]).as_deref(),
            Some(dir.as_path())
        );
    }

    #[test]
    fn candidates_are_tried_in_order_and_absent_ones_skipped() {
        let base = std::env::temp_dir().join("ov-locate-order");
        let absent = base.join("absent");
        let present = base.join("present");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&present).expect("create the present candidate");

        assert_eq!(
            resolve_from(None, &[absent, present.clone()]).as_deref(),
            Some(present.as_path()),
            "must skip the absent candidate and take the next"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn nothing_anywhere_is_none() {
        assert!(resolve_from(None, &[PathBuf::from("Z:/nope")]).is_none());
    }

    #[test]
    fn the_error_lists_every_place_it_looked() {
        // Only meaningful when no model is installed, which is the case this
        // message exists for. Where one is present the resolver succeeds.
        if let Err(e) = model_dir() {
            let msg = e.to_string();
            assert!(msg.contains(MODEL_DIR_NAME), "must name the directory: {msg}");
            assert!(msg.contains("OPENVOICE_MODEL_DIR"), "must offer the override: {msg}");
        }
    }
}
