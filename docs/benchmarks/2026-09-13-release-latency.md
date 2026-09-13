# Release latency: baseline (Gate A)

Machine: AMD Ryzen 5 6600H, 6 cores / 12 threads, CPU decode.
Model: parakeet-tdt-0.6b-v2 int8, sherpa-onnx 1.13.7. Commit: ba2c1fd.

## Offline (`ov bench`, whole-utterance decode, 400 ms pause before release)

LibriSpeech `test-clean`, first utterances in id order. The installed app was
running but idle during the runs.

| set | threads | clips | audio | WER | release p50 | p90 | max |
|---|---:|---:|---:|---:|---:|---:|---:|
| short (2–12 s) | 4 | 25 | 170 s | 1.14% | 426 ms | 684 ms | 735 ms |
| short (2–12 s) | 6 | 25 | 170 s | 1.14% | 422 ms | 681 ms | 736 ms |
| long-form (6 × 4–15 s) | 4 | 5 | 254 s | 0.80% | 3,534 ms | 4,618 ms | 4,618 ms |
| long-form (6 × 4–15 s) | 6 | 5 | 254 s | 0.80% | 3,305 ms | 4,416 ms | 4,416 ms |

Thread decision: **stay at 4.** Six threads lowered p50 by 1% on the short set
(426 → 422 ms) and 6.5% on long-form (3,534 → 3,305 ms). The bar for adopting
it was 10% on both, and neither reached it. WER was identical.

Long-form release latency is ≈ 14 ms per second of audio here against ≈ 75 ms
in the app's history. The benchmark decodes clean read speech into a warm model
with nothing else competing for the CPU; the app does not have those luxuries.
The ratio between the sets, not the absolute figure, is what incremental
decoding has to change.

## In the app

### From history (`history.db`, before per-stage logging existed)

194 delivered Parakeet dictations, 2026-09-04 to 2026-09-13. `latency_ms` is
hook release → `Input::Injected`, the same figure `ov latency` reports as
`latency`.

| audio | n | latency p50 | p90 |
|---|---:|---:|---:|
| <3s | 29 | 253 ms | 512 ms |
| 3-7s | 26 | 443 ms | 602 ms |
| 7-15s | 48 | 820 ms | 1,138 ms |
| >15s | 91 | 2,160 ms | 5,441 ms |

### Per stage (`ov latency`)

**Pending.** Needs at least 30 real dictations in a build that includes the
stage log (commit 5296169 or later). Paste the `ov latency` table here when
they exist.

## What stands out

- `>15s`: release p90 is 5.4 s in the app, and 47% of dictations are in this
  bucket. Decode grows with length, so this is where incremental decoding pays.
- `7-15s`: p90 1.1 s, already over a second.
- Offline, long-form release p50 is 8× the short set's. That gap is the thing
  Gate B has to close.
- Per-stage outliers cannot be named until the stage log has data.
