<!--
Title this PR as a Conventional Commit: feat: / fix: / docs: / refactor: / test: / chore:
The prefix decides which CHANGELOG section your entry belongs in; the entry itself
is written by hand, in this PR. See CHANGELOG.md.
-->

## What and why

<!-- What changes, and what problem it solves. Link the issue if there is one:
"Fixes #12". If there is no issue, the paragraph below is the issue. -->

## How to verify

<!-- The commands or steps a reviewer runs to see it work. For a formatting rule,
give the input and the output. For anything touching injection, name the
applications you tried it in and their versions -- injection breaks per-app and
no unit test catches it. -->

## Checks

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] Tests added that fail without this change
- [ ] For a new rule: a test proving it does **not** fire when it shouldn't

## Boundaries

Tick only what applies; leave the rest blank.

- [ ] Touches `ov-core` or `ov-format` — still compiles for `wasm32-unknown-unknown`
      (`cargo check -p ov-core -p ov-format --target wasm32-unknown-unknown`).
      No OS, filesystem, network, audio, GUI, or async-runtime dependency was added.
- [ ] Adds a dependency — `cargo deny check` still passes, and the reason it is
      worth the supply-chain surface is in the description above.
- [ ] Touches `crates/ov-input/` — the keyboard hook or synthetic input. Expect
      close review; this is the largest blast radius in the project. Confirm the
      hook still stores nothing and still passes non-matching keys through.
- [ ] Changes the shape of the system — a new port, a new runtime dependency, a
      different backend. An ADR is included in `docs/adr/`.
- [ ] Changes something a user can observe. `CHANGELOG.md` under `## Unreleased`
      is updated.
- [ ] Changes what the app can do with audio, the network, or stored data.
      `SECURITY.md` and the README's privacy section still describe reality.
