# ADR 0002 — Tauri v2 + Rust core, React/TypeScript frontend

- **Status:** Accepted
- **Date:** 2026-07-31

## Context

OpenVoice is an always-resident background app. It idles far more than it works, and
it must be invisible when idle: if it costs a quarter gigabyte of RAM and shows up in
Task Manager, the user uninstalls it no matter how good the transcription is.

It also needs two pieces of deep OS integration that are unusually hostile to
scripting runtimes:

1. A `WH_KEYBOARD_LL` hook whose callback must return in well under 10 ms or Windows
   silently evicts it. Any GC pause or IPC hop in that path breaks the app in a way
   that is intermittent and miserable to debug.
2. Synthetic input injection via `SendInput` with correct modifier-state handling.

## Decision

Rust core, Tauri v2 shell, React + TypeScript + Vite frontend.

## Rationale

| | Tauri + Rust | Electron + Node |
|---|---|---|
| Idle RAM | ~60 MB | ~250 MB |
| Installer | ~15 MB* | ~120 MB |
| Keyboard hook | native, in-process, no GC | native module (node-gyp) or helper exe |
| Realtime audio callback | no GC pauses | GC pauses in the callback path |
| Setup cost | rustup + MSVC (~6 GB, one time) | none |

\* The shell alone. The shipped installer is **68 MB**, because it also carries the
frozen speech engine — a cost belonging to ADR 0003, not to this decision, and one
Electron would have paid identically.

The Electron path front-loads convenience and back-loads permanent cost, in exactly
the areas (footprint, hook latency) where this product cannot compromise. The Rust
toolchain install is a one-time cost paid on day one.

React/TypeScript for the frontend rather than a Rust-native GUI: the settings window
is ordinary CRUD and form UI where the web stack is genuinely more productive, and
Tauri's webview is already in the process.

## Consequences

**Good.** Small, fast, no native-module build pain, one self-contained binary,
first-class Win32 access, and a webview UI that is pleasant to build.

**Costs.** Requires rustup + MSVC Build Tools + Windows SDK before the first build
(~6 GB). Contributors face the same barrier — `CONTRIBUTING.md` must make setup
frictionless. Two languages in the repo.

**Note on this machine.** C: had insufficient free space, so `CARGO_HOME`,
`RUSTUP_HOME`, the cargo target directory, and VS Build Tools all live on D:. This
is machine-local configuration held in environment variables and the global cargo
config; no absolute path from this machine appears anywhere in the repository.
