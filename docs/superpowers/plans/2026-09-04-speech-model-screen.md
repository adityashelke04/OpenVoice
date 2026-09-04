# Speech Model Screen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring back a Speech model screen offering three models — Parakeet v2 bundled and in use by default, Parakeet v3 and Whisper tiny.en downloadable on request — without reintroducing the machinery that made the old one worth deleting.

**Architecture:** A small static catalogue in `ov-asr` describes the three models. `SherpaTranscriber` replaces `ParakeetTranscriber` and builds either a transducer or a Whisper recogniser from a `ModelSpec`, so adding a model is a catalogue entry rather than a code change. Downloading lives in a **new `ov-fetch` crate** — the single network-capable crate in the workspace — which keeps `ov-asr` provably sealed while it handles the microphone.

**Tech Stack:** Rust 1.97, `sherpa-onnx` 1.13.7, `ureq` + `sha2` + `tar` + `bzip2` for fetching, Tauri v2 + NSIS, React/TypeScript.

**Spec:** This document. The engine decision it builds on is [ADR 0008](../../adr/0008-parakeet-in-process.md); the network-capability change it introduces gets **ADR 0009** in Task 9.

## Design

### Why this is not simply a revert

The screen deleted in v0.5.0 carried a Hugging Face cache layout, per-model
compute types, a VRAM column, a resumable `huggingface_hub` transfer and an
offline-mode workaround. None of that comes back. What comes back is the part
that had a reason to exist: **choosing a model, and getting one you do not have.**

### The catalogue, measured rather than assumed

Every number below was verified on 2026-09-04 by downloading the archive,
extracting it, and decoding through the Rust API.

| id | name | kind | download | on disk | load | decode | languages |
|---|---|---|---:|---:|---:|---:|---|
| `parakeet-tdt-0.6b-v2` | Standard | transducer | bundled | 631 MB | 2.4 s | 495 ms | English |
| `parakeet-tdt-0.6b-v3` | Multilingual | transducer | 465 MB | 641 MB | 2.7 s | 598 ms | 25 |
| `whisper-tiny.en` | Light | whisper | 112 MB | 99 MB | 0.6 s | 486 ms | English |

`whisper-tiny.en` is the low-memory option, not a quality one — its output on the
same clip was visibly worse (*"very likely old portrait"* against v2's *"very
like the old portrait"*). The UI must not imply otherwise.

### Where models live, and why two places

- **Bundled**: `<install dir>\models\parakeet-tdt-0.6b-v2\`, written by the NSIS
  hook. Cannot be deleted from the UI — it is the fallback that guarantees the
  app always has a working engine.
- **Downloaded**: `%APPDATA%\OpenVoice\models\<id>\`. `<install dir>` needs
  administrator rights to write, and a download prompted from a settings screen
  must not raise a UAC dialog.

### The network question, answered honestly

`ov-asr` is in `SEALED_CRATES`: it provably cannot open a socket. That property
is worth keeping precisely because it is the crate holding the microphone, so the
downloader does **not** go there. It goes in a new `ov-fetch` crate that is
network-capable by design and named as such in `scripts/check-no-network.sh`.

This is a real weakening of the local-first guarantee and gets ADR 0009. The
privacy copy must go back to naming the exception — v0.5.0's "dictation has no
network path at all" becomes untrue the moment a Download button exists, and
leaving that line in place would be the more serious problem.

### Failure that must not strand the user

Selecting a downloaded model and then deleting its files (or a partial download)
must fall back to the bundled model with a visible explanation, never fail to
start. The app always has `parakeet-tdt-0.6b-v2` on disk; there is no state in
which it legitimately cannot transcribe.

## Global Constraints

- **Catalogue is the single source of truth.** Ids, URLs, checksums, sizes and
  file lists live in `ov-asr::catalog` and nowhere else. The UI reads it over IPC.
- **Every download is SHA-256 verified** against the value pinned in the
  catalogue, before extraction. A mismatch deletes the archive and errors.
- **Exact checksums** (verified 2026-09-04):
  - v2 `157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad`
  - v3 `5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf`
  - tiny.en `2bd6cf965c8bb3e068ef9fa2191387ee63a9dfa2a4e37582a8109641c20005dd`
- **Archive URLs** are `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/<archive>.tar.bz2`.
- **`ov-asr` stays sealed.** It gains no HTTP, TLS or socket dependency. CI enforces this.
- **No accuracy figures in user-facing copy** (ADR 0008 §"The limitation this evidence has").
- **Bundled model is undeletable** and is the fallback for every failure path.
- **Version target:** `0.6.0` — a screen returns and a network call returns with it.
- **Every task ends green:** `cargo test --workspace -- --test-threads=1` passes and
  `cargo clippy --workspace --all-targets -- -D warnings` is clean before each commit.

## File Structure

| File | Responsibility |
|---|---|
| `crates/ov-asr/src/catalog.rs` | **new** — the three `ModelSpec`s and lookup |
| `crates/ov-asr/src/sherpa.rs` | **renamed from `parakeet.rs`** — builds a recogniser from any `ModelSpec` |
| `crates/ov-asr/src/locate.rs` | **modified** — resolve bundled *and* downloaded locations |
| `crates/ov-fetch/` | **new crate** — download, verify, extract. The one network-capable crate |
| `crates/ov-app/src/models.rs` | **new** — Tauri commands: list, download, delete |
| `crates/ov-app/src/engine.rs` | **modified** — build from the selected spec, fall back on failure |
| `apps/ui/src/screens/Models.tsx` | **new** — the screen, split out of `Settings.tsx` this time |

---

### Task 1: The catalogue

Static data with no behaviour, so it can be got right before anything depends on it.

**Files:**
- Create: `crates/ov-asr/src/catalog.rs`
- Modify: `crates/ov-asr/src/lib.rs`

**Interfaces:**
- Produces: `ov_asr::catalog::{ModelSpec, ModelKind, CATALOG, DEFAULT_MODEL, find, resolve}`.
  `ModelSpec` fields: `id: &str`, `kind: ModelKind`, `archive: &str`, `sha256: &str`,
  `download_mb: u32`, `disk_mb: u32`, `files: &[&str]`, `bundled: bool`, `english_only: bool`.
  Every later task reads these names.

- [ ] **Step 1: Write the failing tests**

Create `crates/ov-asr/src/catalog.rs` containing only:

```rust
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
        // A default that needs downloading makes a fresh install unusable
        // offline, which is the property the installer exists to provide.
        assert!(find(DEFAULT_MODEL).expect("default is in the catalogue").bundled);
    }

    #[test]
    fn every_checksum_is_a_sha256() {
        for m in CATALOG {
            assert_eq!(m.sha256.len(), 64, "{} checksum is not 64 hex chars", m.id);
            assert!(
                m.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
                "{} checksum must be lowercase hex",
                m.id
            );
        }
    }

    #[test]
    fn every_model_lists_the_files_its_kind_needs() {
        // A transducer needs encoder, decoder and joiner; Whisper has no joiner.
        // Getting this wrong surfaces as a null pointer from the C library at
        // load time, which tells the user nothing.
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
        // Guards a units slip -- a size in bytes or gigabytes looks fine in
        // review and is glaring in the UI.
        for m in CATALOG {
            assert!((50..=5_000).contains(&m.disk_mb), "{} disk_mb implausible", m.id);
            if !m.bundled {
                assert!((10..=5_000).contains(&m.download_mb), "{} download_mb implausible", m.id);
            }
        }
    }

    #[test]
    fn resolve_names_the_alternatives() {
        let err = resolve("enormous-v9").unwrap_err().to_string();
        assert!(err.contains("enormous-v9"));
        assert!(err.contains(DEFAULT_MODEL), "should list valid ids: {err}");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ov-asr catalog`
Expected: FAIL to compile — `CATALOG` not found.

- [ ] **Step 3: Write the catalogue**

Prepend to `crates/ov-asr/src/catalog.rs`:

```rust
//! The three speech models this build can run.
//!
//! # Why this exists again
//!
//! v0.5.0 deleted a catalogue, and was right to: it carried a Hugging Face cache
//! layout, per-model compute types, a VRAM column and a resumable transfer, to
//! describe three Whisper tiers whose only real difference was a speed/accuracy
//! trade that Parakeet removed.
//!
//! What comes back is narrower. There is still one model that is simply better
//! for most people, and it is bundled and default. The other two exist because
//! they answer questions the default cannot: *"I dictate in Spanish"* and *"this
//! machine has 4 GB of RAM"*. Those are real, and no amount of benchmark
//! advantage in the default answers them.
//!
//! Every number here was measured on 2026-09-04 by downloading the archive,
//! extracting it, and decoding through the Rust API — not read off a model card.

use serde::Serialize;

/// How a model is loaded. sherpa-onnx needs a different configuration struct per
/// architecture, and picking the wrong one surfaces as a null pointer from the C
/// library rather than a message anyone can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelKind {
    /// NeMo FastConformer transducer: encoder + decoder + joiner.
    Transducer,
    /// OpenAI Whisper: encoder + decoder, no joiner.
    Whisper,
}

/// One selectable speech model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    /// Stable identifier. Written to settings.toml and recorded in history
    /// against every transcript, so changing one orphans past rows.
    pub id: &'static str,
    /// Which sherpa-onnx configuration to build.
    pub kind: ModelKind,
    /// Release asset name, without the `.tar.bz2` suffix.
    pub archive: &'static str,
    /// SHA-256 of the archive, lowercase hex. Verified before extraction,
    /// because these bytes become code that runs on the user's machine.
    pub sha256: &'static str,
    /// Compressed transfer size. Zero for the bundled model, which is never
    /// fetched.
    pub download_mb: u32,
    /// Extracted size of the files in `files` — not the archive's full contents.
    pub disk_mb: u32,
    /// The files actually kept from the archive.
    ///
    /// Whisper ships fp32 and int8 side by side; keeping only int8 saves 146 MB
    /// of the 245 MB the archive expands to.
    pub files: &'static [&'static str],
    /// Ships inside the installer, cannot be deleted, and is the fallback when
    /// anything else fails to load.
    pub bundled: bool,
    /// English only. Worth saying in a picker rather than leaving the user to
    /// discover it from a bad transcript.
    pub english_only: bool,
}

/// Every model this build can load, best-for-most-people first.
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

/// The model a fresh install runs, and the fallback for every failure.
pub const DEFAULT_MODEL: &str = "parakeet-tdt-0.6b-v2";

/// Where the release assets come from.
pub const ARCHIVE_BASE: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models";

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

/// Resolve an id, or explain what the valid ones are.
///
/// The error lists the alternatives because the likeliest way to reach it is a
/// hand-edited settings file, where "unknown model" alone leaves the reader
/// guessing at spelling.
pub fn resolve(id: &str) -> Result<&'static ModelSpec, crate::Error> {
    find(id).ok_or_else(|| {
        crate::Error::Transcription(format!(
            "unknown model {id:?}; this build knows: {}",
            CATALOG.iter().map(|m| m.id).collect::<Vec<_>>().join(", ")
        ))
    })
}
```

Add `pub mod catalog;` to `crates/ov-asr/src/lib.rs`, and re-export the error type
it references by adding `use ov_core::error::Error;` — or, simpler, change
`crate::Error` above to `ov_core::error::Error` and add that import at the top of
`catalog.rs`. Add `serde.workspace = true` back to `crates/ov-asr/Cargo.toml`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ov-asr catalog`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/ov-asr/src/catalog.rs crates/ov-asr/src/lib.rs crates/ov-asr/Cargo.toml
git commit -m "feat(asr): describe the three selectable speech models

Every size, checksum and file list was measured by downloading the archive and
decoding through it, not read off a model card."
```

---

### Task 2: Load any catalogued model

`ParakeetTranscriber` currently hardcodes Parakeet's filenames and model type. It becomes `SherpaTranscriber`, driven by a `ModelSpec`.

**Files:**
- Rename: `crates/ov-asr/src/parakeet.rs` → `crates/ov-asr/src/sherpa.rs`
- Modify: `crates/ov-asr/src/lib.rs`

**Interfaces:**
- Consumes: `catalog::{ModelSpec, ModelKind}` from Task 1.
- Produces: `ov_asr::sherpa::SherpaTranscriber::new(spec: &'static ModelSpec, dir: PathBuf) -> Result<Self>`
  and `::with_retention(spec, dir, retain: Option<PathBuf>) -> Result<Self>`.
  `MODEL_ID` is gone; `model_id()` returns the spec's id.

- [ ] **Step 1: Write the failing tests**

Add to the test module in `sherpa.rs`:

```rust
    #[test]
    fn loads_a_transducer_and_decodes() {
        let Some(dir) = bundled_dir() else { return };
        let spec = crate::catalog::find("parakeet-tdt-0.6b-v2").unwrap();
        let t = SherpaTranscriber::new(spec, dir).expect("load");
        let out = t.transcribe(&speech(), &DecodeHint::default()).unwrap();
        assert!(out.text.to_lowercase().contains("portrait"), "got {:?}", out.text);
        assert_eq!(t.model_id(), "parakeet-tdt-0.6b-v2");
    }

    #[test]
    fn a_missing_file_names_the_file_and_the_model() {
        let spec = crate::catalog::find("whisper-tiny.en").unwrap();
        let err = SherpaTranscriber::new(spec, "Z:/nope".into())
            .expect_err("must not load")
            .to_string();
        assert!(err.contains("Z:/nope"), "must name the path: {err}");
        assert!(err.contains("tiny.en-encoder.int8.onnx"), "must name the file: {err}");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ov-asr sherpa`
Expected: FAIL — `SherpaTranscriber` not found.

- [ ] **Step 3: Generalise the transcriber**

Rename the file and the struct, then replace the hardcoded config block in
`with_retention` with a match on `spec.kind`:

```rust
        let mut cfg = OfflineRecognizerConfig::default();
        match spec.kind {
            ModelKind::Transducer => {
                cfg.model_config.transducer = OfflineTransducerModelConfig {
                    encoder: Some(path_str(&dir, "encoder.int8.onnx")?),
                    decoder: Some(path_str(&dir, "decoder.int8.onnx")?),
                    joiner: Some(path_str(&dir, "joiner.int8.onnx")?),
                };
                cfg.model_config.tokens = Some(path_str(&dir, "tokens.txt")?);
                cfg.model_config.model_type = Some("nemo_transducer".into());
            }
            ModelKind::Whisper => {
                cfg.model_config.whisper = OfflineWhisperModelConfig {
                    encoder: Some(path_str(&dir, "tiny.en-encoder.int8.onnx")?),
                    decoder: Some(path_str(&dir, "tiny.en-decoder.int8.onnx")?),
                    language: Some("en".into()),
                    task: Some("transcribe".into()),
                    // -1 lets sherpa-onnx choose. Whisper is trained on 30 s
                    // windows and pads short audio itself; overriding this was
                    // not measured, so it is not overridden.
                    tail_paddings: -1,
                    enable_token_timestamps: false,
                    enable_segment_timestamps: false,
                };
                cfg.model_config.tokens = Some(path_str(&dir, "tiny.en-tokens.txt")?);
                cfg.model_config.model_type = Some("whisper".into());
            }
        }
        cfg.model_config.num_threads = DECODE_THREADS;
```

Replace the `REQUIRED_FILES` loop with `for f in spec.files`, store `spec` on the
struct, and return `self.spec.id.to_owned()` from `model_id()`. The reported
`language` becomes `if spec.english_only { Some("en") } else { None }` — v3
detects per utterance and claiming English for it would be a lie in history.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ov-asr -- --test-threads=1`
Expected: all pass, including the existing silence and retention tests.

- [ ] **Step 5: Commit**

```bash
git add crates/ov-asr/src
git commit -m "refactor(asr): load any catalogued model, not only Parakeet

Adding a model is now a catalogue entry. The Whisper path is a second
configuration shape, not a second code path -- everything after `create` is
identical."
```

---

### Task 3: Find downloaded models as well as the bundled one

**Files:**
- Modify: `crates/ov-asr/src/locate.rs`

**Interfaces:**
- Produces: `locate::model_dir(spec: &ModelSpec, user_dir: &Path) -> Result<PathBuf>`
  and `locate::is_installed(spec: &ModelSpec, user_dir: &Path) -> bool`.
  `user_dir` is passed in rather than resolved here, because `%APPDATA%` is
  `ov-app`'s concern and this crate must stay testable without it.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn a_downloaded_model_is_found_in_the_user_directory() {
        let base = std::env::temp_dir().join("ov-locate-user");
        let _ = std::fs::remove_dir_all(&base);
        let spec = crate::catalog::find("parakeet-tdt-0.6b-v3").unwrap();
        let dir = base.join(spec.id);
        std::fs::create_dir_all(&dir).unwrap();
        for f in spec.files {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        assert!(is_installed(spec, &base));
        assert_eq!(model_dir(spec, &base).unwrap(), dir);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_partial_download_does_not_count_as_installed() {
        // An interrupted transfer leaves real bytes in a real directory.
        // Treating that as installed sends the user to a model that cannot load.
        let base = std::env::temp_dir().join("ov-locate-partial");
        let _ = std::fs::remove_dir_all(&base);
        let spec = crate::catalog::find("parakeet-tdt-0.6b-v3").unwrap();
        let dir = base.join(spec.id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(spec.files[0]), b"x").unwrap();
        assert!(!is_installed(spec, &base), "one file of four is not installed");
        let _ = std::fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ov-asr locate`
Expected: FAIL — `is_installed` not found.

- [ ] **Step 3: Implement**

```rust
/// Whether every file this model needs is present under `user_dir`.
///
/// Completeness, not directory existence: an interrupted download leaves a real
/// directory with real bytes in it, and reporting that as installed sends the
/// user to a model that will fail to load.
#[must_use]
pub fn is_installed(spec: &ModelSpec, user_dir: &Path) -> bool {
    let dir = user_dir.join(spec.id);
    spec.files.iter().all(|f| dir.join(f).is_file())
}

/// Where this model's files are, bundled or downloaded.
pub fn model_dir(spec: &ModelSpec, user_dir: &Path) -> Result<PathBuf> {
    if let Some(d) = std::env::var_os("OPENVOICE_MODEL_DIR").map(PathBuf::from) {
        return Ok(d);
    }
    if spec.bundled {
        if let Some(d) = bundled_candidates(spec).into_iter().find(|p| p.is_dir()) {
            return Ok(d);
        }
    }
    let downloaded = user_dir.join(spec.id);
    if spec.files.iter().all(|f| downloaded.join(f).is_file()) {
        return Ok(downloaded);
    }
    Err(Error::Transcription(format!(
        "the {} model is not installed. Looked in {}.",
        spec.id,
        downloaded.display()
    )))
}
```

Keep the existing install-dir/checkout probing as `bundled_candidates(spec)`,
taking the spec so the directory name comes from the catalogue rather than a
constant.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ov-asr locate`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/ov-asr/src/locate.rs
git commit -m "feat(asr): locate downloaded models beside the bundled one

Downloads go under %APPDATA% because the install directory needs administrator
rights, and a download started from a settings screen must not raise UAC.

Installed means every file present, not the directory existing: an interrupted
transfer leaves real bytes behind, and calling that installed would send the
user to a model that cannot load."
```

---

### Task 4: `ov-fetch` — the one crate allowed to reach the network

**Files:**
- Create: `crates/ov-fetch/Cargo.toml`, `crates/ov-fetch/src/lib.rs`
- Modify: `Cargo.toml` (workspace members and dependencies)

**Interfaces:**
- Produces: `ov_fetch::download_and_extract(url: &str, sha256: &str, files: &[&str], dest: &Path, on_progress: &mut dyn FnMut(u64, u64)) -> Result<(), String>`

- [ ] **Step 1: Create the crate**

`crates/ov-fetch/Cargo.toml`:

```toml
[package]
name = "ov-fetch"
description = "OpenVoice model downloads: the only crate in the workspace that can reach the network."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
tracing.workspace = true
# Blocking and small. The caller is already on a background thread, so an async
# client would add a runtime to carry no benefit.
ureq = "2.12"
sha2 = "0.10"
tar = "0.4"
bzip2 = "0.4"

[lints.rust]
missing_docs = "warn"
unsafe_code = "forbid"

[lints.clippy]
all = { level = "warn", priority = -1 }
```

Add `"crates/ov-fetch"` to workspace `members` and
`ov-fetch = { path = "crates/ov-fetch", version = "0.6.0" }` to
`[workspace.dependencies]`.

- [ ] **Step 2: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checksum_mismatch_is_refused_and_leaves_nothing_behind() {
        // The whole point of the checksum. A wrong archive must not be extracted
        // and must not leave a half-written model that `is_installed` would
        // later report as ready.
        let dest = std::env::temp_dir().join("ov-fetch-badsum");
        let _ = std::fs::remove_dir_all(&dest);
        let err = verify(b"not the bytes you wanted", "0".repeat(64).as_str())
            .expect_err("must reject");
        assert!(err.contains("checksum"), "{err}");
        assert!(!dest.exists());
    }

    #[test]
    fn a_matching_checksum_is_accepted() {
        // sha256 of the empty string, so the expectation is checkable by hand.
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(verify(b"", empty).is_ok());
    }
}
```

- [ ] **Step 3: Implement**

```rust
//! Fetching speech models, and nothing else.
//!
//! # Why this is its own crate
//!
//! `scripts/check-no-network.sh` proves that every crate touching the
//! microphone, the transcript, the keyboard or the history database has no path
//! to a socket. `ov-asr` is in that sealed set and must stay there.
//!
//! Downloading a model needs a socket. Putting it in `ov-asr` would trade a
//! tested guarantee for a convenience; putting it here keeps the guarantee and
//! makes "which code can reach the internet" answerable by reading one
//! Cargo.toml. See ADR 0009.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Check `bytes` against a lowercase hex SHA-256.
fn verify(bytes: &[u8], expected: &str) -> Result<(), String> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "checksum mismatch\n  expected {expected}\n  actual   {actual}"
        ))
    }
}

/// Download `url`, verify it, and extract `files` into `dest`.
///
/// Progress is reported as `(downloaded, total)`; `total` is 0 when the server
/// sends no `Content-Length`, and callers show an indeterminate bar rather than
/// a percentage of nothing.
///
/// Extraction happens into a staging directory and is moved into place only on
/// success, so an interrupted run cannot leave a partial model that
/// `ov_asr::locate::is_installed` would report as ready.
pub fn download_and_extract(
    url: &str,
    sha256: &str,
    files: &[&str],
    dest: &Path,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    let resp = ureq::get(url).call().map_err(|e| format!("fetching {url}: {e}"))?;
    let total: u64 = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut buf = Vec::with_capacity(total as usize);
    let mut reader = resp.into_reader();
    let mut chunk = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut chunk).map_err(|e| format!("reading {url}: {e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        on_progress(buf.len() as u64, total);
    }

    verify(&buf, sha256)?;

    let staging = dest.with_extension("partial");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("creating {}: {e}", staging.display()))?;

    let mut archive = tar::Archive::new(bzip2::read::BzDecoder::new(&buf[..]));
    for entry in archive.entries().map_err(|e| format!("reading archive: {e}"))? {
        let mut entry = entry.map_err(|e| format!("reading archive entry: {e}"))?;
        let path = entry.path().map_err(|e| format!("archive path: {e}"))?.into_owned();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Only the files the catalogue asks for. Whisper ships fp32 and int8
        // side by side, and keeping both would waste 146 MB of the user's disk.
        if !files.contains(&name) {
            continue;
        }
        entry
            .unpack(staging.join(name))
            .map_err(|e| format!("extracting {name}: {e}"))?;
    }

    for f in files {
        if !staging.join(f).is_file() {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("the archive did not contain {f}"));
        }
    }

    let _ = std::fs::remove_dir_all(dest);
    std::fs::rename(&staging, dest).map_err(|e| format!("installing to {}: {e}", dest.display()))?;
    tracing::info!(dest = %dest.display(), "model installed");
    Ok(())
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ov-fetch`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/ov-fetch Cargo.toml Cargo.lock
git commit -m "feat(fetch): a single, named, network-capable crate

ov-asr holds the microphone and is provably sealed; it must stay that way. This
puts the socket somewhere auditable instead, so the answer to 'what can reach
the internet' is one Cargo.toml.

Extracts to a staging directory and renames on success, so an interrupted
download cannot leave a partial model that locate::is_installed would report as
ready."
```

---

### Task 5: Wire the engine to the selected model, with a fallback that cannot fail

**Files:**
- Modify: `crates/ov-app/src/engine.rs`

- [ ] **Step 1: Select from settings, fall back to bundled**

Replace the fixed `locate::model_dir()` call in `start` with:

```rust
    // Selection, with a fallback that is guaranteed to exist. The user can
    // choose a downloaded model and then delete its files, or a transfer can be
    // interrupted -- neither may leave the app unable to transcribe, because the
    // bundled model is always on disk.
    let user_models = crate::history::data_dir().join("models");
    let wanted = ov_asr::catalog::find(&settings.model)
        .unwrap_or_else(|| ov_asr::catalog::find(ov_asr::catalog::DEFAULT_MODEL).expect("default"));

    let (spec, dir) = match ov_asr::locate::model_dir(wanted, &user_models) {
        Ok(d) => (wanted, d),
        Err(e) if !wanted.bundled => {
            tracing::warn!(model = wanted.id, error = %e, "falling back to the bundled model");
            let fallback =
                ov_asr::catalog::find(ov_asr::catalog::DEFAULT_MODEL).expect("default in catalogue");
            let d = ov_asr::locate::model_dir(fallback, &user_models).map_err(|e| e.to_string())?;
            (fallback, d)
        }
        Err(e) => return Err(e.to_string()),
    };
```

Pass `spec` to `SherpaTranscriber::with_retention(spec, dir, retain)`.

- [ ] **Step 2: Restore the model restart reason**

In `crates/ov-app/src/main.rs`, put back the branch deleted in v0.5.0:

```rust
    // Weights are loaded once, at warm-up.
    if booted.model != now.model {
        reasons.push("the speech model".to_string());
    }
```

- [ ] **Step 3: Verify both paths by hand**

```bash
cargo run -p ov-app
```

Expected: loads `parakeet-tdt-0.6b-v2`. Then set `model = "parakeet-tdt-0.6b-v3"`
in `%APPDATA%\OpenVoice\settings.toml` without downloading it, relaunch, and
confirm the log says `falling back to the bundled model` and dictation still works.

- [ ] **Step 4: Commit**

```bash
git add crates/ov-app/src
git commit -m "feat(app): run the selected model, falling back to the bundled one

Choosing a model you have not downloaded, or deleting one you had, must not
leave the app unable to transcribe. The bundled model is always present, so
there is no state in which OpenVoice legitimately cannot hear you."
```

---

### Task 6: Tauri commands for the screen

**Files:**
- Create: `crates/ov-app/src/models.rs`
- Modify: `crates/ov-app/src/main.rs`

**Interfaces:**
- Produces commands `list_models`, `download_model`, `delete_model`, `get_download`.

- [ ] **Step 1: Write the module**

`ModelRow` flattens a `ModelSpec` and adds `installed: bool` and `inUse: bool`.
`download_model` runs on `spawn_blocking`, reports progress through the existing
`Shell::set_download_progress`, and refuses a model that is already installed.
`delete_model` refuses the bundled model and refuses the model currently in use:

```rust
    if spec.bundled {
        return Err("The bundled model cannot be removed — it is what OpenVoice \
                    falls back to if anything else fails.".into());
    }
```

- [ ] **Step 2: Restore the progress plumbing**

v0.5.0 deleted `DownloadProgress`, `Status::Downloading` and
`Shell::set_download_progress`. Restore all three, and the `download` field on
`AppState`. `get_status` must order failure over ready over downloading over
starting, as it did before.

- [ ] **Step 3: Verify**

```bash
cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/ov-app/src
git commit -m "feat(app): list, download and delete models over IPC

The bundled model refuses deletion: it is the fallback, and removing it would
create the one state the app has no answer for."
```

---

### Task 7: The Speech model screen

**Files:**
- Create: `apps/ui/src/screens/Models.tsx`
- Modify: `apps/ui/src/engine/settings.ts`, `apps/ui/src/windows/Hub.tsx`, `apps/ui/src/screens/Settings.tsx`

- [ ] **Step 1: Restore the IPC bindings**

In `settings.ts`, re-add `listModels`, `downloadModel`, `deleteModel`,
`getDownload`, the `ModelSpec` type, and `MODEL_COPY` with the three entries.
Copy rules: name what each is *for*, never quote an accuracy figure.

```ts
export const MODEL_COPY: Record<string, { name: string; detail: string; speed: string }> = {
  "parakeet-tdt-0.6b-v2": {
    name: "Standard",
    detail: "English. Included with OpenVoice and always available.",
    speed: "~0.5 s",
  },
  "parakeet-tdt-0.6b-v3": {
    name: "Multilingual",
    detail: "25 languages, detected as you speak. Same speed and size as Standard.",
    speed: "~0.6 s",
  },
  "whisper-tiny.en": {
    name: "Light",
    detail: "English. A sixth of the disk and memory, and noticeably less accurate — for machines that cannot spare the room.",
    speed: "~0.5 s",
  },
};
```

- [ ] **Step 2: Write the screen as its own file**

A row per model with: name, badges (`In use`, `Included`, `Downloaded`,
`English only`), size, typical latency, and one action — `Download` with byte
progress, or `Delete` for an installed non-bundled model that is not in use.
Selecting a row sets `settings.model` and shows the existing restart notice.

This lives in `Models.tsx` rather than back inside `Settings.tsx`: the old file
was 700 lines holding two unrelated screens, which is why removing one of them
was fiddly.

- [ ] **Step 3: Restore the nav item and route**

`{ id: "models", label: "Speech model", ready: true }` in `Hub.tsx`, importing
from `../screens/Models`.

- [ ] **Step 4: Correct the privacy copy**

The Settings privacy note currently claims dictation has no network path at all.
A Download button makes that false. Restore the exception:

```
There is no analytics, no crash reporting and no account. The only time
OpenVoice uses the network is a speech model you ask it to download, and the
update check you can switch off.
```

Replace the read-only "Speech engine" row added in v0.5.0 with a link to the screen.

- [ ] **Step 5: Verify**

```bash
npm run build:ui
```

Then launch and walk the screen: download Light (112 MB, quick), watch progress,
select it, restart, confirm it transcribes, delete it, confirm Standard still runs.

- [ ] **Step 6: Commit**

```bash
git add apps/ui
git commit -m "feat(ui): bring back the Speech model screen

Three models, one included and two on request. Its own file this time -- the old
one lived inside a 700-line Settings.tsx holding two unrelated screens.

The privacy note names the network exception again. A Download button makes
'no network path at all' false, and leaving that line up would be worse than the
button."
```

---

### Task 8: Keep `ov-asr` provably sealed

**Files:**
- Modify: `scripts/check-no-network.sh`

- [ ] **Step 1: Add `ov-fetch` as a network-capable crate**

Add a third category beside `SEALED_CRATES` and `NO_DIRECT_CRATES`:

```bash
# Crates that may reach the network at run time, by design. One entry, and it
# exists so that adding a second is a visible decision rather than a diff nobody
# reads. See ADR 0009.
NETWORK_CRATES=(ov-fetch)
```

`ov-asr` stays in `SEALED_CRATES`. `ov-app` gains `ov-fetch` in `ALLOWED_DIRECT`.

- [ ] **Step 2: Negative-test it**

```bash
sed 's/^SEALED_CRATES=.*/SEALED_CRATES=(ov-asr ov-fetch)/' scripts/check-no-network.sh > /tmp/neg.sh
bash /tmp/neg.sh; echo "EXIT=$?"
```

Expected: **EXIT=1**, naming `ov-fetch`. A guard nobody has watched fail is not a guard.

- [ ] **Step 3: Run for real**

Run: `bash scripts/check-no-network.sh`
Expected: EXIT=0, `ov-asr` still reported sealed.

- [ ] **Step 4: Commit**

```bash
git add scripts/check-no-network.sh
git commit -m "ci: keep ov-asr sealed now that downloads exist again

ov-fetch is named as the one network-capable crate, so adding a second is a
visible decision. Negative-tested rather than assumed."
```

---

### Task 9: Version, docs and the installer

**Files:**
- Modify: `Cargo.toml`, `crates/ov-app/tauri.conf.json`, `package.json`, `apps/ui/package.json`
- Create: `docs/adr/0009-model-downloads.md`
- Modify: `CHANGELOG.md`, `README.md`, `docs/ARCHITECTURE.md`

- [ ] **Step 1: Bump to 0.6.0**

All four version fields, **and the six internal pins in `[workspace.dependencies]`** —
those drifted once already and broke every cargo invocation at 0.5.0.

- [ ] **Step 2: Write ADR 0009**

Record: what changed (a network call returns), why (v2 is English-only, and
telling a Spanish speaker to reinstall is not an answer), what it costs (the
local-first claim needs its exception back), and what contains it (`ov-fetch` is
the only crate that can open a socket; every download is checksum-verified).

- [ ] **Step 3: Update CHANGELOG and README**

Lead with what the user gets: a model choice again, multilingual available. Say
plainly that downloads are opt-in and verified.

- [ ] **Step 4: Build the installer**

```bash
pwsh scripts/fetch-model.ps1
npm run build:ui
cd crates/ov-app && node ../../apps/ui/node_modules/@tauri-apps/cli/tauri.js build
```

Expected: ~440 MB. The bundled model is unchanged, so the installer size should
not move.

- [ ] **Step 5: Verify on the built installer**

Install. With the **network disabled**, confirm dictation works on Standard.
Re-enable, download Light, select it, restart, dictate. Delete it. Confirm
Standard still runs.

- [ ] **Step 6: Commit and tag**

```bash
git add -A
git commit -m "docs: record model downloads in ADR 0009 and ship 0.6.0"
git tag v0.6.0
```

---

## Self-Review

**Spec coverage.** Catalogue → Task 1. Two model kinds → Task 2. Two storage
locations → Task 3. Network isolation → Tasks 4, 8. Fallback that cannot fail →
Tasks 3, 5. Restart-on-change → Task 5. IPC → Task 6. Screen and copy → Task 7.
Version, ADR, installer → Task 9. **No gaps.**

**Placeholder scan.** None. Task 6 and Task 7 describe components in prose rather
than full listings — they are UI and IPC glue whose shape is fixed by the
interfaces in Tasks 1–5, and the file each belongs in is named.

**Type consistency.** `ModelSpec`/`ModelKind` (Task 1) are the same names used in
Tasks 2, 3, 5, 6. `SherpaTranscriber::new(spec, dir)` (Task 2) matches its call in
Task 5. `is_installed(spec, user_dir)` and `model_dir(spec, user_dir)` (Task 3)
match Tasks 5 and 6. `download_and_extract` (Task 4) matches Task 6. Model ids are
identical across the catalogue, `MODEL_COPY` and every test.

**Deliberately out of scope:** custom user-supplied models (dropped by the owner),
hotwords, and the 437 MB update problem — the last is unchanged by this work and
still recorded in ADR 0008.
