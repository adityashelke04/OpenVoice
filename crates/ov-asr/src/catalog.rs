//! The model catalogue: the single source of truth for which speech models exist.
//!
//! # Why this is here and not in the sidecar
//!
//! It used to be in both. `MODEL_PRESETS` in `sidecar/openvoice_asr/engine.py` held
//! the repository ids and compute types, and a `MODELS` array in
//! `apps/ui/src/engine/settings.ts` independently held the ids, download sizes and
//! measured latencies shown to the user. Nothing connected them. A model added to
//! one was invisible to the other, and a size corrected in one silently disagreed
//! with the other — a class of bug that no test could catch because neither file
//! was wrong on its own terms.
//!
//! `ov-asr` owns the sidecar's process lifetime, so it is the natural owner of the
//! question "which models can that process load". The sidecar is now told what to
//! load — repository, compute type, fallback — and holds no catalogue at all. The
//! UI asks this crate rather than repeating it.
//!
//! # What deliberately stays in the UI
//!
//! Facts live here: ids, repositories, sizes, VRAM, language coverage. The
//! *presentation* of those facts — that `large-v3-turbo` is called "Accurate", and
//! that it decoded in about 650 ms on one particular laptop — is copy and
//! measurement, not a property of the model, and it stays in the frontend. A model
//! added here with no matching copy still appears in the UI, labelled with its id
//! and its correct size, rather than vanishing.

use serde::Serialize;

/// One selectable speech model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    /// Stable identifier, used in config and on the wire. Never shown as a label
    /// without the UI's own copy, except as a fallback for an unknown model.
    pub id: &'static str,
    /// Hugging Face repository holding the CTranslate2 conversion.
    pub repo: &'static str,
    /// Preferred CTranslate2 compute type.
    ///
    /// `float16` beats `int8_float16` on a GPU despite the larger weights: int8
    /// weights are dequantized on every forward pass, which costs more than the
    /// memory bandwidth it saves. Measured on the reference machine at a median
    /// 623 ms against 661 ms, loading in half the time, with byte-identical
    /// transcripts.
    pub compute_type: &'static str,
    /// Compute type to retry with when the preferred one will not fit.
    ///
    /// Almost always an out-of-VRAM failure. A 4 GB laptop GPU is also running a
    /// desktop compositor and a browser, and smaller weights beat no dictation.
    /// `None` where there is nothing smaller to fall back to.
    pub fallback_compute: Option<&'static str>,
    /// Approximate download size in megabytes.
    ///
    /// Static rather than queried, because the size has to be shown *before* the
    /// user agrees to the download, and asking Hugging Face for it would mean a
    /// network call to render a settings screen. See the test below for the
    /// sanity bounds.
    ///
    /// These are the totals for the files actually fetched (`MODEL_FILES` in the
    /// sidecar), measured against the live repositories -- not the repository's
    /// full size, and not an estimate. Two of the three were previously about
    /// half the truth: `base.en` was listed at 75 MB and transfers 148, `small.en`
    /// at 250 and transfers 486. Understating the one number a user weighs before
    /// agreeing to a wait makes the wait look like a hang, which is the state in
    /// which people kill the app and leave a part-fetched model behind.
    pub size_mb: u32,
    /// Approximate VRAM required, in megabytes. Zero means it runs comfortably on
    /// the CPU.
    pub vram_mb: u32,
    /// Whether the model is English-only.
    ///
    /// The `.en` Whisper conversions are trained on English alone and produce
    /// confident nonsense on anything else, which is worth saying out loud in a
    /// picker rather than leaving the user to discover it.
    pub english_only: bool,
}

/// Every model this build can load, in the order a picker should show them:
/// most capable first.
///
/// Adding a model is a one-line change here and nothing else. The sidecar receives
/// whatever this says; the UI lists whatever this contains.
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "large-v3-turbo",
        // A third-party conversion; the others are Systran's own. Every repo is
        // written out in full, because the sidecar no longer expands short names
        // and a half-expanded id would only fail once a download was attempted.
        repo: "deepdml/faster-whisper-large-v3-turbo-ct2",
        compute_type: "float16",
        fallback_compute: Some("int8_float16"),
        size_mb: 1620,
        vram_mb: 1600,
        english_only: false,
    },
    ModelSpec {
        id: "small.en",
        repo: "Systran/faster-whisper-small.en",
        compute_type: "float16",
        fallback_compute: Some("int8_float16"),
        size_mb: 486,
        vram_mb: 600,
        english_only: true,
    },
    ModelSpec {
        id: "base.en",
        repo: "Systran/faster-whisper-base.en",
        // int8 with no fallback: this is already the smallest thing here, and it
        // is the model that has to work on a machine with no GPU at all.
        compute_type: "int8",
        fallback_compute: None,
        size_mb: 148,
        vram_mb: 0,
        english_only: true,
    },
];

/// The model a fresh install starts on.
///
/// `base.en`, not the most accurate one: the installed engine is CPU-only (ADR
/// 0003), and `large-v3-turbo` on a CPU is the slowest model on the slowest path —
/// a 1.6 GB download for an experience worse than the 148 MB alternative.
pub const DEFAULT_MODEL: &str = "base.en";

/// Look a model up by id.
#[must_use]
pub fn find(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|m| m.id == id)
}

/// Every known id, for help text and error messages.
#[must_use]
pub fn ids() -> Vec<&'static str> {
    CATALOG.iter().map(|m| m.id).collect()
}

/// Resolve an id, or explain what the valid ones are.
///
/// The error names the alternatives because the most likely reason to reach it is
/// a hand-edited config file, where "unknown model" alone leaves the user guessing
/// at spelling.
pub fn resolve(id: &str) -> Result<&'static ModelSpec, crate::Error> {
    find(id).ok_or_else(|| {
        crate::Error::Transcription(format!(
            "unknown model {id:?}; this build knows: {}",
            ids().join(", ")
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for m in CATALOG {
            assert!(seen.insert(m.id), "duplicate model id {:?}", m.id);
        }
    }

    #[test]
    fn the_default_model_is_in_the_catalog() {
        // A default that does not resolve would fail at startup on a fresh install
        // — the one run where there is no config file to correct it.
        assert!(find(DEFAULT_MODEL).is_some());
    }

    #[test]
    fn the_default_model_needs_no_gpu() {
        // The installed engine is CPU-only. A default requiring VRAM would make a
        // fresh install fail on exactly the hardware it is meant to work on.
        assert_eq!(find(DEFAULT_MODEL).unwrap().vram_mb, 0);
    }

    #[test]
    fn every_repo_is_fully_qualified() {
        // The sidecar no longer expands a bare name into `Systran/faster-whisper-*`,
        // so a bare name here would reach huggingface_hub as an invalid repo id and
        // fail at download time rather than at build time.
        for m in CATALOG {
            assert!(
                m.repo.contains('/'),
                "{} has a bare repo name {:?}; it must be `org/name`",
                m.id,
                m.repo
            );
        }
    }

    #[test]
    fn sizes_are_plausible() {
        // Guards against a units slip -- a size entered in bytes or gigabytes is
        // the kind of error that looks fine in review and is glaring in the UI.
        for m in CATALOG {
            assert!(
                (10..=10_000).contains(&m.size_mb),
                "{} has an implausible size of {} MB",
                m.id,
                m.size_mb
            );
        }
    }

    #[test]
    fn a_fallback_compute_differs_from_the_preferred_one() {
        // Retrying with the identical compute type would burn a second model load
        // to reproduce the same out-of-memory failure.
        for m in CATALOG {
            if let Some(f) = m.fallback_compute {
                assert_ne!(f, m.compute_type, "{} falls back to itself", m.id);
            }
        }
    }

    #[test]
    fn resolve_names_the_alternatives() {
        let err = resolve("enormous-v9").unwrap_err().to_string();
        assert!(err.contains("enormous-v9"));
        assert!(
            err.contains("base.en"),
            "error should list valid ids: {err}"
        );
    }

    #[test]
    fn catalog_is_ordered_most_capable_first() {
        // The picker renders this order directly, and "biggest first" is the
        // convention the Models screen was designed around.
        let sizes: Vec<u32> = CATALOG.iter().map(|m| m.size_mb).collect();
        let mut sorted = sizes.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(sizes, sorted, "catalogue is not ordered largest-first");
    }
}
