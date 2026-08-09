//! Finding out that a new version exists.
//!
//! # The one request we make that nobody asked for
//!
//! Every other outbound request in OpenVoice is a model download the user chose
//! from a picker. This one is different in kind: nobody asks to be told about a
//! release, so it is the single place the local-first guarantee is deliberately
//! traded. ADR 0005 records the trade; this module is the implementation, and it
//! is written so the promise is visible in the code rather than only in prose.
//!
//! Three properties, all enforced here:
//!
//! - **Check, never install.** [`check`] resolves a version and stops.
//!   [`install`] only ever runs from a command the user invoked by pressing a
//!   button. There is no path from "a newer version exists" to "it was applied".
//! - **Nothing rides along.** The request fetches a static, signed manifest. No
//!   identifier, no current-version histogram, no machine fingerprint — because
//!   there is nowhere in this code to put one.
//! - **Off means off.** [`check_on_launch`] returns before touching the network
//!   when the setting is false. Not "sends an opt-out flag": makes no request.
//!
//! # Why the signature matters more than the transport
//!
//! The installer is not code-signed, so Windows SmartScreen already warns about
//! it. An updater that merely downloaded an executable over HTTPS would be a
//! genuinely worse security position than the status quo, because the user would
//! stop seeing that warning while the app silently fetched code.
//!
//! So the artifact is verified against a minisign public key compiled into the
//! binary (`tauri.conf.json`, `plugins.updater.pubkey`). A tampered or
//! substituted update fails verification and is discarded. The private key is not
//! in this repository and never has been; it lives in the release workflow's
//! secrets. That check does not depend on trusting GitHub, TLS, or us.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

/// What a check found.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// Whether a newer release is available.
    pub available: bool,
    /// The version offered, when one is.
    pub version: Option<String>,
    /// Release notes, when the manifest carries them.
    pub notes: Option<String>,
    /// The version running now, so the UI never has to guess.
    pub current_version: String,
}

impl UpdateStatus {
    fn none() -> Self {
        Self {
            available: false,
            version: None,
            notes: None,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Ask whether a newer version exists.
///
/// Errors are returned rather than swallowed, because this is reached from a
/// button the user pressed and silence would be indistinguishable from "you are
/// up to date" — the one wrong answer that cannot be retried.
pub async fn check(app: &AppHandle) -> Result<UpdateStatus, String> {
    let updater = app
        .updater()
        .map_err(|e| format!("the updater is unavailable in this build: {e}"))?;

    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateStatus {
            available: true,
            version: Some(update.version.clone()),
            notes: update.body.clone(),
            current_version: update.current_version.clone(),
        }),
        Ok(None) => Ok(UpdateStatus::none()),
        Err(e) => Err(explain(&e.to_string())),
    }
}

/// Turn an updater failure into something worth reading.
///
/// The raw text is written for whoever wrote the plugin. "Could not fetch a valid
/// release JSON from the remote" describes an HTTP response; it does not tell
/// someone whether their app is broken, their network is down, or nothing has
/// been published yet — and the alarming reading is the wrong one, because by far
/// the most common cause is the last.
///
/// The detail is kept, after the explanation, so a real fault is still
/// diagnosable from a screenshot.
fn explain(raw: &str) -> String {
    let lower = raw.to_lowercase();
    // A missing or unparseable manifest is what a repository with no published
    // update feed returns, and it is indistinguishable from being offline
    // without inspecting the status code the plugin has already discarded.
    let benign = lower.contains("release json")
        || lower.contains("404")
        || lower.contains("not found")
        || lower.contains("status code")
        || lower.contains("dns")
        || lower.contains("connect");

    if benign {
        format!(
            "No update information was available. You may be offline, or no \
             release has been published yet — either way this copy keeps working. \
             ({raw})"
        )
    } else {
        format!("Could not check for updates: {raw}")
    }
}

/// Download, verify and apply an update, then restart.
///
/// Only ever called from the `install_update` command — that is, from a button.
/// The signature is verified by the plugin before anything is executed; a failure
/// there surfaces here as an error rather than as an installed binary.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let updater = app
        .updater()
        .map_err(|e| format!("the updater is unavailable in this build: {e}"))?;

    let update = updater
        .check()
        .await
        .map_err(|e| format!("could not check for updates: {e}"))?
        .ok_or_else(|| "there is no update to install".to_string())?;

    tracing::info!(version = %update.version, "installing update");

    // Progress is logged rather than surfaced. The download is a single installer
    // of a few hundred megabytes at most, the window stays responsive, and a
    // second progress bar in an app that already has one for model downloads is
    // more chrome than the moment deserves.
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| format!("could not install the update: {e}"))?;

    Ok(())
}

/// Run the once-per-launch check, if the user has left it on.
///
/// Spawned rather than awaited: a slow or unreachable network must never delay
/// the app becoming usable. The result is emitted to the Hub, which decides
/// whether to show anything — a failed check is deliberately silent, because an
/// error toast about an update nobody asked for is worse than no update.
pub fn check_on_launch(app: &AppHandle, enabled: bool) {
    if !enabled {
        tracing::info!("update check on launch is off; not contacting the network");
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match check(&app).await {
            Ok(status) if status.available => {
                tracing::info!(version = ?status.version, "an update is available");
                let _ = app.emit("update-available", &status);
            }
            Ok(_) => tracing::info!("running the latest version"),
            // Not surfaced. Offline is the normal state for this app, and being
            // told the update check failed every time you open it on a train is
            // exactly the kind of nagging that makes people disable a feature
            // they would otherwise want.
            Err(e) => tracing::info!(reason = %e, "update check did not complete"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::explain;

    #[test]
    fn a_missing_manifest_reads_as_ordinary_rather_than_broken() {
        // The exact string a repository with no published update feed produces.
        let msg = explain("Could not fetch a valid release JSON from the remote");
        assert!(
            msg.starts_with("No update information was available"),
            "{msg}"
        );
    }

    #[test]
    fn being_offline_reads_the_same_way() {
        assert!(explain("failed to connect to host").starts_with("No update information"));
    }

    #[test]
    fn the_original_text_is_always_kept_for_diagnosis() {
        // A screenshot has to remain enough to debug from.
        assert!(explain("some novel failure").contains("some novel failure"));
        assert!(explain("404 not found").contains("404 not found"));
    }

    #[test]
    fn an_unrecognised_failure_is_not_dressed_up_as_routine() {
        let msg = explain("signature verification failed");
        assert!(msg.starts_with("Could not check for updates"), "{msg}");
    }
}
