<div align="center">

# OpenVoice

**Local-first voice dictation.**
Hold a key, speak, release — correctly formatted text appears at your cursor, in any app.
No cloud, no account, no telemetry.

[![CI](https://github.com/adityashelke04/OpenVoice/actions/workflows/ci.yml/badge.svg)](https://github.com/adityashelke04/OpenVoice/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
![Status](https://img.shields.io/badge/status-alpha-orange)

<img src="docs/images/flow-bar.png" width="420" alt="The Flow Bar: a small floating pill reading &quot;Hold Right Ctrl&quot;">

</div>

> **Status: alpha.** It runs end to end — hotkey, capture, transcription,
> formatting, injection, and history all work on Windows. What is not done is
> distribution polish: the installer is built by CI but no release is published
> yet, and nothing is code-signed, so Windows will warn you about an unknown
> publisher.

---

<div align="center">
<img src="docs/images/hub-home.png" width="880" alt="OpenVoice home screen showing speaking speed against typing and average speech, time saved, day streak, and recent dictations">
<br>
<sub>Home, on a fresh install. Your speaking speed fills in against the 40 wpm
typing baseline once you have dictated something.</sub>
</div>

---

## Why

Most people speak around 150 words a minute and type around 40. That gap is the
entire reason dictation exists, and the reason almost nobody uses it is that raw
transcription is not writing. It gives you your filler words, no punctuation you
did not say out loud, and no idea which app you were talking into.

Whisper transcribes beautifully and formats nothing. Ask it to transcribe someone
writing code and you get `use effect` for `useEffect`, `cube control` for
`kubectl`, the literal words "open parenthesis", and `Git status.` — capitalized,
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

Both of those are real output from `ov format`, not an illustration.

It works the same in an email, a chat box, a document or a terminal. What changes
is how it formats: OpenVoice knows which app has focus and applies the rules that
app deserves.

## Principles

1. **Local by default, network by exception.** The only outbound request OpenVoice
   makes is a model download you asked for. Every crate that touches your
   microphone, your transcripts, your keyboard or your history is *sealed*: it has
   no path to an HTTP client, TLS stack or socket library anywhere in its
   dependency graph. That is [checked in CI](scripts/check-no-network.sh), not just
   promised here. The one honest caveat: the Tauri shell links `reqwest`
   transitively, because Tauri does, so an HTTP client is present in the binary
   even though no OpenVoice code calls it. The CI job asserts that no OpenVoice
   crate takes a network dependency of its own — telemetry or an update ping
   cannot be added quietly.
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
                  mono      GPU or CPU   pipeline      clipboard
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
| `sidecar/` | faster-whisper, as a supervised child process (frozen for release) |

Full design: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md). Decisions and their
rationale: [`docs/adr/`](docs/adr/).

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
  **171 seconds** per load before falling back to the cache it already had. The
  tell was that the delay repeated to the millisecond — compute does not do that,
  a network timeout does. Offline mode is now the default and is lifted only for
  the duration of a download the user asked for.

## Requirements

- Windows 10/11 (macOS and Linux planned for v0.5)
- ~2 GB disk for the app, plus the model you choose (75 MB to 1.6 GB)
- An NVIDIA GPU with ≥2 GB VRAM is optional but makes the large model practical.
  See the note under [Installing](#installing) about what the installer ships.

## Installing

**No release is published yet.** When one is, it will be a single
`OpenVoice_0.1.0_x64-setup.exe` on the [releases
page](https://github.com/adityashelke04/OpenVoice/releases), built by
[CI](.github/workflows/release.yml) — nothing to configure, no Python to install.

Two things worth knowing before that release exists:

- **The installer is not code-signed.** Windows SmartScreen will warn about an
  unknown publisher. A certificate is on the v0.5 roadmap.
- **The bundled speech engine is CPU-only.** The CUDA libraries are 1.9 GB — 88%
  of the dependency tree — and useless without an NVIDIA GPU, so they are not
  shipped. An installed copy runs on the CPU, which works everywhere but is
  markedly slower on the large model. To use the GPU today, run from source.

## Building it yourself

```sh
# 1. Rust + MSVC build tools (Windows needs the C++ workload, not just VS Code)
winget install Rustlang.Rustup
winget install --id Microsoft.VisualStudio.2022.BuildTools \
  --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# 2. The ASR sidecar. Drop the two nvidia packages for a CPU-only setup.
uv venv && uv pip install -e sidecar nvidia-cublas-cu12 nvidia-cudnn-cu12

# 3. Run the tests
cargo test --workspace

# 4. Run it, against the Python sidecar in your checkout
cd crates/ov-app && node ../../apps/ui/node_modules/@tauri-apps/cli/tauri.js dev
```

### Producing an installer

The installer has to carry a speech engine, because the machine it lands on has
no Python. `build-sidecar.ps1` freezes the sidecar into a standalone folder that
`tauri build` then bundles as a resource:

```powershell
pwsh scripts/build-sidecar.ps1 -Clean       # ~240 MB, verifies itself on the protocol
cd crates/ov-app
node ../../apps/ui/node_modules/@tauri-apps/cli/tauri.js build
```

The freeze must run first. Without it `tauri build` still succeeds — and produces
an installer with no speech engine inside, which only shows up when a user runs
the app. The [release workflow](.github/workflows/release.yml) checks for it
explicitly for that reason.

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
| **v0.1** | Walking skeleton | hotkey → capture → transcribe → inject → history ✅ |
| **v0.2** | The differentiator | formatting pipeline, dictionary, app profiles, settings UI ✅ |
| **v0.3** | Feel | overlay waveform, streaming partials, sub-700 ms p50 |
| **v0.4** | Intelligence | optional local LLM polish, prompt mode for AI agents |
| **v0.5** | Distribution | published release, code signing, optional GPU pack, auto-update, macOS |
| **v1.0** | Stability | plugin API for formatter rules, frozen API |

Every phase ships something usable. None is a refactor-only phase.

## Design

One accent colour, `#44D62C`, and it means exactly one thing: the microphone is
open. Never success, never links, never emphasis. The surface ladder spans 28 hex
points end to end, because a dark UI that jumps `#111 → #222 → #333` is the
clearest marker of a system nobody thought about.

<div align="center">
<img src="docs/images/design-system.png" width="880" alt="The OpenVoice design system sheet: the surface and text ladders with their contrast ratios, the single accent colour, and the type scale">
</div>

The whole system is a live page rather than a document — run
`npm run dev:ui` and open
[`localhost:5199/?window=sheet`](http://localhost:5199/?window=sheet). Screenshots
in this README are captured from the running UI by
[`scripts/screenshots.mjs`](scripts/screenshots.mjs), so they cannot drift from
the interface they claim to show.

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
