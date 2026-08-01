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

# ASR sidecar
uv venv
uv pip install -e sidecar nvidia-cublas-cu12 nvidia-cudnn-cu12

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

Tests that need model weights are `#[ignore]`d and run nightly. Downloading 1.6 GB on
every push would make CI slower than the review it supports.

### The app-compatibility matrix

Text injection breaks per-application in ways no unit test catches. Before a release
we manually verify against VS Code, Cursor, Windows Terminal, Chrome, Slack, Discord,
Notion, IntelliJ, and Obsidian. If you find an app where injection misbehaves, that's
a valuable bug report — include the app, its version, and whether the text was
partially delivered.

## Commits and PRs

[Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`,
`docs:`, `refactor:`, `test:`, `chore:`. The changelog is generated from these.

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
