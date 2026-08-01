# Security Policy

## Reporting a vulnerability

Please **do not open a public issue.** Use GitHub's private reporting:
[Report a vulnerability](https://github.com/adityashelke04/OpenVoice/security/advisories/new).

Include what you did, what happened, what you expected, and the OpenVoice version.
A proof of concept helps enormously.

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
| Audio could be exfiltrated | Every crate that touches audio, transcripts, the keyboard or history — `ov-core`, `ov-format`, `ov-audio`, `ov-input`, `ov-asr`, `ov-cli` — is *sealed*: no HTTP client, TLS stack, or socket library anywhere in its transitive graph, build scripts included. A CI job (`scripts/check-no-network.sh`) fails the build if that changes. **Caveat, stated plainly:** the Tauri shell (`ov-app`) links `reqwest` transitively because Tauri depends on it unconditionally. No OpenVoice code calls it, and the same CI job asserts that `ov-app` takes no network dependency of its own, so telemetry or an update ping cannot be added without failing the build — but an HTTP client is in the shipped binary and it would be dishonest to say otherwise. |
| Audio could be retained | Held in RAM, dropped after transcription. Writing audio to disk requires explicitly enabling `privacy.retain_audio`. |
| Transcripts could leak secrets | Configurable regex redaction runs before anything is written to history or logs. Defaults cover common API key and token formats. |
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

Pre-1.0: only the latest release gets fixes. Once 1.0 ships, the current minor
version and the one before it will be supported.

## Disclosure

Coordinated disclosure. We'll agree a timeline with you, defaulting to 90 days or
until a fix ships, whichever comes first. Credit given unless you'd rather not be
named.
