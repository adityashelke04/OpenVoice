//! The Models screen's side of the IPC boundary.
//!
//! Four commands: what exists, what is on disk, fetch one, remove one. The
//! catalogue in `ov_asr::catalog` is the single source of truth for every fact
//! about a model; nothing here invents a number, and the frontend adds only the
//! words used to describe them.
//!
//! # Why downloading works without a running engine
//!
//! It used to require one, and that was a trap: a first run that failed to get
//! its weights had no engine, so the screen offering to fetch them was
//! unavailable in the one situation where it was the fix. Nothing here touches
//! the engine at all — `ov-fetch` writes to a directory, and the next start
//! reads it.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::engine::DownloadProgress;
use crate::AppState;

/// Where downloaded models live.
///
/// Under `%APPDATA%` rather than beside the executable: the install directory
/// needs administrator rights, and a download started from a settings screen
/// must not raise a UAC prompt.
pub fn user_models_dir() -> PathBuf {
    crate::history::data_dir().join("models")
}

/// A catalogue entry plus what this machine knows about it.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    #[serde(flatten)]
    spec: ov_asr::catalog::ModelSpec,
    /// Every file present. A partial download is not installed — see
    /// `ov_asr::locate::is_installed`.
    installed: bool,
    /// The model the app is configured to load at the next start.
    selected: bool,
}

/// Every model this build can run, and its state on this machine.
///
/// Deliberately needs no running engine: the catalogue is static data, and a
/// screen that could not render until the engine was warm would be unavailable
/// during exactly the failure it exists to fix.
#[tauri::command]
pub fn list_models(state: tauri::State<'_, AppState>) -> Vec<ModelRow> {
    let dir = user_models_dir();
    let selected = state.settings.get().model;
    ov_asr::catalog::CATALOG
        .iter()
        .map(|spec| ModelRow {
            spec: *spec,
            installed: ov_asr::locate::is_installed(spec, &dir),
            selected: spec.id == selected,
        })
        .collect()
}

/// Bytes fetched so far, or `None` when nothing is downloading.
///
/// Polled rather than pushed. A 465 MB transfer can start before the window has
/// finished loading, so an event would be published to nobody; asking cannot be
/// missed.
#[tauri::command]
pub fn get_download(state: tauri::State<'_, AppState>) -> Option<DownloadProgress> {
    state.download.lock().expect("download").clone()
}

/// Fetch a model's weights now, without selecting it or restarting.
///
/// Downloading and using are separate decisions. Fetching 465 MB and committing
/// your next dictation to it are different things to agree to, and conflating
/// them is how the old screen made people discover a download only after they
/// had already switched.
#[tauri::command]
pub async fn download_model(app: AppHandle, id: String) -> Result<(), String> {
    // On a blocking pool: this transfers up to 465 MB and would otherwise hold
    // the async runtime for the duration.
    tauri::async_runtime::spawn_blocking(move || {
        let spec = ov_asr::catalog::resolve(&id).map_err(|e| e.to_string())?;
        let dir = user_models_dir();

        if spec.bundled {
            return Err(
                "That model is included with OpenVoice; there is nothing to \
                        download."
                    .into(),
            );
        }
        if ov_asr::locate::is_installed(spec, &dir) {
            return Ok(());
        }

        let state = app.state::<AppState>();
        let set = |p: Option<DownloadProgress>| {
            *state.download.lock().expect("download") = p;
        };

        set(Some(DownloadProgress {
            model: spec.id.to_string(),
            done: 0,
            total: u64::from(spec.download_mb) * 1_000_000,
        }));

        let result = ov_fetch::download_and_extract(
            &spec.url(),
            spec.sha256,
            spec.files,
            &dir.join(spec.id),
            &mut |done, total| {
                set(Some(DownloadProgress {
                    model: spec.id.to_string(),
                    done,
                    // Falls back to the catalogue's figure when the server sends
                    // no Content-Length, so the bar shows a real proportion
                    // instead of jumping to an indeterminate state mid-transfer.
                    total: if total > 0 {
                        total
                    } else {
                        u64::from(spec.download_mb) * 1_000_000
                    },
                }));
            },
        );

        // Cleared on both paths. A failed download that left the bar up would
        // strand the screen on a transfer that is not happening.
        set(None);
        result.map_err(|e| format!("Could not download {}: {e}", spec.id))
    })
    .await
    .map_err(|e| format!("download task failed: {e}"))?
}

/// Delete a downloaded model's weights.
#[tauri::command]
pub fn delete_model(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let spec = ov_asr::catalog::resolve(&id).map_err(|e| e.to_string())?;

    if spec.bundled {
        return Err(
            "The included model cannot be removed — it is what OpenVoice falls \
                    back to if anything else fails."
                .into(),
        );
    }
    if state.settings.get().model == id {
        return Err(
            "That is the model in use. Choose a different one first, then \
                    delete it."
                .into(),
        );
    }

    let dir = user_models_dir().join(spec.id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| format!("Could not delete {}: {e}", dir.display()))?;
    }
    tracing::info!(model = %id, "deleted model weights");
    Ok(())
}

/// Total bytes the downloaded models occupy, for the screen's footer.
#[tauri::command]
pub fn models_on_disk() -> u64 {
    let dir = user_models_dir();
    ov_asr::catalog::CATALOG
        .iter()
        .filter(|s| !s.bundled && ov_asr::locate::is_installed(s, &dir))
        .map(|s| u64::from(s.disk_mb) * 1_000_000)
        .sum()
}
