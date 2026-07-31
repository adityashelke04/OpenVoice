# OpenVoice — System Design

> Status: **DRAFT / awaiting approval on open decisions (§14)**
> Owner: @adityashelke04 · Last updated: 2026-07-31
>
> This document is the single source of truth for architecture. It is written to be
> readable by both humans and coding agents: every module has an explicit contract,
> every decision has a rationale, and every open question is listed in §14 rather
> than being silently assumed.

---

## 1. Product definition

**One line:** OpenVoice is a local-first, open-source push-to-talk dictation tool that
turns speech into *correctly formatted developer text* at your cursor, in any
application, with sub-second latency and zero network calls.

**The job to be done.** A developer is in VS Code / Cursor / a browser / a terminal.
They hold a key, speak a sentence — a commit message, a prompt to an AI agent, a
Slack reply, a code comment, a variable name — release the key, and the text appears
where the caret was. No window switching, no copy-paste, no cloud.

### 1.1 Why this is not "just Whisper"

Raw Whisper output is unusable for developers. It writes `use effect` instead of
`useEffect`, `cube CTL` instead of `kubectl`, `open parenthesis` as literal words,
and capitalizes shell commands. **The formatting pipeline is the product**; the ASR
model is a swappable commodity. This inverts the usual instinct to spend all the
effort on the model, and it is the central architectural bet of this project.

### 1.2 Explicit non-goals (v1)

| Not doing | Why |
|---|---|
| Real-time streaming captions / subtitles | Different latency contract, different UX. Separate product. |
| Speaker diarization, meeting transcription | Not the job. Would drag in a whole feature surface. |
| Cloud sync of history | Contradicts local-first. Users can sync `%APPDATA%` themselves. |
| Voice *control* of the OS ("open Chrome") | Scope explosion; different risk profile. |
| Any paid API, any account, any telemetry | Hard product constraint. |
| Mobile | Out of scope. |

### 1.3 Product principles

1. **Local by default, network by exception.** The only outbound request the app may
   ever make is an explicit, user-initiated model download. Enforced in code (§9.2),
   not just by convention.
2. **Never lose a word.** If injection fails, the transcript still lands in history
   and on the clipboard. A failed transcription is a recoverable event, never a
   silent drop.
3. **Zero-friction hot path.** Idle cost must be negligible (< 60 MB RAM, ~0% CPU).
   The app is always running; it must never be something you notice.
4. **Deterministic where it can be.** Formatting is rule-based and unit-testable.
   Probabilistic components (ASR, optional LLM) are isolated behind seams so their
   output can be golden-tested and their failures contained.
5. **The user owns their data.** Plain SQLite, documented schema, one-click export,
   audio never persisted unless explicitly enabled.

---

## 2. Architecture overview

### 2.1 Style: hexagonal (ports & adapters)

The domain core — the recording session state machine and the formatting pipeline —
has **no dependency on the OS, on audio libraries, on the GUI, or on the ASR
runtime**. Everything platform-specific lives in an adapter behind a trait.

This is not architecture astronautics. It buys three concrete things:

- The formatter and state machine are testable in milliseconds with no audio device,
  no GPU, and no window manager. That is what makes daily iteration cheap.
- Swapping ASR runtimes (whisper.cpp → faster-whisper → Parakeet → a future model)
  is a new impl of one trait, not a refactor.
- macOS/Linux support later becomes "write three adapters", not "rewrite the app".

```
                    ┌───────────────────────────────────────────┐
                    │              ov-core (pure)                │
   inbound ports    │  ┌─────────────────┐  ┌────────────────┐  │   outbound ports
   ───────────────► │  │ Session FSM     │─►│ Format Pipeline│  │ ────────────────►
                    │  └─────────────────┘  └────────────────┘  │
                    │         ▲  event bus (broadcast)  │        │
                    └─────────┼───────────────────────┼─────────┘
                              │                       │
        ┌─────────────────────┴───────┐   ┌───────────┴──────────────────────┐
        │ HotkeyListener  AudioSource  │   │ Transcriber  TextSink  History   │
        │ (adapters in)                │   │ AppContext   (adapters out)      │
        └──────────────────────────────┘   └──────────────────────────────────┘
```

### 2.2 The six ports (the entire contract surface)

```rust
// crates/ov-core/src/ports.rs  — the ONLY way core talks to the world.

/// Emits Press/Release for the configured chord, globally, without stealing focus.
pub trait HotkeyListener: Send + Sync {
    fn subscribe(&self) -> Receiver<HotkeyEvent>;
    fn rebind(&self, chord: &Chord) -> Result<()>;
}

/// Streams mono f32 PCM at 16 kHz. Adapter owns resampling and device changes.
pub trait AudioSource: Send + Sync {
    fn start(&self) -> Result<Receiver<AudioFrame>>;   // 20 ms frames
    fn stop(&self) -> Result<()>;
    fn devices(&self) -> Result<Vec<DeviceInfo>>;
}

/// Speech -> text. Implementations: WhisperCpp, FasterWhisperSidecar, Mock.
#[async_trait]
pub trait Transcriber: Send + Sync {
    fn capabilities(&self) -> TranscriberCaps;         // streaming? langs? gpu?
    async fn transcribe(&self, audio: &Pcm16k, hint: &DecodeHint) -> Result<Transcript>;
    async fn warm(&self) -> Result<()>;                // preload weights
}

/// Puts text where the caret is. Impls: SendInputUnicode, ClipboardPaste, Mock.
pub trait TextSink: Send + Sync {
    fn inject(&self, text: &str, mode: InjectMode) -> Result<InjectReceipt>;
}

/// Identifies the foreground app so a profile can be selected.
pub trait AppContext: Send + Sync {
    fn foreground(&self) -> Result<ForegroundApp>;     // exe, title, class
}

/// Durable local storage. Impl: Sqlite, Memory (tests).
#[async_trait]
pub trait HistoryStore: Send + Sync {
    async fn append(&self, entry: &Utterance) -> Result<UtteranceId>;
    async fn search(&self, q: &Query) -> Result<Vec<Utterance>>;
    async fn purge(&self, older_than: Duration) -> Result<u64>;
}
```

**Rule:** if a new feature wants to reach the OS, it either uses one of these six or
adds a seventh port with an ADR. No ad-hoc `#[cfg(windows)]` inside core. Ever.

### 2.3 Repository layout

```
openvoice/
├── crates/
│   ├── ov-core/        # domain: FSM, events, config types, ports. NO os/io deps.
│   ├── ov-format/      # formatting pipeline + dictionary + voice commands (pure)
│   ├── ov-audio/       # cpal/WASAPI capture, resample, ring buffer, VAD
│   ├── ov-asr/         # Transcriber impls + model manager (download/verify/cache)
│   ├── ov-input/       # low-level keyboard hook + text injection + foreground app
│   ├── ov-store/       # SQLite (sqlx) history, settings persistence, migrations
│   └── ov-app/         # Tauri binary: composition root, wires adapters, IPC
├── apps/ui/            # React + TS + Vite: settings window + overlay window
├── docs/
│   ├── DESIGN.md       # this file
│   └── adr/            # 0001-...md  immutable decision records
├── fixtures/           # wav + expected-transcript pairs for regression tests
├── .github/workflows/  # ci.yml, release.yml
└── Cargo.toml          # workspace
```

`ov-core` and `ov-format` must compile to `wasm32-unknown-unknown` with no features.
That is a mechanically-checked proof (in CI) that the purity boundary hasn't leaked.

---

## 3. The session state machine

The single most important piece of correctness in the app. Modeled explicitly as a
typestate-ish enum, not as a pile of booleans.

```
        ┌──────────────────────────────── Cancel (Esc) ─────────────────────────┐
        │                                                                       │
        ▼                                                                       │
    ┌────────┐  hotkey down   ┌───────────┐  hotkey up   ┌────────────┐        │
    │  Idle  │───────────────►│ Capturing │─────────────►│ Finalizing │        │
    └────────┘                └───────────┘              └────────────┘        │
        ▲                          │  │                         │              │
        │                          │  └── > max_duration ───────┤              │
        │                          │                            ▼              │
        │                          │                     ┌─────────────┐       │
        │                          │                     │Transcribing │───────┤
        │                     (VAD silence,              └─────────────┘       │
        │                      hands-free mode)                 │              │
        │                                                       ▼              │
        │                                                ┌────────────┐        │
        │                                                │ Formatting │────────┤
        │                                                └────────────┘        │
        │                                                       │              │
        │        ┌──────────┐         ┌──────────┐              ▼              │
        └────────│ Recover  │◄────────│ Injecting│◄─────────────┘              │
                 └──────────┘  fail   └──────────┘                             │
                   (clipboard +                                                 │
                    toast + history)                                            │
```

**Invariants (enforced by tests):**

- `Idle` is the only state with no allocated audio buffer and no GPU residency.
- Every terminal path — success, cancel, or failure — writes exactly one `Utterance`
  row (with a status field). No state can exit without accounting for the audio.
- A second hotkey press during `Transcribing` **queues** a new session rather than
  dropping it or interleaving. Transcription is serialized; capture is not.
- `Cancel` is reachable from every non-`Idle` state and always completes in < 50 ms.
- Recordings shorter than `min_duration` (default 300 ms) are discarded as fat-finger
  presses, without a toast.

### 3.1 Latency budget (target, 10 s utterance, RTX 3050)

| Stage | Budget | Notes |
|---|---:|---|
| Hotkey release → capture stop | 10 ms | hook thread does nothing but post a message |
| Finalize + VAD trim + resample | 25 ms | already 16 kHz mono; trim leading/trailing silence |
| ASR decode (`large-v3-turbo` q5, CUDA) | 400–600 ms | ~15–25× realtime on this GPU |
| Format pipeline | < 5 ms | pure string work, no allocation storms |
| Injection (clipboard path) | 60–120 ms | dominated by target app's paste handler |
| **Total (p50)** | **~700 ms** | perceived as "instant enough" |

Every stage emits a `tracing` span; the debug panel renders the last 50 sessions as a
stacked bar so regressions are visible without profiling tooling. **Measure from day
one** — retrofitting latency instrumentation into a shipped app never happens.

---

## 4. Audio pipeline

- **Capture:** `cpal` → WASAPI shared mode, native device rate, mono downmix.
- **Resample:** `rubato` (sinc) to exactly 16 kHz f32. Whisper's contract.
- **Buffering:** lock-free SPSC ring (`rtrb`) sized for `max_duration` (default 120 s).
  The audio callback never allocates, never locks, never logs — classic realtime
  discipline. Overrun drops the *oldest* frames and flags the session.
- **Pre-roll:** the ring is *always* running once armed, so the ~200 ms before the
  hotkey registers is retained. This fixes the single most common dictation
  complaint: clipped first syllables.
- **VAD:** Silero VAD (ONNX, ~2 MB, CPU) for (a) trimming silence before decode —
  a real latency win, and (b) hands-free auto-stop mode.
- **Level meter:** RMS + peak per 20 ms frame, sent to the overlay at 30 Hz for the
  waveform. Also drives a "your mic is muted / nothing was heard" warning, which is
  otherwise a baffling failure mode.

---

## 5. ASR layer

### 5.1 Model selection for 4 GB VRAM

| Model | Size on disk | VRAM | Quality | Verdict |
|---|---:|---:|---|---|
| `large-v3-turbo` q5_0 | ~574 MB | ~1.6 GB | Excellent | **Default.** Fits comfortably. |
| `distil-large-v3` | ~750 MB | ~1.9 GB | Very good, EN-only | Alternate |
| `small.en` q5_1 | ~190 MB | ~0.6 GB | Good | Low-VRAM / battery profile |
| `base.en` q5_1 | ~60 MB | CPU-ok | Mediocre | CPU fallback, first-run smoke test |

Ship with `base.en` downloadable in seconds so the very first run works within 30 s
of install, then prompt to upgrade to `large-v3-turbo` in the background.

### 5.2 Decode hints — the cheapest accuracy win available

Whisper accepts an `initial_prompt` (~224 tokens) that biases decoding. We populate it
per-session with:

1. Top-N terms from the user dictionary, ranked by recency × frequency.
2. Terms scraped from the foreground app's window title (e.g. a filename, a repo name).
3. A short style exemplar matching the active profile.

This makes `kubectl`, `useMemo`, `tanstack`, and the user's own variable names come
out right **at decode time**, which is strictly better than fixing them afterwards
with fuzzy string replacement. Budgeted at 224 tokens with a deterministic packer.

### 5.3 Model manager

Downloads from Hugging Face over HTTPS with **SHA-256 verification against a manifest
committed to the repo**. Resumable, cancellable, stored in `%LOCALAPPDATA%\OpenVoice\models`.
A model is never loaded unless its hash matches — this is the app's one network
surface, so it gets the strictest handling.

---

## 6. The formatting pipeline (the differentiator)

An ordered list of composable, individually-testable stages. Each is
`fn apply(&self, doc: &mut Doc, ctx: &FormatCtx)` where `Doc` carries the token
stream plus spans, so later stages know what earlier ones touched.

| # | Stage | Example |
|---|---|---|
| 1 | `NormalizeWhitespace` | collapse ASR artifacts |
| 2 | `StripFillers` | "um, so like, the thing" → "the thing" (3 aggressiveness levels) |
| 3 | `VoiceCommands` | "new line", "new paragraph", "open paren", "semicolon", "tab", **"scratch that"** |
| 4 | `Dictionary` | "use effect" → `useEffect`; "cube cuttle" → `kubectl` |
| 5 | `CaseTransforms` | "camel case user name" → `userName`; "snake case" / "kebab case" / "screaming" |
| 6 | `Punctuation` | fix sentence-initial caps; strip trailing period in terminal profile |
| 7 | `ProfilePolicy` | per-app trailing newline, lowercase-first, wrap width |
| 8 | `LlmPolish` *(opt-in, off)* | local Qwen3-1.7B via llama.cpp for prose cleanup |

**Design decisions inside the pipeline:**

- **Escape hatch is mandatory.** "literally new line" emits the words. Without this,
  voice commands make certain sentences untypeable, which is infuriating.
- **Dictionary matching is phonetic, not literal.** Double Metaphone + edit distance
  over the ASR output, because Whisper's errors are phonetic errors. A literal
  `HashMap<String,String>` catches maybe 40% of what a phonetic index catches.
- **Every stage is reversible for debugging.** The debug panel shows the text after
  each stage, so when the output is wrong you know exactly which rule did it. This
  turns "the formatter is weird" into a 10-second diagnosis.
- **Golden-file tests.** `fixtures/format/*.txt` — input, profile, expected output.
  Adding a rule that breaks a past case fails CI. This is what makes the pipeline
  safe to extend for years.

### 6.1 App profiles

Keyed by executable name, matched most-specific-first, with a fallback default:

```toml
[profile.terminal]
match      = ["WindowsTerminal.exe", "powershell.exe", "wt.exe"]
capitalize = false          # `git status`, not `Git status`
end_period = false
dictionary = ["shell", "git"]

[profile.editor]
match      = ["Code.exe", "Cursor.exe", "idea64.exe"]
capitalize = "sentence"
dictionary = ["code", "user"]
llm_polish = false          # never rewrite code identifiers

[profile.prose]
match      = ["slack.exe", "chrome.exe", "Notion.exe"]
capitalize = "sentence"
llm_polish = true           # opt-in, if the model is loaded
```

### 6.2 "Prompt mode" — a deliberate bet

A second hotkey that captures longer, rambling speech and formats it as a *prompt for
an AI coding agent*: filler stripped, restructured into an instruction, requirements
bulleted. This is precisely how you are using dictation right now, and it is the
feature most likely to make OpenVoice better than the commercial tools for this
specific audience. Requires the optional local LLM. Phase 4.

---

## 7. Text injection (Windows)

Genuinely the hardest thing to get right; it fails in different ways per target app.

| Strategy | How | Good for | Fails at |
|---|---|---|---|
| **A. `SendInput` + `KEYEVENTF_UNICODE`** | synthesize per-codepoint key events | short text, terminals, games, anything | slow above ~200 chars; some apps drop fast input |
| **B. Clipboard + `Ctrl+V`** | set clipboard, synthesize paste, restore | long text, instant regardless of length | clobbers clipboard; blocked in some secure fields; app may not honor Ctrl+V |
| **C. UI Automation `TextPattern`** | insert via accessibility API | cleanest where supported | inconsistent support; heavier |

**Chosen policy:** length-based with restore and verification.
- `len <= 120` → **A** (no clipboard side effects, the common case).
- `len > 120` → **B**, saving *all* clipboard formats and restoring them after a
  120 ms delay. Restoring only `CF_TEXT` — which most tools do — silently destroys
  copied images and rich text. We restore the full format set.
- Injection returns an `InjectReceipt`. On any failure the text is left on the
  clipboard and a toast says "copied — press Ctrl+V", never a silent loss.
- Modifier hygiene: if the push-to-talk key is a modifier (e.g. Right Ctrl), it must
  be released in the synthetic input stream before pasting, or `Ctrl+V` becomes
  `Ctrl+Ctrl+V` and nothing happens. This bug eats an afternoon if unplanned.

---

## 8. Hotkey capture

`RegisterHotKey` is unusable here: it gives a single "pressed" notification, not
down/up, so push-to-talk is impossible. We install a **low-level keyboard hook**
(`SetWindowsHookEx(WH_KEYBOARD_LL)`) on a dedicated thread with its own message pump.

Non-negotiable constraints for that thread:
- The hook callback must return in **well under 10 ms** or Windows silently evicts
  the hook and all hotkeys stop working. It therefore does *nothing* but compare a
  key code and post to a channel. No logging, no locks, no allocation.
- Never swallow the key unless it's a dedicated dictation key, so normal typing is
  untouched.
- Self-heal: watchdog re-installs the hook if Windows drops it (happens after UAC
  prompts and some full-screen transitions).

**Default binding:** `Right Ctrl` held (push-to-talk) — rarely used, easy to reach,
not a chord. Alternates: `Ctrl+Space` toggle, `F13`+ for those with a macro key.

---

## 9. Data, privacy, security

### 9.1 Storage

`%APPDATA%\OpenVoice\` — `config.toml` (versioned, migrated on load),
`history.db` (SQLite via `sqlx`, migrations in-tree), `logs/` (rotating, 7 days).

```sql
CREATE TABLE utterance (
  id            INTEGER PRIMARY KEY,
  created_at    INTEGER NOT NULL,      -- unix ms
  duration_ms   INTEGER NOT NULL,
  raw_text      TEXT NOT NULL,         -- straight from ASR
  final_text    TEXT NOT NULL,         -- after formatting
  profile       TEXT NOT NULL,
  target_app    TEXT,
  model         TEXT NOT NULL,
  status        TEXT NOT NULL,         -- ok | cancelled | asr_error | inject_failed
  latency_ms    INTEGER,
  audio_path    TEXT                   -- NULL unless debug.retain_audio
);
CREATE VIRTUAL TABLE utterance_fts USING fts5(final_text, content='utterance');
```

Keeping `raw_text` alongside `final_text` is what makes the formatter improvable:
you can replay the entire history through a new pipeline version and diff the
results before shipping it. That is the core of the day-by-day improvement loop.

### 9.2 Privacy, enforced not promised

- **No telemetry. No analytics. No crash reporting uploads.** Not configurable-off —
  simply absent from the codebase.
- Audio is held in RAM and dropped after transcription unless `debug.retain_audio`.
- A CI check greps the dependency tree and fails the build if any HTTP client is
  reachable from a crate other than `ov-asr::model_manager`. The privacy claim is
  a test, not a README sentence.
- Redaction: a configurable regex list scrubs secrets (API keys, tokens) from
  history and logs before they are written.
- "Panic purge": a hotkey/menu item that wipes history and audio immediately.

### 9.3 Threat model (brief, honest)

The app installs a global keyboard hook and can synthesize input — the same
capabilities as a keylogger. Mitigations that matter for an OSS tool people are
asked to trust: reproducible builds, the hook stores no key data (only compares
against the bound chord), signed releases once a cert is available, and a
`SECURITY.md` with a disclosure path. Documented plainly in the README rather than
buried, because users *should* be suspicious of this class of software.

---

## 10. UI

Two windows plus a tray icon.

**Overlay** — a ~280×64 frameless, always-on-top, **non-activating**
(`WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`) pill near the bottom-center. Never steals
focus — if it does, the caret position is lost and the whole product breaks. Shows
state (idle/listening/thinking), a live waveform from the RMS stream, elapsed time,
and the ~last 300 ms of partial text once streaming lands. Fades in ~120 ms.

**Main window** — React + TypeScript + Vite + Tailwind + shadcn/ui, Zustand for state:
- *Dictate* — big status, recent utterances, re-insert / copy / delete per row.
- *History* — FTS search, filter by app/date/status, export JSON/CSV/Markdown.
- *Dictionary* — add terms, import from a repo (scan symbols), test a phrase live.
- *Profiles* — per-app rules with a live preview pane.
- *Models* — download/switch/benchmark, VRAM and speed shown honestly.
- *Settings* — hotkeys, mic, VAD, privacy, launch-at-login.
- *Debug* — the stage-by-stage formatter trace and the latency waterfall.

**Frontend discipline:** the UI holds **no business logic**. It renders events from
the core's bus and issues commands. Any logic in the UI is logic that cannot be
tested headlessly and cannot be reused by a future CLI. A `ov-cli` binary that
transcribes a wav file through the identical pipeline is the forcing function that
keeps this honest — and it doubles as the integration test harness.

---

## 11. Testing strategy

| Layer | Approach | Runs in |
|---|---|---|
| `ov-core` FSM | exhaustive transition tests + `proptest` for cancel-at-any-point | ms, CI |
| `ov-format` | golden files in `fixtures/format/` | ms, CI |
| Dictionary/phonetics | property tests: no rule may alter text lacking its trigger | ms, CI |
| `ov-asr` | fixture wavs → WER threshold, `#[ignore]` by default (needs weights) | nightly |
| `ov-input` | mock `TextSink`; manual matrix doc for real apps | CI + manual |
| End-to-end | `ov-cli transcribe fixture.wav --profile editor` | CI |
| Latency | criterion benches on the format pipeline; runtime spans in prod | CI + app |

The **app-compatibility matrix** (VS Code, Cursor, Windows Terminal, Chrome, Slack,
Discord, Notion, IntelliJ, Obsidian) is a checklist in `docs/compat.md`, re-run
before each release. Injection breaks per-app in ways no unit test can catch.

---

## 12. Open-source engineering from day one

- **License:** Apache-2.0 (explicit patent grant; the safer choice for a tool
  companies may adopt) — see §14 Q4.
- **Docs:** README with a demo GIF above the fold, `CONTRIBUTING.md`,
  `CODE_OF_CONDUCT.md` (Contributor Covenant), `SECURITY.md`, `ARCHITECTURE.md`
  pointing here, and **ADRs** in `docs/adr/` — immutable, numbered, one per real
  decision. Six months from now the ADRs are what stop you from re-litigating.
- **Commits:** Conventional Commits → `release-please` generates CHANGELOG + tags.
- **CI** (`ci.yml`): `cargo fmt --check`, `clippy -D warnings`, `cargo test`,
  `cargo deny` (licenses + advisories), the wasm purity check, `tsc --noEmit`,
  eslint, vitest. Required for merge.
- **Release** (`release.yml`): tag → Tauri bundle (NSIS + MSI) → GitHub Release with
  SHA-256 sums. `cargo-dist` later for multi-platform.
- **Repo hygiene:** `rust-toolchain.toml` pinned, `.editorconfig`, issue/PR
  templates, `CODEOWNERS`, `good-first-issue` labels, dependabot.
- **Naming caution:** "OpenVoice" is already a well-known TTS project from MyShell
  (~30k stars). Discoverability will suffer and users will confuse the two. Worth
  a rename before the repo gains traction — e.g. *Vox*, *Dictate*, *Utter*,
  *Speakeasy*, *Larynx*. Your call; flagging it now because renaming later is
  expensive.

---

## 13. Roadmap

| Phase | Contents | Definition of done |
|---|---|---|
| **v0.1** — walking skeleton | PTT hotkey → capture → whisper → inject → tray → history. All six ports real (no mocks in the hot path), even if each impl is minimal. | You dictate a sentence into VS Code and it appears, correctly. |
| **v0.2** — the differentiator | Formatting pipeline, dictionary + phonetic matching, voice commands, app profiles, settings UI. | Dictating code comments and shell commands stops being annoying. |
| **v0.3** — feel | Overlay + waveform, streaming partials, model manager UI, latency panel, pre-roll buffer. | p50 under 700 ms, and it *feels* instant. |
| **v0.4** — intelligence | Optional local LLM polish, prompt mode, repo-symbol dictionary import. | Rambling speech → a clean agent prompt. |
| **v0.5** — distribution | Auto-update, signed installer, macOS adapters, docs site. | Someone who isn't you can install and use it. |
| **v1.0** | Plugin API for formatter stages, benchmark suite, stability. | API frozen, semver honored. |

Each phase must ship a usable app. No phase is a refactor-only phase.

---

## 14. Open decisions — need your approval

**Q1 — Stack.** Rust/Tauri is the right long-term answer (60 MB idle vs Electron's
~250 MB, native hooks without native-module pain, single 12 MB binary), but Rust
isn't installed on this machine — rustup + MSVC Build Tools is a ~3–5 GB, ~25 min
setup. Electron gets you testing today at a permanent cost in footprint and in the
quality of the OS integration. See options in the question prompt.

**Q2 — Day-1 ASR backend.** `faster-whisper` in a Python sidecar (CUDA works out of
the box, `uv` already installed, fastest path to good accuracy, costs a bundled
Python runtime at distribution time) vs `whisper.cpp` in-process (pure native, clean
distribution, but Windows GPU builds need CUDA Toolkit or a prebuilt binary).
Either way it sits behind `Transcriber`, so this is reversible.

**Q3 — Activation.** Push-to-talk vs toggle vs both, and the default binding.

**Q4 — License.** Apache-2.0 vs MIT.

Once these are settled, ADRs 0001–0004 get written and v0.1 implementation begins.
