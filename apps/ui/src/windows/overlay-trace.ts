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

/** Record a phase. Console by default; also into the app's log file when the
 *  timeline is being collected from a user who cannot open devtools. */
let lastFlushAt = 0;

export function mark(phase: Phase, fields: Record<string, unknown> = {}): void {
  // Timed whether or not anyone is tracing, so `watchViewport` always has a
  // start point for its delta.
  if (phase === "flush") lastFlushAt = performance.now();
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
export function checkInvariants(el: HTMLElement | null, expectedMargin: number): Reading | null {
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

  // The pill is the right size but in the wrong place.
  //
  // This is the case the two size checks below cannot see, and it is the shape of
  // the bug that survived every geometry fix: the window is moved up and left by
  // the glow margin precisely so the pill can move down and right by the same
  // amount inside it, and the two cancel. If the pill does not take its half of
  // that bargain — if it paints at the window's top-left corner instead of
  // centred — the cancellation fails and the whole bar appears displaced by the
  // margin, even though every number the window was given is correct.
  //
  // Checked against the pill's own centring rather than against the expected
  // margin, so it stays true whatever the window happens to be: a centred pill
  // has `spare` on each side by definition.
  if (Math.abs(at.x - spare.x) > slack || Math.abs(at.y - spare.y) > slack) {
    console.error(
      `[flowbar] INVARIANT: pill is at ${at.x},${at.y} inside a ${view.w}x${view.h} ` +
        `window but should be centred at ${spare.x},${spare.y}. The bar will look ` +
        `displaced by the difference.`,
      reading,
    );
    toLog("error", "pill not centred in window", reading);
    return reading;
  }

  // The pill does not fit. It is cropped at the window edge: its rounded ends and
  // side borders are outside the surface entirely, so what is left reads as a
  // square black box with the content cut off.
  if (pill.w > view.w + slack || pill.h > view.h + slack) {
    console.error(
      `[flowbar] INVARIANT: pill ${pill.w}x${pill.h} exceeds window ${view.w}x${view.h}. ` +
        `The pill is being painted before the window has been resized to hold it.`,
      reading,
    );
    toLog("error", "pill exceeds window", reading);
    return reading;
  }

  // There is window with nothing in it. On a transparent Windows webview that
  // area is undefined rather than clear, and it is what shows up as a rectangle
  // around the bar.
  if (spare.x > expectedMargin + slack || spare.y > expectedMargin + slack) {
    console.error(
      `[flowbar] INVARIANT: ${spare.x}x${spare.y}px of bare window around the pill, ` +
        `expected ${expectedMargin}px. Bare window area paints as a rectangle.`,
      reading,
    );
    toLog("error", "bare window around pill", { ...reading, expectedMargin });
    return reading;
  }

  mark("render", reading as unknown as Record<string, unknown>);
  return reading;
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
export function watchViewport(): () => void {
  if (typeof ResizeObserver === "undefined") return () => {};
  const ro = new ResizeObserver(() => {
    const now = performance.now();
    const w = document.documentElement.clientWidth;
    const h = document.documentElement.clientHeight;
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
    const lag = lastFlushAt ? Math.round(now - lastFlushAt) : null;
    toLog("debug", "viewport settled", { w, h, sinceFlushMs: lag });
    if (verbose) console.info(`[flowbar] ${t()}ms viewport`, { w, h, sinceFlushMs: lag });
    lastFlushAt = 0;
  });
  ro.observe(document.documentElement);
  return () => ro.disconnect();
}
