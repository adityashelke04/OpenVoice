# ADR 0012 — Capture streams, and dictation is decoded while it is spoken

- **Status:** Accepted
- **Date:** 2026-09-13
- **Builds on:** ADR 0001 (ports), ADR 0008 (Parakeet in-process)
- **Evidence:** `docs/benchmarks/2026-09-13-release-latency.md`

## Context

Releasing the dictation key waited for the whole recording to be decoded in
one call. Parakeet costs about 75 ms per second of audio on the reference
machine, and 47% of real dictations were longer than 15 s, so the typical long
dictation waited 2.1 s after the key came up. The decoder was never slow. It
was started late.

## Decision

1. `AudioSource` gains `start_streaming(levels, pcm)`, a provided method that
   defaults to `start`. `CpalAudioSource` converts to 16 kHz on its own thread
   every 40 ms and hands each chunk to `pcm`; what it hands over, concatenated,
   is exactly what `stop` returns. This extends a port rather than adding one,
   so ADR 0001's six ports stay six.
2. `ov-asr` decodes while the user speaks. A pure `SegmentPlanner` finds pauses
   in the audio. A 240 ms pause triggers a speculative decode of the audio so far.
   A 640 ms pause, once the segment is at least 3 s long, commits that segment
   (480 ms was tried first and cut too many sentences; see the Gate B record).
   An `IncrementalDecoder` runs those decodes on one long-lived thread and, on
   release, decodes only what is left, or nothing if the last speculative decode
   already covers it.
3. Anything unexpected falls back to decoding the whole utterance once: an
   adapter that did not stream, a sample-count mismatch, a segment that failed.
   The user's text never depends on the fast path working.

## Consequences

- Release latency stops growing with how long the user spoke, when they pause
  before releasing, which is the common case.
- The machine does decoding work while the user is still talking. Speculative
  decodes that speech then overtakes are wasted CPU, bounded by replacing any
  that have not started.
- Cutting at a pause can change words or punctuation at the join. The benchmark
  gate holds this to within 0.5 pp WER on long-form speech, and the pause
  thresholds are tunable from `ov bench`.
- The microphone's lifetime is unchanged. Streaming happens only between press
  and release.

## Rejected

- **Silero VAD** for pause detection: better in noise, but a second model file to
  ship and verify. Energy against an adaptive floor is tried first; Silero is the
  escalation if accuracy fails the gate.
- **GPU (DirectML)**: the prebuilt sherpa-onnx Windows static library is CPU-only,
  and a custom build undoes ADR 0008's single binary.
- **A streaming (online) transducer**: Parakeet TDT is an offline model. Changing
  model family would give up the accuracy ADR 0008 chose it for.
