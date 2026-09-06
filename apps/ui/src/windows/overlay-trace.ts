/** Instrumentation for the Flow Bar's window/content handshake.
 *
 * # Why this file exists
 *
 * The bar's size is agreed between two processes. The web side decides how wide
 * the pill should be and asks Rust to make the window that size; Rust resizes the
 * window and WebView2 resizes the surface it is drawn on. Three values, three
 * owners, and no shared clock. When they disagree the user sees a black rectangle
 * with the pill clipped inside it — and every report of that looks identical
 * regardless of which of the three was late.
 *
 * Nothing on this path used to say anything at all: no `console` call existed
 * anywhere in this app's source, and the Rust command at the centre of it logged
 * nothing. Reproducing a one-frame artefact by watching for it is not a debugging
 * strategy, so the disagreement is measured instead.
 *
 * # What is always on
 *
 * [`checkInvariants`] runs on every layout pass and is silent unless something is
 * actually wrong. The two failures it catches are the two shapes of the bug:
 *
 *   - the pill wider than the viewport, which crops it and hides its rounded ends
 *   - bare window either side of the pill beyond the glow margin, which paints as
 *     a rectangle because a transparent Windows webview does not reliably show
 *     through where nothing is drawn
 *
 * Both are cheap to test and impossible to argue with, so they report themselves
 * whether or not anyone is looking. A user who can reproduce this only has to open
 * the webview devtools and read the error.
 *
 * # What is opt-in
 *
 * Everything else — the phase-by-phase timeline — is behind [`tracing`], because
 * it fires several times per state change and would otherwise bury the errors
 * above. Turn it on from the overlay's devtools console:
 *
 * ```js
 * localStorage.ov_trace = "1"; location.reload();
 * ```
 *
 * The timeline pairs with the `overlay set_box` field on the Rust side. Read them
 * together: `render` and `flush` are what the web side wanted and when, `set_box`
 * is what Rust did about it, and `viewport` is when the surface finally agreed.
 * The gap between `flush` and `viewport` is the exact duration of the artefact.
 */

/** A phase of the resize handshake, in the order they occur. */
export type Phase =
  /** React has decided on a new pill size. */
  | "render"
  /** The resize is about to cross the IPC boundary. */
  | "flush"
  /** The resize command has returned. */
  | "flushed"
  /** The layout viewport actually changed. This is the one that ends the artefact. */
  | "viewport"
  /** Rust has placed the window and said where. */
  | "parked"
  /** A reported move was recognised as one this side commanded. */
  | "moved-echo"
  /** A reported move was accepted as the user dragging the bar. */
  | "dragged"
  /** A post-drag slide began, with its origin and destination. */
  | "settle-start"
  /** A slide was abandoned because something else re-placed the window. */
  | "settle-cancelled"
  /** A slide finished and set the anchor from its destination. */
  | "settle-end"
  /** The display scale factor changed under the window. */
  | "scale-changed";

/** Tracing is read once: it must not change mid-session, or the timeline has
 *  holes in it that look like dropped frames. */
const verbose = (() => {
  try {
    return (
      localStorage.getItem("ov_trace") === "1" ||
      new URLSearchParams(location.search).get("trace") === "1"
    );
  } catch {
    // `localStorage` throws rather than returning null when storage is
    // partitioned or disabled. Tracing off is the right answer, not a crash in
    // the window whose whole job is to be reliable.
    return false;
  }
})();

const t = () => Math.round(performance.now());

/**
 * Send a line to the app's log file as well as the console.
 *
 * The console is the wrong place for this on its own. A release Flow Bar has no
 * devtools, so every report of "it looked wrong for a second" arrives with no
 * evidence attached and cannot be reproduced on demand. The Rust half of this
 * same handshake has been logging to `openvoice.log` for weeks, and reading it is
 * what proved the window geometry was correct all along — the web half needs to
 * land in the same file, on the same clock, or half the handshake stays invisible.
 *
 * Fire-and-forget: instrumentation that can reject is instrumentation that can
 * break the thing it is watching.
 */
function toLog(level: "debug" | "warn" | "error", msg: string, data: unknown): void {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
  void import("@tauri-apps/api/core")
    .then(({ invoke }) =>
      invoke("overlay_log", { level, msg, data: JSON.stringify(data ?? {}) }),
    )
    .catch(() => {});
}

/**
 * The last box the window was asked for, and when.
 *
 * Kept rather than consumed. The previous version of this was a bare
 * `lastFlushAt` zeroed by the first `ResizeObserver` callback after each flush,
 * which is wrong in the case the measurement exists for: one commanded resize
 * produces *several* RO callbacks — WebView2 sets the controller bounds
 * synchronously on `WM_SIZE` and posts the child `SetWindowPos` separately
 * (`wry/src/webview2/mod.rs:1232` and `:1248`), and width and height need not
 * land in the same layout pass — so the first callback took the number and every
 * later one, including the one that actually reached the commanded size,
 * reported `null`. The delta that matters is flush -> *settled*, not
 * flush -> first twitch.
 */
type Flush = { at: number; w: number; h: number; m: number };
let lastFlush: Flush | null = null;

/**
 * Where the window was last *told* to be, in logical screen pixels.
 *
 * Separate from `lastFlush` because `settleTo` moves the window with
 * `setPosition` sixty times a second without resizing it, so the position and
 * the flush clock have different lifetimes. Reading it costs nothing and needs
 * no IPC, which is the point: `outerPosition()` is a round trip, and a position
 * fetched a round trip after the pill was measured belongs to a different
 * instant — the exact mistake ADR 0006 is about.
 */
let winAt: { x: number; y: number } | null = null;

/** Record a position this side has commanded, so the pill's absolute screen
 *  rectangle can be reconstructed synchronously. Called by `flush` (via
 *  `mark("flush")`) and by every frame of `settleTo`. */
export function noteWindowAt(x: number, y: number): void {
  winAt = { x, y };
}

/** Record a phase. Console by default; also into the app's log file when the
 *  timeline is being collected from a user who cannot open devtools. */
export function mark(phase: Phase, fields: Record<string, unknown> = {}): void {
  // Timed whether or not anyone is tracing, so `watchViewport` always has a
  // start point for its delta.
  // No position here any more: a flush carries a shape, not a box, because the
  // window no longer moves when the bar changes state. `winAt` is maintained by
  // the two things that do move it -- parking and the post-drag settle.
  if (phase === "flush") {
    lastFlush = {
      at: performance.now(),
      w: Number(fields.w),
      h: Number(fields.h),
      m: Number(fields.m),
    };
  }
  if (!verbose) return;
  console.info(`[flowbar] ${t()}ms ${phase}`, fields);
  toLog("debug", phase, fields);
}

/** What the window and its content currently disagree about. */
export type Reading = {
  /** The layout viewport, which is the window's client area. */
  view: { w: number; h: number };
  /** The pill as it will actually be painted. */
  pill: { w: number; h: number };
  /** Bare window either side of the pill. Should be the glow margin, or zero. */
  spare: { x: number; y: number };
  /** Where the pill actually sits inside the window. Should equal `spare`. */
  at: { x: number; y: number };
};

/**
 * Measure the pill against the window it is painted in, and complain if the two
 * cannot both be right.
 *
 * `expectedMargin` is the space the pill is *entitled* to leave bare — the glow
 * margin while listening, zero otherwise. One pixel of slack on top of it, because
 * a logical coordinate makes a lossy round trip through physical pixels on a
 * fractional display scale and coming back half a pixel short is not a bug.
 *
 * Returns the reading whether or not it was valid, so callers can log it.
 */
export function checkInvariants(
  el: HTMLElement | null,
  pillTop: number,
  expectedPillH?: number,
): Reading | null {
  if (!el) return null;

  const view = {
    w: document.documentElement.clientWidth,
    h: document.documentElement.clientHeight,
  };
  const r = el.getBoundingClientRect();
  const pill = { w: Math.round(r.width), h: Math.round(r.height) };
  const spare = {
    x: Math.round((view.w - pill.w) / 2),
    y: Math.round((view.h - pill.h) / 2),
  };
  // Where the pill actually sits inside the window, as opposed to how big it is.
  const at = { x: Math.round(r.left), y: Math.round(r.top) };
  const reading: Reading = { view, pill, spare, at };

  const slack = 1;

  // The pill does not fit its window. Under the fixed-window design this should
  // be impossible — the window is wider than the widest state — so it means a
  // pill tier has outgrown `OVERLAY_W` and needs the window widening to match.
  if (pill.w > view.w + slack || pill.h > view.h + slack) {
    console.error(
      `[flowbar] INVARIANT: pill ${pill.w}x${pill.h} exceeds window ${view.w}x${view.h}. ` +
        `A pill tier has outgrown the fixed window; OVERLAY_W/H must be raised.`,
      reading,
    );
    toLog("error", "pill exceeds window", reading);
    return reading;
  }

  // The pill height collapsed or deviated from the expected height.
  if (expectedPillH !== undefined && Math.abs(pill.h - expectedPillH) > slack) {
    console.error(
      `[flowbar] INVARIANT: pill height ${pill.h} deviates from expected height ${expectedPillH}. ` +
        `The pill is squished, crunched or incorrectly sized.`,
      reading,
    );
    toLog("error", "pill height deviated", { ...reading, expectedPillH });
    return reading;
  }

  // The pill is not horizontally centred.
  //
  // The bar's screen position is the window's fixed origin plus this offset, so
  // this is the one measurement that can still put the bar somewhere the user
  // did not choose. It replaces the old "pill not centred" check, which compared
  // the pill against the *viewport* on both axes — and the viewport was the
  // lagging quantity, so it was self-consistent by construction and fired zero
  // times across the entire life of the bug it was written to catch.
  if (Math.abs(at.x - spare.x) > slack) {
    console.error(
      `[flowbar] INVARIANT: pill is at x=${at.x} inside a ${view.w}px window but ` +
        `should be centred at x=${spare.x}. The bar will look displaced.`,
      reading,
    );
    toLog("error", "pill not centred horizontally", reading);
    return reading;
  }

  // The pill's top edge has moved.
  //
  // Vertically the pill is *not* centred — it sits at a constant `PILL_TOP` from
  // the window's top, with the glow's margin above it and the menu's room below.
  // That constant is shared with `overlay.rs`, which clips the window to a region
  // computed from it, so a drift here means the painted bar and the clickable
  // region have come apart.
  if (Math.abs(at.y - pillTop) > slack) {
    console.error(
      `[flowbar] INVARIANT: pill's top edge is at y=${at.y}, expected ${pillTop}. ` +
        `The window's region is computed from that constant, so the bar and its ` +
        `clickable area no longer agree.`,
      reading,
    );
    toLog("error", "pill top edge moved", { ...reading, pillTop });
    return reading;
  }

  // The shape the window is clipped to, against the pill that is actually
  // painted. See `reportClippedPill`.
  reportClippedPill(pill.w);

  // Check internal content centering if el has a .flowbar child
  checkContentCentering(el);

  mark("render", reading as unknown as Record<string, unknown>);
  return reading;
}

/** Shape-versus-paint disagreements already reported, so a standing one is one
 *  line rather than one every layout pass. Same reasoning as `stuck`. */
const clipped = new Set<string>();

/**
 * How long after a flush the painted pill is allowed to disagree with it.
 *
 * The box is on a `width`/`height` transition — 300ms plus a 90ms stagger on the
 * slower axis — so mid-morph it is legitimately neither size. Anything past that
 * is not an animation, it is a disagreement.
 */
const MORPH_SETTLE_MS = 600;

/**
 * The window is clipped to a pill the bar is no longer painting.
 *
 * This is the check that was missing, and its absence is why the cold-launch
 * crop went unreported for as long as it did. Every other alarm in this file
 * passed throughout it: the pill fitted its window, it was centred, its top edge
 * was where it belongs, and the shape this side wanted was the shape it had
 * sent. The one pair nobody compared was the shape Rust was told and the pill
 * Chromium drew — and those are the two that were different, by five pixels,
 * because `geometry()` sizes the pill from a canvas `measureText` and that
 * measurement changes underneath React when a web font arrives. See
 * `useFontsReady` for the cause and the fix.
 *
 * The user sees this as the bar's rounded ends sliced flat, because the window's
 * region clips painting: whatever the pill draws outside the last shape sent
 * simply is not there.
 *
 * Compared against the *pill's* width rather than the shape's rectangle, which
 * is that plus the glow margin on both sides. The margin can only ever widen the
 * region, so leaving it out makes this conservative: it fires when the pill is
 * clipped even with the glow's allowance, and never on the allowance alone.
 */
export function reportClippedPill(paintedW: number): void {
  const f = lastFlush;
  if (!f) return;
  // Still animating towards the shape that was asked for. Not a disagreement.
  if (performance.now() - f.at < MORPH_SETTLE_MS) return;
  // One pixel of slack, as everywhere else here: a logical coordinate makes a
  // lossy round trip through physical pixels on a fractional display scale.
  const over = paintedW - f.w;
  if (over <= 1) return;
  const key = `${f.w}+${f.m}<${Math.round(paintedW)}`;
  if (clipped.has(key)) return;
  clipped.add(key);
  const reading = { shape: { w: f.w, h: f.h, m: f.m }, painted: paintedW, over };
  console.error(
    `[flowbar] CLIPPED: the window is clipped to a ${f.w}px pill but a ${paintedW}px ` +
      `pill is being painted, so ${(over / 2).toFixed(1)}px is sliced off each end. ` +
      `The shape and the paint were computed from different measurements.`,
    reading,
  );
  toLog("error", "painted pill is wider than the window's shape", reading);
}

/**
 * Measure horizontal content centering within the Flow Bar pill.
 * Asserts that distLeft (pill.left -> first child) == distRight (last child -> pill.right).
 */
export function checkContentCentering(
  el: HTMLElement | null,
  slack = 2,
): { centered: boolean; leftGap: number; rightGap: number; diff: number } | null {
  if (!el) return null;
  const pillEl = el.classList.contains("flowbar")
    ? el
    : el.querySelector<HTMLElement>(".flowbar");
  if (!pillEl) return null;

  const pillRect = pillEl.getBoundingClientRect();
  if (pillRect.width === 0) return null;

  const contentEls = Array.from(
    pillEl.querySelectorAll<HTMLElement>(
      ".flowbar-mic, .flowbar-idle > *, .flowbar-msg, .flowbar-working-text, .flowbar-wave, .flowbar-time, .flowbar-cancel, .flowbar-go",
    ),
  ).filter((e) => {
    const r = e.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  });

  if (contentEls.length === 0) return null;

  let minLeft = Infinity;
  let maxRight = -Infinity;

  for (const child of contentEls) {
    const r = child.getBoundingClientRect();
    if (r.left < minLeft) minLeft = r.left;
    if (r.right > maxRight) maxRight = r.right;
  }

  const leftGap = Math.round((minLeft - pillRect.left) * 10) / 10;
  const rightGap = Math.round((pillRect.right - maxRight) * 10) / 10;
  const diff = Math.round(Math.abs(leftGap - rightGap) * 10) / 10;
  const centered = diff <= slack;

  if (!centered && pillRect.width > 200) {
    console.error(
      `[flowbar] INVARIANT: Content not centered horizontally within pill (${Math.round(pillRect.width)}px pill). ` +
        `leftGap=${leftGap}px vs rightGap=${rightGap}px (diff=${diff}px).`,
      { pillW: Math.round(pillRect.width), leftGap, rightGap, diff },
    );
    toLog("error", "content not centered in pill", { pillW: Math.round(pillRect.width), leftGap, rightGap, diff });
  }

  return { centered, leftGap, rightGap, diff };
}

/** A window box: the size asked of the window, and the margin inside it. */
type Box = { w: number; h: number; m: number };

/** Boxes already complained about, so a stuck window says so once rather than
 *  twice a second for as long as it stays stuck. */
const stuck = new Set<string>();

/**
 * The window did not end up the size this side asked for.
 *
 * Always on, like the invariants above, and for the same reason: this is the
 * failure that has now happened twice in two different forms — the first resize
 * dropped at mount, and the last one dropped on cancel, leaving the bar stuck at
 * its listening width — and both times it was invisible from the code's point of
 * view because the request looked fine. Only the arrival was missing.
 *
 * `inFlight` distinguishes "still trying" from "gave up", which is the difference
 * between a slow machine and a bug.
 */
export function reportStuckBox(sent: Box, want: Box, inFlight: boolean): void {
  const key = `${sent.w}x${sent.h}+${sent.m}->${want.w}x${want.h}+${want.m}`;
  if (stuck.has(key)) return;
  stuck.add(key);
  console.error(
    `[flowbar] STUCK: window is ${sent.w}x${sent.h} (margin ${sent.m}) but should be ` +
      `${want.w}x${want.h} (margin ${want.m}) — a resize was dropped` +
      `${inFlight ? ", and one is still in flight" : " and nothing is retrying"}.`,
    { sent, want, inFlight },
  );
  toLog("error", "window stuck at wrong box", { sent, want, inFlight });
}

/**
 * Report when the layout viewport actually settles, and how long it took.
 *
 * This is the measurement that decides whether a given artefact was the web side
 * painting early or the window resizing late: the delta between the `flush` that
 * asked for a size and the `viewport` that got it is precisely how long the bar
 * spent disagreeing with itself. Returns a teardown.
 */
/** The layout viewport, in CSS pixels. The quantity every rule below turns on. */
export function viewportNow(): { w: number; h: number } {
  return {
    w: document.documentElement.clientWidth,
    h: document.documentElement.clientHeight,
  };
}

/**
 * Whether the webview is laying out in the window it was given.
 *
 * # The bug
 *
 * These two are the same number until WebView2 changes its rasterization scale,
 * which it can do on a display change, a lock, or a monitor waking, with no DPI
 * message reaching the app. When it does, the viewport becomes the window's
 * *physical* size — 505x800 for a 404x640 window on a 125% panel — and every CSS
 * pixel is one physical pixel instead of 1.25.
 *
 * The window is clipped to a region computed in Rust, and that region used to be
 * derived from `OVERLAY_W` times the monitor's scale. So the bar painted at
 * physical y 300..324 while the window was clipped to y 375..405: disjoint, in
 * every state, until the app was restarted. The user's report was that the Flow
 * Bar "gets invisible and pressing Ctrl does nothing".
 *
 * Reported once per transition, in both directions, for the same reason
 * `probePill` reports an onset and a recovery: what matters is when it started
 * and whether it stopped, not that it is still true this frame.
 */
export function noteViewportContract(view: { w: number; h: number }, w: number, h: number): void {
  const wrong = view.w !== w || view.h !== h;
  if (wrong === contractWrong) return;
  contractWrong = wrong;
  const fields = { view, window: { w, h }, ratio: w === 0 ? null : view.w / w };
  if (wrong) {
    console.error(
      `[flowbar] SCALE: the webview is laying out in ${view.w}x${view.h} but its ` +
        `window is ${w}x${h}. Everything positioned from the viewport is now in ` +
        `the wrong place relative to the window's clip region.`,
      fields,
    );
    toLog("error", "webview viewport is not the window", fields);
  } else {
    toLog("debug", "webview viewport agrees with the window again", fields);
  }
}

/** Whether the viewport and the window currently disagree. See above. */
let contractWrong = false;

/**
 * @param onChange called with the new viewport whenever it actually changes, so
 *   the shape can be re-sent. Before this existed the viewport could change with
 *   no flush to follow it — the log has three such changes 217 seconds after the
 *   last flush — leaving Rust clipping the window to a region derived from a
 *   viewport that no longer existed.
 */
export function watchViewport(onChange?: (v: { w: number; h: number }) => void): () => void {
  // Three sources, deliberately, because it is not settled which of them
  // actually fires when a WebView2 host window is resized.
  //
  // `ResizeObserver` on `document.documentElement` is the obvious one and should
  // work — `global.css` gives `html`, `body` and `#root` `height: 100%`, so the
  // root element's box tracks the viewport in both axes. But this measurement
  // has produced *zero* lines in a log that contains sixty-four errors from the
  // instrumentation next to it, and until that is explained the observer cannot
  // be assumed to fire. `window.onresize` and `visualViewport.onresize` cost a
  // listener each and remove the doubt: if any of the three notices, the line
  // gets written, and `via` says which one did.
  //
  // Whichever source reports, the numbers come from
  // `document.documentElement.clientWidth/clientHeight` — the layout viewport,
  // which is the quantity the pill's CSS is derived from and therefore the only
  // one whose lag explains the artefact.
  let last = {
    w: document.documentElement.clientWidth,
    h: document.documentElement.clientHeight,
  };
  // Which flush these ticks belong to, and how many have arrived for it. One
  // commanded resize can settle in stages, so the line where `settled` first
  // turns true is the answer rather than the first line after the flush.
  let clock = -1;
  let tick = 0;

  const report = (via: string) => {
    const now = performance.now();
    const w = document.documentElement.clientWidth;
    const h = document.documentElement.clientHeight;
    // Nothing actually moved. Three sources watching one quantity means the same
    // change is notified up to three times; only the change is interesting.
    if (w === last.w && h === last.h) return;
    last = { w, h };

    const f = lastFlush;
    if (f && f.at !== clock) {
      clock = f.at;
      tick = 0;
    }
    tick += 1;

    // The number the whole investigation turns on.
    //
    // The window is moved up and left by the glow margin precisely so the pill
    // can re-centre down and right by the same amount, and the two cancel. The
    // re-centring is done in CSS from `100vw`/`100vh`, so it cannot happen until
    // the layout viewport has actually grown — and until it does, the bar is
    // painted at the window's new top-left, displaced by exactly the margin.
    // This delta is how long that lasts.
    //
    // It goes to the log file unconditionally, not behind the verbose flag. Every
    // previous version of this measurement was gated, which meant a release build
    // reproducing the bug produced no evidence at all — the exact failure this
    // file was written to end. One line per resize is affordable; not being able
    // to answer the question is not.
    //
    // `settled` is the field to read. The delta on the line where it first turns
    // true is how long the layout viewport lagged the window, and therefore how
    // long the CSS that centres the pill was working from the old size.
    const fields = {
      via,
      w,
      h,
      want: f ? { w: f.w, h: f.h } : null,
      settled: f ? w === f.w && h === f.h : null,
      tick,
      sinceFlushMs: f ? Math.round(now - f.at) : null,
    };
    toLog("debug", "viewport settled", fields);
    if (verbose) console.info(`[flowbar] ${t()}ms viewport`, fields);

    // Before the pill is re-measured, because the shape this triggers is what
    // makes the region right again — and a probe against a region that is about
    // to be replaced measures the problem rather than the fix.
    onChange?.({ w, h });

    // The pill is re-measured here as well as in the layout effect, because a
    // viewport change does not require React to render: if the pill is displaced
    // and nothing re-renders, the layout effect never runs and the recovery
    // would go unrecorded.
    pollPill();
  };

  const onWindow = () => report("window");
  const onVisual = () => report("visualViewport");
  window.addEventListener("resize", onWindow);
  const vv = window.visualViewport;
  vv?.addEventListener("resize", onVisual);

  const ro =
    typeof ResizeObserver === "undefined"
      ? null
      : new ResizeObserver(() => report("ResizeObserver"));
  // `observe()` always delivers one callback with the element's current size
  // before anything has resized. `last` is primed above, so that delivery is
  // dropped as "nothing moved" rather than reported as a settle with a delta
  // against whatever flush happened to be outstanding at mount.
  ro?.observe(document.documentElement);

  return () => {
    ro?.disconnect();
    window.removeEventListener("resize", onWindow);
    vv?.removeEventListener("resize", onVisual);
    stopPoll();
  };
}

/* -- where the pill is actually painted ------------------------------------- */

/** Where the pill belongs on screen: its centre-x and top edge, in logical
 *  pixels. The same `Anchor` `Overlay.tsx` keeps; duplicated rather than
 *  imported to keep this module free of the component. */
export type Anchor = { cx: number; top: number };

/** The element and anchor the poller should keep re-measuring. Refreshed by
 *  every `probePill` call, which the layout effect makes on every pass. */
let subject: { el: HTMLElement; anchor: Anchor } | null = null;

/** When the pill first left its anchor, or 0. */
let offAnchorSince = 0;
/** The rAF poll that runs only while the pill is off its anchor. */
let poll = 0;

function pollPill(): void {
  if (!subject) return;
  probePill(subject.el, subject.anchor);
}

function stopPoll(): void {
  if (poll) cancelAnimationFrame(poll);
  poll = 0;
  offAnchorSince = 0;
  subject = null;
}

/**
 * Measure where the pill is actually painted **on screen** and compare it to
 * where it belongs.
 *
 * This is the measurement the whole investigation was missing. `checkInvariants`
 * asks whether the pill is centred *in its window*, and it can be — perfectly —
 * while the bar is visibly in the wrong place, because the window is deliberately
 * moved up and left by the glow margin so the pill can move down and right by the
 * same amount inside it. If the viewport has not grown yet, the pill centres
 * itself in the *old* box inside the *new* window rectangle: still centred, still
 * passing every existing invariant, and displaced on screen by exactly the
 * difference between the two boxes.
 *
 * The absolute rectangle is reconstructed without IPC. `winAt` is the position
 * the window was commanded to, recorded synchronously before the call that moves
 * it; `getBoundingClientRect` is the pill inside that window. Their sum is the
 * pill in screen coordinates, sampled in one tick with no await between the two
 * halves.
 *
 * Reports the onset once and the recovery once, with the duration between them.
 * Two lines per artefact rather than one per frame, and nothing at all when the
 * bar is behaving.
 */
export function probePill(el: HTMLElement | null, anchor: Anchor | null): void {
  if (!el || !anchor) return;
  // A detached node measures as all zeros, which reads as a permanent
  // displacement and would leave the poll below spinning forever against an
  // element nobody can see.
  if (!el.isConnected) {
    stopPoll();
    return;
  }
  const at = winAt;
  if (!at) return;
  subject = { el, anchor };

  const r = el.getBoundingClientRect();
  const view = {
    w: document.documentElement.clientWidth,
    h: document.documentElement.clientHeight,
  };
  // The pill in logical screen pixels.
  const screen = { x: Math.round(at.x + r.left), y: Math.round(at.y + r.top) };
  const dx = Math.round(screen.x + r.width / 2 - anchor.cx);
  const dy = Math.round(screen.y - anchor.top);

  // One pixel of slack, for the same reason `checkInvariants` allows it: a
  // logical coordinate makes a lossy round trip through physical pixels.
  if (Math.abs(dx) <= 1 && Math.abs(dy) <= 1) {
    if (offAnchorSince) {
      toLog("error", "pill back on anchor", {
        heldMs: Math.round(performance.now() - offAnchorSince),
        view,
        screen,
        anchor,
      });
      offAnchorSince = 0;
    }
    if (poll) {
      cancelAnimationFrame(poll);
      poll = 0;
    }
    return;
  }

  if (offAnchorSince) return;
  offAnchorSince = performance.now();
  const f = lastFlush;
  toLog("error", "pill displaced from anchor", {
    // Negative dx and dy are up and to the left, which is the reported symptom.
    dx,
    dy,
    view,
    // What the window was told to become, and whether the viewport has caught
    // up with it yet. `viewportBehind` true alongside a displacement is the
    // hypothesis this was written to test: the pill centred in the old viewport
    // inside the new window rectangle.
    want: f ? { w: f.w, h: f.h } : null,
    // The verdict of `noteViewportContract`, rather than a comparison made here.
    //
    // What stood here compared the layout viewport against the *pill's* width —
    // 505 against 96 — so it read `true` on every line it ever printed and said
    // nothing at all. The number that mattered was sitting in `view` beside it,
    // unexamined. Deferring to the one place that knows what the window is
    // supposed to be means there is no second rule to get wrong again.
    viewportIsWindow: !contractWrong,
    sinceFlushMs: f ? Math.round(performance.now() - f.at) : null,
    box: at,
    pill: { w: Math.round(r.width), h: Math.round(r.height) },
    at: { x: Math.round(r.left), y: Math.round(r.top) },
    anchor,
  });

  // A layout pass is not guaranteed while this lasts — nothing has to re-render
  // for the viewport to catch up — so the recovery is polled for rather than
  // waited on. The loop exists only while the bar is wrong, so it costs nothing
  // in the case that is not being investigated.
  const step = () => {
    poll = requestAnimationFrame(step);
    pollPill();
  };
  poll = requestAnimationFrame(step);
}
