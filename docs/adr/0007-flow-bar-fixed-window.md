# ADR 0007 — The Flow Bar's window is a fixed rectangle

- **Status:** Accepted (Amended 2026-08-28)
- **Date:** 2026-08-23
- **Supersedes:** the *position* decision in ADR 0006, and its two stated premises
- **Closes:** #10

## Context

ADR 0006 made the pill's painted size a property of the window rather than of
JavaScript, which removed a real race. It left the pill's *position* computed the
old way, and the amendment of 2026-08-21 extended the same rule to the position
without changing the underlying arrangement. That arrangement was:

- the window is sized exactly to the pill, so it changes size five times a
  dictation;
- to keep the pill visually still while the window grows, the window is moved in
  the opposite direction by half the growth;
- the pill re-centres itself inside the larger window using CSS derived from the
  layout viewport.

The bar therefore stays still only if the window move and the internal
re-centring are presented in the **same composited frame**.

They cannot be. The move is applied by the compositor synchronously, inside
`SetWindowPos`. The re-centring depends on `100vw`/`100vh`, which reach the
renderer — a different process, on a different schedule — afterwards. In the gap
the pill is painted at its old size, centred in its old viewport, inside the
window's new rectangle.

Four rounds of fixes tightened that bargain. Each found and fixed a genuine
defect; none removed the artefact, because the artefact is not a defect in the
mechanism. It is the mechanism.

## Evidence

Measured in production, on a real hotkey press, on the machine that reported it
(Windows 11, single display, **scale 1.25**):

```
overlay set_box  logical=(626,706,284,84)   <- idle to listening
pill displaced from anchor  dx=-67 dy=-22 viewportBehind=true
    view={151,40}  want={284,84}  pill={151,40}  at={0,0}
```

`-67` is `-(284-150)/2`. `-22` is `-(84-40)/2`. Exactly half the size delta on
each axis, up and to the left — the reported symptom, to the pixel. The other two
transitions in a dictation displace by `(+57,+22)` and `(+10,0)`, which is the
same arithmetic in the other direction.

Two supporting measurements:

- The **HWNDs are not what lags.** `parent_client` and `child_client` both read
  the new size 420µs *before* the webview reported the old viewport, so
  `sync_webview_child` was doing its job and the surface was not behind. The
  renderer's layout viewport was.
- The **viewport settles in 6–20ms** (ten samples, median ~10, measured against a
  wall clock from outside the app). One frame. That does *not* by itself account
  for an artefact users describe as lasting about a second, and the discrepancy
  is not resolved: `probePill` runs in the renderer, so it cannot time a renderer
  stall — if the compositor holds a stale frame at the moved window while the
  renderer is busy, JavaScript is not running to observe it. Under this ADR the
  question stops mattering, because a window that never moves has no stale
  position to paint at.

## Decision

**The window is a fixed rectangle. The pill moves inside it.**

```
OVERLAY_W = 404      (widest pill, the 360 alert clamp, + 22 glow margin each side)
OVERLAY_H = 248      (22 glow margin + 40 pill + 186 for the right-click menu)
PILL_TOP  = 22       (the pill's top edge, from the window's top)
```

The window's position is computed only from the anchor, and only when the anchor
changes:

```
win.x = anchor.cx  - OVERLAY_W / 2
win.y = anchor.top - PILL_TOP
```

No term depends on the bar's state. That is the whole decision. The pill is
centred in a viewport that never changes, so a late viewport cannot displace it.

**What crosses the IPC boundary on a state change is a shape, not a box.**
`overlay_set_box(x,y,w,h)` is replaced by `overlay_set_shape(pill_w, pill_h,
margin)`. The frontend no longer knows or says where the window is. A shape can
only ever hide painting; it can never move it.

**The window is clipped to the pill with `SetWindowRgn`.** A transparent window
still swallows OS clicks across its whole rectangle — `pointer-events: none`
governs the webview, not the window — so a permanently 404×248 window would punch
a dead zone into whatever is underneath. The region is the pill *plus the margin
its glow paints into*, which is byte-for-byte the rectangle the window occupied in
each state before this change: the dead zone does not grow, and nothing that was
painted before is clipped.

**Grow now, shrink late.** The region is applied as the union of the old and new
immediately, and the exact new region after `SHAPE_SETTLE_MS` (120ms), cancelled
by any newer shape. Growing early costs nothing, because nothing is painted in the
area being uncovered; shrinking early would clip a pill still painted at its
previous, larger size. This makes the one failure mode a region can introduce
structurally impossible rather than merely unlikely.

**The persisted placement is the pill's anchor, not the window's origin**, with a
one-time migration from the old `x`/`y`. Storing the window origin would tie every
user's saved position to the window's dimensions, so changing those again would
silently move their bar.

## Two premises of ADR 0006 that were false

Both were load-bearing, and both are struck.

**"Bare transparent area paints as a rectangle."** It does not, on this stack.
`transparent: true` reaches tao, which enables per-pixel alpha through DWM with an
empty blur region (`tao-0.35.3/.../window.rs:1284-1296`), and reaches WebView2,
where wry sets `DefaultBackgroundColor` to `(0,0,0,0)` three times
(`wry-0.55.1/src/webview2/mod.rs:127-131`, `:398-404`, `:452-454`). Microsoft
documents alpha 0 as supported on everything except Windows 7.

The rectangle people actually saw had two other causes, both already fixed: an
element painting a background before script ran (`global.css:25-42`, commit
`65af0fc`), and a pill cropped by a window smaller than itself. Confirmed
empirically — the listening state has shipped for ten days with a genuinely
alpha-0 22px border (both glow layers have *no spread*, so the window's corners at
31px lie outside both) with no rectangle reported, and a screenshot of the new
404×248 window shows the desktop cleanly through every unpainted pixel.

**"`SetWindowRgn` would clip the glow."** True only if the region is the pill.
Make it the pill plus its margin and nothing is clipped. Probed against the
running app before this was written:

```
SetWindowRgn returned=1   GetWindowRgnBox=2 (SIMPLEREGION)
PAINT  kept-area hash before=1248075244 after=1248075244  identical=True
INPUT  centre  263070 -> 263070      (the overlay's own WebView2 child)
INPUT  clipped 263070 -> 11535732    (a different process's window, beneath)
```

Painting inside the region is byte-identical, input inside it still reaches the
overlay, and input in the clipped strip reaches the window underneath. Tauri
relies on the same behaviour in `undecorated_resizing.rs:413-441`, and never
touches the overlay's own region because that path requires `resizable: true`.

## Alternatives rejected

- **`WM_NCHITTEST` → `HTTRANSPARENT`.** The hit-test search never leaves the
  calling thread, so it can never route a click to another application.
- **`set_ignore_cursor_events` driven by a cursor poll.** Every toggle makes tao's
  `apply_diff` rewrite the whole `GWL_EXSTYLE` word, and `WS_EX_TOOLWINDOW` is not
  among tao's own `WindowFlags` — so a poller would re-fire the production
  style-stripping bug 60×/second. It also adds `WS_EX_LAYERED` to a window that
  never calls `SetLayeredWindowAttributes`, and lags the cursor by 2–4 frames.
- **Keeping the content-sized window but anchoring the pill to a fixed offset
  from its top-left.** Turns the displacement into clipping rather than removing
  it, reintroduces the JS-owned pill size ADR 0006 removed, and leaves the entire
  compensating-move mechanism — and its four rounds of fixes — load-bearing. It
  is the correct retreat if the region ever proves unworkable, and nothing else.

## Consequences

- The bar cannot be displaced by a late viewport. The class of bug is gone, not
  narrowed.
- `snap_box` and `park` now work on the pill's rectangle rather than the window's,
  which also closes the state-dependent snap-line defect ADR 0006 admitted was
  still open. A bottom-snapped bar puts the *pill* on the edge.
- The compensating move, `box.current` as an input to the anchor, the viewport
  derivation in CSS, and the per-state settle cancellation are all deleted.
- `checkInvariants` had to change meaning. It compared the pill against the
  viewport on both axes — the lagging quantity on both sides of the comparison —
  which is why "pill not centred" fired **zero** times across the entire life of
  the bug it was written to catch. It now checks horizontal centring and the
  pill's top edge against the constant Rust clips to.
- **Resolved in amendment (2026-08-28):** a bottom-snapped bar's right-click menu
  previously extended below the screen when opened downward. The fixed window has
  been expanded symmetrically to 404×640 with `PILL_TOP = 300`, allowing the menu
  to flip upward when docked near the bottom edge (`anchor.top >= 340`) and downward
  when near the top, clipping accurately via directional `SetWindowRgn` calculations.

## Amendment, 2026-08-28 — Bidirectional menu opening (upward flip) and transparency artifacts

The initial fixed-window design provided a 404×248 envelope with `PILL_TOP = 22` (and later 404×360 with `PILL_TOP = 44`), which only budgeted space below the pill. When docked at the bottom of the screen or near the taskbar, opening the right-click menu caused rows to extend off-screen. Additionally, expanding the envelope exposed pill height collapse due to CSS percentage overrides during menu display, as well as white rectangular non-client borders rendered by Windows DWM over certain dark backgrounds.

### Changes:

1. **Symmetrical 404×640 fixed envelope (`PILL_TOP = 300`):**
   The overlay window height is enlarged to 640px, placing the 40px pill vertically at `PILL_TOP = 300px` (`crates/ov-app/src/overlay.rs`, `crates/ov-app/tauri.conf.json`, `apps/ui/src/windows/Overlay.tsx`, `apps/ui/src/windows/overlay.css`). This provides a symmetrical 300px headroom above for an upward-flipping menu, and 300px headroom below for downward opening.

2. **Upward flip placement logic:**
   In `Overlay.tsx`, placement is derived from screen position: `menuAbove = (anchor.current?.top ?? 900) >= 340`. If the bar is near the top of the screen (`top < 340`), the menu opens below; otherwise, it flips upward above the pill.

3. **Directional `SetWindowRgn` clipping (`above: bool`):**
   `shape_rect` in `overlay.rs` receives the boolean `above` flag over IPC from `overlay_set_shape`. When opening upward (`above = true`), the clipping region spans `[PILL_TOP - (pill_h - PILL_H) - margin, PILL_TOP + PILL_H + margin]`, while downward opening spans `[PILL_TOP - margin, PILL_TOP + pill_h + margin * 2]`. This completely eliminates transparent click dead zones in both configurations while ensuring no painted menu elements or glows are clipped.

4. **Pill height decoupling & CSS keyframes:**
   The pill dimensions remain strictly 40px (`--pill-h: 40px`) during menu visibility rather than expanding to the envelope height. Dedicated CSS classes `.overlay-menu--above` (with bottom-center transform origin) and `.overlay-menu--below` (with top-center transform origin) handle positioning and appear animations.

5. **DWM transparency artifact suppression:**
   Windows 11 non-client frame borders and caption area rendering artifacts are suppressed in `crates/ov-app/src/main.rs` and `crates/ov-app/src/overlay.rs` using DWM window attributes (`DWMWA_BORDER_COLOR = 0xFFFFFFFE`, `DWMWA_NCRENDERING_POLICY = DWMNCRP_DISABLED`), and `SetWindowRgn` is applied with `bRedraw = false` for seamless region resizing without white flicker.

