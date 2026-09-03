# Design — Parakeet as the only speech model, in-process

- **Date:** 2026-09-03
- **Status:** Approved for implementation
- **Supersedes:** ADR 0003 (faster-whisper in a Python sidecar)
- **Target release:** v0.5.0

## 1. What this changes, in one paragraph

OpenVoice stops shipping Whisper and stops shipping Python. Speech recognition
becomes NVIDIA Parakeet TDT 0.6B v2, running in-process inside the Rust binary
via the official `sherpa-onnx` crate. The model ships inside the installer, so a
fresh install dictates immediately with no download and no network. The model
picker, the download manager, the language setting, the confidence score, the
hallucination-rejection layer, the CUDA discovery code and the entire
`sidecar/` tree are deleted, not ported.

## 2. Why — the measured case

Benchmarked on this machine, CPU only, 25 dictation-length clips (1.9–11.5 s)
from LibriSpeech `test-clean`, decoded through OpenVoice's real `Engine` for the
Whisper rows. Full method in §11.

| model | median | p90 | WER | disk |
|---|---:|---:|---:|---:|
| `base.en` ("Fastest", today's default) | 879 ms | 1738 ms | 13.8% | 148 MB |
| `small.en` ("Light") | 2706 ms | 3097 ms | 6.8% | 486 MB |
| **Parakeet TDT 0.6B v2 int8** | **535 ms** | **1169 ms** | **1.9%** | 631 MB |

Parakeet is simultaneously faster than the fastest Whisper tier and more
accurate than the most accurate one. There is no trade to manage, which is why
a single-model catalogue is defensible where it would otherwise be a
regression.

Two secondary findings carried real design weight:

- **Silence returns `""`.** Digital silence, −50 dBFS room tone and −34 dBFS
  hiss each produced empty output. Whisper's tendency to invent words from room
  tone is the reason `engine.py` carries a VAD, a `no_speech_prob` gate, an
  `avg_logprob` gate and a `duration_after_vad` gate. Parakeet does not have the
  failure, so the defences are deleted rather than reimplemented.
- **The rejection layer was not flattering Whisper.** Zero segments were dropped
  across all 25 clips, so 13.8% is genuinely `base.en`'s error rate on this set,
  not an artefact of our filtering.

### 2.1 The honest limitation

LibriSpeech `test-clean` is in-domain for Parakeet — NVIDIA trained on Granary,
which includes it — and it is clean read speech, not technical dictation over a
laptop fan. **1.9% is a ceiling, not a forecast.** The direction of the result is
robust (a 7× gap does not come from domain overlap alone), but the absolute
number should not be quoted in user-facing copy. §11 defines the real-voice
validation that must run before v0.5.0 ships.

## 3. Architecture

### 3.1 Today

```
ov-app ──> ov-asr::SidecarTranscriber
             │  spawn + supervise + Windows job object (job.rs)
             │  newline-delimited JSON over stdin/stdout (protocol.py)
             ▼
           openvoice-asr.exe   (181 MB PyInstaller freeze)
             └─ faster-whisper ──> CTranslate2 ──> model.bin from HF cache
```

Four moving parts to keep alive: a child process, an IPC protocol, a job object
to stop orphaning, and a Hugging Face download manager.

### 3.2 After

```
ov-app ──> ov-asr::ParakeetTranscriber
             └─ sherpa-onnx (static, in-process) ──> *.onnx beside the binary
```

One moving part. No process, no IPC, no supervision, no orphan risk, no
download manager.

### 3.3 Why this is now possible, when ADR 0003 said it was not

ADR 0003 chose a Python sidecar because the in-process alternative
(whisper.cpp via `whisper-rs`) required the CUDA Toolkit or a hand-built binary
on Windows, and it recorded "revisit in-process before v0.5 (distribution)" as
explicit follow-up. That follow-up is now cheap for a reason the ADR could not
have known: **k2-fsa publishes official Rust bindings** (`sherpa-onnx` 1.13.7,
crates.io, updated 2026-09-01), whose `sherpa-onnx-sys` build script downloads
prebuilt static libraries rather than compiling C++.

Verified on this machine, not assumed:

- `cargo build` succeeded with no CMake, no CUDA Toolkit, and no manual setup.
- The release binary is **18.9 MB with zero DLLs beside it** — statically
  linked. It ran correctly from an unrelated working directory.
- End-to-end Parakeet decode in Rust: **509 ms median over the same 25 clips**,
  marginally faster than the Python path.

The 1.1 GB of prebuilt libraries the `-sys` crate fetches is build-time only and
never reaches a user.

### 3.4 The port `ov-core::Transcriber` stays untouched

```rust
pub trait Transcriber: Send + Sync {
    fn warm(&self) -> Result<()>;
    fn transcribe(&self, audio: &Pcm16k, hint: &DecodeHint) -> Result<Transcript>;
    fn model_id(&self) -> String;
}
```

ADR 0003 predicted this: "a different implementation can land later and be
selected by config with no change to `ov-core`, `ov-format`, or the UI." That
prediction is being cashed in.

**The `Transcriber` port itself does not change, and no crate behind it changes
shape.** `ov-format`, `ov-audio`, `ov-input` and `ov-store` are untouched.
`ov-core` keeps every public type exactly as it is — including
`DecodeHint.language` and `Transcript.confidence`, which become unused rather
than deleted (§4). Persisted and versioned types are not worth a migration to
tidy. The hexagonal boundary from ADR 0001 is what makes a swap this large a
contained one, and honouring it means resisting the urge to renovate behind it.

## 4. What gets deleted

Deletion is the majority of this work and the majority of its value.

| Deleted | Size / lines | Why it has no job |
|---|---|---|
| `sidecar/` entire tree | ~181 MB frozen | No Python at runtime |
| `crates/ov-asr/src/job.rs` | 84 lines | No child process to contain |
| Sidecar spawn/IPC in `lib.rs` | ~500 of 762 lines | In-process call |
| `crates/ov-asr/src/store.rs` | 338 lines | Model ships with the app |
| `catalog.rs` multi-model machinery | ~180 of 236 | One model |
| CUDA DLL discovery (`engine.py`) | ~90 lines | Static CPU build |
| VAD + `no_speech`/`logprob` gates | ~60 lines | Parakeet does not hallucinate |
| `scripts/build-sidecar.ps1` | whole file | Nothing to freeze |
| PyInstaller + `uv` steps in `release.yml` | ~30 lines | No Python in CI |
| Models screen (`Settings.tsx`) | ~170 lines | Nothing to choose |
| `MODEL_COPY`, `formatSize`, download IPC | ~60 lines | Nothing to download |
| `config.language` **UI only** | picker + copy | Parakeet v2 is English-only |
| `Transcript.confidence` **display only** | history column | Transducers emit no logprob |

**The last two rows are deliberately not deleted from `ov-core`.** `Config` is
versioned and `Transcript` is persisted, so removing either field would force a
settings migration and a history-schema migration on live user data — a
destructive change bought for nothing, since an unused field costs a few bytes.
`config.language` stays in the struct and is ignored; `Transcript.confidence`
stays and is always `None`. Both disappear from the UI. If they are ever needed
again — Parakeet v3 is multilingual (§10) — they are still there.

**Net effect: the application gets smaller and structurally simpler.** That is
the headline, not the model swap.

## 5. What gets built

### 5.1 `ov-asr` becomes a thin wrapper

New `crates/ov-asr/src/lib.rs`, roughly 150 lines replacing 762:

```rust
pub struct ParakeetTranscriber {
    recognizer: OfflineRecognizer,   // Send + Sync per crate docs
    model_dir: PathBuf,
}

impl ParakeetTranscriber {
    pub fn new(model_dir: PathBuf, threads: i32) -> Result<Self>;
}

impl Transcriber for ParakeetTranscriber {
    fn warm(&self) -> Result<()>;                       // no-op; loaded in new()
    fn transcribe(&self, audio: &Pcm16k, hint: &DecodeHint) -> Result<Transcript>;
    fn model_id(&self) -> String;                       // "parakeet-tdt-0.6b-v2"
}
```

Configuration, fixed at construction (no user-facing knobs):

| setting | value | rationale |
|---|---|---|
| `model_type` | `nemo_transducer` | required for this model |
| `num_threads` | 4 | measured: 535 ms at 4 threads vs 645 ms at 12; leaves headroom for a game. Revisit only with a measurement. |
| `decoding_method` | `greedy_search` | 494 ms vs 543 ms for beam; beam earns nothing without hotwords |
| `feature_dim` | 80 | model requirement |

**Load happens once, in `new()`.** Loading costs ~2.5 s and 757 MB resident. The
existing warm-on-start path already covers this; `warm()` becomes a no-op.

### 5.2 Model location and discovery

The model is **not** in a Hugging Face cache and is never downloaded at runtime.
Resolution order, which deliberately satisfies either §7.2 outcome so that
Phases 1–3 are not blocked on that decision:

1. `OPENVOICE_MODEL_DIR` — developer / test override.
2. `<install dir>/resources/models/parakeet-tdt-0.6b-v2/` — §7.2 option A.
3. `<install dir>/models/parakeet-tdt-0.6b-v2/` — §7.2 option B.
4. `<repo root>/models/parakeet-tdt-0.6b-v2/` — checkout, for `cargo run`.

Probing both installed locations costs two `stat` calls at startup and removes
the coupling between engine code and the packaging decision.

If none resolve, the app must fail with a specific, actionable error naming the
expected path — never a spinner. This is the single most likely packaging bug,
so it gets an explicit test (§9).

### 5.3 Threading and cancellation

`transcribe` is blocking and already called off the UI thread. `OfflineRecognizer`
is `Send + Sync`, so one instance is shared behind the existing `Arc`. No new
concurrency primitives. Loss of process isolation is addressed in §10.

## 6. UI/UX

Design principle: **removing a decision is the feature.** The user previously had
to understand VRAM, download sizes and an accuracy/speed trade to get good
dictation. Now there is nothing to get wrong.

### 6.1 The Models screen is deleted, not emptied

A screen listing one permanent, undeletable, already-installed item is a screen
that wastes a click and asks a question with one answer. The nav item goes.

Its one piece of still-true information — what the engine is — moves to a
read-only block at the foot of Settings:

```
Speech engine
Parakeet TDT 0.6B v2 · English · runs on this computer
Typical response 0.5 s · 631 MB installed
```

Rules for this block: no Download button, no Delete button, no progress bar, no
"In use" badge, no model id as a primary label. It is a fact, not a control.

### 6.2 Copy changes

| Location | Now | Becomes |
|---|---|---|
| Models screen lead | "Bigger models understand more, but need more of your graphics card." | *(screen deleted)* |
| Settings header | "'Accurate' and 'Light' rather than `large-v3-turbo`…" | rewritten; the abstraction it explains is gone |
| Settings.tsx:663 | "choose **Light**, because the accurate model needs the card mostly to itself" | deleted — no card, no choice |
| Privacy note | "…except to download a speech model you ask for." | "The code has no way to reach the internet at all." **Now literally true.** |
| Language setting | ISO code picker | deleted; English-only stated once in the engine block |

The privacy line is the quiet win: with the model bundled and the download
manager gone, OpenVoice has no network path in the dictation flow whatsoever.
`scripts/check-no-network.sh` should be extended to enforce it.

### 6.3 Restart semantics

`restart_reasons()` currently diffs the live engine's boot model against the
saved model. With one model that reason disappears; the mechanism stays for the
hotkey and audio-device reasons it also serves. Verify no dead branch is left
asserting a model-change restart that can never occur.

## 7. Packaging and distribution

### 7.1 The installer carries the model

Decided by the product owner: users should not wait on a 482 MB first-run
download. Consequences, stated plainly:

- Installer grows from **68 MB to roughly 550 MB** (631 MB of int8 ONNX, LZMA
  compressed).
- A fresh install works offline, immediately, with no progress bar to abandon.
- The download-progress UI, its IPC commands and its error states all disappear.

### 7.2 The updater problem — decide before implementing

`tauri.conf.json` sets `createUpdaterArtifacts: true`, and the NSIS updater
downloads the **whole installer** on every update. Naively bundling the model
therefore turns every patch release into a ~550 MB download for every user.

Three options; **B is recommended**:

| | Installer | Each update | Cost |
|---|---:|---:|---|
| A. Model as `bundle.resources` | 550 MB | **550 MB** | None — but painful for a project at 0.4.4 shipping frequent patches |
| **B. Model installed to a persistent path by an NSIS hook; updater ships app only** | 550 MB | **~30 MB** | One `installerHooks` `.nsh`; CI publishes a slim updater artifact and a full installer |
| C. First-run download | 68 MB | 30 MB | Rejected by the product owner |

B keeps both properties the owner asked for — offline-ready install *and* a
sane update channel — at the cost of one NSIS hook and a CI step. The model is a
fixed, versioned asset that never changes between app releases, so excluding it
from the app's update channel is correct in principle, not just convenient.

**Decided (2026-09-03): option B.** Concretely:

- The model installs to `<install dir>/models/parakeet-tdt-0.6b-v2/` via an NSIS
  `installerHooks` script, not via `bundle.resources`.
- The hook skips writing the model when a directory of the same version is
  already present, so a re-install or repair does not rewrite 631 MB.
- The uninstall hook removes it, so uninstalling does not strand 631 MB.
- CI publishes two artifacts: the full installer (download page) and a slim
  app-only installer wired to `latest.json` (updater channel).
- `latest.json` must therefore point at the **slim** artifact. Pointing it at the
  full installer silently reintroduces option A's cost, so §9 gains a release
  check asserting the updater artifact is under 100 MB.

### 7.3 CI

`release.yml` loses `setup-python`, `astral-sh/setup-uv`, the venv creation and
`build-sidecar.ps1` — roughly 30 lines and several minutes. It gains a step that
fetches and verifies the model tarball.

**The model download must be checksummed.** ADR 0003 carries a correction
admitting that a claimed SHA-256 manifest never existed. Do not repeat that: pin
the SHA-256 of
`sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2` in the repo and verify it
in CI before bundling. Unverified weights in an installer is a supply-chain
hole, and this time the claim will be backed by code.

## 8. Feature parity — every behaviour audited

| Feature | Today | After | Verdict |
|---|---|---|---|
| Push-to-talk dictation | Whisper | Parakeet | **Better** — 535 ms vs 879 ms, 1.9% vs 13.8% |
| Punctuation / casing | Whisper | native | Equal |
| Silence rejection | VAD + 3 gates | model returns `""` | **Better**, less code |
| Profiles / formatting | `ov-format` | unchanged | Equal |
| Dictionary — post-hoc fix | `ov-format` | unchanged | Equal |
| **Dictionary — decode biasing** | `initial_prompt` | **lost initially** | **Regression** — see §8.1 |
| History, retention, redaction | `ov-store` | unchanged | Equal |
| Confidence in history | `avg_logprob` | none | Lost — cosmetic |
| Language selection | Whisper multilingual | English only | Lost — accepted |
| Model choice | 3 models | 1 | Removed by design |
| Crash isolation | separate process | in-process | **Regression** — see §10 |
| Offline first run | 148 MB download | none | **Better** |

### 8.1 The dictionary regression, and how it is repaid

`engine.py` argues correctly that biasing at decode time beats repairing output
afterwards, because the decoder still has acoustic evidence that
post-processing has thrown away. `build_hint()` packs the dictionary into
Whisper's `initial_prompt`; Parakeet has no equivalent input.

`OfflineRecognizer::create_stream_with_hotwords(&str)` exists in the Rust API
and is the intended replacement. It was **not** validated in the spike — the
Python equivalent needed `modified_beam_search` plus a `bpe.model` file the
sherpa-onnx release does not ship, and passing `modeling_unit` without it
**segfaulted the process**. The Rust signature takes a plain string and may not
share that constraint, but this is unproven.

Decision: **ship v0.5.0 without decode-time biasing.** The dictionary still
works via `ov-format`, and Parakeet's far lower base error rate means fewer
words need correcting in the first place. Hotword support is a follow-up with
its own spike, gated on a crash-safety test — a segfault in-process now takes
the whole app down (§10), so this must not be enabled casually.

## 9. Testing

Existing `ov-core` / `ov-format` / `ov-store` tests must pass untouched. That is
the check that the port boundary held.

New tests in `ov-asr`:

| Test | Asserts |
|---|---|
| `model_dir_resolution_order` | env var beats install dir beats checkout |
| `missing_model_is_a_named_error` | error text contains the expected path — never a hang |
| `decodes_a_known_fixture` | committed WAV → expected text (tolerance-matched) |
| `silence_yields_empty_text` | the §2 finding is locked in as a regression test |
| `short_utterance_under_budget` | a 2 s clip decodes within a generous CI ceiling |
| `model_id_is_stable` | history attribution does not silently change |

`fixtures/audio/` is currently empty. It gets a small committed set: one short
utterance, one longer one, one silence, one room-tone clip.

Rewrite the catalogue tests: `sizes_are_plausible`, `catalog_is_ordered_*`,
`a_fallback_compute_differs_*` and `every_repo_is_fully_qualified` all describe a
multi-model Hugging Face world that no longer exists. Delete them rather than
contort them.

Manual QA before release: install from the built installer on a clean path,
confirm first dictation works with **the network disabled** — the strongest
proof that bundling worked.

## 10. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| **Loss of process isolation** — a native crash in ONNX now kills the app, where a sidecar crash only degraded it | High | Do not enable hotwords (the one path with an observed segfault) without a spike. Keep the audio file on disk so a crash never loses a recording. Watch for panics at the `transcribe` boundary. |
| Real-voice accuracy below the LibriSpeech figure | Medium | §11 gate before release. §2.1 forbids quoting 1.9% in UI copy. |
| 757 MB resident RAM breaks the "runs alongside a game" promise | Medium | Measure on the 4 GB reference laptop under load; revise copy if needed. |
| 550 MB installer deters users | Medium | Product owner's explicit call; §7.2 keeps updates small. |
| `sherpa-onnx-sys` fetches prebuilt libs at build time — network dependency in CI | Low | Pin the exact crate version; `Swatinem/rust-cache` already caches it. |
| Users who need non-English are stranded | Low (owner dictates English) | Parakeet **v3** is 25 languages, same runtime, same size, same code — a three-file swap. The door is left unopened, not locked. |
| No rollback once Whisper is deleted | Medium | §12 sequencing: Parakeet ships and is proven working *before* any deletion lands. |

## 11. Validation gate before release

The spike measured LibriSpeech, which §2.1 establishes is optimistic. Before
v0.5.0 ships:

1. Enable audio retention; record ≥12 real utterances covering the actual
   workload — a commit message, a shell command with flags, a technical term
   from the user's dictionary, ordinary prose.
2. Transcribe with both `base.en` (current shipping default) and Parakeet.
3. Compare by hand. The bar is **not** "beats 1.9%" — it is "clearly better than
   `base.en` on the user's own voice, with no new failure class."
4. Note specifically whether losing decode-time dictionary biasing (§8.1)
   is noticeable on technical terms. If it is, hotwords move from follow-up to
   blocker.

If Parakeet does not clear this bar on real audio, the whole plan is void and
`base.en` stays. Sequencing in §12 keeps that reversal cheap.

## 12. Phases

Ordered so the app is working at every commit, and so the irreversible step
comes last.

**Phase 0 — Validation gate, deferred to the owner.** §11 was written as a
pre-implementation gate. The owner has instead chosen to validate by installing
and dictating with the finished build, which tests the same thing on the same
audio, later. Implementation therefore proceeds without it, and the risk this
accepts is stated honestly: if Parakeet fails §11 on real speech, Phases 1–4 are
wasted. Phases 1 and 2 keep the retreat cheap, and no Whisper code is deleted
until Phase 3.

**Phase 1 — Parakeet works, Whisper untouched.**
Add the `sherpa-onnx` dependency and `ParakeetTranscriber`. Select it behind
`OPENVOICE_ENGINE=parakeet`. Model loaded from `OPENVOICE_MODEL_DIR`. Ship
nothing. At the end of this phase both engines exist and can be A/B'd live.

**Phase 2 — Parakeet becomes the default.**
Flip the default; Whisper still reachable by env var. Dictate through it for
real work. This is the last cheap point of retreat.

**Phase 3 — Delete Whisper.** §4, in this order: `ov-asr` internals → `ov-app`
wiring → UI screens and copy → `sidecar/` tree → CI steps → docs. Each step
compiles and passes tests on its own.

**Phase 4 — Packaging.** §7. Model into the installer, updater decision from
§7.2 applied, CI reworked, checksum pinned. Build the installer.

**Phase 5 — Verification and release.** §9 manual QA including the
network-disabled test. Version to **0.5.0** — a rebuilt engine, not a 1.0
stability claim. Update ADR 0003 to Superseded, add ADR 0008 recording this
decision and the measurements behind it. Rewrite `CHANGELOG.md`.

## 13. Out of scope

- Parakeet v3 / multilingual — the swap is documented, not performed.
- GPU acceleration — CPU is fast enough; CUDA was 88% of the old dependency tree.
- Hotwords / decode-time biasing — §8.1, follow-up with its own spike.
- Streaming / partial results — architectural, unrelated to this change.
- Any 1.0.0 version claim — §12 Phase 5.
