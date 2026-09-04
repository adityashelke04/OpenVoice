# ADR 0009 — Optional models, downloaded on request, from one network-capable crate

- **Status:** Accepted
- **Date:** 2026-09-04
- **Amends:** [ADR 0008](0008-parakeet-in-process.md) (Parakeet as the only model)

## Context

ADR 0008 shipped one model and deleted the picker, and the reasoning held: on the
measurements, Parakeet TDT 0.6B v2 was faster than the fastest Whisper tier *and*
more accurate than the most accurate, so there was no trade left to expose.

Two questions survived it, and neither is answered by a better default:

- **"I dictate in Spanish."** Parakeet v2 is English-only. v0.5.0's answer was to
  remove the language setting, which does not so much answer the question as
  decline it.
- **"This machine has 4 GB of RAM."** The model occupies ~750 MB resident. That
  is fine on the reference laptop and is not fine everywhere.

Shipping all three in the installer is not an option: it would be roughly 1.2 GB
of models for two that most people will never load.

## Decision

Restore a Speech model screen with three entries.

| id | role | ships | on disk |
|---|---|---|---:|
| `parakeet-tdt-0.6b-v2` | default, English | in the installer | 631 MB |
| `parakeet-tdt-0.6b-v3` | 25 languages | downloaded on request | 641 MB |
| `whisper-tiny.en` | low memory | downloaded on request | 99 MB |

Downloading lives in a **new `ov-fetch` crate**, which is the only crate in the
workspace permitted to open a socket.

## Why a separate crate, and not just `ov-asr`

`scripts/check-no-network.sh` proves that every crate touching the microphone,
the transcript, the keyboard or the history database has no path to an HTTP
client, TLS stack or socket anywhere in its graph. `ov-asr` is in that sealed
set, and it is the crate that holds the microphone.

Putting the downloader there would have traded a tested guarantee for a
convenience. Putting it in its own crate keeps the guarantee and makes "which
code can reach the internet" a question answered by reading one `Cargo.toml`.

The guard enforces this in two ways rather than one: `ov-fetch` is matched by the
same pattern as any other network client — it wraps `ureq`, so without a name of
its own it could be added to a sealed crate and match none of the existing
words — and a `NEVER_FETCH` check asserts that no sealed crate takes it as a
run-time dependency. `ov-asr` does take it as a *dev*-dependency, for one ignored
end-to-end test; that distinction is asserted, not assumed.

## Consequences

**Good.** Multilingual dictation is available again without reinstalling.
Machines that cannot spare 750 MB have an option. The default is unchanged, still
bundled, and still works offline from the moment the installer finishes — nobody
who does not want a download is made to have one.

**Costs, stated plainly:**

- **The local-first claim needs its exception back.** v0.5.0 could say dictation
  had no network path at all. That was briefly true and is now false, and the
  privacy copy says so again: two network uses, both user-initiated — a model you
  choose to download, and the update check you can switch off. Leaving the
  stronger sentence in place would have been the worse outcome.
- **Downloaded weights are code the app executes**, so every archive is verified
  against a SHA-256 pinned in the catalogue before a byte is extracted, and
  extraction stages elsewhere and moves into place only when complete. A refused
  or interrupted download leaves nothing that `locate::is_installed` would later
  report as ready. This is the claim ADR 0003 once made falsely; it is now code,
  and tested.
- **A second storage location.** Downloads go under `%APPDATA%`, because the
  install directory needs administrator rights and a download started from a
  settings screen must not raise a UAC prompt. The bundled model stays beside
  the executable where the uninstaller reclaims it.

## The failure mode this is designed around

A user can select a downloaded model and then delete its files; a transfer can be
interrupted; `settings.toml` can be hand-edited to a name that was never valid.
None of those may leave the app unable to transcribe.

The bundled model is always on disk, cannot be deleted from the UI, and is what
everything falls back to. **There is no state in which OpenVoice legitimately
cannot hear you**, and that is the property the whole design is arranged to
protect — it is why exactly one model is bundled, why the screen refuses to
delete it, and why selecting an absent model is disabled rather than merely
discouraged.

## What was deliberately not built

**User-supplied models.** An earlier proposal offered an "Add your own model"
folder picker. It was dropped by the product owner, and the reasoning is worth
recording: a model needs its kind, its tokeniser, its sample rate and its file
layout to be right, and a misconfigured one does not fail — it transcribes
badly, which reads as *OpenVoice is broken* rather than *I wired this up wrong*.
A curated list of three answers the same questions with none of that surface.
