# Contributing to OpenVoice

Thanks for looking. This document is meant to get you from clone to a passing test
run without guesswork.

## The one thing to understand first

OpenVoice is split into **pure** crates and **adapter** crates, and the split is
strict.

`ov-core` and `ov-format` must not depend on the operating system, the filesystem,
the network, an audio library, a GUI toolkit, or an async runtime. CI compiles them
for `wasm32-unknown-unknown` to prove it.

If that job fails, **do not add a feature flag to make it pass.** Move the offending
code into an adapter crate. The boundary is what keeps the formatting rules testable
in milliseconds instead of requiring a microphone, and it's the single most valuable
property this codebase has.

The happy consequence: **most contributions need no special hardware at all.** A new
formatting rule, a dictionary entry, or a state machine fix is pure logic with a fast
test. You don't need a GPU or even a working microphone.

## Setup

### Windows (full app)

```powershell
# Rust
winget install Rustlang.Rustup

# MSVC build tools -- required. VS Code is NOT sufficient; you need the C++ workload.
winget install --id Microsoft.VisualStudio.2022.BuildTools `
  --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# ASR sidecar, in a uv-managed virtualenv (https://astral.sh/uv)
winget install astral-sh.uv
uv venv
uv pip install -e sidecar nvidia-cublas-cu12 nvidia-cudnn-cu12

# Frontend. Also installs the Tauri CLI that "Running the app" below invokes --
# without this step that path does not exist yet.
npm --prefix apps/ui ci

cargo test --workspace
```

If `cargo` reports **`linker link.exe not found`**, the C++ workload didn't install.
Rust needs a linker even for `cargo check`, because proc-macro crates like `serde_derive`
are compiled as host DLLs. Re-run the Build Tools step.

**Short on disk?** The toolchain, build artifacts, and CUDA libraries come to roughly
16 GB. You can relocate all of it:

```powershell
setx RUSTUP_HOME D:\dev\rustup
setx CARGO_HOME  D:\dev\cargo
# then put  [build] target-dir = "D:\\dev\\cargo-target"  in %CARGO_HOME%\config.toml
```

**Python environment somewhere unusual?** The app looks for an interpreter in
`OPENVOICE_PYTHON`, then `VIRTUAL_ENV`, then `.venv/` and `sidecar/.venv/` in the
repo. Nothing else — no absolute path to anyone's machine is compiled in, and
none should be added. If yours lives elsewhere, name it once:

```powershell
setx OPENVOICE_PYTHON D:\dev\openvoice-venv\Scripts\python.exe
```

`ov` also takes `--python <path>` for a one-off.

### Any platform (pure crates only)

```sh
cargo test -p ov-core -p ov-format
```

No MSVC, no GPU, no microphone. This is where most work happens.

### Running the app

```powershell
cd crates/ov-app
node ../../apps/ui/node_modules/@tauri-apps/cli/tauri.js dev
```

A debug build always prefers the Python sidecar in your checkout over any frozen
one, so edits to `sidecar/` take effect on the next restart. A release build
prefers the frozen engine it was packaged with. Setting `OPENVOICE_ROOT` or
`OPENVOICE_PYTHON` forces the checkout in either case.

The first launch downloads `base.en` (~75 MB) before the window becomes usable.

### When something doesn't work

`ov` is the same engine without the GUI, and each subcommand isolates one link in
the chain — which is usually faster than reading a log:

```powershell
cargo run -p ov-cli -- doctor      # is the environment even ready?
cargo run -p ov-cli -- keytest     # is the hotkey reaching us at all?
cargo run -p ov-cli -- devices     # what microphones does Windows report?
cargo run -p ov-cli -- mictest     # record, save, and transcribe: capture only
cargo run -p ov-cli -- type "hi"   # injection only, no microphone, no model
cargo run -p ov-cli -- format "..." --trace   # the formatter, rule by rule
```

The app's own log is at `%APPDATA%\OpenVoice\openvoice.log`. It may contain text
you dictated, so read it before pasting it anywhere.

## Packaging

The machine that installs OpenVoice has no Python, so the sidecar is frozen into
a standalone folder and bundled as a Tauri resource:

```powershell
pwsh scripts/build-sidecar.ps1 -Clean
cd crates/ov-app
node ../../apps/ui/node_modules/@tauri-apps/cli/tauri.js build
```

Three things about this that are easy to get wrong:

- **The freeze must run first.** Tauri's resource walker accepts an empty folder,
  so on its own the build would produce an installer with no speech engine — a
  failure that only surfaces when someone runs the app. Two independent guards
  exist: `crates/ov-app/build.rs` panics on a release build when
  `sidecar/dist/openvoice-asr/openvoice-asr.exe` is missing, and
  `.github/workflows/release.yml` asserts the same thing before invoking
  `tauri build`. On a *debug* build the same `build.rs` creates an empty
  placeholder instead, which is why a fresh checkout can run `cargo test` without
  standing up Python and PyInstaller first.
- **`build-sidecar.ps1` is not finished when PyInstaller succeeds.** A frozen
  binary can die on its first import because a hidden import was missed, which
  static analysis cannot see. The script sends a real `probe` request over the
  protocol and fails if it does not get a valid reply.
- **CUDA is excluded on purpose.** `nvidia-cublas-cu12` and `nvidia-cudnn-cu12`
  are 1.9 GB — 88% of the dependency tree — against roughly 260 MB for everything
  else combined, and they do nothing on a machine without an NVIDIA GPU. A packaged
  build runs on CPU; `engine.py` picks up `OPENVOICE_CUDA_DIR` when the libraries
  are available by another route. The frozen folder comes to ~173 MB and the
  installer to 68 MB.

Tauri's own build hooks live in the root `package.json` rather than being spelled
out as relative paths in `tauri.conf.json`. `npm run` searches upwards for a
manifest, so `npm run build:ui` resolves identically from any directory in the repo
— where a `--prefix ../../apps/ui` baked into the config silently resolved against
the wrong root depending on who invoked the build.

## Working on the formatter

This is the highest-leverage part of the project and the friendliest to newcomers.

A rule is a struct implementing `Rule`, roughly thirty lines. Add it to
`crates/ov-format/src/rules.rs`, insert it in the pipeline order in `lib.rs`, and add
tests.

**Order in the pipeline is load-bearing.** Fillers run first so a stray "um" can't
break a multi-word command match. Capitalization runs late, after identifiers have
become `Tok::Lit`, which it is forbidden to touch — capitalizing `useEffect` into
`UseEffect` turns working code into a compile error.

Three rules for rule-writing:

1. **Always provide an escape hatch.** `literally comma` must emit the word "comma".
   Without escapes, some sentences become undictatable, which is maddening and hard
   to diagnose.
2. **Never silently delete the user's words.** If a transform can't complete — "camel
   case" with nothing after it — leave the input alone.
3. **Add a test that would fail without your rule, and one that proves it doesn't
   fire when it shouldn't.** The second is the one that prevents regressions.

## Testing

```sh
cargo test --workspace                                        # everything
cargo test -p ov-format                                       # fast inner loop
cargo check -p ov-core -p ov-format --target wasm32-unknown-unknown   # purity
cd sidecar && uv run --with pytest pytest -q                  # sidecar protocol
```

Every one of those is model-free and runs in seconds. That is deliberate:
downloading 1.6 GB of weights on each push would make CI slower than the review it
supports, so nothing in the automated suite loads a model. The cost is that
accuracy is verified by hand — record a WAV into `fixtures/audio/` (gitignored, so
nobody's voice ends up in the repository) and run
`cargo run -p ov-cli -- transcribe your.wav`.

CI runs these on every push and pull request, split across jobs so a failure names
its own cause: the full workspace on Windows, the platform-independent crates on
Linux, the `wasm32` purity check, `scripts/check-no-network.sh`, `cargo deny`, the
sidecar's ruff and pytest, and the frontend's lint, typecheck and build. See
[`.github/workflows/ci.yml`](.github/workflows/ci.yml).

### The app-compatibility matrix

Text injection breaks per-application in ways no unit test catches. Before a release
we manually verify against VS Code, Cursor, Windows Terminal, Chrome, Slack, Discord,
Notion, IntelliJ, and Obsidian. If you find an app where injection misbehaves, that's
a valuable bug report — include the app, its version, and whether the text was
partially delivered.

## Commits and PRs

[Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`,
`docs:`, `refactor:`, `test:`, `chore:`.

The changelog is **written by hand, in the same PR as the change** — not generated
from commits at release time. `CHANGELOG.md` explains why, and which prefix maps to
which section. If your change alters something a user can observe, add an entry
under `## Unreleased` while you still have the context.

Keep PRs to one concern. A PR that fixes a bug and reorganizes a module is two PRs.

## Architecture decisions

Anything that changes the shape of the system — a new port, a new runtime dependency,
a different ASR backend — gets an ADR in `docs/adr/`. Copy the format of an existing
one. Record the alternatives you rejected and why; six months later that's the part
that matters, and it's what stops a settled question from being re-litigated.

## Security

The app installs a global keyboard hook and synthesizes input. Changes in
`crates/ov-input/` get extra scrutiny — not distrust of you, just proportionate care
for the blast radius. If you find a vulnerability, see [`SECURITY.md`](SECURITY.md)
rather than opening a public issue.

## Code of conduct

[Contributor Covenant](CODE_OF_CONDUCT.md). Be decent.
