# PRODUCT.md — OpenVoice

Durable product truth. Visual decisions live in `DESIGN.md`; system architecture
lives in `docs/ARCHITECTURE.md`.

## What it is

A local-first, open-source push-to-talk dictation tool for developers. Hold Right
Ctrl, speak, release — correctly formatted text appears at the caret in whatever
application had focus. No cloud, no account, no telemetry.

## Unique mechanism

**The speech model is a commodity; the formatting pipeline is the product.** Raw
Whisper gives developers `use effect`, `cube control`, the literal words "open
parenthesis", and `Git status.` — all accurate transcriptions, all useless. OpenVoice
puts its effort into the deterministic, testable, rule-based layer that turns a
transcript into text a developer can actually use, tuned per target application.

## Audience and scene

A working developer, alone, at a laptop, usually late, usually in a dim room with a
bright editor. They are mid-task in VS Code, Cursor, a terminal, or a browser tab
holding an AI agent. Dictation is a **secondary action inside a primary task**: they
never come to OpenVoice, they use it while doing something else.

The app is therefore judged by what it does when unobserved. The overlay is seen for
two seconds at a time, in peripheral vision, while the user's attention is on their
own code. The settings window is opened rarely, deliberately, to fix something.

## Jobs to be done

1. Dictate a commit message, code comment, Slack reply, or variable name into the app
   already in focus.
2. Dictate a long, rambling prompt to an AI coding agent and get back something
   structured.
3. Fix a term the model keeps getting wrong, once, so it stays fixed.
4. Recover a transcript when injection failed or the wrong window had focus.

## Product principles

1. **Local by default, network by exception.** Enforced by a CI job, not a promise.
2. **Never lose a word.** Injection failure falls back to clipboard plus history.
3. **Invisible when idle.** ~60 MB RAM, ~0% CPU. Never something the user notices.
4. **Deterministic where it can be.** Rules are unit-tested; probabilistic parts are
   isolated behind seams.
5. **The user owns their data.** Plain SQLite, documented schema, one-click export.

## Non-goals

Meeting transcription, diarization, live captions, cloud sync, OS voice control, any
paid API, mobile.

## Committed constraints

- **Platform:** Windows 10/11 first; macOS and Linux at v0.5.
- **Stack:** Tauri v2 + Rust core, React + TypeScript frontend. Approved 2026-07-31.
- **Activation:** hold Right Ctrl (push-to-talk). Approved 2026-07-31.
- **License:** Apache-2.0. Approved 2026-07-31.
- **Reference hardware:** RTX 3050 Laptop, 4 GB VRAM. Constrains model choice and
  forbids a resident cleanup LLM alongside the ASR model.

## Surfaces

| Surface | Mode | Notes |
|---|---|---|
| Recording overlay | Operate | ~280×64, frameless, always-on-top, **non-activating**. Must never take focus — if it does, the caret is lost and the product breaks. |
| Main window | Operate | Dictate, History, Dictionary, Profiles, Models, Settings, Debug. |
| Landing page | Persuade | Not yet built. |

## Brand commitments

None inherited. The name collides with MyShell's OpenVoice (a TTS project); a rename
is under consideration and the identity should not lean hard on the wordmark yet.

## Assumptions

Labeled per `init.md` — inferred from the build brief and approved decisions in this
project's history, not from a separate user interview:

- The primary user is the project author, a developer, dictating while coding.
- Dark-first was stated in the brief; the physical scene (dim room, bright editor,
  late) independently supports it.
- Peripheral-vision legibility of the overlay matters more than its expressiveness.
