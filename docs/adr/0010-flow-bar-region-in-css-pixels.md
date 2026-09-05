# ADR 0010 — The Flow Bar's clip region is computed in the webview's CSS pixels

- **Status:** Accepted
- **Date:** 2026-09-05
- **Amends:** ADR 0007, which established the region without saying what unit it is in

## Context

ADR 0007 made the Flow Bar's window a fixed `404x640` rectangle and made the
bar's state changes a change of *shape*: `SetWindowRgn` clips the window to the
pill, so the rest of the rectangle neither paints nor swallows clicks. That
decision stands and this does not disturb it.

What it left implicit is the unit the region is expressed in. `SetWindowRgn`
takes **physical** pixels. The pill's position is decided by CSS, in **CSS**
pixels. Something has to convert, and the conversion used was the monitor's
scale factor:

```rust
let (l, t, r, b) = region_box(shape_rect(pill_w, pill_h, margin, above), win.scale_factor()?);
```

with `shape_rect` centring the pill in the constant `OVERLAY_W`.

Both halves of that assert the same unstated premise: **that the webview lays out
one CSS pixel per logical pixel** — that its `devicePixelRatio` equals the
window's scale factor, and that its layout viewport is therefore `OVERLAY_W` wide.

Nothing enforces that premise, and WebView2 does not always honour it. It keeps
its own rasterization scale, and it can drop that to 1 — on a display change, a
lock, a monitor waking from sleep — with no DPI message reaching the process and
no Tauri event fired.

## Evidence

From the frontend's own instrumentation on a 125% panel, 2026-09-05, with no
shape sent for the preceding 217 seconds — so nothing this app did provoked it:

```
04:16:04  flowbar: viewport settled   via="window"  w=505 h=800    <- 404x640 x 1.25
04:16:04  flowbar: pill displaced from anchor  dx=51  view={505,800}  at={205,300}
04:16:08  flowbar: viewport settled   via="window"  w=404 h=640    <- back
04:16:12  flowbar: viewport settled   via="window"  w=505 h=800    <- and gone again
```

The layout viewport became the window's *physical* size. One CSS pixel was now
one physical pixel. Meanwhile the region was still being computed the old way:

```
overlay set_shape  scale=1.25  logical=(154,300,250,324)  physical=(192,375,313,405)
```

So the collapsed bar painted at physical `y 300..324` and the window was clipped
to `y 375..405`. **The two rectangles do not touch.** Nor do they in any other
state, because every state's region was multiplied by 1.25 while every state
painted at 1.0:

| State     | Bar painted (physical y) | Window clipped to (physical y) | Overlap |
| --------- | ------------------------ | ------------------------------ | ------- |
| Put away  | 300..324                 | 375..405                       | none    |
| Idle      | 300..340                 | 375..425                       | none    |
| Listening | 300..340                 | 347..453                       | none    |

The listening region is the tallest of the three because it reserves the glow
margin, and it still starts seven pixels below the bottom of the bar. What did
fall inside it was empty margin.

Which is exactly how it was reported: *"it gets invisible… I'm trying to hit
Ctrl, the flowbar doesn't pop up at all… nor the compacted flow bar, nor the
entire flow bar, even when clicking and without clicking… I have to restart."*

There was never anything wrong with the bar. It was being drawn, correctly, in a
part of the window that had been cut away — and nothing in the app re-derived the
region, so it stayed cut away until the process was restarted.

The same desync is in the log from 2026-08-23 (`view={505,310}`, `dx=51`). Before
ADR 0007 it could only ever *displace* the bar by half the viewport error, which
is why it read as a nuisance and was filed as one. Clipping the window is what
turned the same 51 pixels into a bar that disappears.

## Decision

**The region is computed in the webview's CSS pixels, and the conversion to
physical pixels is measured rather than assumed.**

1. The frontend sends the layout viewport it measured alongside every shape.
   `shape_rect` centres the pill in *that*, not in `OVERLAY_W`, reproducing what
   the stylesheet actually did.
2. `css_to_physical` is `win.inner_size().width / view_w` — the client area in
   physical pixels over the CSS pixels the webview was given to lay out in. That
   ratio is correct for any scale the webview picks, including one this process
   was never told about. It falls back to `scale_factor()` when the viewport is
   unknown, which reproduces the old behaviour exactly.
3. The viewport is part of the shape's identity, so **a viewport change is a
   shape change** and the existing convergence loop re-sends it. No new sender,
   and no listener that has to remember to fire.

## Consequences

- A webview that loses the window's scale now renders the bar at 80% of its
  intended physical size, in the right place, fully visible and clickable. That
  is a cosmetic degradation of a WebView2 behaviour this app cannot control, and
  it replaces a bar that vanished until restart.
- One assumption is removed rather than narrowed. There is no longer a premise
  about `devicePixelRatio` for a future display change to falsify: both sides of
  the conversion are read from the same two live measurements.
- The desync is now **nameable**. `overlay_state` reports `webview_scale`,
  `monitor_scale` and `scale_desyncs`, and both sides log the onset and the
  recovery once each. This follows `topmost.rs`: the previous version of this bug
  was reported as a feeling ("it hides itself") because nothing in the app could
  tell it apart from a snooze, a lost z-order, or a crash.
- Three detectors, deliberately: `window.onresize`, `visualViewport`, and a
  `ResizeObserver` — all of which fired for this incident — plus a 500ms poll
  that did not exist before. The poll is not the mechanism; it is the answer to
  "what if one day none of them fires", and it bounds the worst case at half a
  second instead of at a restart.

## Related

- ADR 0006 — the pill's size is a property of the window
- ADR 0007 — the window is a fixed rectangle; state changes its region
- `css_to_physical` in `crates/ov-app/src/overlay.rs` carries the short version
- `noteViewportContract` in `apps/ui/src/windows/overlay-trace.ts` is the alarm
