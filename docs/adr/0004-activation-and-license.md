# ADR 0004 — Push-to-talk on Right Ctrl; Apache-2.0 license

- **Status:** Accepted
- **Date:** 2026-07-31

Two small decisions recorded together because neither warrants its own record.

## Activation: hold Right Ctrl (push-to-talk)

### Context

Three models were considered: hold-to-talk, press-to-toggle, and both.

### Decision

Push-to-talk, default binding **Right Ctrl**, held. Toggle mode is deferred to v0.2
and will be a *second, separate* binding rather than a mode switch on the first.

### Rationale

Push-to-talk has one property the alternatives lack: **the microphone cannot be left
open**. Release the key and capture stops, unconditionally. For an always-running app
with microphone access, that is a privacy guarantee the user can verify with their
own fingers, not a promise in a settings dialog.

Right Ctrl specifically: present on essentially every Windows keyboard, almost never
used (most people use Left Ctrl for shortcuts), reachable without a chord, and not
claimed by common IDE keymaps.

### Implementation constraint

`RegisterHotKey` is unusable — it delivers a single "pressed" notification with no
key-up event, so push-to-talk cannot be built on it. This forces
`SetWindowsHookEx(WH_KEYBOARD_LL)`. See [`../ARCHITECTURE.md`](../ARCHITECTURE.md)
§8 for the constraints that imposes on the hook thread.

Because the trigger is itself a modifier key, the injector must explicitly release
Ctrl in the synthetic input stream before sending `Ctrl+V` for clipboard paste.
Otherwise the physical Ctrl state corrupts the paste and it silently does nothing.
This is recorded here because it is non-obvious and will otherwise be rediscovered
painfully.

### Consequences

Not ideal for very long dictation — holding a key for 90 seconds is uncomfortable.
That is what the toggle binding is for. Some keyboards lack a right Ctrl; the
binding is user-configurable.

> **Status note (2026-08-03).** The toggle binding slipped past v0.2 and is still
> unbuilt. `ActivationMode::Toggle` exists in the config schema and nothing reads
> it; activation is push-to-talk only. The bindable keys are a closed enum — Right
> Ctrl, Right Alt, Right Shift, Caps Lock, F13, F14, Scroll Lock.

## License: Apache-2.0

### Decision

Apache-2.0, with `LICENSE` at the repository root and SPDX headers omitted from
source files (the license file and `Cargo.toml` metadata are sufficient).

### Rationale

Apache-2.0 grants patent rights explicitly and terminates them on patent litigation.
MIT is silent on patents. For a tool that may be adopted inside companies, the
explicit grant removes a question that legal review would otherwise raise, at no
practical cost to individual users. It is also the prevailing norm for serious Rust
tooling, so it will not surprise contributors.

### Consequences

Slightly longer license text and a `NOTICE` convention to respect. Dependency
licenses are checked in CI by `cargo deny` — copyleft dependencies (GPL/AGPL) are
rejected so the distribution terms stay coherent.
