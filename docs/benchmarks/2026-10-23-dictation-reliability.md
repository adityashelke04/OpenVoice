# Dictation reliability review — 2026-10-23

Reviewed the Windows desktop app for one local user, with particular attention to
hotkey activation, capture, decoding, cancellation, focus, delivery and recovery.
The starting checkout was 1.3.0; it was fast-forwarded to main at `945fc32`
(1.3.2). The installed executable was 1.3.1. Changes below are prepared for
the 1.3.3 release.

## Confirmed problems and fixes

1. **Copied screenshots blocked automatic delivery.** The installed app's log
   contained `clipboard has formats that cannot be preserved` failures into Orca,
   matching the reported “Held in OpenVoice” card. Windows advertises a bitmap
   handle alongside its device-independent image data; the snapshot treated that
   handle as an unrecoverable format and refused to paste. Preserve the DIB/DIBV5
   bytes and let Windows recreate the bitmap. Native tests reproduce the old
   failure and verify pixel data survives restoration for both DIB and bitmap
   source clipboards. This uses Windows' documented
   [synthesized clipboard formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats).

2. **Clipboard recovery interfered with normal copying.** Successful paste held
   speech on the clipboard for 15 seconds; failures bypassed restoration, and
   multi-part paste wrote a separate complete-text recovery copy. Remove that
   behavior. Normal paste borrows the clipboard, allows one second for the target
   to read it, then restores the original on success or failure. Sequence numbers
   and generations prevent overwriting intervening copies or newer dictations.
   Unsupported clipboard objects use paced typing without borrowing the clipboard.
   Recovery stays in OpenVoice until explicit Copy.

3. **Cancelling a virtual hold consumed the next hotkey press.** Hands-free and
   click-to-talk do not always produce a later physical release. Cancellation left
   the session's key-down flag set, so the next genuine press was ignored.
   Cancellation now rearms it; activation control also resets coherently after
   recording limits, capture faults and shortcut changes.

4. **Starting hands-free mode could discard previous speech.** Double-tap used
   global cancellation to remove its first, short tap. That also cancelled earlier
   queued dictations. The gesture now identifies and removes only its own stub.
   Earlier speech and a previous capture still stopping remain intact.

5. **An old stop request could stop a newer recording.** Stop was enqueued from a
   newly spawned thread, allowing abort and restart to overtake it. Enqueue stop
   on the session loop, then wait for its result off-thread. Control decisions
   also run on that loop, so stale failure cleanup cannot release a new virtual hold.

6. **Retry still copied automatically and could hide recovered text.** Remove
   Retry's clipboard write, refresh Home immediately, and explicitly retain the
   recovered result even when its original session predates a newer dictation.
   Keep the session watermark so late background results cannot overwrite it.

## Validation

- Full Rust workspace tests: 428 reported passing; three explicitly ignored
  integration tests. Model-dependent tests also self-skip when their repository
  fixtures are absent; this run is not a speech-accuracy benchmark.
- Full frontend suite: 373 tests passed across 40 files.
- Native Windows EDIT control acceptance: 30/30 exact text matches, comprising
  ten short keystroke inputs, ten long clipboard pastes and ten Unicode inputs.
  The disposable test window was closed afterwards. This isolates actual Windows
  delivery without microphone or model variability.
- Native isolated clipboard tests cover screenshots, native bitmaps, unsupported
  display formats, consecutive dictations, failed partial delivery restoration,
  cancellation and preservation of a new non-text copy.
- Rust formatting, strict workspace Clippy, UI build and network-boundary gate pass.
  UI lint has nine existing Fast Refresh warnings in unchanged files. The native
  linker reports an existing `LIBCMT` warning; builds and tests complete.
- Independent review covered the combined changes and connected callers. Two
  additional recovery/lifecycle edge cases were reproduced, fixed and re-reviewed.
- Domain purity check passes for `wasm32-unknown-unknown`.

## Coverage and limits

Read the hotkey hook, tap gestures, session machine, engine effects, audio command
queue, clipboard ownership/injection, recovery and relevant Home UI. Reviewed
worker cancellation/recovery and sampled settings, history, download/update and
IPC paths. This was a focused application reliability review, not a line-by-line
audit of dependencies or the separate marketing website.

The logs identify a real cause, but cannot measure the user's overall failure
rate: a successful `SendInput` return does not prove every target app consumed it.
The 30-input acceptance result applies to a Windows EDIT control, not to Orca,
browser editors, elevated apps, or microphone-to-text accuracy. Slow target
clipboard consumption and paced typing with unusual held modifiers still need
live target-app acceptance. No new reliability percentage is claimed.
