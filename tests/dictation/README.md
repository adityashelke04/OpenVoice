# Dictation quality fixtures

`public.json` is deliberately blocked: no approved real clips were available in the implementation environment. An empty manifest must fail, never report perfect WER. Synthetic unit-test samples only validate the harness.

Populate a version-1 manifest with `approved: true`, `personal`, `license`, `source`, and `clips`. Each clip has `id`, relative `path`, the actual file `sha256`, a human-corrected `reference`, `categories`, `names`, and `numbers`. Names/numbers are explicit terms present in the reference, not inferred from capitalization. Use 16kHz mono signed PCM16 WAV or mono 16kHz FLAC. Forced-cut clips must exceed 20 seconds.

Cover `onset`, `low-volume`, `noise`, `names`, `numbers`, `code`, `punctuation`, `no-pause-release`, `forced-cut`, and `continuous`. Keep the same IDs/audio/references for whole and incremental comparisons. No-pause clips use zero release silence; others use 400ms. Retain punctuation in corrected references for separate punctuation review.

For a personal corpus, create an untracked folder outside this repository, record only with explicit consent, set `personal: true`, and pass `--allow-personal`. Nothing uploads recordings or transcripts. Do not add private audio or corrected text to CI artifacts.
