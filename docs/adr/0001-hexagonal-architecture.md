# ADR 0001 — Hexagonal architecture with six ports

- **Status:** Accepted
- **Date:** 2026-07-31

## Context

OpenVoice must touch a lot of hostile, platform-specific surface: a low-level Windows
keyboard hook, WASAPI audio capture, GPU inference, synthetic input injection, and a
GUI. Each of those is slow to test, flaky in CI, and impossible to exercise on a
headless machine.

At the same time, the part of the app that actually determines product quality — the
session lifecycle and the text formatting pipeline — is pure logic that we want to
iterate on many times per day.

If those two categories are interleaved, every formatter change requires launching a
GUI, holding a key, and speaking into a microphone to verify. That feedback loop is
too slow to sustain, and it is the single most likely reason a hobby project like
this stalls.

## Decision

Adopt ports and adapters. The domain core (`ov-core`, `ov-format`) depends on
nothing platform-specific. All outside contact happens through six traits:

| Port | Direction | Adapters (v0.1) |
|---|---|---|
| `HotkeyListener` | in | `WinLowLevelHook`, `Mock` |
| `AudioSource` | in | `CpalWasapi`, `WavFile`, `Mock` |
| `Transcriber` | out | `FasterWhisperSidecar`, `Mock` |
| `TextSink` | out | `WinInject`, `Clipboard`, `Mock` |
| `AppContext` | out | `WinForeground`, `Static` |
| `HistoryStore` | out | `Sqlite`, `Memory` |

Adding a seventh port requires a new ADR. Platform `#[cfg]` blocks are forbidden
inside `ov-core` and `ov-format`.

## Enforcement

The purity boundary is not a convention, it is a CI job: `ov-core` and `ov-format`
must compile for `wasm32-unknown-unknown` with default features. Nothing that links
to Win32, cpal, sqlx, or tokio's IO can pass that check. If the build goes red, the
boundary leaked.

## Consequences

**Good.** The formatter and state machine test in milliseconds with no hardware.
`ov-cli transcribe fixture.wav` exercises the entire pipeline headlessly, so CI
covers the real code path. macOS/Linux later means writing three adapters, not
rewriting the app. Swapping ASR runtimes is one trait impl.

**Costs.** More crates and more indirection than a single-binary app would need —
roughly 300 lines of trait and wiring code that a monolith would not have. Composition
happens in exactly one place (`ov-app`), which is a file that will get long and must
be kept readable.

**Rejected alternative.** A single `src/` with modules. Faster to start by perhaps a
day, but it makes the formatter untestable without hardware, which is precisely the
property this project cannot afford to lose.
