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
| Audio could be exfiltrated | Every crate that touches audio, transcripts, the keyboard or history — `ov-core`, `ov-format`, `ov-audio`, `ov-input`, `ov-asr`, `ov-store`, `ov-cli` — is *sealed*: no HTTP client, TLS stack, or socket library anywhere in its transitive graph, build scripts included. A CI job (`scripts/check-no-network.sh`) fails the build if that changes. **Caveat, stated plainly:** the Tauri shell (`ov-app`) links `reqwest` transitively because Tauri depends on it unconditionally. No OpenVoice code calls it, and the same CI job asserts that `ov-app` takes no network dependency of its own, so telemetry or a crash uploader cannot be added without failing the build — but an HTTP client is in the shipped binary and it would be dishonest to say otherwise. Since 2026-08-09 `ov-app` also depends on `tauri-plugin-updater` for the update check ([ADR 0005](docs/adr/0005-in-app-updates.md)); it is named in the guard's allow-list, so it is a recorded exception rather than an unnoticed one, and adding any *other* network dependency still fails. The one other allowance is `ov-fetch` ([ADR 0009](docs/adr/0009-model-downloads.md)), which downloads an optional speech model only when you ask for one, and verifies it against a pinned SHA-256 before extracting it. |
| Audio could be retained | Held in RAM. The speech engine runs inside the app process, so audio never has to cross to another process and is not written to disk. The one exception is *Keep recordings* (`privacy.retain_audio`), off by default and meant for diagnosing a bad transcript: when you turn it on, recordings are saved to `%APPDATA%\OpenVoice\audio` and deleted after `privacy.audio_days` (7 by default). Source: `crates/ov-asr/src/recordings.rs`. |
| Transcripts could leak secrets | Text matching `privacy.redact_patterns` — by default OpenAI-style keys, GitHub tokens and AWS access key ids — is replaced with `[redacted]` before a transcript is written to history or logs (`crates/ov-core/src/redact.rs`). The text typed into your app is not altered. **Limit, stated plainly:** only secrets shaped like a pattern are caught. A password you dictate is ordinary words and will be stored. Set `privacy.history_days` to `0` to keep no history at all. |
| Telemetry | There is none. Not disabled by default — absent from the codebase, and kept absent by the same CI job. |
| Malicious release binary | Releases are built by public GitHub Actions from a tagged commit, with SHA-256 checksums published alongside. Updates are also signed with a minisign key whose public half is compiled into the app; an update that fails that check is discarded without running. The installer itself is not yet Authenticode code-signed, so Windows SmartScreen may warn on first run. |

### Not covered

- **Malware already on your machine.** If something else is running with your
  privileges, it can read OpenVoice's history database and hook the keyboard itself.
  OpenVoice cannot defend against that and doesn't claim to.
- **Injection into privileged windows.** Windows blocks synthetic input to elevated
  processes from a non-elevated one. This is correct behaviour and OpenVoice will not
  work around it.
- **Physical access.** History is stored unencrypted at rest. Encryption is
  planned; until then, treat `%APPDATA%\OpenVoice\history.db` as readable by anyone
  with your account.

## Supported versions

Fixes land on `main` first and reach you in the next release, which the app
offers to install for you. The current minor version and the one before it are
supported.

| Version | Supported |
|---|---|
| `1.0.x` (latest) | ✅ |
| `main` | ✅ |
| Anything older than 1.0 | ❌ |

## Disclosure

Coordinated disclosure. We'll agree a timeline with you, defaulting to 90 days or
until a fix ships, whichever comes first. Credit given unless you'd rather not be
named.
