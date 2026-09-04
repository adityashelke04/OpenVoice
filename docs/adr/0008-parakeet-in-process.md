# ADR 0008 — Parakeet TDT 0.6B v2, decoded in-process, as the only speech model

- **Status:** Accepted
- **Date:** 2026-09-03
- **Supersedes:** ADR 0003 (faster-whisper in a Python sidecar)

## Context

ADR 0003 chose faster-whisper in a Python sidecar and was explicit that the
choice was cheap to reverse. It recorded a follow-up in as many words: *"Before
v0.5 (distribution), revisit whisper.cpp in-process to remove the Python
dependency from installers. Log as a tracked issue, not a vague intention."*

Two things changed since.

**The models moved.** NVIDIA's Parakeet TDT 0.6B v2 is a FastConformer
transducer, a different architecture from Whisper — CTranslate2 cannot run it at
all, so adopting it was never a catalogue edit.

**The bindings moved.** k2-fsa now publish an official `sherpa-onnx` Rust crate
(1.13.7, updated 2026-09-01), whose `-sys` build script fetches **prebuilt static
libraries**. The toolchain friction ADR 0003 was avoiding — the CUDA Toolkit, a
C++ build, a hand-shipped third-party binary — is simply absent.

## Decision

Replace faster-whisper with Parakeet TDT 0.6B v2, decoding **in this process**
through the `sherpa-onnx` crate. Ship the weights inside the installer. Delete
the Python sidecar, the model catalogue, the download manager and the model
picker rather than porting them.

`ov_core::ports::Transcriber` is unchanged.

## Evidence

Measured on the reference machine, CPU only, 25 dictation-length clips
(1.9–11.5 s) from LibriSpeech `test-clean`. Whisper rows decoded through
OpenVoice's real engine, not an idealised harness, at four threads:

| model | median | p90 | WER | disk |
|---|---:|---:|---:|---:|
| `base.en` (the shipped default) | 879 ms | 1738 ms | 13.8% | 148 MB |
| `small.en` | 2706 ms | 3097 ms | 6.8% | 486 MB |
| **Parakeet TDT 0.6B v2 int8** | **535 ms** | **1169 ms** | **1.9%** | 631 MB |

Parakeet is faster than the fastest Whisper tier *and* more accurate than the
most accurate one. That absence of a trade-off is what makes a single-model
catalogue an improvement rather than a removal of choice.

Verified rather than assumed, before any code was written:

- The crate builds on Windows with no CMake and no CUDA Toolkit.
- The release binary is **18.9 MB with zero DLLs beside it** — statically
  linked, and it runs from an unrelated working directory.
- Decoding the same clips from Rust: **509 ms median**.
- In the real app: loads in 3.4 s, 747 MB resident, **no Python process**.

**Silence returns empty text.** Digital silence, −50 dBFS room tone and −34 dBFS
hiss all produced `""`. Whisper invents words from all three, which is why the
sidecar carried a Silero VAD, a `no_speech_prob` gate and an `avg_logprob` gate.
Those are deleted, not reimplemented, because the failure they defended against
does not occur. `silence_yields_empty_text` pins it.

## Consequences

**Good.** Faster and markedly more accurate dictation. One process instead of
two; no IPC, no job object, no orphan risk, no supervision. `ov-asr` drops from a
1,508-line process supervisor to three small modules, and `unsafe_code` goes from
`allow` to `forbid`. A fresh install works offline immediately. The privacy claim
"no network path in the dictation flow" is finally literally true.

**Costs, stated plainly:**

- **English only.** Parakeet v2 does not do other languages, so the language
  setting is gone from the UI. Parakeet **v3** covers 25 languages on the same
  runtime, same size, same code — a three-file swap if it is ever wanted.
- **The proper-noun decode hint is lost.** Not the dictionary, which still works
  via `ov-format`: specifically `Entry.hint`, the curated subset offered to
  Whisper's `initial_prompt` so that "Claude" is a candidate against "cloud".
  That distinction cannot be repaired downstream once the audio is gone.
  sherpa-onnx hotwords are the replacement and degrade gracefully rather than
  crashing (verified), but need a `bpe.model` the release asset omits. Follow-up.
- **No confidence score.** A transducer emits no per-segment log-probability.
- **Process isolation is gone.** A native fault now takes the app down where a
  sidecar crash only degraded it.
- **A ~440 MB installer**, against 68 MB, and **updates cost the same**.

  This is a correction to an earlier draft of this ADR, which claimed installing
  the model via an NSIS hook rather than as a `bundle.resources` entry kept it
  out of the updater payload. Building it disproved that. The hook embeds the
  weights into the installer executable with `File`, and the updater artifact is
  a zip of that same executable — so the bytes travel either way, and the choice
  between the two mechanisms only decides where they land on disk.

  The hook is still the right place for it, for a different reason: it puts the
  model beside the app where the uninstaller reclaims it, rather than inside
  Tauri's resource tree.

  Genuinely small updates need a second, model-free build published as the
  updater artifact while the full installer serves downloads — plus a tested
  answer for the uninstall hook removing `$INSTDIR\models` during an upgrade's
  uninstall-previous step, which would otherwise leave an updated app with no
  model. That is real work, it is not done, and CI now reports the update size
  rather than asserting a ceiling it cannot meet.
- **A build-time network dependency.** `sherpa-onnx-sys` downloads prebuilt
  libraries during compilation. Nothing network-capable is linked into the
  shipped binary — `scripts/check-no-network.sh` now proves those two claims
  separately — but the build step is real and is recorded here rather than
  waved past.

## The limitation this evidence has

LibriSpeech `test-clean` is **in-domain** for Parakeet: NVIDIA trained on
Granary, which includes it. It is also clean, read audiobook speech, not
technical dictation over a laptop fan. **1.9% is a ceiling, not a forecast**, and
it must not appear in user-facing copy — the UI states a latency and names the
model, and claims no accuracy figure.

The direction is robust; a sevenfold gap does not come from domain overlap alone.
The magnitude is not. Real-voice validation is the owner's acceptance test on the
shipped build.

## What ADR 0003 got right

Worth recording, because it is the reason this change was affordable. ADR 0003
predicted that a replacement backend "can land later and be selected by config
with no change to `ov-core`, `ov-format`, or the UI." A complete engine swap —
different architecture, different runtime, different language, different
packaging — touched nothing behind the port. `ov-core`, `ov-format`, `ov-audio`,
`ov-input` and `ov-store` are byte-identical and their tests pass unmodified.

That is the strongest evidence available that ADR 0001's hexagonal split earned
its ceremony.
