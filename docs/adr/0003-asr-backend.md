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

## Model plan

| Model | Disk | VRAM | Role |
|---|---:|---:|---|
| `base.en` int8 | ~75 MB | CPU-ok | First-run smoke test; usable in 30 s |
| `small.en` int8 | ~250 MB | ~0.6 GB | Low-power / battery profile |
| `large-v3-turbo` int8_float16 | ~1.6 GB | ~1.6 GB | **Default** |

All downloads are SHA-256 verified against a manifest committed to the repo.
