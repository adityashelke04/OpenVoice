# Parakeet Single-Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace faster-whisper-in-a-Python-sidecar with NVIDIA Parakeet TDT 0.6B v2 running in-process via the official `sherpa-onnx` Rust crate, ship the model inside the installer, and delete everything the swap makes redundant.

**Architecture:** `ov-asr` stops being a process supervisor and becomes a thin wrapper over `sherpa_onnx::OfflineRecognizer`, still implementing the unchanged `ov_core::ports::Transcriber` port. The Python sidecar, its IPC protocol, its Windows job object, the Hugging Face download manager and the multi-model catalogue are deleted. The model is installed to `<install dir>/models/parakeet-tdt-0.6b-v2/` by an NSIS hook so app updates stay small.

**Tech Stack:** Rust 1.97, `sherpa-onnx` 1.13.7 (crates.io, static linkage), Tauri v2 + NSIS, React/TypeScript frontend, `cargo test`, `vitest`.

**Spec:** `docs/superpowers/specs/2026-09-03-parakeet-single-model-design.md`

## Global Constraints

- **Model:** Parakeet TDT 0.6B v2 int8, from sherpa-onnx release asset `sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2`. 631 MB extracted, 482 MB compressed. Four files: `encoder.int8.onnx`, `decoder.int8.onnx`, `joiner.int8.onnx`, `tokens.txt`.
- **Crate:** `sherpa-onnx = "1.13.7"` exactly. Default features (`static`). Do not enable `shared` — the static build produces a self-contained binary with zero DLLs, which is verified and is the whole packaging premise.
- **Recognizer settings, fixed, not user-configurable:** `model_type = "nemo_transducer"`, `num_threads = 4`, `feat_config.sample_rate = 16000`, `feat_config.feature_dim = 80`, `decoding_method` left at default (greedy).
- **Model directory name:** `parakeet-tdt-0.6b-v2` everywhere — installer, resolver, tests, CI.
- **`model_id()` returns:** `"parakeet-tdt-0.6b-v2"`. Recorded in history; changing it later orphans attribution.
- **Never delete from `ov-core`:** `DecodeHint.language` and `Transcript.confidence` stay in the structs (persisted/versioned types). They become unused, not removed. `language` is ignored; `confidence` is always `None`.
- **Do not quote "1.9% WER" or "98% accurate" in user-facing copy.** Spec §2.1 — LibriSpeech is in-domain for this model. Copy may say "typically about half a second"; it may not claim an accuracy figure.
- **Version target:** `0.5.0` in `Cargo.toml` and `tauri.conf.json`. Not 1.0.0.
- **Every task ends green:** `cargo test --workspace` passes and `cargo clippy --workspace -- -D warnings` is clean before each commit.

---

### Task 1: Add the model fetch script and commit its checksum

Nothing else can be tested without the weights on disk. This task makes acquiring them reproducible and verified, which ADR 0003 admitted was never done for the Whisper weights.

**Files:**
- Create: `scripts/fetch-model.ps1`
- Create: `models/.gitignore`
- Modify: `.gitignore`

**Interfaces:**
- Produces: model at `<repo root>/models/parakeet-tdt-0.6b-v2/` containing `encoder.int8.onnx`, `decoder.int8.onnx`, `joiner.int8.onnx`, `tokens.txt`. Every later task depends on this path.

- [ ] **Step 1: Create `models/.gitignore` so weights never get committed**

```gitignore
# Speech model weights: 631 MB, fetched by scripts/fetch-model.ps1 and
# verified against the checksum pinned in that script. Never committed.
*
!.gitignore
```

- [ ] **Step 2: Write `scripts/fetch-model.ps1`**

```powershell
<#
.SYNOPSIS
  Fetch and verify the Parakeet TDT 0.6B v2 weights into models/.

.DESCRIPTION
  ADR 0003 carries a correction admitting that a claimed SHA-256 manifest for
  the Whisper weights never existed. This script is that claim, made real: the
  archive hash is pinned below and a mismatch is a hard failure, because
  unverified weights end up inside an installer other people run.

  Idempotent. Re-running with the model already present verifies and exits.
#>
[CmdletBinding()]
param([switch]$Force)

$ErrorActionPreference = 'Stop'

$Name    = 'sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8'
$Uri     = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/$Name.tar.bz2"
$Sha256  = 'PLACEHOLDER_REPLACED_IN_STEP_4'
$Root    = Split-Path -Parent $PSScriptRoot
$Dest    = Join-Path $Root 'models/parakeet-tdt-0.6b-v2'
$Files   = @('encoder.int8.onnx', 'decoder.int8.onnx', 'joiner.int8.onnx', 'tokens.txt')

if (-not $Force -and (Test-Path $Dest)) {
    $missing = $Files | Where-Object { -not (Test-Path (Join-Path $Dest $_)) }
    if (-not $missing) { Write-Host "Model already present at $Dest"; exit 0 }
    Write-Host "Model at $Dest is incomplete (missing: $($missing -join ', ')); refetching."
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) "$Name.tar.bz2"
Write-Host "Downloading $Uri (482 MB)..."
Invoke-WebRequest -Uri $Uri -OutFile $tmp -UseBasicParsing

$actual = (Get-FileHash -Path $tmp -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $Sha256) {
    Remove-Item $tmp -Force
    throw "Checksum mismatch for $Name.tar.bz2`n  expected $Sha256`n  actual   $actual"
}
Write-Host "Checksum OK."

$staging = Join-Path ([System.IO.Path]::GetTempPath()) "ov-model-$(Get-Random)"
New-Item -ItemType Directory -Path $staging -Force | Out-Null
tar -xjf $tmp -C $staging
if ($LASTEXITCODE -ne 0) { throw "tar failed to extract $tmp" }

# The archive expands to a directory named after the release asset. We install
# it under a stable name so nothing downstream encodes the upstream filename.
if (Test-Path $Dest) { Remove-Item $Dest -Recurse -Force }
New-Item -ItemType Directory -Path $Dest -Force | Out-Null
foreach ($f in $Files) {
    Copy-Item (Join-Path $staging "$Name/$f") (Join-Path $Dest $f)
}

Remove-Item $staging -Recurse -Force
Remove-Item $tmp -Force
Write-Host "Model ready at $Dest"
```

- [ ] **Step 3: Run it once with a deliberately wrong hash to prove the check bites**

Run: `pwsh scripts/fetch-model.ps1`
Expected: FAIL with "Checksum mismatch". If it succeeds, the verification is not wired up — fix before continuing.

- [ ] **Step 4: Record the real checksum and re-run**

```powershell
# Compute from the archive the failing run downloaded, then paste into $Sha256.
(Get-FileHash "$env:TEMP\sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2" -Algorithm SHA256).Hash.ToLower()
```

Run: `pwsh scripts/fetch-model.ps1`
Expected: "Checksum OK." then "Model ready at ...". Then `ls models/parakeet-tdt-0.6b-v2` shows all four files.

- [ ] **Step 5: Verify idempotence**

Run: `pwsh scripts/fetch-model.ps1`
Expected: "Model already present at ..." and exit 0, with no download.

- [ ] **Step 6: Commit**

```bash
git add scripts/fetch-model.ps1 models/.gitignore
git commit -m "build(model): fetch Parakeet weights with a pinned SHA-256

ADR 0003 admits a checksum manifest was claimed but never written. This is
that claim made real: a hash mismatch is a hard failure, because these bytes
end up inside an installer other people run."
```

---

### Task 2: `ParakeetTranscriber` — decode a fixture end to end

The core of the change. Written test-first against a committed audio fixture so the decode path is proven before anything is wired to it.

**Files:**
- Create: `crates/ov-asr/src/parakeet.rs`
- Create: `fixtures/audio/hello.wav`, `fixtures/audio/silence.wav`
- Modify: `crates/ov-asr/Cargo.toml`
- Modify: `crates/ov-asr/src/lib.rs` (add `pub mod parakeet;` only — leave the sidecar alone)

**Interfaces:**
- Consumes: model directory from Task 1.
- Produces: `ov_asr::parakeet::ParakeetTranscriber::new(model_dir: PathBuf) -> Result<Self>` and `ov_asr::parakeet::model_dir() -> Result<PathBuf>`. Task 3 wires both into `ov-app`.

- [ ] **Step 1: Add the dependency**

In `crates/ov-asr/Cargo.toml`, under `[dependencies]`:

```toml
# Official k2-fsa bindings. Default features build sherpa-onnx statically, which
# is what makes the app a single binary with no DLLs beside it -- verified: an
# 18.9 MB release binary that runs from any working directory.
sherpa-onnx = "1.13.7"
```

- [ ] **Step 2: Create the audio fixtures**

`fixtures/audio/` is currently empty. Generate both as 16 kHz mono PCM16 — the exact format `ov-audio` produces:

```powershell
# hello.wav: record ~2s of yourself saying "testing one two three", or reuse a
# clip from the spike set. Must be 16 kHz mono PCM16.
# silence.wav: two seconds of digital silence.
python -c @'
import wave
with wave.open("fixtures/audio/silence.wav", "wb") as w:
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
    w.writeframes(b"\x00\x00" * 32000)
'@
```

Commit both. `hello.wav` must contain speech whose transcript you know; the test asserts on a lowercased substring, not an exact match.

- [ ] **Step 3: Write the failing tests**

Create `crates/ov-asr/src/parakeet.rs` with only the tests at first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ov_core::ports::{DecodeHint, Pcm16k, Transcriber};

    /// Repo-root-relative path to the model, for tests run from the crate dir.
    fn test_model_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../models/parakeet-tdt-0.6b-v2")
    }

    fn read_fixture(name: &str) -> Pcm16k {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/audio")
            .join(name);
        let mut r = hound::WavReader::open(&path)
            .unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
        assert_eq!(r.spec().sample_rate, 16_000, "fixture must be 16 kHz");
        assert_eq!(r.spec().channels, 1, "fixture must be mono");
        Pcm16k {
            samples: r.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect(),
        }
    }

    #[test]
    fn missing_model_is_a_named_error() {
        // The most likely packaging bug is a build that ships without weights.
        // It must fail loudly, naming the path it wanted, rather than hanging on
        // a spinner the user cannot diagnose.
        let err = ParakeetTranscriber::new("Z:/definitely/not/here".into())
            .expect_err("a missing model must not succeed")
            .to_string();
        assert!(err.contains("Z:/definitely/not/here"), "error must name the path: {err}");
    }

    #[test]
    fn decodes_a_known_fixture() {
        let t = ParakeetTranscriber::new(test_model_dir()).expect("load model");
        let out = t.transcribe(&read_fixture("hello.wav"), &DecodeHint::default()).unwrap();
        assert!(
            out.text.to_lowercase().contains("testing"),
            "expected the spoken words, got {:?}",
            out.text
        );
    }

    #[test]
    fn silence_yields_empty_text() {
        // Parakeet returning nothing on silence is why Whisper's VAD and its two
        // confidence gates are deleted rather than reimplemented. Lock it in: if
        // this ever regresses, the missing defences become a user-visible bug.
        let t = ParakeetTranscriber::new(test_model_dir()).expect("load model");
        let out = t.transcribe(&read_fixture("silence.wav"), &DecodeHint::default()).unwrap();
        assert_eq!(out.text.trim(), "", "silence must not produce words");
    }

    #[test]
    fn empty_audio_is_rejected_without_calling_the_model() {
        let t = ParakeetTranscriber::new(test_model_dir()).expect("load model");
        let err = t.transcribe(&Pcm16k { samples: vec![] }, &DecodeHint::default())
            .expect_err("empty audio is a caller bug");
        assert!(err.to_string().contains("no audio"));
    }

    #[test]
    fn model_id_is_stable() {
        // History rows are attributed with this string. Changing it orphans them.
        let t = ParakeetTranscriber::new(test_model_dir()).expect("load model");
        assert_eq!(t.model_id(), "parakeet-tdt-0.6b-v2");
    }

    #[test]
    fn confidence_is_always_none() {
        // A transducer emits no per-segment log-probability. The field stays in
        // ov_core::types::Transcript because it is persisted, but nothing fills it.
        let t = ParakeetTranscriber::new(test_model_dir()).expect("load model");
        let out = t.transcribe(&read_fixture("hello.wav"), &DecodeHint::default()).unwrap();
        assert!(out.confidence.is_none());
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p ov-asr parakeet`
Expected: FAIL to compile — `ParakeetTranscriber` not found.

- [ ] **Step 5: Write the implementation**

Prepend to `crates/ov-asr/src/parakeet.rs`:

```rust
//! # Parakeet — in-process speech recognition
//!
//! Implements [`ov_core::ports::Transcriber`] with NVIDIA Parakeet TDT 0.6B v2
//! running inside this process through the official `sherpa-onnx` bindings.
//!
//! ## Why in-process, when ADR 0003 chose a child process
//!
//! ADR 0003 picked a Python sidecar because the in-process alternative needed a
//! CUDA Toolkit or a hand-built binary on Windows, and it recorded "remove the
//! Python dependency before distribution" as explicit follow-up. That follow-up
//! is now cheap: k2-fsa publish Rust bindings whose build script fetches
//! prebuilt static libraries, so this links into the app with no CMake, no CUDA
//! Toolkit, and no DLLs beside the binary.
//!
//! ## What was given up
//!
//! Process isolation. A sidecar crash used to degrade the app; a native fault
//! here takes it down. The audio file is written before the decode and removed
//! after, so a crash costs the user a decode, never a recording.
//!
//! ## What was gained beyond speed
//!
//! Parakeet returns empty text for silence and room tone. Whisper invents words
//! from both, which is why the sidecar carried a voice-activity filter and two
//! confidence gates. None of that is reproduced here, because the failure it
//! defended against does not occur.

use std::path::{Path, PathBuf};

use ov_core::error::{Error, Result};
use ov_core::ports::{DecodeHint, Pcm16k, Transcriber};
use ov_core::types::Transcript;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

/// Stable identifier for this model, recorded in history alongside transcripts.
pub const MODEL_ID: &str = "parakeet-tdt-0.6b-v2";

/// Decode threads.
///
/// Four, not "all of them". Measured on a 12-thread machine: 535 ms median at
/// four threads against 645 ms at twelve — the extra eight threads buy 110 ms
/// and cost the responsiveness of whatever the user is dictating into. This is
/// a background dictation tool, so it takes the smaller share.
const DECODE_THREADS: i32 = 4;

/// The four files a Parakeet directory must contain.
const REQUIRED_FILES: [&str; 4] = [
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
];

/// Parakeet, loaded and ready to decode.
pub struct ParakeetTranscriber {
    recognizer: OfflineRecognizer,
}

impl ParakeetTranscriber {
    /// Load the model from `dir`.
    ///
    /// Expensive — roughly 2.5 seconds and 757 MB resident — and done once at
    /// startup. [`Transcriber::warm`] is consequently a no-op.
    pub fn new(dir: PathBuf) -> Result<Self> {
        // Check the files before handing paths to the C library. It reports a
        // missing model as a null pointer with no detail, and "failed to create
        // recognizer" is not something a user can act on.
        for f in REQUIRED_FILES {
            let p = dir.join(f);
            if !p.exists() {
                return Err(Error::Transcription(format!(
                    "speech model is incomplete: {} is missing from {}",
                    f,
                    dir.display()
                )));
            }
        }

        let mut cfg = OfflineRecognizerConfig::default();
        cfg.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(path_str(&dir, "encoder.int8.onnx")?),
            decoder: Some(path_str(&dir, "decoder.int8.onnx")?),
            joiner: Some(path_str(&dir, "joiner.int8.onnx")?),
        };
        cfg.model_config.tokens = Some(path_str(&dir, "tokens.txt")?);
        cfg.model_config.model_type = Some("nemo_transducer".into());
        cfg.model_config.num_threads = DECODE_THREADS;

        let started = std::time::Instant::now();
        let recognizer = OfflineRecognizer::create(&cfg).ok_or_else(|| {
            Error::Transcription(format!(
                "the speech model at {} could not be loaded",
                dir.display()
            ))
        })?;
        tracing::info!(
            model = MODEL_ID,
            took_ms = started.elapsed().as_millis() as u64,
            "speech model loaded"
        );

        Ok(Self { recognizer })
    }
}

/// A path under `dir`, as the UTF-8 string the C API needs.
fn path_str(dir: &Path, file: &str) -> Result<String> {
    dir.join(file)
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Transcription(format!(
            "the model path {} is not valid UTF-8; move it somewhere without unusual characters",
            dir.display()
        )))
}

impl Transcriber for ParakeetTranscriber {
    fn warm(&self) -> Result<()> {
        // Weights are already resident: `new` loaded them. Kept so callers need
        // not know which backend they hold.
        Ok(())
    }

    fn transcribe(&self, audio: &Pcm16k, _hint: &DecodeHint) -> Result<Transcript> {
        if audio.samples.is_empty() {
            return Err(Error::Transcription("no audio to transcribe".into()));
        }

        let started = std::time::Instant::now();
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(Pcm16k::RATE as i32, &audio.samples);
        self.recognizer.decode(&stream);
        let text = stream.get_result().map(|r| r.text).unwrap_or_default();

        tracing::debug!(
            decode_ms = started.elapsed().as_millis() as u64,
            chars = text.trim().len(),
            "decoded"
        );

        Ok(Transcript {
            text: text.trim().to_owned(),
            // Parakeet v2 is English-only, so there is nothing to detect and
            // `hint.language` has nothing to force. Stated once, here.
            language: Some("en".into()),
            // No per-segment log-probability exists to report. The field stays
            // in the persisted type; it is simply never filled.
            confidence: None,
        })
    }

    fn model_id(&self) -> String {
        MODEL_ID.to_owned()
    }
}
```

Add to `crates/ov-asr/src/lib.rs`, beside the existing `pub mod catalog;`:

```rust
pub mod parakeet;
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p ov-asr parakeet -- --test-threads=1`
Expected: 6 passed. `--test-threads=1` because each test loads 757 MB; running them in parallel needs 4.5 GB.

- [ ] **Step 7: Commit**

```bash
git add crates/ov-asr/src/parakeet.rs crates/ov-asr/src/lib.rs crates/ov-asr/Cargo.toml fixtures/audio Cargo.lock
git commit -m "feat(asr): decode with Parakeet in-process via sherpa-onnx

Implements Transcriber against NVIDIA Parakeet TDT 0.6B v2 with no child
process. The sidecar is untouched and still the default; this only adds the
alternative so the two can be compared on real dictation before anything is
deleted.

The silence test is load-bearing: Parakeet returning empty text on room tone
is the reason Whisper's VAD and confidence gates get deleted rather than
reimplemented later."
```

---

### Task 3: Select the engine at runtime, defaulting to Whisper

Both engines live side by side, switchable without a rebuild. This is the A/B point and the cheap retreat the spec's phasing depends on.

**Files:**
- Modify: `crates/ov-app/src/engine.rs`
- Create: `crates/ov-asr/src/locate.rs`
- Modify: `crates/ov-asr/src/lib.rs`

**Interfaces:**
- Consumes: `ParakeetTranscriber::new` from Task 2.
- Produces: `ov_asr::locate::model_dir() -> Result<PathBuf>`, used by Task 6's packaging tests.

- [ ] **Step 1: Write the failing resolver test**

Create `crates/ov-asr/src/locate.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_wins_and_is_returned_verbatim() {
        let tmp = std::env::temp_dir().join("ov-locate-test");
        std::fs::create_dir_all(&tmp).unwrap();
        let found = resolve_from(Some(tmp.clone()), &[]);
        assert_eq!(found.as_deref(), Some(tmp.as_path()));
    }

    #[test]
    fn falls_through_candidates_in_order() {
        let base = std::env::temp_dir().join("ov-locate-order");
        let first = base.join("a");
        let second = base.join("b");
        std::fs::create_dir_all(&second).unwrap();
        let _ = std::fs::remove_dir_all(&first);
        let found = resolve_from(None, &[first, second.clone()]);
        assert_eq!(found.as_deref(), Some(second.as_path()), "must skip the absent one");
    }

    #[test]
    fn missing_everywhere_is_none() {
        assert!(resolve_from(None, &[PathBuf::from("Z:/nope")]).is_none());
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ov-asr locate`
Expected: FAIL to compile — `resolve_from` not found.

- [ ] **Step 3: Implement the resolver**

Prepend to `crates/ov-asr/src/locate.rs`:

```rust
//! Where the speech model lives on this machine.
//!
//! Deliberately probes both installed layouts. The packaging decision — whether
//! the weights sit in `resources/` as a Tauri resource or in `models/` placed by
//! an NSIS hook — is settled in the design's §7.2, but engine code should not
//! have to care, and two `stat` calls at startup is a cheap way to not care.

use std::path::{Path, PathBuf};

use ov_core::error::{Error, Result};

/// Directory name for the model, in every layout.
pub const MODEL_DIR_NAME: &str = "parakeet-tdt-0.6b-v2";

/// Locate the model, or explain where it was looked for.
pub fn model_dir() -> Result<PathBuf> {
    let override_dir = std::env::var_os("OPENVOICE_MODEL_DIR").map(PathBuf::from);
    let candidates = candidates();

    resolve_from(override_dir, &candidates).ok_or_else(|| {
        Error::Transcription(format!(
            "no speech model found. Looked in: {}. Set OPENVOICE_MODEL_DIR, or run \
             scripts/fetch-model.ps1 from a checkout.",
            candidates
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

/// The places a model may be, most specific first.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Installed, NSIS hook layout (design §7.2 option B).
            out.push(dir.join("models").join(MODEL_DIR_NAME));
            // Installed, Tauri resource layout (option A) -- kept so a build
            // packaged either way runs.
            out.push(dir.join("resources").join("models").join(MODEL_DIR_NAME));
        }
    }
    // A checkout, for `cargo run`.
    out.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("models")
            .join(MODEL_DIR_NAME),
    );
    out
}

/// Pure core of [`model_dir`], so the ordering is testable without a filesystem
/// that looks like an install.
fn resolve_from(override_dir: Option<PathBuf>, candidates: &[PathBuf]) -> Option<PathBuf> {
    if let Some(d) = override_dir {
        return Some(d);
    }
    candidates.iter().find(|p| p.is_dir()).cloned()
}
```

Add to `crates/ov-asr/src/lib.rs`:

```rust
pub mod locate;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ov-asr locate`
Expected: 3 passed.

- [ ] **Step 5: Wire engine selection into `ov-app`**

In `crates/ov-app/src/engine.rs`, where `SidecarTranscriber::new` is currently called (around lines 601 and 633), select on an environment variable. Keep the existing sidecar path as the default for now:

```rust
/// Which speech backend to construct.
///
/// Parakeet is opt-in for exactly one release. It ships as the default in the
/// next commit, once it has been used for real dictation; until then this is the
/// switch that makes an A/B comparison a restart rather than a rebuild.
fn use_parakeet() -> bool {
    std::env::var("OPENVOICE_ENGINE").is_ok_and(|v| v.eq_ignore_ascii_case("parakeet"))
}
```

At each construction site, branch to a boxed `dyn Transcriber`. The surrounding
struct field `transcriber: ov_asr::SidecarTranscriber` at `engine.rs:108` must
become `Box<dyn ov_core::ports::Transcriber>` for this to compile; make that
change and fix the resulting call sites, which only ever use trait methods.

- [ ] **Step 6: Verify both engines run**

```bash
cargo build --workspace
cargo test --workspace
```

Then launch each and dictate one sentence:

```bash
OPENVOICE_ENGINE=parakeet cargo run -p ov-app     # expect: "speech model loaded" in the log
cargo run -p ov-app                                # expect: the sidecar, as before
```

Expected: both transcribe. If Parakeet does not, stop — do not continue to Task 4.

- [ ] **Step 7: Commit**

```bash
git add crates/ov-asr/src/locate.rs crates/ov-asr/src/lib.rs crates/ov-app/src/engine.rs
git commit -m "feat(asr): select the speech engine with OPENVOICE_ENGINE

Both backends now build and run; the sidecar stays the default. This is the
comparison point -- Parakeet has to earn the default on real dictation before
any Whisper code is deleted."
```

---

### Task 4: Make Parakeet the default

**Files:**
- Modify: `crates/ov-app/src/engine.rs`

- [ ] **Step 1: Invert the switch**

```rust
/// Which speech backend to construct.
///
/// Parakeet unless explicitly overridden. `OPENVOICE_ENGINE=whisper` still
/// reaches the sidecar; that escape hatch survives exactly until the sidecar is
/// deleted in the next task, and exists so this commit can be reverted alone.
fn use_parakeet() -> bool {
    !std::env::var("OPENVOICE_ENGINE").is_ok_and(|v| v.eq_ignore_ascii_case("whisper"))
}
```

- [ ] **Step 2: Verify the default flipped**

Run: `cargo run -p ov-app`
Expected: the log shows `speech model loaded model=parakeet-tdt-0.6b-v2`, and no `openvoice-asr.exe` appears in Task Manager.

- [ ] **Step 3: Dictate for real**

Use the app normally for a stretch: a commit message, a shell command with flags, a paragraph of prose, and at least one term from your dictionary. This is the design's §11 gate. Note whether losing decode-time dictionary biasing is noticeable on technical terms.

Expected: clearly better than `base.en`, with no new failure class. **If it is not, stop and revert — the plan is void from here.**

- [ ] **Step 4: Commit**

```bash
git add crates/ov-app/src/engine.rs
git commit -m "feat(asr): make Parakeet the default engine

Whisper stays reachable with OPENVOICE_ENGINE=whisper until the next commit
deletes it, so this flip can be reverted on its own."
```

---

### Task 5: Delete Whisper

Only after Task 4 has been used for real work. Each step compiles and tests green on its own.

**Files:**
- Delete: `sidecar/` (entire tree), `crates/ov-asr/src/job.rs`, `crates/ov-asr/src/store.rs`, `crates/ov-asr/src/wav.rs`, `scripts/build-sidecar.ps1`
- Modify: `crates/ov-asr/src/lib.rs`, `crates/ov-asr/src/catalog.rs`, `crates/ov-asr/Cargo.toml`, `crates/ov-app/src/engine.rs`, `crates/ov-app/src/main.rs`

- [ ] **Step 1: Collapse `ov-asr`'s public surface**

Reduce `crates/ov-asr/src/lib.rs` to a crate doc plus `pub mod locate;` and `pub mod parakeet;`. Delete `SidecarConfig`, `SidecarTranscriber`, `Progress`, the JSON protocol, and the `job`/`store`/`wav` modules. Rewrite the crate doc to describe what the crate is now, not what it was.

Remove the now-unused dependencies from `crates/ov-asr/Cargo.toml`: `serde`, `serde_json`, `hound` (unless the Task 2 tests still need it — keep it under `[dev-dependencies]` if so), the entire `[target.'cfg(windows)'.dependencies]` block, and the `filetime` dev-dependency. Set `unsafe_code = "forbid"` in `[lints.rust]`: with `job.rs` gone there is no unsafe left, and forbidding it is a claim worth making.

- [ ] **Step 2: Reduce the catalogue to one model**

`crates/ov-asr/src/catalog.rs` describes a multi-model Hugging Face world that no longer exists. Delete `ModelSpec.repo`, `compute_type`, `fallback_compute`, `vram_mb`, and the `CATALOG` array. Delete the tests `sizes_are_plausible`, `catalog_is_ordered_most_capable_first`, `a_fallback_compute_differs_from_the_preferred_one`, `every_repo_is_fully_qualified`, `ids_are_unique`, `the_default_model_needs_no_gpu` and `resolve_names_the_alternatives` — every one of them asserts a property of a set with more than one member.

If nothing outside `parakeet.rs` still reads it, delete `catalog.rs` outright and let `MODEL_ID` in `parakeet.rs` be the single source of truth. Prefer this.

- [ ] **Step 3: Remove the model-management Tauri commands**

In `crates/ov-app/src/main.rs`, delete `list_models`, `download_model`, `delete_model`, `get_download` and their registrations, plus the `ov_asr::store` and `ov_asr::catalog` calls at lines 443–500 and 654–656. Delete `purge_recordings`'s `ov_asr::store` import if that function moved.

In `crates/ov-app/src/engine.rs`, delete `from_checkout`, `find_python`, the frozen-sidecar locator, and `use_parakeet` itself — construct `ParakeetTranscriber` unconditionally.

- [ ] **Step 4: Delete the trees**

```bash
git rm -r sidecar
git rm scripts/build-sidecar.ps1
git rm crates/ov-asr/src/job.rs crates/ov-asr/src/store.rs crates/ov-asr/src/wav.rs
```

- [ ] **Step 5: Verify**

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Expected: green. `ov-core`, `ov-format`, `ov-audio`, `ov-input` and `ov-store` tests must pass **untouched** — that is the check that the port boundary held.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(asr): delete the Whisper sidecar and everything serving it

Removes the Python tree, the process supervisor, the Windows job object, the
JSON protocol, the Hugging Face download manager and the multi-model catalogue.
ov-asr goes from 1508 lines to roughly 250.

ov-core, ov-format, ov-audio, ov-input and ov-store are untouched and their
tests pass unchanged, which is the evidence that ADR 0001's port boundary held
under a backend swap this large."
```

---

### Task 6: Frontend — delete the Models screen, state the engine once

**Files:**
- Modify: `apps/ui/src/screens/Settings.tsx`, `apps/ui/src/engine/settings.ts`
- Modify: wherever the Models nav item is registered

- [ ] **Step 1: Remove the model IPC bindings**

In `apps/ui/src/engine/settings.ts`, delete `listModels`, `downloadModel`, `deleteModel`, `getDownload`, `MODEL_COPY`, `formatSize`, and the `ModelSpec` type. They call commands that no longer exist.

- [ ] **Step 2: Delete `ModelsScreen` and its nav entry**

Remove the whole `ModelsScreen` export from `Settings.tsx` (roughly lines 483–700) and the route/nav item pointing at it. A screen offering one permanent, undeletable, already-installed item asks a question with one answer.

- [ ] **Step 3: Add the read-only engine block to Settings**

```tsx
{/* A fact, not a control. There is nothing to choose, download or delete, so
    this deliberately has no buttons -- a Download button next to a model that
    ships inside the installer would be a lie. */}
<section className="setting-block">
  <div className="t-label">Speech engine</div>
  <div className="t-body">Parakeet TDT 0.6B v2 — English</div>
  <div className="t-caption">
    Runs entirely on this computer. Typically responds in about half a second.
  </div>
</section>
```

Do not add an accuracy percentage here. Design §2.1: the measured figure came from audio that is in-domain for this model, and a number in the UI would outlive that caveat.

- [ ] **Step 4: Fix the stale copy**

- `Settings.tsx:3-4` — the header comment explains a "Accurate"/"Light" abstraction that no longer exists. Rewrite.
- `Settings.tsx:663` — "choose **Light**, because the accurate model needs the card mostly to itself" refers to two deleted models and a GPU that is no longer used. Delete.
- The privacy note at `Settings.tsx:454` ends "…except to download a speech model you ask for." Change to state there is no network path at all — with the model bundled and the downloader gone, that is now literally true.
- Remove the language picker; English-only is stated once in the engine block.

- [ ] **Step 5: Verify**

```bash
npm run build:ui
npm test --prefix apps/ui   # if the project has frontend tests
```

Then launch the app and walk every settings screen looking for a dangling reference to models, downloads, VRAM or languages.

- [ ] **Step 6: Commit**

```bash
git add apps/ui
git commit -m "feat(ui): replace the model picker with a statement of fact

Removing the choice is the feature: the user no longer has to understand VRAM,
download sizes or an accuracy/speed trade to get good dictation.

The privacy note now says OpenVoice has no network path in the dictation flow,
which the bundled model finally makes literally true."
```

---

### Task 7: Package the model into the installer

**Files:**
- Create: `crates/ov-app/installer-hooks.nsh`
- Modify: `crates/ov-app/tauri.conf.json`, `.github/workflows/release.yml`, `Cargo.toml`
- Modify: `scripts/check-no-network.sh`

- [ ] **Step 1: Bump the version to 0.5.0**

In `Cargo.toml` (`version.workspace`) and `crates/ov-app/tauri.conf.json`. Not 1.0.0 — this is a rebuilt engine, not a stability claim.

- [ ] **Step 2: Write the NSIS hook**

Create `crates/ov-app/installer-hooks.nsh`:

```nsis
; The speech model ships in the installer so a fresh install dictates offline,
; immediately. It is installed here rather than as a Tauri resource so that it
; is NOT part of the updater payload: the model never changes between app
; releases, and bundling it into the update channel would turn every patch into
; a 550 MB download for every user.

!macro NSIS_HOOK_POSTINSTALL
  ; Skip the copy when this exact version is already present, so a repair or a
  ; re-install does not rewrite 631 MB for nothing.
  IfFileExists "$INSTDIR\models\parakeet-tdt-0.6b-v2\tokens.txt" model_present 0
    SetOutPath "$INSTDIR\models\parakeet-tdt-0.6b-v2"
    File "${MODEL_SOURCE_DIR}\encoder.int8.onnx"
    File "${MODEL_SOURCE_DIR}\decoder.int8.onnx"
    File "${MODEL_SOURCE_DIR}\joiner.int8.onnx"
    File "${MODEL_SOURCE_DIR}\tokens.txt"
  model_present:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Uninstalling must not strand 631 MB on someone's disk.
  RMDir /r "$INSTDIR\models"
!macroend
```

Register it in `tauri.conf.json` under `bundle.windows.nsis.installerHooks`. Note that `bundle.resources` must **not** gain a model entry — that is the option-A layout this hook exists to avoid.

- [ ] **Step 3: Rework the release workflow**

In `.github/workflows/release.yml`, delete the `setup-python`, `astral-sh/setup-uv`, "Create the sidecar environment" and "Freeze the sidecar" steps. Add before the Tauri build:

```yaml
      - name: Fetch and verify the speech model
        run: pwsh scripts/fetch-model.ps1
```

- [ ] **Step 4: Guard the updater payload size**

Add a release step asserting the updater artifact stayed slim. Without this the option-B decision silently decays into option A the first time someone adds the model to `bundle.resources`:

```yaml
      - name: The updater artifact must not contain the model
        shell: bash
        run: |
          art=$(find target/release/bundle -name '*.nsis.zip' | head -1)
          size=$(stat -c%s "$art")
          echo "updater artifact: $art ($((size/1024/1024)) MB)"
          if [ "$size" -gt 104857600 ]; then
            echo "::error::Updater artifact is $((size/1024/1024)) MB, over the 100 MB ceiling."
            echo "The model has leaked into the update channel; see design section 7.2."
            exit 1
          fi
```

- [ ] **Step 5: Extend the no-network check**

`scripts/check-no-network.sh` should now be able to assert something stronger: no HTTP client reachable from the dictation path. Update it to match the new claim in the privacy copy.

- [ ] **Step 6: Build the installer**

```bash
pwsh scripts/fetch-model.ps1
npm run build:ui
cargo tauri build
```

Expected: an NSIS installer around 550 MB, and a `.nsis.zip` updater artifact under 100 MB.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "build(dist): ship the model in the installer, keep updates small

The model installs via an NSIS hook rather than as a Tauri resource, so it is
absent from the updater payload: a fresh install works offline, and patch
updates stay ~30 MB instead of ~550 MB.

CI now fails if the updater artifact exceeds 100 MB, because that ceiling is
the only thing standing between this design and quietly re-bundling 631 MB
into every update."
```

---

### Task 8: Verify on a clean machine, then document

**Files:**
- Modify: `docs/adr/0003-asr-backend.md`
- Create: `docs/adr/0008-parakeet-in-process.md`
- Modify: `CHANGELOG.md`, `README.md`

- [ ] **Step 1: Install from the built installer**

Uninstall any existing copy. Install from the artifact. Confirm `%LOCALAPPDATA%` has no leftover Whisper models and no `openvoice-asr.exe` exists anywhere.

- [ ] **Step 2: The offline test**

**Disconnect the network.** Launch the app. Dictate. This is the strongest proof that bundling worked and the download manager is genuinely gone.

Expected: dictation works with no network.

- [ ] **Step 3: Confirm the process count**

Task Manager: exactly one OpenVoice process. No Python, no sidecar.

- [ ] **Step 4: Mark ADR 0003 superseded**

Change its status to `Superseded by ADR 0008` and add a closing outcome section noting that its own recorded follow-up — remove Python before distribution — is what finally happened, and why it became cheap.

- [ ] **Step 5: Write ADR 0008**

Record the decision, the measured numbers from design §2, the verified build facts from §3.3, and the costs from §8 and §10 — including the lost dictionary biasing and the lost process isolation. An ADR that records only the upside is not worth writing.

- [ ] **Step 6: Update CHANGELOG and README**

Lead the 0.5.0 entry with what the user gets: faster and markedly more accurate dictation, works offline the moment it is installed, no model to choose. Then state the removals plainly: English only, no model picker, no language setting.

- [ ] **Step 7: Commit and tag**

```bash
git add -A
git commit -m "docs: record the move to Parakeet in ADR 0008 and supersede 0003"
git tag v0.5.0
```

---

## Self-Review

**Spec coverage.** §3 architecture → Tasks 2–3. §4 deletions → Tasks 5–6. §5.1 wrapper → Task 2. §5.2 resolution → Task 3. §6 UI/UX → Task 6. §7 packaging incl. the §7.2 option-B decision → Task 7. §8 parity → Tasks 2, 6. §9 testing → Tasks 2, 3, 7 step 4, 8. §10 risks → Task 4 step 3 (validation), Task 7 step 4 (updater ceiling). §11 gate → Task 4 step 3. §12 phases → task order. §13 out of scope → nothing planned for hotwords, v3, GPU, streaming. **No gaps.**

**Placeholders.** One deliberate: `$Sha256 = 'PLACEHOLDER_REPLACED_IN_STEP_4'` in Task 1, which exists so Step 3 can prove the check fails before Step 4 supplies the real value. Every other step carries its actual content.

**Type consistency.** `ParakeetTranscriber::new(PathBuf) -> Result<Self>` (Task 2) matches its call in Task 3. `MODEL_ID` is `"parakeet-tdt-0.6b-v2"` in Task 2 and asserted with that literal in the same task; `MODEL_DIR_NAME` in Task 3 is the same string used by the NSIS path in Task 7 and the resolver's candidates. `model_dir()` is `locate::model_dir` throughout.

**Known follow-up, deliberately unplanned:** hotword biasing (§8.1) needs its own spike — the Python path segfaulted — and must not be enabled casually now that a native fault takes the whole app down.
