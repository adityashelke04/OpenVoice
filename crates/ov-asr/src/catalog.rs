//! The three speech models this build can run.
//!
//! # Why this exists again
//!
//! v0.5.0 deleted a catalogue, and was right to: it carried a Hugging Face cache
//! layout, per-model compute types, a VRAM column and a resumable transfer, all
//! to describe three Whisper tiers whose only real difference was a
//! speed-versus-accuracy trade that Parakeet removed.
//!
//! What comes back is narrower. There is still one model that is simply better
//! for most people, and it is bundled and default. The other two exist because
//! they answer questions the default cannot: *"I dictate in Spanish"* and *"this
//! machine has 4 GB of RAM"*. Those are real questions, and no amount of
//! benchmark advantage in the default answers either of them.
//!
//! # These numbers were measured
//!
//! Every size, checksum and file list here was established on 2026-09-04 by
//! downloading the archive, extracting it, and decoding audio through the Rust
//! API — not read off a model card. The sizes a user weighs before agreeing to a
//! download are the ones most worth not guessing at.

use serde::Serialize;

use ov_core::error::{Error, Result};

/// How a model is loaded.
///
/// sherpa-onnx takes a different configuration struct per architecture, and
/// filling in the wrong one surfaces as a null pointer from the C library rather
/// than a message anyone can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelKind {
    /// NeMo FastConformer transducer: encoder, decoder and joiner.
    Transducer,
    /// OpenAI Whisper: encoder and decoder, no joiner.
    Whisper,
}

/// One selectable speech model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    /// Stable identifier. Written to `settings.toml` and recorded in history
    /// against every transcript, so changing one orphans the rows already
    /// attributed to it.
    pub id: &'static str,
    /// Which sherpa-onnx configuration to build.
    pub kind: ModelKind,
    /// Release asset name, without the `.tar.bz2` suffix.
    pub archive: &'static str,
    /// SHA-256 of the archive, lowercase hex.
    ///
    /// Verified before extraction, because these bytes become code that runs on
    /// the user's machine. ADR 0003 once claimed weights were checksum-verified
    /// when nothing of the sort existed; that is not repeated here.
    pub sha256: &'static str,
    /// Compressed transfer size in megabytes. Zero for the bundled model, which
    /// is never fetched.
    pub download_mb: u32,
    /// Extracted size of the files named in `files` — not the archive's full
    /// contents, which for Whisper is more than twice as large.
    pub disk_mb: u32,
    /// The files actually kept from the archive.
    ///
    /// Whisper ships fp32 and int8 weights side by side; keeping only int8 saves
    /// 146 MB of the 245 MB the archive expands to.
    pub files: &'static [&'static str],
    /// Ships inside the installer, cannot be deleted, and is what the app falls
    /// back to when anything else fails to load.
    pub bundled: bool,
    /// English only. Worth saying in a picker rather than leaving someone to
    /// deduce it from a bad transcript.
    pub english_only: bool,
}

/// Every model this build can load, best-for-most-people first.
///
/// Adding one is an entry here and nothing else: the loader reads `kind` and
/// `files`, the downloader reads `archive` and `sha256`, the UI reads the rest.
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "parakeet-tdt-0.6b-v2",
        kind: ModelKind::Transducer,
        archive: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8",
        sha256: "157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad",
        download_mb: 0,
        disk_mb: 631,
        files: &[
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ],
        bundled: true,
        english_only: true,
    },
    ModelSpec {
        id: "parakeet-tdt-0.6b-v3",
        kind: ModelKind::Transducer,
        archive: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
        sha256: "5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf",
        download_mb: 465,
        disk_mb: 641,
        files: &[
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ],
        bundled: false,
        english_only: false,
    },
    ModelSpec {
        id: "whisper-tiny.en",
        kind: ModelKind::Whisper,
        archive: "sherpa-onnx-whisper-tiny.en",
        sha256: "2bd6cf965c8bb3e068ef9fa2191387ee63a9dfa2a4e37582a8109641c20005dd",
        download_mb: 112,
        disk_mb: 99,
        files: &[
            "tiny.en-encoder.int8.onnx",
            "tiny.en-decoder.int8.onnx",
            "tiny.en-tokens.txt",
        ],
        bundled: false,
        english_only: true,
    },
];

/// The model a fresh install runs, and the fallback for every failure path.
pub const DEFAULT_MODEL: &str = "parakeet-tdt-0.6b-v2";

/// Where the release assets come from.
pub const ARCHIVE_BASE: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models";

impl ModelSpec {
    /// Full download URL for this model's archive.
    #[must_use]
    pub fn url(&self) -> String {
        format!("{ARCHIVE_BASE}/{}.tar.bz2", self.archive)
    }
}

/// Look a model up by id.
#[must_use]
pub fn find(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|m| m.id == id)
}

/// The model a fresh install runs. Never fails: the test below pins it.
#[must_use]
pub fn default_spec() -> &'static ModelSpec {
    find(DEFAULT_MODEL).expect("the default model is in the catalogue")
}

/// Resolve an id, or explain what the valid ones are.
///
/// The error lists the alternatives because the likeliest way to reach it is a
/// hand-edited settings file, where "unknown model" alone leaves the reader
/// guessing at spelling.
pub fn resolve(id: &str) -> Result<&'static ModelSpec> {
    find(id).ok_or_else(|| {
        Error::Transcription(format!(
            "unknown model {id:?}; this build knows: {}",
            CATALOG.iter().map(|m| m.id).collect::<Vec<_>>().join(", ")
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
            assert!(seen.insert(m.id), "duplicate id {:?}", m.id);
        }
    }

    #[test]
    fn exactly_one_model_is_bundled() {
        // The bundled model is the fallback for every failure path. Two would
        // make "which one is guaranteed present" ambiguous; none would mean a
        // fresh install has no engine at all.
        assert_eq!(CATALOG.iter().filter(|m| m.bundled).count(), 1);
    }

    #[test]
    fn the_default_is_the_bundled_one() {
        // A default that needs downloading would make a fresh install unusable
        // offline, which is the property the installer exists to provide.
        assert!(default_spec().bundled);
    }

    #[test]
    fn every_checksum_is_a_lowercase_sha256() {
        for m in CATALOG {
            assert_eq!(m.sha256.len(), 64, "{} checksum is not 64 hex chars", m.id);
            assert!(
                m.sha256
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "{} checksum must be lowercase hex",
                m.id
            );
        }
    }

    #[test]
    fn every_model_lists_the_files_its_kind_needs() {
        // A transducer needs a joiner; Whisper has none. Getting this wrong
        // surfaces as a null pointer from the C library at load time, which
        // tells the user nothing at all.
        for m in CATALOG {
            assert!(
                m.files.iter().any(|f| f.contains("tokens")),
                "{} lists no tokens file",
                m.id
            );
            let joiners = m.files.iter().filter(|f| f.contains("joiner")).count();
            match m.kind {
                ModelKind::Transducer => assert_eq!(joiners, 1, "{} needs a joiner", m.id),
                ModelKind::Whisper => assert_eq!(joiners, 0, "{} must not have a joiner", m.id),
            }
        }
    }

    #[test]
    fn sizes_are_plausible() {
        // Guards a units slip. A size entered in bytes or gigabytes looks fine
        // in review and is glaring in the UI.
        for m in CATALOG {
            assert!(
                (50..=5_000).contains(&m.disk_mb),
                "{} has an implausible disk size of {} MB",
                m.id,
                m.disk_mb
            );
            if m.bundled {
                assert_eq!(m.download_mb, 0, "{} ships in the installer", m.id);
            } else {
                assert!(
                    (10..=5_000).contains(&m.download_mb),
                    "{} has an implausible download of {} MB",
                    m.id,
                    m.download_mb
                );
            }
        }
    }

    #[test]
    fn urls_are_fully_qualified_https() {
        for m in CATALOG {
            let url = m.url();
            assert!(url.starts_with("https://"), "{} is not https: {url}", m.id);
            assert!(
                url.ends_with(".tar.bz2"),
                "{} is not an archive: {url}",
                m.id
            );
        }
    }

    #[test]
    fn resolve_names_the_alternatives() {
        let err = resolve("enormous-v9").unwrap_err().to_string();
        assert!(err.contains("enormous-v9"));
        assert!(err.contains(DEFAULT_MODEL), "should list valid ids: {err}");
    }
}
