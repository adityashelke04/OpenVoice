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

Nothing has been released yet, so there is no upgrade path to describe and no
compatibility promise in force. Hotkey → capture → transcribe → inject → history
now works end to end, and the installer builds; `v0.1.0` is tagged once that
installer has been tested on a machine that has never had the repository on it.

### Added

- **An installer.** `OpenVoice_0.1.0_x64-setup.exe`, 68 MB, with the speech
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

<!--
Template for the first real release. Delete the placeholders above when using it.

## [0.1.0] - YYYY-MM-DD

### Added
### Changed
### Deprecated
### Removed
### Fixed
### Security

[Unreleased]: https://github.com/adityashelke04/OpenVoice/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/adityashelke04/OpenVoice/releases/tag/v0.1.0
-->

[Unreleased]: https://github.com/adityashelke04/OpenVoice/commits/main
