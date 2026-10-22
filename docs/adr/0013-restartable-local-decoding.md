# ADR 0013: restartable local decoding and guarded delivery

Date: 2026-10-22
Status: implemented; native worker and installer payload verified; live app acceptance pending

## Context

Native decoder faults cannot be caught by Rust panic handling. A timeout around a hung in-process thread leaves later dictations blocked. Executable-name comparisons cannot distinguish two windows of one app, and clipboard text equality cannot establish ownership after an intervening copy.

## Decision

Use the same executable's internal worker mode before Tauri or logging initialization. A warm child owns native inference. Bounded binary PCM, hints, session and request identities travel over standard pipes; no temporary audio files or network transcription are introduced. A request deadline or child exit kills and reaps the child; the next request restarts it. Cancellation is session-specific. Incremental decoding continues through this transcriber boundary.

Keep failed utterance audio and its original hints in a single memory slot, limited to ten minutes of audio and ten minutes of retention. Explicit Retry copies recovered text for manual paste. Completed segment text remains explicitly partial if the rest fails. Cancelled sessions do not become recovery recordings. Durable recordings retain the existing explicit opt-in.

Carry an opaque window handle and process identity from capture to delivery. Check focus and cancellation before every input piece. Never force focus. Win32 cannot make a focus check atomic with SendInput; that OS race remains. Keep complete text available in memory when automatic delivery is held.

Clipboard writes and restoration check sequence ownership while the clipboard is open. A shared lease preserves the original snapshot across consecutive dictations and supersedes obsolete restore jobs. An intervening copy survives later chunks and restoration. Unsupported snapshots fail without mutation and direct the user to Copy. Only an identifiable Claude executable gets chunk pacing; ordinary windows get one paste.

## Alternatives and consequences

An in-process timeout cannot recover from native access violations or release panic=abort. A separate packaged helper adds deployment coordination without improving isolation over the same executable. JSON sample arrays inflate transfers. Restarting a child sacrifices warm state after a failure but preserves the UI and later sessions.

Full linked native tests now pass with verified v2 weights. The actual release worker also passes controlled crash/restart, 60-second timeout/restart, cancellation and stale-session checks; the extracted installer executable loads its verified bundled model offline. These checks do not establish broad speech accuracy or app release-to-delivery latency. Native delivery could not acquire foreground under the sandbox, and the complete public corpus, other model weights, physical microphone checks and opted-in personal measurements remain acceptance work.
