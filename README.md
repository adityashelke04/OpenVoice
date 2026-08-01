<div align="center">

# OpenVoice

**Local-first voice dictation for developers.**
Hold a key, speak, release — correctly formatted text appears at your cursor, in any app.
No cloud, no account, no telemetry.

[![CI](https://github.com/adityashelke04/OpenVoice/actions/workflows/ci.yml/badge.svg)](https://github.com/adityashelke04/OpenVoice/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
![Status](https://img.shields.io/badge/status-pre--alpha-orange)

</div>

> **Status: pre-alpha, under active construction.** The architecture, the pure crates
> (`ov-core`, `ov-format`), and the ASR sidecar are in place. The adapters, the Tauri
> shell, and the UI are being built. It does not run end to end yet.

---

## Why

Whisper is excellent at transcription and useless at writing code. Ask it to
transcribe a developer and it produces `use effect` for `useEffect`, `cube control`
for `kubectl`, the literal words "open parenthesis", and `Git status.` — capitalized,
punctuated, and unrunnable.

Those aren't model bugs. They're accurate transcriptions of what you actually said.
Fixing them is a text-processing problem, and text processing is deterministic,
testable, and improvable one rule at a time.

**So OpenVoice treats the speech model as a swappable commodity and puts all its
effort into what happens next.** That's the whole bet.

```
you say:   "um so we need to call use effect here comma then return null"
you get:   "So we need to call useEffect here, then return null"

you say:   "cube control get pods"          (in a terminal)
you get:   "kubectl get pods"               (no capital, no period — it runs)
```

## Principles

1. **Local by default, network by exception.** The only outbound request the app can
   make is a model download you asked for. This is [enforced in
   CI](scripts/check-no-network.sh), not just promised here.
2. **Never lose a word.** If injection fails, the text is still on your clipboard and
   in your history. A failure is recoverable, never a silent drop.
3. **Invisible when idle.** ~60 MB RAM, ~0% CPU. You should never notice it running.
4. **Deterministic where it can be.** Formatting is rule-based and unit-tested.
   Probabilistic parts are isolated behind seams.
5. **Your data is yours.** Plain SQLite, documented schema, one-click export. Audio is
   never written to disk unless you explicitly turn that on.

## How it works

```
  Right Ctrl ──► capture ──► Whisper ──► format ──► inject at caret
   (held)        16 kHz       local       rules        SendInput /
                  mono         GPU       pipeline      clipboard
```

Built as [ports and adapters](docs/adr/0001-hexagonal-architecture.md): the domain
core has no dependency on the OS, audio, the GPU, or the GUI. Everything
platform-specific sits behind one of six traits.

That isn't architecture for its own sake. It means the formatting rules — the part
that actually determines whether this tool is good — can be tested in milliseconds
with no microphone, no GPU, and no window manager. A rule change is verified in a
second instead of by launching a GUI and talking to it.

The boundary is enforced mechanically: CI compiles the core crates for
`wasm32-unknown-unknown`, a target where none of those dependencies can link.

| Crate | Role |
|---|---|
| `ov-core` | Session state machine, ports, events, config. **Pure.** |
| `ov-format` | Formatting pipeline, dictionary, voice commands. **Pure.** |
| `ov-audio` | WASAPI capture, resampling, ring buffer, VAD |
| `ov-asr` | Transcriber implementations, model manager |
| `ov-input` | Keyboard hook, text injection, foreground app detection |
| `ov-store` | SQLite history |
| `ov-app` | Tauri shell — the composition root |
| `sidecar/` | faster-whisper, as a supervised child process |

Full design: [`docs/DESIGN.md`](docs/DESIGN.md). Decisions and their rationale:
[`docs/adr/`](docs/adr/).

### Measured on the reference machine

RTX 3050 Laptop (4 GB VRAM), Ryzen 5 6600H:

| | |
|---|---:|
| Model load, warm cache | 1.4 s |
| Decode, ~5 s utterance, `base.en` | ~190–600 ms |
| Realtime factor | ~27× |

Two findings from bringing this up, both now handled in code and worth knowing if
you hit them elsewhere:

- **pip-installed CUDA libraries aren't on the Windows DLL search path.** ctranslate2
  reports a healthy GPU, loads the model, then fails at the *first decode* with
  `Library cublas64_12.dll is not found`. `engine.register_cuda_dll_dirs()` fixes it.
- **`huggingface_hub` revalidates cached models over the network**, and blocked for
  **171 seconds** per load before falling back to the cache it already had. Offline
  mode is now the default; downloads are the model manager's job.

## Requirements

- Windows 10/11 (macOS and Linux planned for v0.5)
- A GPU with ≥2 GB VRAM for the default model, or CPU for the small ones
- ~3 GB disk for the toolchain and weights

## Building from source

```sh
# 1. Rust + MSVC build tools (Windows needs the C++ workload, not just VS Code)
winget install Rustlang.Rustup
winget install --id Microsoft.VisualStudio.2022.BuildTools \
  --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# 2. The ASR sidecar
uv venv && uv pip install -e sidecar nvidia-cublas-cu12 nvidia-cudnn-cu12

# 3. Build and test
cargo test --workspace
```

Details and troubleshooting: [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Privacy, stated plainly

**OpenVoice installs a global keyboard hook and can synthesize keystrokes.** Those are
the same capabilities a keylogger has. You should be suspicious of any program that
wants them, including this one.

What we do about that:

- The hook stores nothing. It compares a virtual key code against your configured
  chord and discards the event. [Read it yourself](crates/ov-input/).
- Audio lives in RAM and is dropped after transcription unless you enable retention.
- No telemetry, no analytics, no crash uploads — not "off by default", **absent from
  the codebase**, with a CI job that keeps it that way.
- Releases are built in public CI with published checksums.

Found something wrong? [`SECURITY.md`](SECURITY.md).

## Roadmap

| | | |
|---|---|---|
| **v0.1** | Walking skeleton | hotkey → capture → transcribe → inject → history |
| **v0.2** | The differentiator | formatting pipeline, dictionary, app profiles, settings UI |
| **v0.3** | Feel | overlay waveform, streaming partials, sub-700 ms p50 |
| **v0.4** | Intelligence | optional local LLM polish, prompt mode for AI agents |
| **v0.5** | Distribution | signed installer, auto-update, macOS |
| **v1.0** | Stability | plugin API for formatter rules, frozen API |

Every phase ships something usable. None is a refactor-only phase.

## Contributing

The pure crates are the easiest place to start — they need no hardware and no GPU,
and a new formatting rule is about 30 lines plus a test. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## Prior art

[Wispr Flow](https://wisprflow.ai) and [superwhisper](https://superwhisper.com) are
excellent and worth paying for. OpenVoice exists because I wanted something local,
open, and tuned specifically for writing code and prompting agents.

Built on [faster-whisper](https://github.com/SYSTRAN/faster-whisper),
[CTranslate2](https://github.com/OpenNMT/CTranslate2), and
[Whisper](https://github.com/openai/whisper).

> **Name note:** [MyShell's OpenVoice](https://github.com/myshell-ai/OpenVoice) is an
> unrelated and well-known text-to-speech project. No affiliation. A rename is under
> consideration.

## License

[Apache-2.0](LICENSE).
