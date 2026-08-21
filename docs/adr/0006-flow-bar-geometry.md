# ADR 0006 — The Flow Bar reads its geometry off its window

- **Status:** Accepted
- **Date:** 2026-08-20

## Context

The Flow Bar is a frameless, transparent, always-on-top window sized exactly to the
pill painted in it. Sizing it exactly is not cosmetic. Two properties depend on it:

1. A transparent Windows webview does not reliably composite as clear where nothing
   is painted. Window area with no content in it shows up as a rectangle around the
   bar — the artefact this window has been fighting since it was written.
2. A transparent window still swallows OS-level clicks across its whole rectangle.
   `pointer-events: none` governs the webview, not the window, so any window bigger
   than the pill punches a dead zone into whatever is underneath it.

The bar changes size on every state change: idle, listening, transcribing, alerting,
and the right-click menu are five different widths, and listening additionally
reserves a margin for its glow.

The problem is that the size is agreed between three parties on two sides of a
process boundary, with no shared clock:

- **React** decides how wide the pill should be.
- **Rust** owns the window and resizes it, over IPC, a round trip later.
- **WebView2** owns the surface the page is actually drawn on, and resizes it on its
  own schedule again.

Until now the pill's width was a React number applied as an inline style, and the
same number was sent to Rust to size the window with. One value, two consumers,
reached at two different times. That is a race by construction, and it produced two
distinct artefacts that looked like one bug:

- **The pill ahead of the window.** React paints 240px of pill inside a window still
  150px wide. The pill is cropped: both rounded ends outside the surface, both side
  borders gone, the timer cut mid-glyph. What is left reads as a black rectangle.
- **The window ahead of the surface.** `wry` handles `WM_SIZE` in two steps with two
  timings — `ICoreWebView2Controller::SetBounds` synchronously, so the page relayouts
  at once, then `SetWindowPos` on the WebView2 child with `SWP_ASYNCWINDOWPOS`, which
  only *posts* the move. For at least one turn of the message pump the page is laid
  out at the new size while the surface on screen is still the old rectangle at the
  client origin. The user sees the top-left crop of a correctly drawn pill: one
  rounded corner, a border running into a hard cut, two square edges.

Narrowing either window was tempting and would have been wrong. A one-frame artefact
that appears on some machines and not others cannot be verified by looking at it.

## Decision

**The pill's painted size is derived from the viewport, in CSS, and is never told to
it by JavaScript.**

The window is the pill plus the glow margin on all four sides, so with the pill's
height fixed the margin is recoverable from the viewport and the pill's width with
it:

```css
.overlay-hit {
  height: var(--pill-h);
  width: calc(100vw - 100vh + var(--pill-h));
  max-width: 100vw;
}
```

Both terms come from the same viewport and therefore cannot be read at different
instants. JavaScript still computes the widths — they are the *target* for the
window, which is all they ever should have been — and the pill follows the window
there.

This does not make the resize instantaneous. It makes the intermediate state
*correct*: mid-resize the pill is briefly the previous size, which is a correct pill
of the wrong width rather than a broken one. The race is removed rather than
narrowed, because there is no longer a second copy of the size to be stale.

**Second, the WebView2 child window is resized synchronously** after the parent, in
`overlay.rs::sync_webview_child`. `wry`'s own posted call lands afterwards with the
same values and is a no-op. `SWP_NOCOPYBITS` is set on both, because every call here
changes position and size together and there are no client bits worth salvaging.

**Third, the disagreement is instrumented rather than watched for.** `checkInvariants`
in `overlay-trace.ts` measures the pill against its viewport after every layout pass
and is silent unless one of the two invariants above is violated, in which case it
says which one and by how many pixels. The Rust side logs the requested box, the
scale factor, the physical box, and the parent's and child's client rects — the pair
that decides whether the surface kept up. A verbose timeline behind
`localStorage.ov_trace` covers the timing.

## Amendment, 2026-08-21 — the same rule applies to the pill's *position*

The decision above fixed the pill's size and left its position computed the old
way, which was the wrong place to stop. Two bugs followed, and both are the
original mistake in a new location.

**The bar jumped up and left when the hotkey was spammed.** The pill is held still
across a resize by an anchor — where the pill belongs, as its centre-x and top
edge — from which each window position is computed. Three places convert a window
position back into an anchor: the `overlay-parked` listener, `settleTo`, and the
`onMoved` handler. All three multiplied the window's position by `box.current`,
which is the box this side *intends* the window to become, written synchronously
in the layout effect before the resize has even been sent.

`onMoved` fires for every move including the ones we command ourselves. When one
arrived after the next state had already updated `box.current`, the anchor was
rebuilt with the wrong half-width and the wrong margin. Listening → transcribing
displaced it by `(170 − 284) / 2 = −57`px horizontally and `0 − 22 = −22`px
vertically; the next press displaced it back. Around a full cycle it sums to
zero, which is why the bar appeared to jump away and return rather than drift.

The conversion now reads the window — `anchorFrom` in `Overlay.tsx` — using the
same height identity as the width derivation: `cx = x + clientWidth / 2` and
`top = y + (clientHeight − PILL_H) / 2`. Synchronous, no IPC, no React state, and
re-deriving the anchor from a move we commanded ourselves now *reproduces* the
anchor we already had instead of corrupting it.

**The bar stuck at its listening width after a cancel.** The reconciliation loop
was edge-triggered: it decided at the top whether a send was needed. Comparing
against the desired box dropped the first resize at mount; comparing against the
last-sent box dropped the last resize on cancel, because returning to a box that
had already been sent while a different one was still in flight made both the
effect and the loop's identity check agree there was nothing to do. The window
stayed wide with an idle pill in it — and because the pill now fills its window
exactly, that showed as a correctly-shaped, over-long pill rather than as the
rectangle it would have been before.

Both holes come from asking "has something changed?" instead of "are we there
yet?". `flush` is now a convergence loop that terminates only when the box the
window has been told equals the box this side wants, **compared by value**, and
the effect unconditionally records its target and calls it. Disagreement is the
loop's continuation condition, so no interleaving can leave the two out of sync.

**Two supporting corrections.** The "user is dragging" flag was raised on
mouse-down and lowered only by a debounce armed inside the `onMoved` handler, so
a click that moved the bar zero pixels left it raised for the rest of the
session — and `startDragging` cannot lower it, because it dispatches
`WM_NCLBUTTONDOWN` and returns immediately rather than when the move loop ends.
It is now a timestamp refreshed by every move a real drag produces and expired
after `DRAG_TTL`. And a stuck box is now reported: `reportStuckBox` fires if the
window and the UI disagree for more than half a second, naming both boxes.

## Amendment, 2026-08-21 (second) — the move nobody could see

The amendment above was written from the app's own log, which records every
`overlay_set_box` with its scale factor, its physical rectangle, and the parent
and child client rects. Two hundred and seventy-seven entries, spanning weeks and
including heavy hotkey spamming, and **every single commanded rectangle is
arithmetically correct** — every box centred on the same point, `ok=true`,
`synced=true`, no fallback warnings, no parking mid-session.

That looked like proof the geometry was fine. It was proof of something narrower:
that the *logged* path was fine.

`overlay_set_box` is not the only thing that moves this window. `settleTo` calls
`win.setPosition` directly, sixty times a second for `SETTLE_MS`, and that path
is logged nowhere. It is also, until the day before this was written, a path that
**had never once executed**: `core:window:allow-set-position` was missing from
`capabilities/overlay.json` and is not in `core:window`'s default set, so every
call had been silently denied for the lifetime of the feature. Granting the
permission — while fixing the drift it was supposed to prevent — switched on
220ms of unlogged, unexercised, uncancellable window movement.

The evidence is in the log as an absence. At `05:56:20.986` the bar is placed at
`(860, 882)`, implying an anchor of `(935, 882)`. At `05:56:41.400` the next box
is `(818, 922)`, which implies an anchor of `(960, 944)`. **The anchor moved, and
no `set_box` line stands between them.** A window move happened that the log could
not see, and `setPosition` is the only thing that could have made it.

Four defects compound in that path, and all four are fixed here.

**The `onMoved` handler had no way to recognise our own moves.** Windows reports
every move including the ones we command, with nothing on the report to say which
— and the only filter was a `dragging` flag, a heuristic about a Windows move loop
that swallows its own mouse-up. When it was wrong, the resize that follows every
hotkey press was committed as a deliberate drag: snapped, written to disk as the
user's chosen position, and slid there. Every commanded position is now recorded
before it is sent, on both sides of the boundary, and a report matching one is
provably ours. That is a fact rather than a guess, and it closes the path
independently of whether the drag flag is right.

**`snap` measures against whatever size the window happens to be — still true.**
Every snap line is the screen less the window, so between the idle box and the
listening box the horizontal centre line moves 67px and the bottom line moves
44px. A commit arriving while the bar was momentarily the wrong size snaps it to a
line computed for a different state.

`snap_box` now takes the box explicitly, but review established that this is so
far only a refactor: both callers still pass `win_size(win)`, so the behaviour is
unchanged and **the defect is live**. Closing it needs the box the drag actually
ended in, which only the frontend knows, so it has to be passed across with the
commit rather than re-read on arrival. Recorded here rather than quietly claimed
as fixed.

**`settleTo` could not be stopped and wrote the anchor from a stale track.** It
captured its origin after three awaits, ran its full 220ms regardless of anything
else, wrote sixty positions a second against a resize that writes one, and set
`anchor.current` on every frame from coordinates computed before the round trip.
A resize landing mid-slide left the anchor permanently wrong by `(±67, ±22)`. It
now takes its origin from the same event that produced the commit, carries a
generation token that any state change or newer settle invalidates, and sets the
anchor once at the end from its known destination.

**`is_echo` kept one position.** Every resize overwrote it, and moves are reported
over an IPC round trip while the hotkey produces resizes 26ms apart — so by the
time a move came back, the position it echoed had usually been overwritten. Both
sides now keep the last eight.

**And the instrumentation was blind in exactly the place that mattered.**
`checkInvariants` compared the pill against the viewport; `reportStuckBox`
compared the sent box against the wanted box; `trace_box` logged `set_box`. All
three were correct throughout every one of these failures, because none of them
watched *position*. `move_to`, `snap` and `settleTo` now log; a new invariant
catches a pill that is the right size but not centred in its window; and an
`overlay_log` command puts the webview's half of the handshake into the same file
as Rust's, on the same clock, so the next report of this arrives with evidence.

**Unrelated, found while measuring.** The live window's extended style read
`0x00040118` — no `WS_EX_NOACTIVATE`, no `WS_EX_TOOLWINDOW`, `WS_EX_APPWINDOW`
set. `configure_overlay` applies those bits once at startup by OR-ing them into
`GWL_EXSTYLE` behind tao's back, and tao rewrites the whole word from its own
`WindowFlags` whenever it touches visibility or z-order. The bar could therefore
take focus — the one thing it exists not to do, because taking focus costs the
user the caret their dictation was aimed at.

**The durable fix is to stop working behind tao and let it own the bit.** tao
derives `WS_EX_NOACTIVATE` from its own `FOCUSABLE` flag
(`tao/src/platform_impl/windows/window_state.rs:296`), so `"focusable": false` on
the overlay window in `tauri.conf.json` makes it part of the state tao applies,
and every `apply_diff` puts it back instead of taking it away.

Re-asserting it by hand was not enough on its own, and the reason is an ordering
that is easy to get wrong: `apply_diff` issues `ShowWindow(SW_SHOW)` at the top
and rewrites `GWL_EXSTYLE` further down. The preceding `hide()` had already
cleared the bits, so a show-from-hidden — snooze, or "only show while dictating",
then the hotkey — activated the window *before* anything could restore them.
Restoring afterwards was shutting the door behind the horse. `ensure_noactivate`
now runs on both sides of the show, and also clears `WS_EX_APPWINDOW`, which
forces a window into the taskbar and outranks `WS_EX_TOOLWINDOW` — so setting the
tool-window bit while leaving that one standing had achieved nothing.

## Consequences

- The pill can never be wider than the window it is drawn in. That is now a property
  of the stylesheet rather than of the arithmetic, and `max-width: 100vw` holds it
  even if the arithmetic is wrong.
- `--pill-h` in `overlay.css` and `PILL_H` in `Overlay.tsx` are one identity written
  twice, once in each language. There is no way to share it — the window height is
  computed from it in JS and the pill width is recovered with it in CSS — so both
  ends carry a comment naming the other.
- The menu state is exempt: it claims the window below the pill, so the window's
  height is no longer the pill's plus a margin. The margin is zero whenever the menu
  is open, which makes the pill exactly the viewport width, and the derivation is
  trivial rather than absent.
- The component sheet renders this same component in an ordinary browser window,
  where the viewport is the whole page. `data-fit` scopes the derivation to the real
  overlay window.
- `sync_webview_child` walks a child chain that belongs to `wry`, and depends on its
  webview being an ordinary child HWND. If that changes, the call becomes a no-op
  rather than a fault, and the Rust log's `synced` field says so.
