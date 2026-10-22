# Dictation reliability evaluation — 2026-10-22

The Windows package, full native tests and actual decoder-worker recovery tests now pass. The user supplied the native archive after sandbox downloads failed; its official SHA256 was verified. Cached Parakeet v2 weights and one upstream public smoke clip were independently verified against the primary maintainer source. A limited real-model worker/thread probe was performed below. **No corpus WER or real app release-to-delivery latency result exists.** The complete approved corpus and other model weights are absent; personal recordings were neither authorized nor inspected. Synthetic unit tests prove harness behavior only.

## Decision

Retain bundled Parakeet v2, CPU execution, and four decode threads. The limited probe below provides no basis for changing defaults: four threads have a lower median decode wall time and lower CPU use than six in these three samples, while broader corpus/UI responsiveness and other model comparisons are unavailable. v2/v3/tiny.en corpus accuracy, cold app startup, whole-app peak memory, UI responsiveness, GPU behavior, fallback rate and delivery failure rate remain **unmeasured**. Existing catalogue measurements are historical, not a substitute for this evaluation. No new model/backend was integrated.

## Measured native worker smoke probe

Hardware metadata: AMD Ryzen 5 6600H with Radeon Graphics (Windows CPU registry), 12 logical processors (.NET). This does not establish an available/supported GPU inference provider. Same new release EXE, same verified 7.435-second public human clip, unconditioned hints, CPU, three decodes per fresh worker. Installed OpenVoice stayed running; concurrent installer extraction used disk/CPU. These are small smoke measurements under that workload, not a statistically controlled benchmark or an app latency gate.

| Threads | Cold worker warm, ms | Three whole-decode wall times, ms | Child CPU for three decodes, ms | Peak child working set, bytes |
|---:|---:|---|---:|---:|
| 1 | 2116 | 1081, 1472, 1449 | 3984.375 | 829026304 |
| 2 | 2060 | 564, 694, 1022 | 5046.875 | 829407232 |
| 4 | 2143 | 477, 476, 845 | 8578.125 | 830734336 |
| 6 | 2114 | 470, 509, 753 | 12687.5 | 831688704 |

Text was nonempty and consistent within each setting. The maintainer's public sample output is not an independently human-corrected reference, so no WER is assigned to this probe. Warm time includes child creation and native model construction, not app startup. Wall times include the worker IPC but exclude microphone stop, formatting and delivery. Peak working set is the worker process lifetime, not the parent/UI or all app processes. No bucket p50/p90 acceptance claim is made from three samples.

The actual production worker supervisor recovered from controlled process termination and a suspended native child at its **60,056ms** deadline; active cancellation/restart took **56ms**, stale cancellation preserved a newer request, and no owned workers remained afterward. These are native fault/recovery results, not microphone failures or spontaneous ONNX crashes. Full native workspace tests also exercised real speech whole/incremental agreement, silence/noise and vocabulary regressions.

Raw measurements: `.build/native-worker-smoke/run.jsonl`; details `.superpowers/sdd/2026-10-22-dictation-reliability/native-worker-smoke-report.md`. The exact NSIS payload executable separately decoded the same public clip against the extracted and rehashed bundled weights, matching the generic release executable. Native owned-window tests safely skipped because foreground acquisition was denied; clipboard was nonempty/unavailable. No live delivery result is counted.

## Reproducible quality runs

Approve and populate `tests/dictation/public.json` following its README. Keep fixture IDs, bytes, references, annotations, release pauses and thread counts fixed between comparisons. Archives are verified before extraction; manifests pin every fixture's SHA-256. `scripts/fetch-model.ps1` pins the bundled archive and verifies installed-file digests. No actual public fixture hash is invented here.

```powershell
./scripts/fetch-model.ps1
cargo run --release -p ov-cli -- quality --manifest tests/dictation/public.json --model-dir models/parakeet-tdt-0.6b-v2 --max-wer 10 --threads 4 --show-text
```

`--max-wer 10` is an explicit example gate policy, not a measured score or a claim this model meets it. Set the agreed accuracy limit before evaluation. Both whole and paced incremental use identical clips and the default unconditioned DecodeHint (no personal dictionary/hotword bias). The command fails on empty/incomplete corpus, absent weights, checksum mismatch, missing required categories/annotations, excess WER, annotated name/number losses, formatter word changes, or incremental WER worse than whole. It prints WER, decoder-only p50/p90/p95/max, reuse and fallback counts, name/number missing-term counts, punctuation mismatch clips and formatter word changes. `--show-text` prints local references/hypotheses for human meaning review; no transcript is uploaded. Punctuation mismatch is a review flag, not a semantic-equivalence score.

Decoder release timings exclude capture-stop, formatting and injection. `model_load_warm_ms` measures model construction/warmup, not cold app startup. Peak memory and app delivery failures are explicitly unmeasured by this decoder harness. Capture those independently during the app gate; never label a worker benchmark end-to-end.

For v3/tiny.en, explicitly supply their catalogue model ID and directory containing their real weights. Run the same corpus with `--threads 2`, `4`, and `6`, recording responsiveness and process peak working set externally. Compare only runs on the same machine/power configuration. Evaluate a GPU provider only after verifying current official support and installed hardware; this change adds no GPU path and changes no CPU fallback.

## Live app Gate C

Deliberately collect a fresh single app run with at least 30 successful dictations: at least five each under 3s, 3–7s, 7–15s, and 15s or longer. Use approved spoken fixtures, record target/window identity and intended delivery, and record any failures rather than excluding them. The timing log must include `outcome=delivered` plus capture-stop, queue, decode, format, inject and total. Copy only those explicitly approved timing records into a local evaluation file; do not read private history or unrelated app logs to assemble a corpus.

```powershell
cargo run --release -p ov-cli -- latency --log D:/approved-evaluation/app-timings.txt --end-to-end --gate
```

Gate C rejects missing stages, malformed records, repeated session IDs, absent/failed delivery outcomes, inconsistent stage totals, fewer than 30 records, or undersampled buckets. Every bucket needs p50 ≤250ms and p90 ≤600ms. These thresholds are the existing target. **Gate C has not been performed or passed.** Parser tests using artificial timestamps are not app measurements.

## Opt-in personal workflow

Create a private folder outside the repository only after consent. Record intentional test dictations, manually correct references, annotate exact names/numbers, include the same categories and hashes, and set `personal: true` and `approved: true`. Run `quality --manifest <private-path> --model-dir <weights> --max-wer <agreed-limit> --allow-personal`. The opt-in flag is required before opening personal audio. Keep recordings, corrected transcripts, and `--show-text` output local. Personal accent/vocabulary accuracy remains unverified until this is done.

## Verification status

Actual `wer.rs` regression failed when `3.14` and `3 14` normalized identically, then passed after retaining decimal/sign distinctions. Exact annotated terms also distinguish Ann/Joanne and signed numeric values. Actual latency modules failed incomplete-stage/empty-gate regressions, then passed validation tests including complete and slow artificial runs. Empty LibriSpeech selection failed the new regression before returning an explicit error. Missing manifest/weights, unavailable fixtures and personal opt-in behavior are tested using disposable data.

The separate manually dispatched `dictation-quality.yml` requires an approved populated public manifest and checksum-pinned weights; the checked-in empty manifest intentionally blocks it until real fixtures are approved. Lightweight module tests can run with `scripts/test-quality.ps1` without native speech libraries. Local native model/unit and worker smoke checks now pass; corpus quality CI and app/hardware measurements remain open, with no fabricated scores. Local SAPI synthetic fixture generation failed `HRESULT 0x80045040`; it supplied no substitute corpus or synthetic ASR result.

Cached weights are prepared at `D:/Orca/workspaces/OpenVoice/skimmer/models/parakeet-tdt-0.6b-v2`. Per-file verification is recorded in `.build/model-verification.json` and `.superpowers/sdd/2026-10-22-dictation-reliability/model-provenance.md`. Four binary hashes match published maintainer LFS pointers; tokens match independently reconstructed immutable upstream contents. The pinned compressed model archive was **not** downloaded or checked in this session. No `verified-files.json` claiming archive verification was fabricated.
