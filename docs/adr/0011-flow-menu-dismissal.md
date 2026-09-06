# ADR 0011 — The Flow Menu is dismissed by a menu-scoped mouse hook

- **Status:** Accepted
- **Date:** 2026-09-05
- **Builds on:** ADR 0007, which fixed the window and made the region the thing
  that changes per state

## Context

Right-clicking the Flow Bar opened its menu, and nothing could close it. Clicking
anywhere else on the screen did nothing at all; the panel stayed up — an opaque
280px rectangle over whatever the user was working in — until they came back and
right-clicked the bar a second time. Reported as "the right click stays open
forever".

This is not a missing handler. Three properties of the window combine to make the
menu structurally undismissable, and each of them is deliberate:

1. **`WS_EX_NOACTIVATE`.** The bar never takes focus, so it never loses it. The
   `blur` listener that looks like it should do this job cannot fire; it only ever
   ran in the component sheet, where the same component sits in an ordinary
   browser window.
2. **`SetWindowRgn` clips the window to the pill.** Every pixel outside that
   region belongs to some other window, so no click there is delivered here. There
   is no invisible backdrop to catch one, and there must not be: an oversized
   overlay window punches a dead zone into whatever is underneath.
3. **No keyboard path.** Escape is seen only by `ov-input`'s global keyboard hook,
   and it only ever cancelled a session. The webview receives no key events
   because it has no focus to receive them with.

It is the same failure as `TrackPopupMenu` called without `SetForegroundWindow`,
and for the same reason: a menu whose owner is not foreground is never told about
the click that should dismiss it.

## Decision

A **`WH_MOUSE_LL` hook, installed only while the menu is open** and removed the
moment it closes (`crates/ov-app/src/clickaway.rs`). On a button-down outside the
window's current region it emits `overlay-menu-dismiss`, which the webview turns
into `setMenu(false)`.

Four further mechanisms, each independent of that one:

- A `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` on the same pumped thread, for the
  ways of leaving a menu that are not clicks: alt-tab, the Win key, a notification.
- Escape, routed through `Shell::on_cancel_key` from the existing keyboard hook.
- An emit on the hide path of `Overlay::apply`, so a snoozed bar does not come
  back an hour later still wearing an open panel.
- A **15-second unattended timeout** in the frontend (`useMenuTimeout`).

The hit test is against the **region**, not the window. ADR 0007 fixed the window
at 404x640 and made the region the thing that changes; a click inside that
rectangle but outside the region lands on whatever is painted underneath, so
treating the window as the boundary would leave a 404x640 patch of screen where
clicking did nothing.

The callback obeys the rules `ov-input`'s keyboard hook is written to: four
integer comparisons against a cached box and one non-blocking `GetWindowRect`, no
locks, no allocation, no logging, nothing stored, and `CallNextHookEx` on every
path so the click still belongs to whatever the user aimed it at.

## Rejected

**Native `TrackPopupMenu` / Tauri's `popup_menu`.** The OS dismisses native menus
on an outside click because it runs a modal loop with mouse capture — but only if
the owner window is made foreground first. Making the Flow Bar foreground costs
the user the caret their dictation is aimed at, which is the one thing this
window's design exists to prevent. Without it, we reproduce exactly the bug being
fixed.

**A full-screen transparent backdrop window.** Swallows the dismissing click, so
the user must click twice to press a button in their own application. Also breaks
ADR 0007's fixed-window invariant, and risks leaving a fullscreen click-eater on
screen if a bug ever fails to take it down — a class of failure this codebase has
already been bitten by.

**`SetCapture`.** Mouse capture is per-thread and is broken the moment another
process's window activates. Classic Win32 menus need `WH_MSGFILTER` alongside it.

**The foreground hook alone.** It does not fire when the user clicks inside the
application that already has focus, which is the most common case in the report.
Kept as a secondary signal, not the primary one.

**`WindowFromPoint` inside the callback.** Region-aware for free, but it sends
`WM_NCHITTEST` to the windows it probes, so a hung third-party window under the
cursor could block the callback past the hook-eviction timeout. Cached arithmetic
cannot block.

## Consequences

- The app claims a mouse-hook capability it did not have. It is scoped to the
  seconds a menu is on screen, the callback stores nothing, and the module
  documents both — but the capability is real, and EDR heuristics may notice it.
- The mouse hook is subject to the same silent eviction as any low-level hook:
  if its callback ever runs long, Windows removes it with no error anywhere.
- Because of that, the frontend timeout is the actual guarantee. The failure mode
  of every Windows-side mechanism here is a 15-second delay, not a stuck bar.
- `apply_region` gained a second responsibility: it publishes the box it computes
  so the callback does not have to ask GDI for it. Published after the
  `SetWindowRgn` succeeds, so a refused region never becomes a hit test.
