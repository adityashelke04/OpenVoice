<div align="center">

# OpenVoice

**Local-first voice dictation.**
Hold a key, speak, release — correctly formatted text appears at your cursor, in any app.
No cloud, no account, no telemetry.

[![Release](https://img.shields.io/github/v/release/adityashelke04/OpenVoice?include_prereleases&label=release&color=44D62C)](https://github.com/adityashelke04/OpenVoice/releases)
[![CI](https://github.com/adityashelke04/OpenVoice/actions/workflows/ci.yml/badge.svg)](https://github.com/adityashelke04/OpenVoice/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
![Status](https://img.shields.io/badge/status-alpha-orange)
[![Website](https://img.shields.io/badge/website-openvoice--dictation.vercel.app-44D62C)](https://openvoice-dictation.vercel.app)

**[openvoice-dictation.vercel.app](https://openvoice-dictation.vercel.app)**

**[⬇ Download for Windows](https://github.com/adityashelke04/OpenVoice/releases/download/download/OpenVoice-x64-setup.exe)** — one `.exe`, speech engine included.
[All releases →](https://github.com/adityashelke04/OpenVoice/releases) · [Docs](https://openvoice-dictation.vercel.app/docs) · [Changelog](https://openvoice-dictation.vercel.app/changelog)

<img src="docs/images/flow-bar.png" width="420" alt="The Flow Bar: a small floating pill reading &quot;Hold Right Ctrl&quot;">

</div>

> **Status: alpha.** It runs end to end — hotkey, capture, transcription,
> formatting, injection, and history all work on Windows, and `v0.4.2` is
> published as a pre-release you can install today. What is not done is
> distribution polish: nothing is code-signed, so Windows will warn you about an
> unknown publisher, and the bundled speech engine is CPU-only.

---

<div align="center">
<img src="docs/images/hub-home.png" width="880" alt="OpenVoice home screen: 9,540 words dictated, a speaking speed of 152 words per minute charted against a 40 wpm typing baseline and 150 wpm average speech, 2 hours 56 minutes saved, a 6 day streak, most-used app VS Code, and a list of recent dictations">
<br>
<sub>Home. Your speaking speed is measured from how long you actually spoke, not
from wall-clock time, and placed against the 40 wpm typing baseline — because the
alternative to dictating is typing, not silence. The figures here are fixture
data; see <a href="#design">Design</a>.</sub>
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

Both of those are real output from `ov format`, not an illustration — they are
asserted as tests in [`crates/ov-format/src/lib.rs`](crates/ov-format/src/lib.rs).
You can run the formatter yourself without a microphone, a GPU, or the GUI:

```sh
cargo run -p ov-cli -- format "um so we need to call use effect here comma then return null"
cargo run -p ov-cli -- format "cube control get pods" --profile terminal --trace
```

It works the same in an email, a chat box, a document or a terminal. What changes
is how it formats: OpenVoice knows which app has focus and applies the rules that
app deserves.

## What it looks like

One window and one floating pill. The window is where you teach it; the pill is
the only part you see while you work.

<div align="center">
<img src="docs/images/hub-advanced.png" width="880" alt="The Advanced screen. A box contains the phrase 'um so we need to call use effect here comma then return null', and beneath it a table shows the sentence after each rule in turn: raw, fillers, dictionary, commands, capitalize. Below that, a Files card gives the log path and the settings folder">
<br>
<sub><b>The formatter, rule by rule.</b> Every stage the sentence passed through
and what it looked like after each one. When a transcript comes out wrong this
turns "the dictation is bad" into the name of the rule that did it — and it is
the same trace <code>ov format --trace</code> prints in a terminal.</sub>
</div>

<br>

<div align="center">
<img src="docs/images/hub-dictionary.png" width="880" alt="The Dictionary screen. A phrase typed into 'Try a phrase' reads 'um so we need to call use effect here comma then return null', and the box below shows the formatted result: 'So we need to call useEffect here, then return null.' Underneath, a table of corrections maps spoken forms such as 'use effect' and 'jason' to written forms useEffect and JSON">
<br>
<sub><b>Dictionary.</b> Type what OpenVoice wrote, and watch the rules run on it.
The corrections below are the ones it ships with; add your own and the box above
changes as you type.</sub>
</div>

<br>

<div align="center">
<img src="docs/images/flow-bar-states.png" width="880" alt="The Flow Bar in six states over a white document: idle showing 'Hold Right Ctrl', listening with a green waveform and a 0:04 timer, working showing 'Writing…', landed, a clipboard fallback reading 'Copied to clipboard — press Ctrl+V', and a failure reading 'No text was produced'">
<br>
<sub><b>The Flow Bar</b>, every state, over a white document — one of the four
backdrops it is reviewed against, because a border that reads on black and
disappears on white is a defect worth catching before you ship it.</sub>
</div>

<details>
<summary><b>Writing style and Settings</b></summary>

<br>

<div align="center">
<img src="docs/images/hub-writing-style.png" width="880" alt="The Writing style screen with tabs for default, Terminals, Code editors, and Messages and documents. The default tab shows toggles for capitalising sentences, ending with a full stop, spoken punctuation and spoken naming styles, and a dropdown for filler word removal set to Light">
<br>
<sub>The same words should look different depending on where they land. Four sets
of rules, and OpenVoice picks one from whichever app has focus — a chat message
gets a capital letter and a full stop, a terminal command gets neither.</sub>
</div>


<br>

<div align="center">
<img src="docs/images/hub-settings.png" width="880" alt="The Settings screen: shortcut set to Right Ctrl, activation set to Hold to talk, microphone on the system default, language on auto-detect, sound feedback on, maximum recording of 2 minutes, and an Updates section explaining that the launch check sends one request carrying no identifier">
<br>
<sub>Nothing here is decorative. Transcript redaction, audio retention, toggle
activation and the recording limit were all controls with nothing behind them
until v0.2.0; each now does what it says.</sub>
</div>

</details>

## Principles

1. **Local by default, network by exception.** OpenVoice makes exactly one kind of
   outbound request, and it is listed here: an **update check you can switch off**
   (Settings → Updates). Nothing else. Dictation itself has no network path at
   all — the speech model ships in the installer, so there is nothing to fetch.
   Every crate that touches your microphone, your transcripts, your keyboard or
   your history is *sealed*: it has no path to an HTTP client, TLS stack or socket
   library anywhere in its dependency graph. That is
   [checked in CI](scripts/check-no-network.sh), not just promised here.

   Two honest caveats. The Tauri shell links `reqwest` transitively, because Tauri
   does, so an HTTP client is present in the binary even though no OpenVoice code
   calls it. And the update check is a genuine exception to "you asked for it" —
   nobody asks to be told about a release. It fetches one signed manifest, carries
   no identifier and no usage data, never installs anything on its own, and makes
   no request at all when turned off. The reasoning, including why it defaults on,
   is in [ADR 0005](docs/adr/0005-in-app-updates.md).

   The CI job names every permitted network dependency in an allow-list, so a new
   one cannot appear quietly — telemetry or a crash uploader still fails the build.
2. **Never lose a word.** If injection fails, the text is still on your clipboard and
   in your history. A failure is recoverable, never a silent drop.
3. **Invisible when idle.** ~60 MB RAM, ~0% CPU. You should never notice it running.
4. **Deterministic where it can be.** Formatting is rule-based and unit-tested.
   Probabilistic parts are isolated behind seams.
5. **Your data is yours.** History is a plain SQLite file at
   `%APPDATA%\OpenVoice\history.db` with a [documented
   schema](crates/ov-store/src/schema.rs) — open it with any SQLite browser, no
   export feature required. (A built-in export is v0.3 work.) Recorded audio is
   not kept unless you ask for it: the only copy that touches disk is a temporary
   WAV handed to the speech engine, deleted the moment the decode returns.
   Turning on *Keep recordings* — off by default, and there to diagnose a
   transcription problem — writes them to `%APPDATA%\OpenVoice\audio` instead,
   where they are cleared after a week. Secrets matching
   `privacy.redact_patterns` are stripped from history and logs either way.

## How it works

```
  Right Ctrl ──► capture ──► Parakeet ──► format ──► inject at caret
   (held)        16 kHz      in-process     rules        SendInput /
                  mono          CPU       pipeline      clipboard
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

```mermaid
flowchart LR
    subgraph driving["Driving adapters — they call in"]
        direction TB
        HK["Keyboard hook<br/>ov-input"]
        MIC["WASAPI capture<br/>ov-audio"]
    end

    subgraph core["Pure domain — no OS, no I/O, no async, no GUI"]
        direction TB
        SM["Session state machine<br/>ov-core"]
        FMT["Formatting pipeline<br/>ov-format"]
        SM --> FMT
    end

    subgraph driven["Driven adapters — the core calls out"]
        direction TB
        ASR["Parakeet, in-process<br/>ov-asr"]
        APP["Foreground app<br/>ov-input"]
        SINK["SendInput / clipboard<br/>ov-input"]
        DB["SQLite + FTS5<br/>ov-store"]
    end

    HK   -- "HotkeyListener" --> SM
    MIC  -- "AudioSource"    --> SM
    SM   -- "Transcriber"    --> ASR
    SM   -- "AppContext"     --> APP
    SM   -- "HistoryStore"   --> DB
    FMT  -- "TextSink"       --> SINK
```

The six labelled edges are the entire contract surface — every trait is
declared in [`ov-core/src/ports.rs`](crates/ov-core/src/ports.rs), and nothing
reaches the operating system except through one of them. Adding a seventh
requires an ADR.


| Crate | Role |
|---|---|
| `ov-core` | Session state machine, the six ports, events, config. **Pure.** |
| `ov-format` | Formatting pipeline, dictionary, voice commands. **Pure.** |
| `ov-audio` | WASAPI capture via cpal; downmix and resample to 16 kHz mono |
| `ov-asr` | Parakeet speech recognition, decoded in this process |
| `ov-input` | Keyboard hook, text injection, foreground app detection |
| `ov-store` | SQLite history with FTS5 search |
| `ov-cli` | `ov` — the same pipeline, headless. The integration harness |
| `ov-app` | `openvoice` — the Tauri shell, and the composition root |

Full design: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md). Decisions and their
rationale: [`docs/adr/`](docs/adr/).

### Measured on the reference machine

RTX 3050 Laptop (4 GB VRAM), Ryzen 5 6600H:

| | |
|---|---:|
| Model load, at startup | ~2.5–3.4 s |
| Decode, ~5 s utterance | ~500 ms median |
| Decode, p90 across clip lengths | ~1.2 s |
| Memory while loaded | ~750 MB |

Measured on the reference machine, CPU only, at four decode threads. Four rather
than all twelve: the extra eight buy about 110 ms and cost the responsiveness of
whatever you are dictating into.

Accuracy is deliberately not quoted as a number here. It was measured — see
[ADR 0008](docs/adr/0008-parakeet-in-process.md) — but on a public benchmark that
is in-domain for this model, so the figure is a ceiling rather than a forecast,
and a number in a README outlives that caveat.

## Requirements

- Windows 10/11 (macOS and Linux planned for v0.5)
- ~700 MB disk: the app plus the speech model, which is included rather than
  downloaded
- No GPU needed. The engine runs on the CPU and takes about 750 MB of memory
  while loaded.
  See the note under [Installing](#installing) about what the installer ships.

Building from source needs considerably more: the Rust toolchain and MSVC build
tools come to roughly 14 GB, plus the model. See
[`CONTRIBUTING.md`](CONTRIBUTING.md) if that is a problem — it can all be
relocated to another drive.

## Installing

Download **[`OpenVoice-x64-setup.exe`](https://github.com/adityashelke04/OpenVoice/releases/download/download/OpenVoice-x64-setup.exe)** and run it. That link always
serves the newest build — it is deliberately not tied to a version number, because
a hand-written one goes stale the moment a release ships without someone
remembering to edit it. The [releases page](https://github.com/adityashelke04/OpenVoice/releases)
has the version history.

Nothing to configure, no Python to install, and no model to download. The speech
engine and its weights are both inside the installer, so OpenVoice works offline
from the moment setup finishes — including the very first sentence. That is why
the installer is around 550 MB: you download those bytes once, while you are
already expecting to wait, rather than on first launch when you are trying to
use the app.

Then hold **Right Ctrl**, speak, and release.

Two things worth knowing before you install:

- **The installer is not code-signed.** Windows SmartScreen will warn about an
  unknown publisher, and you will have to click *More info → Run anyway*. A
  certificate is on the v0.5 roadmap. Because you are being asked to override
  that warning, every release ships a SHA-256 beside the installer and is built
  by a public [GitHub Actions run](.github/workflows/release.yml) from the
  tagged commit — so you can check that what you downloaded is what CI built:

  ```powershell
  Get-FileHash .\OpenVoice-x64-setup.exe -Algorithm SHA256
  ```

  Compare that against the hash in the release notes and the `.sha256` file
  published next to the installer.
- **The bundled speech engine is CPU-only.** The CUDA libraries are 1.9 GB — 88%
  of the dependency tree — and useless without an NVIDIA GPU, so they are not
  shipped. An installed copy runs on the CPU, which works everywhere but is
  markedly slower on the large model. To use the GPU today, run from source.

To uninstall, use Windows *Add or remove programs*; that removes the speech model
along with the app. Your history (`%APPDATA%\OpenVoice\history.db`) and settings
are left behind because they are yours — delete that folder if you want them gone
too.

## Building it yourself

```powershell
# 1. Rust + MSVC build tools (Windows needs the C++ workload, not just VS Code)
winget install Rustlang.Rustup
winget install --id Microsoft.VisualStudio.2022.BuildTools `
  --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# 2. The speech model (~482 MB download, verified against a pinned SHA-256).
#    There is no Python and no virtualenv any more; the engine links into the
#    binary. This only fetches weights.
pwsh scripts/fetch-model.ps1

# 3. Frontend dependencies. This also installs the Tauri CLI used in step 5.
npm --prefix apps/ui ci

# 4. Run the tests. --test-threads=1 because each ov-asr test that touches the
#    model loads ~750 MB, and running them in parallel needs several gigabytes.
cargo test --workspace -- --test-threads=1

# 5. Run it
cd crates/ov-app
node ../../apps/ui/node_modules/@tauri-apps/cli/tauri.js dev
```

The model is found in `models/` in the checkout, or wherever `OPENVOICE_MODEL_DIR`
points. Nothing is downloaded at run time.

That sequence builds the full Windows app. You do not need any of it to work on
the parts that matter most: `ov-core` and `ov-format` are pure Rust and build and
test on any platform, with no MSVC, no GPU, and no microphone.

```sh
cargo test -p ov-core -p ov-format
```

### Producing an installer

The engine links into the binary, so the only extra payload is the model itself:

```powershell
pwsh scripts/fetch-model.ps1                 # ~482 MB, SHA-256 verified
$env:MODEL_SOURCE_DIR = "$PWD\models\parakeet-tdt-0.6b-v2"
cd crates/ov-app
node ../../apps/ui/node_modules/@tauri-apps/cli/tauri.js build
```

`MODEL_SOURCE_DIR` is read by
[`installer-hooks.nsh`](crates/ov-app/installer-hooks.nsh), which installs the
weights into the application directory. They are deliberately *not* a
`bundle.resources` entry: Tauri's updater downloads the whole installer on every
update, so a bundled model would turn every patch release into a ~550 MB download
for every user. Leave it unset and the installer builds without a model — useful
for testing packaging, useless to a user — which is why the [release
workflow](.github/workflows/release.yml) asserts the model is present before
bundling.

Details and troubleshooting: [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Privacy, stated plainly

**OpenVoice installs a global keyboard hook and can synthesize keystrokes.** Those are
the same capabilities a keylogger has. You should be suspicious of any program that
wants them, including this one.

What we do about that:

- The hook stores nothing. It compares a virtual key code against your configured
  chord and discards the event. [Read it yourself](crates/ov-input/).
- Audio lives in RAM. The one exception, stated plainly: the speech engine runs as a
  separate process, and the audio reaches it as a temporary WAV under
  `%TEMP%\openvoice\`, deleted immediately after the decode — success or failure.
  Nothing else writes audio to disk, and there is no retention path today.
- No telemetry, no analytics, no crash uploads — not "off by default", **absent from
  the codebase**, with a CI job that keeps it that way.
- The update check is the one request you did not individually ask for. It fetches a
  single signed manifest, sends no identifier and no usage data, and never installs
  anything without you pressing a button. Turning it off in Settings → Updates means
  no request is made — not that one is made with a flag attached.
- Releases are built by public GitHub Actions from a tagged commit, with a
  SHA-256 published beside the installer so you can verify what you downloaded.
  Every release is also signed with a minisign key whose public half is compiled
  into the app, and an update that fails that check is discarded without running.
  The [build log](https://github.com/adityashelke04/OpenVoice/actions/workflows/release.yml)
  for every release is public.

Found something wrong? [`SECURITY.md`](SECURITY.md).

## Roadmap

| | | |
|---|---|---|
| **v0.1** | Walking skeleton | hotkey → capture → transcribe → inject → history ✅ |
| **v0.2** | The differentiator | formatting pipeline, dictionary, app profiles, settings UI ✅ |
| **v0.3** | Feel | overlay waveform ✅, sound feedback ✅, history search ✅ — still to do: streaming partials, history export, sub-700 ms p50 |
| **v0.4** | Intelligence | optional local LLM polish, prompt mode for AI agents |
| **v0.5** | Distribution | published Windows installer ✅, auto-update ✅ — still to do: code signing, optional GPU pack, macOS |
| **v1.0** | Stability | plugin API for formatter rules, frozen API |

Every phase ships something usable. None is a refactor-only phase.

Nothing in the settings is decorative any more. Transcript redaction, audio
retention, toggle activation and the maximum-recording limit were all fields or
toggles with no implementation behind them until v0.2.0; each now does what it
says.

## Design

One accent colour, `#44D62C`, and it means exactly one thing: the microphone is
open. Never success, never links, never emphasis. The surface ladder spans 28 hex
points end to end, because a dark UI that jumps `#111 → #222 → #333` is the
clearest marker of a system nobody thought about.

<div align="center">
<img src="docs/images/design-system.png" width="880" alt="The OpenVoice design system sheet: the surface and text ladders with their contrast ratios, the single accent colour, and the type scale">
</div>

The whole system is a live page rather than a document. It needs no Rust and no
Windows — the frontend runs standalone in any browser:

```sh
npm --prefix apps/ui ci
npm run dev:ui          # then open http://localhost:5199/?window=sheet
```

`?window=hub` is the main window, `?window=overlay` is the Flow Bar on its own,
and `?window=flowbar` is every Flow Bar state at once, over the four kinds of
surface it has to stay legible against.

Every screenshot in this README is captured from that same running UI by
[`scripts/screenshots.mjs`](scripts/screenshots.mjs), so none of them can drift
from the interface it claims to show. What the screens are *reading* is fixture
data from [`scripts/screenshot-fixtures.mjs`](scripts/screenshot-fixtures.mjs) —
the dictionary, profiles and model catalogue copied from the Rust sources they
mirror, the history rows taken from tests in `ov-format`, and one invented
month's worth of totals. Nothing here is a mocked component or a drawing of a
screen: the real `Hub` renders against the real `invoke` boundary, and only the
far side of it is canned. That is also why these are safe to recapture — the
previous versions came off a live machine and published whatever had last been
dictated into it.

## Contributing

The pure crates are the easiest place to start, and they are also where the
interesting work is. `ov-core` and `ov-format` need no Windows, no GPU, no
microphone and no MSVC — `cargo test -p ov-core -p ov-format` is the whole setup. A
new formatting rule is about thirty lines plus two tests: one that would fail
without it, and one proving it does not fire when it shouldn't.

Good first things to look at: a missing term in
[`dictionary.rs`](crates/ov-format/src/dictionary.rs) (the model mistranscribes
some tool name you use — add what it actually hears), a voice command in
[`rules.rs`](crates/ov-format/src/rules.rs), or an app profile for an editor nobody
has covered yet. Setup, the boundary rules, and how to write a rule that behaves:
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## Prior art

[Wispr Flow](https://wisprflow.ai) and [superwhisper](https://superwhisper.com) are
excellent and worth paying for. OpenVoice exists because I wanted something local,
open, and tuned specifically for writing code and prompting agents.

Built on [Parakeet TDT](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2) from
NVIDIA NeMo, running on [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) from
k2-fsa. Earlier releases used
[faster-whisper](https://github.com/SYSTRAN/faster-whisper) and
[Whisper](https://github.com/openai/whisper); see
[ADR 0008](docs/adr/0008-parakeet-in-process.md) for why that changed.

> **Name note:** [MyShell's OpenVoice](https://github.com/myshell-ai/OpenVoice) is an
> unrelated and well-known text-to-speech project. No affiliation. A rename is under
> consideration.

## License

[Apache-2.0](LICENSE).
