# Changelog

All notable changes to OpenVoice are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries are written for the person deciding whether to upgrade, not for the
person who wrote the commit. "Fixed a race in the supervisor" is a commit
message; "dictating twice in quick succession no longer drops the second
utterance" is a changelog entry.

Commit prefixes map to sections: `feat:` → Added, `fix:` → Fixed,
`refactor:`/`perf:` → Changed, and anything that removes or breaks behaviour →
Removed or the **Breaking** note at the top of that release.

<!--
Add to `## Unreleased` in the same PR that changes behaviour. A changelog
assembled at release time from a month of commits is written by whoever has the
least context, at the moment they have the least time.
-->

## [Unreleased]

OpenVoice can now tell you when a new version exists, and two settings that had
never done anything now do what they say.

### Added

- **Update checks.** OpenVoice looks once per launch for a newer release and
  offers it; you press a button to install. It never installs on its own. Every
  update is verified against a signing key built into the app, so a tampered or
  substituted download is discarded without running — worth having while the
  installer itself is still unsigned. Switch it off in Settings → Updates and no
  request is made at all. This is the one request OpenVoice makes that you did not
  individually ask for, and the reasoning is in
  [ADR 0005](docs/adr/0005-in-app-updates.md).
- **Secrets are kept out of your history.** API keys and tokens matching
  `privacy.redact_patterns` are replaced with `[redacted]` before a transcript is
  saved or logged. Defaults cover OpenAI, GitHub and AWS credentials. The text
  delivered to your application is never altered — only the stored copy.

### Fixed

- **"Keep recordings" now keeps recordings.** The toggle had nothing behind it.
  Turned on, captured audio is kept under `%APPDATA%\OpenVoice\audio` from the
  next restart instead of being deleted after each decode. It remains off by
  default, and it is still the only way audio ever persists.
- The Speech model screen lists what the app can actually load. The model list was
  written down twice — once in the speech engine, once in the interface — and the
  two could disagree about which models existed and how large they were.

### Removed

- `model` under `[config]` in `settings.toml`, which was read by nothing. The
  model has always been the top-level `model` key, and that is unchanged. Your
  existing settings file loads as-is; the stale key is ignored, not deleted.

## [0.1.1] - 2026-08-08

A fix release. The visible complaint was that the Flow Bar could multiply into
several copies of itself, each drawn inside an opaque rectangle. The bar was
innocent: OpenVoice could be running several times over, and each copy was
showing its own.

Worth upgrading to even if you never saw duplicate bars — the same fault meant
several copies could be holding the microphone, decoding the same audio, and
writing the same history database at once, none of which is visible from the
interface.

### Fixed

- **Launching OpenVoice while it is already running no longer starts a second
  copy of it.** Closing the Hub hides it rather than quitting, which is
  deliberate — a dictation tool that stops working when you close its window is
  not available when you need it. But nothing stopped the *next* launch from
  starting a complete second app: another global keyboard hook on the same
  chord, another Flow Bar, another 1.6 GB of weights, and two processes writing
  the same history database and overlay placement. Every hotkey hold then opened
  the microphone once per copy, transcribed the same audio that many times, and
  raced to inject the results into each other. Launching again now raises the
  window of the copy already running, which is what it was always meant to mean.
  The duplicates were easiest to notice after "Hide for an hour", because that
  kept each new copy's bar hidden until a dictation forced it on screen — so
  they arrived all at once, on one keypress, looking like one window gone wrong.
- **The Flow Bar no longer flashes a dark rectangle around itself.** The
  overlay window is transparent, but transparency was applied from a script that
  runs after the first paint, while the app's black canvas was painted the
  moment the stylesheet loaded. The window filled itself in for the whole gap
  between the two. The canvas is now something each window opts into, so the
  overlay has nothing to fall back to; the window is also declared at the size
  of the idle pill rather than 280×52, so it no longer appears oversized and
  then shrinks.
- **The Flow Bar's right-click menu now closes when you start dictating.** Its
  only dismissal path was losing window focus — which cannot happen to a window
  built never to take focus. Opening the menu and then dictating left the panel
  standing open behind the bar as an opaque block, with no way to click away
  from it.
- **Auto-placement is no longer recorded as a position you chose.** The Windows
  move loop swallows the mouse-up that ends a drag, so a click on the bar that
  moved nothing could leave it looking permanently dragged; the next time the
  app placed the bar itself, that placement came back as a drag, got snapped to
  an edge, and was saved.

## [0.1.0] - 2026-08-03

The first published build. Hotkey → capture → transcribe → format → inject →
history works end to end on Windows, and it installs from a single `.exe` with
no Python, no Rust and no Node on the target machine.

This is an alpha, and the version number is the promise: there is no upgrade
path from anything, and nothing here is a compatibility commitment yet. Two
things to know before installing, both covered in the README: the installer is
**not code-signed**, so SmartScreen will warn about an unknown publisher (verify
the published SHA-256 instead), and the bundled speech engine is **CPU-only** —
using an NVIDIA GPU still means running from source.

### Added

- **Sound feedback.** A short tone on starting a dictation, and a distinct
  one on a successful finish — both synthesized with the Web Audio API rather
  than shipped as audio assets, so there's nothing to license or bundle.
  Off/on in Settings (`Config.sound_enabled`, default on). The finish chime is
  deliberately withheld on a clipboard-fallback completion, which settles to
  the same "idle" state a clean success does — playing it there would tell the
  user a dictation landed cleanly when it didn't.
- **A language setting.** `ov-asr`/the sidecar have accepted a forced-language
  parameter since day one, but it was hardcoded to English at every call site
  in `ov-app` and `ov-cli` with no way to change it. Now a real setting
  (`Config.language`), exposed as a dropdown in Settings, defaulting to English
  rather than auto-detect — Whisper's auto-detection looks at only the first
  ~30s of audio and is measurably less reliable than being told the language,
  which matters most for exactly the case this app is built around: a short,
  single push-to-talk utterance. Unlike the model, changing it needs no
  restart — it's a per-request decode parameter, not something baked into
  loaded weights, so it reloads the same way the dictionary does. `ov-cli`
  gained a matching `--language` flag (`auto` for auto-detect).
- **An installer.** `OpenVoice_0.1.0_x64-setup.exe`, 47 MB, with the speech
  engine inside it. No Python, no virtualenv, no Rust on the target machine.
  Built by `release.yml` on a tag, with a SHA-256 published beside it.
- **A frozen speech engine.** PyInstaller packages the sidecar into a standalone
  binary (`scripts/build-sidecar.ps1`). The build is not considered finished
  until the frozen binary answers a real request on its protocol — a missed
  hidden import is invisible to PyInstaller and fatal at run time.
- **First-run model download, with progress.** The weights are fetched on first
  launch and reported as a real byte count rather than a spinner. Previously the
  app simply looked frozen for the several minutes this takes.
- **Screenshots that cannot go stale.** `scripts/screenshots.mjs` captures the
  README images from the running UI over the DevTools protocol.

### Changed

- An installed copy keeps its weights under `%APPDATA%\OpenVoice\models` rather
  than in a shared Hugging Face cache, so uninstalling reclaims the space.
- A debug build always prefers the Python sidecar in your checkout; a release
  build prefers the frozen one it shipped with. Edits to `sidecar/` are no longer
  silently shadowed by whatever was last frozen.
- The speech engine ships CPU-only. The CUDA libraries are 1.9 GB against 240 MB
  for everything else and do nothing without an NVIDIA GPU.
- **The installed default model is now `base.en` (~75 MB), not `large-v3-turbo`
  (~1.6 GB).** Pairing the heaviest model with a CPU-only install was the wrong
  default: worst latency, biggest download. Upgrading is a Models-screen action,
  already built, that downloads on demand.
- **PyAV is no longer in the frozen build**, cutting it from 240 MB to 173 MB.
  `engine.py` reads its own WAV files (it is the only producer of them, and
  always writes 16 kHz mono 16-bit PCM) instead of going through
  faster-whisper's `decode_audio`, which is the only thing that needed PyAV's
  bundled FFmpeg. Verified against the real fixture audio, not just unit tests.

### Fixed

- **A dictated accented character, CJK character, or emoji could crash the
  sidecar or silently lose the transcript.** Python's stdio only defaults to
  UTF-8 inside an interactive console; redirected to a pipe — exactly how the
  Rust host launches this process — it fell back to the Windows ANSI code
  page (`cp1252` on a typical install). Confirmed by actually spawning a
  piped child process: a character `cp1252` can't represent at all (CJK,
  emoji) crashed the sidecar outright, and one it *can* represent as a
  different single byte than UTF-8 uses — which includes ordinary accented
  Latin text ("café", "naïve") and the smart quotes/dashes Whisper's own
  punctuation restoration commonly adds — wrote bytes the Rust side rejected
  as invalid UTF-8. The automatic retry couldn't help, because it decodes the
  exact same audio and hits the exact same failure again. Fixed by forcing
  UTF-8 on stdin/stdout/stderr explicitly, first thing in `main()`, rather
  than trusting the launching environment. Verified with a real piped
  subprocess in both directions (fails without the fix, survives with it).
- **Four idempotency bugs in the formatting pipeline**, found by fuzzing a
  battery of realistic phrases through every profile twice (formatting is
  required to be idempotent on its own output — history replay and any
  "preview this phrase" UI depend on it): the `literally` escape hatch only
  protected one word after itself, so a two-word command like `literally fat
  arrow` still had its second word (`arrow`) reinterpreted as `->`; a
  resolved identifier (`userName`) got re-capitalized into `UserName` if
  already-formatted text was re-parsed; `Doc::parse` used `split_whitespace`,
  which silently discarded the literal newlines that "new line"/"new
  paragraph" produce on a second pass; and force-lowercase profiles
  corrupted a surviving `SCREAMING_SNAKE_CASE` constant into `mAX_SIZE`
  instead of leaving it alone.
- **The Flow Bar overlay had permissions it never used and must never use.**
  It shared the Hub's full capability grant, including
  `core:window:allow-set-focus` — directly contradicting the overlay's one
  architectural invariant, that it can never take focus. Nothing currently
  misuses this, but any future or compromised dependency running in that
  webview could have called `getCurrentWindow().setFocus()` on itself and it
  would have been permitted. Split into its own capability file granting
  only what `Overlay.tsx` actually calls.
- **The clipboard-restore decision after a paste couldn't tell "succeeded" from
  "failed, and the caller is now using the clipboard as a safety net"** — both
  end up with the same text sitting on the clipboard, and the old check only
  asked "is the clipboard still ours," which is true either way. Refactored
  into a single, unit-tested decision (`should_restore`) that also checks
  whether the paste actually sent.
- **A failed clipboard-paste recorded the wrong text.** When injection fell back
  to leaving text on the clipboard, history and the `Outcome::ClipboardFallback`
  payload recorded the *raw* ASR transcript instead of the *formatted* text —
  the thing that was actually placed on the clipboard. History could disagree
  with what `Ctrl+V` actually pastes.
- **A muted/disconnected microphone was recorded as "transcription failed."**
  `AudioFailed` (capture never even reached the transcriber) was mapped to the
  same `Outcome::AsrFailed` as a genuine model failure, so a dead microphone
  and a broken sidecar were indistinguishable in history. Split into its own
  `Outcome::CaptureFailed`, threaded through the Rust↔TypeScript wire contract.
- **The engine could report "idle" while still transcribing a queued session.**
  Holding the hotkey again while a previous utterance was still being
  processed queues a second session; if that second session ended as a
  fat-finger tap, silence, or a capture failure, the engine unconditionally
  announced `Idle` — even though the first session was still actively in
  flight. Now only announced once the whole pipeline is genuinely empty.
- **A NaN or negative silence threshold silently disabled muted-mic
  detection.** TOML happily parses `nan`/`-nan` float literals, and
  `rms < silence_rms` is `false` for every possible `rms` when the threshold
  is NaN (or negative, since RMS is never negative) — completely and silently
  turning off the check with no error anywhere. Config validation now rejects
  both.
- A stale, incorrect `tauri_build::build()` assumption meant a fresh checkout
  (a new contributor, or CI, neither of which have ever run the sidecar
  freeze step) would fail the very first `cargo test`/`clippy`/`check` it
  ever ran — `bundle.resources` is validated against the filesystem
  unconditionally, not just for an actual `tauri build`. This had been
  invisible all session because the frozen sidecar already existed locally
  from manual testing. `build.rs` now creates an empty placeholder for
  ordinary dev builds, but still hard-fails a real release build if the
  frozen sidecar is genuinely missing.
- **A failed short dictation left nothing on the clipboard at all.** Only the
  long-text/clipboard-paste path had a fallback that left text behind for
  `Ctrl+V` on failure; the short-text path (`send_unicode`, used for the
  overwhelming majority of dictations, since most utterances are well under
  the paste threshold) returned the error directly with no fallback. A failed
  short dictation was silently gone. Both paths now leave text on the
  clipboard on failure.
- **The clipboard fallback for a failed paste was silently undoing itself.**
  When injection can't type text into the target app directly, the text is
  left on the clipboard so `Ctrl+V` still works — but a background thread was
  restoring the user's *previous* clipboard contents exactly 1 second later,
  regardless of whether the paste actually succeeded. That 1-second window
  existed to solve a different race (giving the target app time to read the
  clipboard) and was never long enough for a *person* to notice a failed paste
  and react to it. Worse, the restore fired even down the known-failure path,
  undoing the exact fallback the error branch had just set up. Fixed two ways:
  the hold is now 15 seconds, and the restore no longer runs at all when the
  paste chord itself failed to send. A failed injection also now shows a
  notice telling the user to press `Ctrl+V` — previously the only way to
  discover an injection had failed was to spot a non-"delivered" badge in
  History afterward.
- **Injection failures were completely invisible in the log.** `Effect::Inject`
  logged nothing on error — the only trace was the in-app notice and a history
  badge, neither of which helps once the moment has passed and someone is
  trying to reproduce an intermittent failure. It now logs the error directly.
  Also added a check for a real, previously undiagnosable class of bug this
  surfaced: transcription and formatting can take several seconds, so the
  window that was foreground when the user started speaking is not
  guaranteed to still be foreground at injection time. That gap was silent;
  it now logs a warning naming both the app the user spoke into and whatever
  is actually foreground at injection, so a report of "it sometimes doesn't
  paste into app X" is diagnosable from one log line instead of a guess.
- History no longer shows a green "delivered" badge on every single row. It
  is the default, expected outcome for nearly every dictation, so repeating it
  down the whole list was decoration, not information — and diluted the one
  place green is supposed to mean something (the microphone is open) into
  noise. A badge now appears only when an outcome needs attention.
- The sidecar no longer flashes a console window on every start and restart.
- `beforeBuildCommand` resolved its path against whichever directory invoked the
  build, so `tauri build` failed depending on how it was called. Build commands
  now live in the root `package.json`, which `npm run` finds by walking up.
- The design-system sheet documented `--faint` as `#6b6b6b` at 3.4:1 — a value
  the tokens had already moved away from, on the one page whose job is to be
  correct about them.
- Excluding PyAV initially broke `import faster_whisper` outright —
  `faster_whisper/__init__.py` imports it unconditionally, whether or not
  `decode_audio` is ever called. Caught by actually running the frozen binary's
  `probe`, not by reading the diff. Fixed with a stub module installed only when
  the real package is absent, scoped to the frozen entry point.
- `ensure_model`'s request nested `"model"` under a literal `"params"` object,
  which the sidecar's own wire format treats as a param named `params` — the
  model name never actually reached the handler, which silently fell back to
  the sidecar's already-configured model instead. Harmless today only because
  `ensure_model` has never been asked for a different model than the one the
  sidecar was launched with. Found by writing a real end-to-end test against
  the frozen binary rather than trusting the unit tests already in place.

[Unreleased]: https://github.com/adityashelke04/OpenVoice/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/adityashelke04/OpenVoice/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/adityashelke04/OpenVoice/releases/tag/v0.1.0
