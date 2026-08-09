# ADR 0005 — An in-app update check, and the local-first promise it spends

- **Status:** Accepted
- **Date:** 2026-08-09
- **Amends:** the "network by exception" principle in `README.md` and `SECURITY.md`

## Context

OpenVoice v0.1.1 has no way to tell a user that v0.1.2 exists. Distribution is a
tagged GitHub Actions build producing an unsigned NSIS installer with a published
SHA-256; discovering a new one requires the user to visit the releases page on
purpose.

That is a bad fit for what this project actually is. The value proposition is a
formatting pipeline improved one rule at a time — a missing dictionary term here,
a punctuation edge case there. Each of those improvements is small, and each is
worthless to anyone still running the build from before it landed. A competitive
review in August 2026 found this to be the single largest gap against every
comparable product: Handy ships the Tauri updater with minisign verification,
BridgeVoice self-updates with a server-driven gate for clients stuck on a known-bad
build, and even the most privacy-conservative comparator (`whisper-local`) offers an
opt-in daily release check.

The tension is real and not rhetorical. The README states that the only outbound
request OpenVoice makes is a model download the user asked for, and
`scripts/check-no-network.sh` turns that into a build failure rather than a
sentence. An update check is *not* a request the user individually asked for.
Adding one weakens a guarantee that is currently, unusually, true.

Worse, it would have weakened it **silently**. The guard matches HTTP and TLS
crates by name (`reqwest`, `hyper`, `rustls`, …). `tauri-plugin-updater` matches
none of those words, so adding it would have left the script's output completely
unchanged — the exact drift the script exists to prevent, defeated by a naming
coincidence.

## Decision

Ship an update **check**, not an auto-updater, on these terms:

1. **`tauri-plugin-updater`, verified with minisign.** The public key is compiled
   into the binary (`crates/ov-app/tauri.conf.json`, `plugins.updater.pubkey`).
   Every downloaded artifact is verified against it before anything executes.
2. **Check and install are separate operations.** `update::check` resolves a
   version and stops. `update::install` runs only from a command the user reached
   by pressing a button. There is no code path from "a newer version exists" to
   "it was applied".
3. **On by default, once per launch, disableable in one click.**
   `Config.updates.check_on_launch` defaults to `true`. When false, no request is
   made at all — it does not send an opt-out flag, it returns before touching the
   network.
4. **The request carries nothing.** A static signed manifest is fetched. No
   identifier, no version histogram, no machine fingerprint, because there is
   nowhere in the implementation to put one.
5. **A failed check is silent.** Offline is a normal state for this application.
   An error toast on every launch aboard a train is how a feature people want gets
   switched off.
6. **The allowance is written into the guard.** `check-no-network.sh` gains an
   explicit `ALLOWED_DIRECT` list naming `tauri-plugin-updater` and citing this
   ADR, and `FORBIDDEN` is extended to match updater and HTTP plugins by name.
   Anything not on the list fails. Verified by running the guard with the
   allowance removed and confirming it fails.

## Rationale

**Why on by default.** This was the one genuine values call, and the cautious
option was rejected deliberately. A privacy setting nobody finds protects nobody:
defaulting the check off would mean approximately everyone stays on the version
they first installed, which is the status quo this ADR exists to fix. A check that
is disclosed on the Settings screen in plain language and switched off in one click
is a decision the user actually gets to make. `whisper-local` defaults its check
off; that is a defensible choice for a tool distributed through pip and winget,
where the package manager already answers the question. OpenVoice has no such
channel yet.

**Why the signature is load-bearing.** The installer is not code-signed, so
SmartScreen already warns about it. An updater that merely downloaded an executable
over HTTPS would be a *worse* security position than the status quo, because the
user would stop seeing that warning while the app fetched code on its own. Minisign
verification against a key inside the binary does not depend on trusting GitHub,
TLS, or the maintainer's account — which is precisely the property that makes this
trade acceptable rather than merely convenient.

**Why not check-only-with-a-link.** Opening a browser to the releases page and
making the user re-run an unsigned installer past a SmartScreen warning is the
flow that already exists, and its completion rate is the reason this ADR exists.

## Consequences

**Good.** Rule improvements reach users. The signing key gives OpenVoice a
verifiable artifact chain for the first time, independent of code signing, which is
a prerequisite for the winget submission on the v0.5 roadmap. Every prior version
stays downloadable, so a bad release is recoverable.

**Costs.** The local-first claim now needs one sentence of qualification wherever
it appears, and `README.md` and `SECURITY.md` are updated in this change rather
than later. A private signing key becomes a release-critical secret: losing it means
existing installs can never be updated again, and rotating it requires every user to
reinstall by hand. `TAURI_SIGNING_PRIVATE_KEY` must be set as a repository secret,
and the release workflow fails loudly on a tag if it is missing, because the failure
mode otherwise is an installer with no signature beside it and a fleet of clients
rejecting an update they were correctly offered.

**Deliberately not done.** No server-driven forced-upgrade gate, of the kind
BridgeVoice operates. It requires an endpoint that can decide what a user's own
machine is allowed to run, which is a different relationship than this project
wants. No release channels, no staged rollout, no update telemetry.

**Follow-up.** Code signing (v0.5) removes the SmartScreen warning and makes the
minisign key the second of two independent verifications rather than the only one.
If a release is ever pulled, the manifest at the fixed `updater` tag is the single
place to revert, which is the reason it is a separate release from the versioned
one.
