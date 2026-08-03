# Security Policy

## Reporting a vulnerability

Please **do not open a public issue.** Use GitHub's private reporting:
[Report a vulnerability](https://github.com/adityashelke04/OpenVoice/security/advisories/new).

Include what you did, what happened, what you expected, and the OpenVoice version
(shown in Settings, and in the installer filename) — or the commit you built
from, if you are running from source. A proof of concept helps enormously.

You'll get an acknowledgement within 72 hours and an assessment within a week. This
is a small project, not a company with a response team — if it's serious and you
haven't heard back, feel free to escalate by any means you can find.

## Threat model

Being direct about this, because users deserve it:

**OpenVoice installs a global low-level keyboard hook and can synthesize keyboard
input.** These are the defining capabilities of a keylogger. Any program that wants
them warrants suspicion, and that includes this one.

What the design does about it:

| Concern | Mitigation |
|---|---|
| Hook could log every keystroke | The hook procedure compares a virtual key code against your configured chord and discards the event. It has no storage and no path to one. Source: `crates/ov-input/`. |
| Audio could be exfiltrated | Every crate that touches audio, transcripts, the keyboard or history — `ov-core`, `ov-format`, `ov-audio`, `ov-input`, `ov-asr`, `ov-store`, `ov-cli` — is *sealed*: no HTTP client, TLS stack, or socket library anywhere in its transitive graph, build scripts included. A CI job (`scripts/check-no-network.sh`) fails the build if that changes. **Caveat, stated plainly:** the Tauri shell (`ov-app`) links `reqwest` transitively because Tauri depends on it unconditionally. No OpenVoice code calls it, and the same CI job asserts that `ov-app` takes no network dependency of its own, so telemetry or an update ping cannot be added without failing the build — but an HTTP client is in the shipped binary and it would be dishonest to say otherwise. |
| Audio could be retained | Held in RAM, with one exception worth naming: the speech engine is a separate process, and audio reaches it as a temporary WAV under `%TEMP%\openvoice\`, deleted immediately after the decode returns — success or failure (`crates/ov-asr/src/wav.rs`). Nothing else writes audio to disk. There is a `privacy.retain_audio` field in the config schema and a toggle for it in Settings, but **no code reads it today**: audio is never retained, and the switch does nothing. Treat it as reserved, not as a control. |
| Transcripts could leak secrets | **Not yet mitigated.** `privacy.redact_patterns` exists in the config schema with sensible defaults for API-key and token shapes, but nothing applies it — history and logs currently record transcripts verbatim. If you dictate a secret, it is in `history.db` and possibly in `openvoice.log`. This is the most significant gap in this table and is tracked for v0.3. |
| Telemetry | There is none. Not disabled by default — absent from the codebase, and kept absent by the same CI job. |
| Malicious release binary | Releases are built by public GitHub Actions from a tagged commit, with checksums published alongside. |

### Not covered

- **Malware already on your machine.** If something else is running with your
  privileges, it can read OpenVoice's history database and hook the keyboard itself.
  OpenVoice cannot defend against that and doesn't claim to.
- **Injection into privileged windows.** Windows blocks synthetic input to elevated
  processes from a non-elevated one. This is correct behaviour and OpenVoice will not
  work around it.
- **Physical access.** History is stored unencrypted at rest in v0.1. Encryption is
  planned; until then, treat `%APPDATA%\OpenVoice\history.db` as readable by anyone
  with your account.

## Supported versions

Pre-1.0, only the latest release gets fixes — currently `v0.1.0`. Fixes land on
`main` first and reach you in the next release. From 1.0: the current minor
version and the one before it.

| Version | Supported |
|---|---|
| `v0.1.0` (latest) | ✅ |
| `main` | ✅ |
| Anything older | ❌ |

## Disclosure

Coordinated disclosure. We'll agree a timeline with you, defaulting to 90 days or
until a fix ships, whichever comes first. Credit given unless you'd rather not be
named.
