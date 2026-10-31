# ADR 0012 - Compact flow bar and event-driven feedback

- **Status:** Accepted
- **Date:** 2026-10-31
- **Builds on:** ADR 0007 (fixed native window) and ADR 0011 (menu dismissal)

## Context

The floating bar had an oversized rainbow treatment and delayed waveform
response. Completion feedback could remain stuck after a quick restart or be
lost when delivery and idle notifications rendered separately. Side-docked
hands-free recording did not expose clear stop and discard controls.

## Decision

Use a small monochrome capsule informed by the observed Wispr Flow desktop
interaction pattern: 40x8 resting paint, 50x30 ready, 73x30 held recording,
102.5x30 hands-free, and 98x30 processing. Keep the existing fixed native window,
nonactivation and DPI-aware region logic. A resting handle retains a 24px hit
height, and hands-free controls retain 24px pointer targets.

The ten-bar waveform responds without a history-fill delay. Convert linear
microphone RMS to a fixed -60..-12 dBFS display range before frame-rate-normalized
smoothing. This calibration is specific to OpenVoice's audio input; it is not a
copy of the reference application's adaptive input normalization. Decorative
breathing and spinner motion stop under reduced motion, while live level
information remains available. Hidden documents stop sampling.

Sound feedback consumes engine events before React batches updates, with one
owner in the overlay. Muting cancels pending and playing cues, delayed resumes
expire, and device failures cannot interrupt dictation. Original synthesized
cues work in ordinary repository builds. A user may supply a private local sound
pack in `apps/ui/public/flow-sounds/` with `start.wav`, `stop.wav`, `success.wav`,
`lock.wav` and `error.wav`; those files are intentionally Git-ignored. Missing or
invalid files fall back immediately. Error recordings play at half volume.

Completion feedback acknowledges each delivered session once, after it reaches
idle. It expires independently of sound preferences and clears on a new session.
Local-model loading, clipboard recovery, errors, compact mode, side docking and
existing idle preferences remain supported.

## Verification and follow-up

Component, lifecycle, geometry and sound regression tests cover the changed
behavior. `node scripts/flowbar-check.mjs`, with the UI served on port 5199,
checks actual rendered bounds, stop/discard actions, center stability during
morphs and reduced motion, and captures four backgrounds under `target/`.

Before claiming exact reference fidelity, still verify these on real Windows:

- Side-by-side reference and OpenVoice captures at matching DPI and state.
- Foreground caret preservation, click-through and WebView2 clipping during use.
- Physical audio output and native screen-reader behavior.

Browser checks and matching source measurements do not establish those runtime
facts. No proprietary reference code or recordings are distributed in this change.
