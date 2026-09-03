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

# The speech model (~482 MB, verified against a pinned SHA-256). There is no
# Python and no virtualenv: the engine links into the binary.
pwsh scripts/fetch-model.ps1

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

**Model somewhere unusual?** The app looks in `OPENVOICE_MODEL_DIR`, then beside
the executable, then in `models/` in the checkout. Nothing else — no absolute path
to anyone's machine is compiled in, and none should be added. If yours lives
elsewhere, name it once:

```powershell
setx OPENVOICE_MODEL_DIR D:\dev\parakeet-tdt-0.6b-v2
```

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

The model is loaded from `models/` in the checkout, or from `OPENVOICE_MODEL_DIR`.
Nothing is downloaded at run time; if the model is absent the app says so and
names the paths it looked in.

Startup loads ~750 MB of weights and takes two to three seconds before the window
is usable.

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

The engine links into the binary, so the only extra payload is the model:

```powershell
pwsh scripts/fetch-model.ps1
cd crates/ov-app
node ../../apps/ui/node_modules/@tauri-apps/cli/tauri.js build
```

Three things about this that are easy to get wrong:

- **The model is not a Tauri resource, and must not become one.** Tauri's NSIS
  updater downloads the whole installer on every update, so a bundled model would
  turn every patch release into a ~550 MB download for every user. It is installed
  by [`installer-hooks.nsh`](crates/ov-app/installer-hooks.nsh.in) instead, and
  `.github/workflows/release.yml` fails the build if the updater artifact ever
  exceeds 100 MB — which is what would happen the moment someone "simplified" this
  back into `bundle.resources`.
- **The hook is generated, not hand-written.** `build.rs` renders
  `installer-hooks.nsh` from the `.in` template with the model paths baked in.
  This matters because NSIS `!ifdef` tests an NSIS *define* and Tauri does not
  forward environment variables as defines: the first version guarded the copy
  with `!ifdef MODEL_SOURCE_DIR`, the guard was never true, and the build
  cheerfully produced a 9 MB installer containing no speech model and no warning.
  Edit the `.in` file; the generated one is gitignored.
- **A missing model warns rather than fails.** Packaging locally without a 482 MB
  download is a legitimate thing to want, so `build.rs` prints a `cargo:warning`
  and emits an empty hook. The real guarantee is in `release.yml`, which asserts
  the model exists before invoking `tauri build`. If you build an installer by
  hand and it comes out around 9 MB, that is this warning you scrolled past.

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
cargo test --workspace -- --test-threads=1                    # everything
cargo test -p ov-format                                       # fast inner loop
cargo check -p ov-core -p ov-format --target wasm32-unknown-unknown   # purity
```

`--test-threads=1` for the full run: each `ov-asr` test that touches the model
loads ~750 MB of weights, and running them in parallel needs several gigabytes.

Most of the suite is model-free and runs in seconds. `ov-asr`'s tests are not —
they decode real audio through the real model, because a mocked recognizer would
only prove the mock works. They **skip rather than fail** when the model is
absent, so a fresh checkout that has not run `scripts/fetch-model.ps1` gets a
green suite and a printed reason instead of a wall of red. CI fetches the model,
so they run there.

What is still verified by hand is accuracy on *your own voice*, which no
benchmark substitutes for: record a WAV into `fixtures/audio/` (gitignored, so
nobody's voice ends up in the repository) and run
`cargo run -p ov-cli -- transcribe your.wav`.

CI runs these on every push and pull request, split across jobs so a failure names
its own cause: the full workspace on Windows, the platform-independent crates on
Linux, the `wasm32` purity check, `scripts/check-no-network.sh`, `cargo deny`, and the
frontend's lint, typecheck and build. See
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
