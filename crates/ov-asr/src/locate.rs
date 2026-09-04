//! Where a model's weights are on this machine.
//!
//! # Two places, for one reason
//!
//! The bundled model ships inside the installer and lands beside the executable.
//! Downloaded models cannot go there: `Program Files` needs administrator
//! rights, and a download started from a settings screen must not raise a UAC
//! prompt. They go under `%APPDATA%` instead.
//!
//! The user directory is passed in rather than resolved here. `%APPDATA%` is
//! `ov-app`'s concern, and taking it as an argument is what lets these functions
//! be tested against a temp directory instead of a real install.

use std::path::{Path, PathBuf};

use ov_core::error::{Error, Result};

use crate::catalog::ModelSpec;

/// Whether every file this model needs is present under `user_dir`.
///
/// Completeness, not directory existence. An interrupted download leaves a real
/// directory with real bytes in it, and reporting that as installed would send
/// the user to a model that fails to load — with the Models screen cheerfully
/// showing it as ready.
#[must_use]
pub fn is_installed(spec: &ModelSpec, user_dir: &Path) -> bool {
    if spec.bundled {
        return bundled_candidates(spec).iter().any(|p| complete(spec, p));
    }
    complete(spec, &user_dir.join(spec.id))
}

/// Where this model's files are, bundled or downloaded.
///
/// The error names the directory it wanted, because reaching it means the app
/// cannot transcribe with this model and the only useful thing it can say is
/// where the weights were expected.
pub fn model_dir(spec: &ModelSpec, user_dir: &Path) -> Result<PathBuf> {
    // Developer and test override, honoured first and returned without an
    // existence check: an override that does not exist should produce
    // SherpaTranscriber's specific "model is incomplete" error naming the file,
    // not a vague "nothing found anywhere".
    if let Some(d) = std::env::var_os("OPENVOICE_MODEL_DIR").map(PathBuf::from) {
        return Ok(d);
    }

    if spec.bundled {
        if let Some(d) = bundled_candidates(spec).into_iter().find(|p| p.is_dir()) {
            return Ok(d);
        }
    }

    let downloaded = user_dir.join(spec.id);
    if complete(spec, &downloaded) {
        return Ok(downloaded);
    }

    Err(Error::Transcription(format!(
        "the {} model is not installed. Expected it in {}.",
        spec.id,
        downloaded.display()
    )))
}

/// Every file present and a real file, not a directory of the same name.
fn complete(spec: &ModelSpec, dir: &Path) -> bool {
    spec.files.iter().all(|f| dir.join(f).is_file())
}

/// Where a bundled model may sit, most specific first.
fn bundled_candidates(spec: &ModelSpec) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Installed, NSIS hook layout — the shipping one.
            out.push(dir.join("models").join(spec.id));
            // Installed, Tauri resource layout. Kept so a build packaged the
            // other way still finds its weights rather than failing at the
            // user's first sentence.
            out.push(dir.join("resources").join("models").join(spec.id));
        }
    }
    // A checkout, for `cargo run` and the tests. Walked up rather than joined
    // with "../..", because this path is printed in logs and in the error a user
    // sees when the model is missing.
    if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2) {
        out.push(root.join("models").join(spec.id));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v3() -> &'static ModelSpec {
        crate::catalog::find("parakeet-tdt-0.6b-v3").expect("in catalogue")
    }

    /// A complete model directory for `spec` under a fresh temp root.
    fn install(spec: &ModelSpec, tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("ov-locate-{tag}"));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join(spec.id);
        std::fs::create_dir_all(&dir).expect("create");
        for f in spec.files {
            std::fs::write(dir.join(f), b"x").expect("write");
        }
        base
    }

    #[test]
    fn a_downloaded_model_is_found_in_the_user_directory() {
        let base = install(v3(), "user");
        assert!(is_installed(v3(), &base));
        assert_eq!(model_dir(v3(), &base).unwrap(), base.join(v3().id));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_partial_download_does_not_count_as_installed() {
        // An interrupted transfer leaves real bytes in a real directory.
        // Treating that as installed sends the user to a model that cannot load.
        let base = install(v3(), "partial");
        std::fs::remove_file(base.join(v3().id).join(v3().files[0])).expect("remove one file");
        assert!(
            !is_installed(v3(), &base),
            "three files of four is not installed"
        );
        assert!(model_dir(v3(), &base).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_missing_model_names_the_directory_it_wanted() {
        let base = std::env::temp_dir().join("ov-locate-absent");
        let _ = std::fs::remove_dir_all(&base);
        let err = model_dir(v3(), &base).unwrap_err().to_string();
        assert!(err.contains(v3().id), "must name the model: {err}");
        assert!(
            err.contains("not installed"),
            "must say what is wrong: {err}"
        );
    }

    #[test]
    fn the_bundled_model_is_not_looked_for_in_the_user_directory() {
        // It ships with the app. Looking under %APPDATA% for it would report it
        // missing on every machine where it is exactly where it should be.
        let empty = std::env::temp_dir().join("ov-locate-empty");
        let _ = std::fs::remove_dir_all(&empty);
        let bundled = crate::catalog::default_spec();
        // In a checkout the fetch script has put it in models/, so this resolves
        // without the user directory containing anything at all.
        if bundled_candidates(bundled).iter().any(|p| p.is_dir()) {
            assert!(is_installed(bundled, &empty));
            assert!(model_dir(bundled, &empty).is_ok());
        }
    }

    #[test]
    fn a_directory_named_like_a_weight_file_is_not_a_weight_file() {
        // `is_file` rather than `exists`: an extraction that created directories
        // where files belong would otherwise pass as complete.
        let base = std::env::temp_dir().join("ov-locate-dirtrap");
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join(v3().id);
        for f in v3().files {
            std::fs::create_dir_all(dir.join(f)).expect("create dir where a file belongs");
        }
        assert!(!is_installed(v3(), &base));
        let _ = std::fs::remove_dir_all(&base);
    }
}
