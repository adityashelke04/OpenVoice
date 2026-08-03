# ADR 0003 — faster-whisper in a Python sidecar as the day-one ASR backend

- **Status:** Accepted
- **Date:** 2026-07-31

## Context

The reference machine has an RTX 3050 Laptop with **4 GB VRAM**. That number
constrains the whole ASR design: it comfortably fits `large-v3-turbo` quantized
(~1.6 GB resident) but leaves no room to co-resident a cleanup LLM.

Two viable local runtimes:

- **faster-whisper** (CTranslate2). CUDA works from a pip wheel. No compiler needed.
  Excellent throughput, ~15–25× realtime for `large-v3-turbo` int8_float16 here.
- **whisper.cpp** via `whisper-rs`. Links in-process, single self-contained binary,
  but GPU builds on Windows require the CUDA Toolkit (~3 GB on top of MSVC) or a
  Vulkan build, or shipping a prebuilt third-party binary.

## Decision

Ship `FasterWhisperSidecar` as the v0.1 `Transcriber` implementation: a long-lived
Python child process managed by `ov-asr`, speaking newline-delimited JSON over
stdin/stdout, with audio passed as a shared-memory handle or a temp WAV.

The process is supervised: health-checked, restarted with backoff on crash, and
killed on parent exit via a Windows job object so it can never orphan.

## Rationale

Toolchain friction was already high (MSVC + Windows SDK). Adding the CUDA Toolkit
before the first working build risks the project stalling in setup. faster-whisper
gets GPU transcription working with `uv add faster-whisper` and no compiler.

This decision is deliberately cheap to reverse. `Transcriber` is one trait with three
methods; a `WhisperCpp` implementation can land later and be selected by config with
no change to `ov-core`, `ov-format`, or the UI. That reversibility is why it was
acceptable to optimize for speed-to-first-build here.

## Consequences

**Good.** Best accuracy-per-latency available today with zero build friction.
Model experimentation is a Python-side change. Process isolation means an ASR crash
degrades rather than kills the app.

**Costs.** A Python runtime must be bundled for distribution — the real price, paid
at v0.5, not now. IPC adds ~5–15 ms per utterance, negligible against a ~500 ms
decode. Two runtimes to supervise.

**Follow-up.** Before v0.5 (distribution), revisit whisper.cpp in-process to remove
the Python dependency from installers. Log as a tracked issue, not a vague intention.

### Outcome (2026-08-01): the bundling cost, measured

The cost this ADR deferred has now been paid, and it came in cheaper than feared
— but only after being split in two.

PyInstaller freezes the sidecar into a **240 MB** folder with no Python on the
target machine (`scripts/build-sidecar.ps1`, `sidecar/openvoice-asr.spec`), which
`tauri build` bundles as a resource. (Since reduced to ~173 MB by excluding PyAV,
whose only purpose was faster-whisper's `decode_audio` — a path this sidecar does
not use, because `engine.py` reads its own WAV files. The resulting installer is
68 MB.) `SidecarConfig::bundled` runs it directly;
`SidecarConfig::dev` still runs the Python package from a checkout, and a debug
build prefers the checkout so sidecar edits are not shadowed by a stale freeze.

The number that decided the shape of this was the dependency breakdown:

| | |
|---|---:|
| `nvidia-cublas-cu12` + `nvidia-cudnn-cu12` + nvrtc | 1985 MB |
| ctranslate2, onnxruntime, PyAV, numpy, tokenizers, everything else | 263 MB |

CUDA is 88% of the tree and does nothing without an NVIDIA GPU, so it is excluded
from the freeze and the shipped engine runs on CPU. `engine.py` picks the
libraries up from `OPENVOICE_CUDA_DIR` when they are present by another route.

This weakens rather than strengthens the case for whisper.cpp: the remaining
Python-specific weight is a fraction of the CUDA runtime that *any* GPU backend
would need. The open question is no longer "how do we ship Python" but "how do we
ship CUDA to the people who can use it", and that is the same question either way.

## Model plan

| Model | Disk | VRAM | Role |
|---|---:|---:|---|
| `base.en` int8 | ~75 MB | CPU-ok | **Installed default** — see 2026-08-02 below |
| `small.en` int8 | ~250 MB | ~0.6 GB | Low-power / battery profile |
| `large-v3-turbo` int8_float16 | ~1.6 GB | ~1.6 GB | Best accuracy, opt-in upgrade |

> **Correction (2026-08-03).** This section originally ended "all downloads are
> SHA-256 verified against a manifest committed to the repo." No such manifest was
> ever written and no independent hash check exists. What actually happens:
> `huggingface_hub` fetches the weights inside the sidecar, with whatever integrity
> checking it performs on its own transfers, and the compute type is chosen by
> preset with a fallback — `float16` first, `int8_float16` only if the larger
> weights will not fit. The manifest remains worth building; recorded here as a
> known gap rather than left as a false claim.

### Outcome (2026-08-02): the default was the wrong model for the installed engine

`large-v3-turbo` was written up here as the default before v0.5 (distribution)
had a shape, on the assumption of a GPU. The 2026-08-01 outcome above settled
that shape: the installed engine is CPU-only. Pairing the heaviest model with
the slowest path meant a fresh install downloaded 1.6 GB to run the model worst
suited to the hardware it would run on.

`crates/ov-app/src/settings.rs` now defaults fresh installs to `base.en`
(~75 MB). Total footprint for someone who never opens Models is ~325 MB rather
than ~1.9 GB. Anyone who wants more accuracy upgrades from the Models screen,
which already downloads on demand — this changes only what a first run gets
before anyone has chosen anything.
