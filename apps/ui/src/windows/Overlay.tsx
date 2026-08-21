/** The Flow Bar window.
 *
 * Frameless, transparent, always on top, and non-activating (`WS_EX_NOACTIVATE`,
 * applied on the Rust side). If this window ever takes focus, the caret in the
 * user's editor is lost and the dictated text goes nowhere.
 *
 * `WS_EX_NOACTIVATE` prevents *focus*, not *input* — so the bar can be dragged and
 * right-clicked while the editor keeps the caret. That is the only reason an
 * interactive always-on-top overlay is viable for a dictation tool.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { FlowBar } from "../ui";
import { playCompletionChime, playStartTone } from "../ui/sound";
import { elapsed, useLiveEngine } from "../engine/useLiveEngine";
import { useSettings } from "../screens/Settings";
import { checkInvariants, mark, reportStuckBox, watchViewport } from "./overlay-trace";
import "./overlay.css";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call(cmd: string, args?: Record<string, unknown>) {
  if (!inTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
}

/**
 * Window space reserved on every side while listening, so the pill's glow has
 * somewhere to land.
 *
 * A `box-shadow` is clipped at the window edge, and the window is sized to the
 * pill exactly — which is why this surface had no glow for so long despite
 * DESIGN.md granting it one. Reserving the margin is the same trick the
 * right-click menu already uses for its own shadow. It is claimed only while
 * listening: the window swallows OS clicks across its whole rectangle, so the
 * dead zone this creates is only acceptable during the seconds the user is
 * holding a key and talking rather than clicking.
 */
const GLOW_MARGIN = 22;

/**
 * The pill's height, in every state but the menu.
 *
 * Duplicated as `--pill-h` in `overlay.css`, deliberately and with a comment at
 * each end. The window's height is computed from it here; the pill's width is
 * recovered from the window using it there. The two are one identity written
 * twice because it has to hold in both languages, and the CSS half is what makes
 * the pill unable to outgrow its window. Change one and the other is wrong.
 */
const PILL_H = 40;

/** How long the bar takes to slide into a snapped position after a drag. */
const SETTLE_MS = 220;

/**
 * How long after the last real movement a drag is still considered to be in
 * progress.
 *
 * Generous, because it is refreshed by every move a drag produces and only has to
 * outlast the pause when someone holds the bar still mid-drag. It exists to
 * expire a drag that ended without saying so — see `dragging`.
 */
const DRAG_TTL = 1500;

/**
 * How many commanded positions to keep so the bar can recognise its own moves
 * coming back.
 *
 * Windows reports every move, including the ones this app asks for, and there is
 * no flag on the report saying which. Telling them apart used to rest entirely on
 * a "the user is dragging" boolean, which is a heuristic about a Windows move
 * loop that eats its own mouse-up — so when it was wrong, an automatic resize was
 * committed as a deliberate drag: snapped, written to disk as the user's chosen
 * position, and then animated there.
 *
 * Comparing against what was actually commanded is not a heuristic. More than one
 * is kept because the hotkey produces resizes 26ms apart and each move is
 * reported over an IPC round trip, so several of ours can be in flight at once.
 */
const COMMANDED_HISTORY = 8;

/** Whether a reported position is one of ours coming back. Two logical pixels:
 *  far below what a deliberate drag covers, far above the rounding error of a
 *  logical -> physical -> logical round trip on a fractional display scale. */
function isEcho(history: Array<[number, number]>, x: number, y: number): boolean {
  return history.some(([cx, cy]) => Math.abs(cx - x) < 2 && Math.abs(cy - y) < 2);
}

/**
 * Everything in the idle pill that is not the shortcut itself: padding, border,
 * the status dot, two gaps, the word "Hold", and the key cap's own chrome.
 *
 * Measured off the rendered component rather than derived from the stylesheet,
 * because it is the sum of seven values that live in four rules. With the
 * default "Right Ctrl" it reproduces the 150px this tier was hardcoded to; the
 * point of computing it is every *other* shortcut, which previously collided
 * with the word "Hold" and got clipped by the pill's own `overflow: hidden`.
 */
const IDLE_CHROME = 85;

/** Padding, border, dot and gap around a message. Same method as above. */
const MSG_CHROME = 49;

/**
 * Width of a string as the bar will actually draw it.
 *
 * A canvas rather than a hidden DOM node: this has to be answered before the
 * window is sized, and a measuring element would need a layout pass inside a
 * window that is still the wrong size to hold it.
 */
function textWidth(text: string, font: string): number {
  const canvas = (textWidth as { c?: HTMLCanvasElement }).c ??
    ((textWidth as { c?: HTMLCanvasElement }).c = document.createElement("canvas"));
  const ctx = canvas.getContext("2d");
  if (!ctx) return text.length * 7;
  const root = getComputedStyle(document.documentElement);
  ctx.font = font.replace("$mono", root.getPropertyValue("--font-mono").trim() || "monospace")
    .replace("$sans", root.getPropertyValue("--font-sans").trim() || "sans-serif");
  return ctx.measureText(text).width;
}

/** How long a failure or a clipboard fallback stays on the bar before it packs
 *  itself away. Long enough to read at a glance, short enough that an
 *  always-on-top window never becomes a notification that has to be dismissed. */
const ALERT_MS = 6000;

/** A discard needs acknowledging, not reading. The user already knows why. */
const DISCARD_MS = 1600;

/** Outcomes where the user ended up with nothing. See `failed` below. */
const FAILED_OUTCOMES = new Set(["asr_failed", "capture_failed"]);

/**
 * Keep the Tauri window exactly the size of what is painted in it, and keep the
 * pill still while that changes.
 *
 * NO SPARE AREA, EVER. This is the invariant the whole window depends on, and
 * breaking it is what made the bar flash a rectangle on key release. Webview
 * transparency on Windows is not reliable enough to lean on, so any window area
 * with nothing painted in it can show up as a translucent box. An earlier
 * version here held the window at the larger size for a quarter of a second so
 * the pill could ease between widths — and on release, live → working, that left
 * a 284×84 window around a 170×40 pill with the glow already switched off. The
 * pill therefore does not animate its width: it is always exactly the window's
 * content box, and the window is resized once per state change. The one
 * deliberate exception is the glow margin while listening, which the glow itself
 * paints into.
 *
 * ANCHORED, ABSOLUTELY. The window grows from its top-left, so without
 * compensation, reserving the glow margin shoves the pill down and to the right
 * by exactly that margin. The fix is an anchor — where the *pill* belongs, as
 * its centre-x and top edge — from which each window position is computed
 * outright.
 *
 * Computing it outright is the part that matters. An earlier version adjusted
 * the previous position by the change in size, which is correct once and wrong
 * forever: every logical coordinate makes a lossy round trip through physical
 * pixels, the measured pill widths are frequently odd so the half-difference is
 * fractional, and each hotkey press folded that residue into the next position.
 * Spamming the key walked the bar across the screen. An absolute anchor cannot
 * drift, because nothing it produces is ever fed back into it.
 *
 * The anchor moves only when the user moves the bar, and is re-derived from the
 * window itself whenever that happens.
 */
type Anchor = { cx: number; top: number };
type Box = { w: number; h: number; m: number };

/**
 * Where the pill is, given where its window is — read off the window, not off
 * React state.
 *
 * This is the same correction ADR 0006 applied to the pill's width, applied to
 * the pill's position, and for the same reason. Every one of these conversions
 * used to multiply the window's position by `box.current`, which is the box this
 * side *intends* the window to become — written synchronously in the layout
 * effect, before the resize has even been sent. The three places that convert a
 * window position into an anchor were all reading it as though it were the box
 * the window *is*.
 *
 * That is what made the bar jump when the hotkey was spammed. `onMoved` fires for
 * every move including the ones we command ourselves, and when one arrived after
 * the next state had already updated `box.current`, the anchor was rebuilt with
 * the wrong half-width and the wrong margin: listening to transcribing put it
 * 57px left and 22px up, and the next press put it back. The displacement is
 * exactly `(w_new - w_prev) / 2` horizontally and `m_new - m_prev` vertically, it
 * survives until the following transition, and around a full cycle it sums to
 * zero — which is why the bar appeared to jump away and come back rather than
 * drifting.
 *
 * The viewport is the honest source and it is synchronous: no IPC, no await, and
 * nothing that can be a state ahead. The window carries its own margin, by the
 * identity ADR 0006 already depends on — the window is the pill plus the margin
 * on all four sides, so with the pill's height fixed the margin is
 * `(clientHeight - PILL_H) / 2`.
 *
 * The menu is the exception, as it is there: it claims the window below the pill,
 * so the height identity does not hold. Its margin is always zero, which makes
 * the conversion simpler rather than absent.
 */
function anchorFrom(x: number, y: number): Anchor {
  const w = document.documentElement.clientWidth;
  const h = document.documentElement.clientHeight;
  // Menu-ness is read off the window, not off React.
  //
  // This used to take a `menuOpen` argument sourced from a ref assigned during
  // render, which is the same "one value, two instants" mistake this function
  // exists to remove — just moved up a level. Between closing the menu and the
  // window shrinking out of the menu box, React says "not a menu" while the
  // window is still 226 tall, and the margin computes as (226-40)/2 = 93 instead
  // of 0. Right-clicking the bar and then dragging it hit exactly that gap and
  // dropped the anchor 93px.
  //
  // The window's own shape answers the question without a second source. A pill
  // window is never taller than the pill plus a glow margin on each side, so
  // anything taller is the menu, whose margin is always zero.
  const isPill = h <= PILL_H + GLOW_MARGIN * 2 + 1;
  const m = isPill ? Math.max(0, (h - PILL_H) / 2) : 0;
  return { cx: x + w / 2, top: y + m };
}
function useWindowGeometry(
  pillW: number,
  pillH: number,
  margin: number,
  anchor: React.MutableRefObject<Anchor | null>,
  want: React.MutableRefObject<Box>,
  ready: boolean,
  commanded: React.MutableRefObject<Array<[number, number]>>,
) {
  const boxW = pillW + margin * 2;
  const boxH = pillH + margin * 2;

  // Whether a command is already in flight. With `want`, this makes the sender
  // single-flight: bursts collapse to the latest desired box instead of
  // queueing, so a stale size can never land last — which is what left the
  // window bigger than the pill, showing as a rectangle, when the hotkey was
  // spammed.
  const sending = useRef(false);

  /** The box Rust has actually been told about, as opposed to the one this side
   *  currently wants. Null until the first command completes. */
  const sent = useRef<Box | null>(null);

  /**
   * Drive the window to `want`, and keep going until it is there.
   *
   * A convergence loop, and it has to be one. Two earlier versions of this were
   * *edge*-triggered — they decided at the top whether a send was needed and then
   * relied on the loop noticing later changes — and each dropped a different
   * resize:
   *
   *   - Comparing the desired box against `want` dropped the very first one. The
   *     mount pass sets `want` and calls this, which bails at `if (!a)` because
   *     the anchor is still being read off the window; when the anchor lands and
   *     the effect re-runs, `want` already holds the target, so it returned early
   *     and the box was never sent at all.
   *   - Comparing it against `sent` dropped the *last* one. Cancelling a dictation
   *     returns the bar to a box it was already at, so if that happened while the
   *     listening box was still in flight, the effect saw `sent` already matching
   *     and returned without touching `want` — and the loop below, terminating on
   *     `want.current === target` by identity, then agreed it was finished. The
   *     window stayed at the listening width with an idle pill in it: the bar
   *     stuck visibly elongated after Escape.
   *
   * Both holes come from the same mistake, which is asking "has something
   * changed?" instead of "are we there yet?". The loop now terminates only when
   * the window has actually been told the box this side wants, compared by value,
   * and the effect below unconditionally records what it wants and calls this. No
   * interleaving can leave the two disagreeing, because disagreeing is the loop's
   * continuation condition.
   */
  const flush = useCallback(async () => {
    if (sending.current) return;
    sending.current = true;
    try {
      for (;;) {
        const target = want.current;
        const s = sent.current;
        if (s && s.w === target.w && s.h === target.h && s.m === target.m) return;

        // No anchor yet: the window's own position is still being read. Returning
        // is safe rather than lossy — `sent` is left untouched, so the next run
        // of the effect (which `geomReady` guarantees) finds the two still
        // disagreeing and converges then.
        const a = anchor.current;
        if (!a) return;

        // Integer logical pixels. A fractional coordinate is rounded on the way
        // to physical pixels and rounded differently on the way back, and with
        // the odd pill widths that come out of measuring text, that residue is
        // what walked the bar sideways a fraction at a time.
        const x = Math.round(a.cx - target.w / 2);
        const y = Math.round(a.top - target.m);
        mark("flush", { x, y, w: target.w, h: target.h, m: target.m });
        // Remembered before the call, not after: the window starts moving inside
        // `overlay_set_box`, and the `onMoved` it produces can reach the handler
        // before this await resolves. A record written afterwards would arrive
        // too late to recognise its own echo.
        commanded.current.push([x, y]);
        if (commanded.current.length > COMMANDED_HISTORY) commanded.current.shift();
        await call("overlay_set_box", { x, y, w: target.w, h: target.h });
        sent.current = target;
        mark("flushed", { w: target.w, h: target.h, m: target.m });
      }
    } finally {
      sending.current = false;
    }
  }, [anchor, want, commanded]);

  // Records what is wanted and asks for it. No conditions, deliberately.
  //
  // Every early return that has ever been added here has dropped a resize, in
  // both directions — see the two named in `flush`. This effect's only job is to
  // state the target; deciding whether anything needs sending is the loop's job,
  // and the loop decides it by comparing where the window is with where it should
  // be rather than by trying to detect a change. `flush` is single-flight, so
  // calling it on a pass that has nothing to do costs one comparison.
  useLayoutEffect(() => {
    if (!inTauri()) return;
    want.current = { w: boxW, h: boxH, m: margin };
    void flush();
  }, [boxW, boxH, margin, ready, flush, want]);

  // A resize that never lands is silent, and that is what made this expensive.
  //
  // Both dropped-resize bugs looked to the user like the bar rendering wrong —
  // stuck wide, or the wrong shape — and neither left any trace, because from
  // this side everything had been asked for correctly. What was missing was
  // anybody checking that it *arrived*. Convergence is normally a frame or two;
  // half a second means a request was dropped, and the loop above is now built so
  // that cannot happen, which is precisely why it is worth an alarm if it does.
  useEffect(() => {
    if (!inTauri()) return;
    const id = window.setInterval(() => {
      const s = sent.current;
      const w = want.current;
      if (!s || (s.w === w.w && s.h === w.h && s.m === w.m)) return;
      reportStuckBox(s, w, sending.current);
    }, 500);
    return () => window.clearInterval(id);
  }, [want]);
}

export function Overlay() {
  const { view, levelRef } = useLiveEngine();
  const { settings } = useSettings();
  const [menu, setMenu] = useState(false);
  const [snapping, setSnapping] = useState(false);
  /**
   * When the current drag was last known to be real, as a `performance.now()`
   * stamp, or 0 for "not dragging".
   *
   * A timestamp rather than a boolean because the boolean had no reliable way
   * back down. It was raised on mouse-down and lowered only by a debounce that is
   * armed inside the `onMoved` handler — so a click on the bar that moved it zero
   * pixels produced no `onMoved`, never armed the debounce, and left the flag
   * raised for the rest of the session. `startDragging` cannot lower it either:
   * it dispatches `WM_NCLBUTTONDOWN` and returns immediately rather than when the
   * Windows move loop ends, and the move loop swallows the mouse-up.
   *
   * A real drag refreshes this on every move it produces. Anything arriving more
   * than `DRAG_TTL` after the last genuine movement is not part of a drag,
   * whatever the flag says.
   */
  const dragging = useRef(0);
  const hit = useRef<HTMLDivElement>(null);
  /** Where the pill belongs on screen. See `useWindowGeometry`. */
  const anchor = useRef<Anchor | null>(null);
  /** The box the window has been told to be. Read only by `useWindowGeometry`:
   *  converting a window position back into an anchor uses `anchorFrom`, which
   *  reads the window rather than this. See the note there. */
  const box = useRef<Box>({ w: 150, h: 40, m: 0 });
  /**
   * Positions this side has commanded, so moves reported back can be recognised
   * as our own rather than guessed at. See `COMMANDED_HISTORY`.
   */
  const commanded = useRef<Array<[number, number]>>([]);
  /**
   * Which settle is allowed to run. Bumped to start one and bumped again by
   * anything that re-places the window, which abandons it at its next frame.
   */
  const settleGen = useRef(0);
  /**
   * The display scale, read once.
   *
   * It is a constant for a monitor, and every place that awaited it was paying a
   * round trip for a fixed number — while opening a gap in which the window could
   * change size, so a position from one instant was combined with a viewport from
   * another. Held here so the conversion is synchronous everywhere.
   */
  const scaleRef = useRef(1);
  const [geomReady, setGeomReady] = useState(false);

  // Rust places the bar at startup and whenever it returns from hidden, and
  // says so. Re-deriving the anchor from that is what keeps "only show while
  // dictating" working: without it, the first resize after the bar reappears
  // would size it around wherever it sat before it was hidden.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      un = await listen<[number, number]>("overlay-parked", ({ payload }) => {
        anchor.current = anchorFrom(payload[0], payload[1]);
        mark("parked", { x: payload[0], y: payload[1], anchor: anchor.current });
      });
    })();
    return () => un?.();
  }, []);

  // Derive the anchor from the window once. Rust places the bar before this
  // window's script runs, so the starting point has to come from the window
  // itself rather than from a constant.
  useEffect(() => {
    if (!inTauri()) return;
    let live = true;
    void (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      const scale = await win.scaleFactor();
      const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()]);
      if (!live) return;
      scaleRef.current = scale;
      box.current = { w: size.width / scale, h: size.height / scale, m: 0 };
      anchor.current = anchorFrom(pos.x / scale, pos.y / scale);
      setGeomReady(true);
    })();
    return () => {
      live = false;
    };
  }, []);

  const live = view.state === "listening";
  const working = view.state === "transcribing" || view.state === "injecting";
  const soundEnabled = settings?.config.sound_enabled ?? true;

  // Two tones: one when the hotkey engages, one when a dictation finishes and
  // actually landed. Tracked off the raw state transition rather than the
  // `live`/`working` booleans above so a clipboard-fallback completion --
  // which settles to "idle" exactly the same way a real success does, per
  // `useLiveEngine`'s reducer -- doesn't get the success chime just because it
  // isn't a hard failure. `view.notice` is set the moment that fallback
  // happens and nothing in this window clears it, so its presence at the
  // instant of the idle transition is a reliable signal the completion wasn't
  // clean, independent of exactly when the Notice and Finished events arrive
  // relative to each other.
  const prevState = useRef(view.state);
  const [confirm, setConfirm] = useState(false);
  useEffect(() => {
    const prev = prevState.current;
    prevState.current = view.state;

    // "Delivered" specifically, not "reached idle without complaining". A
    // dictation cancelled mid-transcribe settles to idle exactly like a
    // successful one, so without the outcome this congratulated the user on
    // landing text they had just thrown away.
    // The outcome alone decides this now. It used to also require no notice,
    // as a proxy for "nothing went wrong" — but a notice from a *previous*
    // queued session arrives after this one started and is never cleared, so a
    // clean dictation following a clipboard fallback silently lost both its
    // ring and its chime. `lastOutcome` only ever reflects the current session.
    const landed =
      (prev === "transcribing" || prev === "injecting") &&
      view.state === "idle" &&
      view.lastOutcome === "delivered";

    // The visual half of the same moment the chime already marks. It runs even
    // with sound off — muting the app should not cost the confirmation.
    if (landed) {
      setConfirm(true);
      // Outlives the 420ms ring, so the animation is never cut off mid-flight.
      const t = window.setTimeout(() => setConfirm(false), 480);
      return () => window.clearTimeout(t);
    }

    if (!soundEnabled) return;
    if (prev !== "listening" && view.state === "listening") playStartTone();
  }, [view.state, view.notice, view.lastOutcome, soundEnabled]);

  // The chime, kept on its own so the effect above can return a cleanup for the
  // confirm timer without swallowing it.
  useEffect(() => {
    if (confirm && soundEnabled) playCompletionChime();
  }, [confirm, soundEnabled]);

  /**
   * The two endings that are not silent success, and the one thing this window
   * has never shown.
   *
   * `view.notice` and `state === "fault"` were both read only by the Hub — which
   * is deliberately not open mid-dictation. So a failed injection, or text
   * diverted to the clipboard, looked from here exactly like nothing having
   * happened. That is the failure mode the "never lose a word" promise cannot
   * survive.
   */
  // Read from the outcome, not from `state === "fault"`.
  //
  // The core emits `Finished` and `Idle` in the same effect batch, so React
  // applies both before it paints and the fault frame never exists. Every ASR
  // and capture failure was therefore completely silent on this window — the
  // one surface that is on screen when it happens. The outcome is what
  // survives the batch, so the outcome is what this reads.
  const failed = view.state === "fault" || FAILED_OUTCOMES.has(view.lastOutcome ?? "");
  // A discard is an outcome the user chose, so it is acknowledged rather than
  // reported: no colour, no six-second dwell, just long enough to confirm the
  // key did something. Without it, pressing Escape looked identical to the
  // dictation having quietly failed.
  const discarded = view.lastOutcome === "cancelled" && !view.notice && !failed;
  const alertText =
    view.notice?.message ??
    (failed ? "Dictation failed — nothing was written" : discarded ? "Discarded" : undefined);
  // Identity of the current alert, so a second one re-arms the timer rather
  // than inheriting the first one's remaining time. The session count is in the
  // key because without it two *identical* consecutive messages — two clipboard
  // fallbacks in a row, say — produced the same key, which the timer had
  // already retired, so the second one never appeared at all.
  const alertKey =
    alertText && !live && !working ? `${view.sessions}:${view.state}:${alertText}` : "";
  const [packedAway, setPackedAway] = useState("");

  useEffect(() => {
    if (!alertKey) return;
    const t = window.setTimeout(() => setPackedAway(alertKey), discarded ? DISCARD_MS : ALERT_MS);
    return () => window.clearTimeout(t);
  }, [alertKey, discarded]);

  const alerting = alertKey !== "" && packedAway !== alertKey;

  /**
   * Slide the window to a resting position.
   *
   * The drag itself is smooth because Windows owns it. The old ending was not:
   * Rust snapped and called `set_position`, so release teleported the bar by up
   * to 28px in a single frame. Rust now returns where the bar belongs and this
   * covers the distance over `SETTLE_MS`, which turns the snap from a glitch
   * into the thing that makes the bar feel magnetic.
   */
  const settleTo = useCallback(async (fx: number, fy: number, tx: number, ty: number) => {
    const dx = tx - fx;
    const dy = ty - fy;
    // Nothing to travel: released away from every snap line.
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) return;

    // Claim this settle. Any settle already running is abandoned at its next
    // frame, and any state change abandons this one — see the layout effect that
    // bumps the token.
    //
    // Without that, this loop wrote a window position sixty times a second for
    // 220ms with nothing able to stop it, against an `overlay_set_box` that
    // writes once. A settle beat every resize it overlapped, and two settles
    // could run at once, each convinced it knew where the bar was.
    const mine = ++settleGen.current;

    const { getCurrentWindow, LogicalPosition } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    if (settleGen.current !== mine) return;

    mark("settle-start", { from: [fx, fy], to: [tx, ty] });
    const t0 = performance.now();
    const step = (now: number) => {
      // Superseded: a newer settle, or a state change that has re-placed the
      // window. Continuing would drag it back off the position the resize just
      // gave it.
      if (settleGen.current !== mine) {
        mark("settle-cancelled", { at: Math.round(now - t0) });
        return;
      }
      const t = Math.min(1, (now - t0) / SETTLE_MS);
      // Exponential out, matching `--ease`: most of the distance early, so it
      // reads as the bar being pulled rather than pushed.
      const e = 1 - Math.pow(1 - t, 3);
      const nx = fx + dx * e;
      const ny = fy + dy * e;
      // Recorded as ours before it is sent, exactly as `flush` does, so the moves
      // this produces are recognised as echoes rather than read back as the user
      // dragging — which is how a settle used to feed itself.
      commanded.current.push([nx, ny]);
      if (commanded.current.length > COMMANDED_HISTORY) commanded.current.shift();
      void win.setPosition(new LogicalPosition(nx, ny));
      // Only at the end, and from the destination rather than from the frame.
      //
      // Writing it every frame from a track computed before the round trip is
      // what left the anchor permanently wrong: any resize landing mid-slide
      // changed the window under a loop that was still interpolating from the
      // old one, and the last frame to run won. The destination is known up
      // front and cannot go stale.
      if (t >= 1) {
        anchor.current = anchorFrom(tx, ty);
        mark("settle-end", { anchor: anchor.current });
      }
      if (t < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }, []);

  // Persist the position after a native drag. Snapping happens in Rust so the
  // rules live in one place rather than being split across the boundary.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    let timer = 0;
    let cueAt = 0;

    (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      un = await win.onMoved(({ payload }) => {
        // Read before anything is awaited.
        //
        // `scaleFactor` is a constant for a monitor, and awaiting it here bought
        // nothing while costing correctness: by the time it resolved, the window
        // could already be a different size, so the position from *this* move was
        // combined with the viewport from the *next* one. That is the same
        // one-value-two-instants mistake ADR 0006 was written about, relocated
        // into an await. The viewport is now sampled in the same turn as the
        // payload it belongs to.
        const scale = scaleRef.current;
        const lx = payload.x / scale;
        const ly = payload.y / scale;

        // Our own move coming back, not the user's hand.
        //
        // This is the gate that matters, and it replaces a heuristic with a fact.
        // Every position this side commands is recorded before it is sent, so a
        // report that matches one is provably ours. The old filter was the
        // "dragging" flag alone — a guess about a Windows move loop that eats its
        // own mouse-up — and when it guessed wrong, the resize that follows every
        // hotkey press was committed as a drag: snapped against whatever size the
        // bar happened to be, written to disk as the user's chosen position, and
        // then animated there by `settleTo`. Up and to the left, for as long as
        // the slide took, and permanently wrong afterwards.
        if (isEcho(commanded.current, lx, ly)) {
          mark("moved-echo", { x: lx, y: ly });
          return;
        }

        // Expired as well as checked. A drag produces a continuous stream of
        // moves, so a genuine one keeps refreshing the stamp; a move arriving
        // long after the last real movement is not part of a drag, however the
        // flag was left.
        if (!dragging.current || performance.now() - dragging.current > DRAG_TTL) return;
        dragging.current = performance.now();

        // The user is moving the bar, so the anchor follows it. Without this the
        // next resize would size around where the bar used to be.
        anchor.current = anchorFrom(lx, ly);
        mark("dragged", { x: lx, y: ly, anchor: anchor.current });

        // While the drag is running, ask Rust — the one place that owns the snap
        // rules — whether releasing here would move the bar, and say so on the
        // pill. Throttled, because onMoved fires far faster than anyone can read.
        const now = performance.now();
        if (now - cueAt > 90) {
          cueAt = now;
          void (async () => {
            const to = (await call("overlay_snap_preview", { x: lx, y: ly })) as
              | [number, number]
              | undefined;
            if (!to) return;
            setSnapping(Math.abs(to[0] - lx) > 0.5 || Math.abs(to[1] - ly) > 0.5);
          })();
        }

        // Debounced: onMoved fires continuously through a drag, and only the
        // resting place is worth committing. Shorter than it was — the settle
        // below is now visible, so any wait in front of it reads as lag.
        window.clearTimeout(timer);
        timer = window.setTimeout(async () => {
          // Cleared before the commit, not after: the settle moves the window
          // too, and those events must not be read as more dragging.
          dragging.current = 0;
          setSnapping(false);
          const to = (await call("overlay_move", { x: lx, y: ly })) as
            | [number, number]
            | undefined;
          // `lx, ly` is where the drag actually ended, captured in the same turn
          // as the move that reported it. Passing it to `settleTo` as the origin
          // removes the three awaits that used to stand between the commit and
          // reading the window's position back — during which a resize could land
          // and leave the slide travelling from somewhere the bar no longer was.
          if (to) void settleTo(lx, ly, to[0], to[1]);
        }, 120);
      });
    })();

    return () => {
      window.clearTimeout(timer);
      un?.();
    };
  }, [settleTo]);

  const startDrag = useCallback(async (e: React.MouseEvent) => {
    if (e.button !== 0 || !inTauri()) return;
    setMenu(false);
    dragging.current = performance.now();
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().startDragging();
  }, []);

  // The window is sized to exactly what is painted, and resized whenever that
  // changes.
  //
  // Two reasons, and both were bugs before:
  //
  //  1. Any window area not covered by the pill shows as a translucent rectangle.
  //     Webview transparency on Windows is unreliable, so rather than depending on
  //     it, there is simply no spare area to reveal.
  //  2. A transparent window still swallows OS-level clicks across its whole
  //     rectangle — `pointer-events: none` governs the webview, not the window. An
  //     oversized window would punch a dead zone into whatever is underneath.
  //
  // The idle case, 150x40, is also what `tauri.conf.json` declares the window at.
  // They have to agree: the window exists before this code does, Rust shows and
  // auto-places it from `overlay.rs` without waiting to be told a size, and when
  // the two disagreed the bar was briefly a rectangle and then moved.
  //
  // These are the *pill's* tiers. The window adds the glow margin on top; see
  // `useWindowBox`.
  //
  // Idle and alert are measured from their own content rather than fixed. The
  // fixed 150px idle tier fit exactly one shortcut — the default — and any
  // remap collided with the word "Hold" and was clipped; the fixed 248px alert
  // tier truncated real engine messages at about thirty characters, which is
  // reliably before the part that says what to do about it.
  const hint = view.ready?.shortcut ?? "Right Ctrl";
  const idleWidth = Math.max(150, Math.ceil(IDLE_CHROME + textWidth(hint, "500 11px $mono")));
  const alertWidth = alertText
    ? Math.min(360, Math.max(200, Math.ceil(MSG_CHROME + textWidth(alertText, "400 12px $sans"))))
    : 248;

  // These are the *window's* target, not the pill's painted size.
  //
  // The pill used to be given this number directly, as an inline width, while
  // the window was told the same number over IPC and got it a round trip later.
  // For that round trip the pill was wider than the window it lives in, and a
  // pill wider than its window is cropped: rounded ends gone, side borders gone,
  // the timer cut mid-glyph — a black rectangle with the bar trapped inside it.
  // The pill now reads its width off the window instead (`.overlay-hit` in
  // overlay.css), so it can be briefly the previous size but never the wrong
  // shape, and this stays what it always meant: where the window is going.
  const pillWidth = menu
    ? 280
    : alerting
      ? alertWidth
      : live
        ? 240
        : working
          ? 170
          : idleWidth;
  const pillHeight = menu ? 226 : PILL_H;

  // Never both: the menu already claims a much larger box for its own purposes,
  // and stacking the glow margin on top would overshoot it. The menu also closes
  // itself the moment a session starts (see below), so this is belt and braces.
  const glowing = live && !menu;
  const margin = glowing ? GLOW_MARGIN : 0;

  useWindowGeometry(pillWidth, pillHeight, margin, anchor, box, geomReady, commanded);

  // Measure what was actually painted against the window it was painted in.
  //
  // Silent unless the two disagree, and the two disagreeing is the entire bug:
  // it is the difference between "the bar looks wrong" and a line in the console
  // saying the pill is 240px wide in a 150px window. Runs after layout on every
  // pass rather than being gated behind a debug flag, because the frames this
  // catches are the ones nobody is watching for. See overlay-trace.ts.
  useLayoutEffect(() => {
    if (!inTauri()) return;
    checkInvariants(hit.current, margin);
  });

  useEffect(() => watchViewport(), []);

  // Keep the cached scale honest.
  //
  // Reading it synchronously is what removed the await that used to let a
  // position from one instant meet a viewport from another — but a display scale
  // is only constant for one monitor. Dragging the bar to a screen at a different
  // DPI, or changing scaling while the app runs, would otherwise leave every
  // conversion wrong for the rest of the session. Tauri reports the change; the
  // read stays synchronous and the cache is refreshed out of band.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    void (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      un = await getCurrentWindow().onScaleChanged(({ payload }) => {
        scaleRef.current = payload.scaleFactor;
        mark("scale-changed", { scale: payload.scaleFactor });
      });
    })();
    return () => un?.();
  }, []);

  // A state change re-places the window, so any settle still interpolating
  // towards an older destination is abandoned. Without this the slide and the
  // resize fight, sixty writes a second against one, and the slide wins.
  useLayoutEffect(() => {
    settleGen.current += 1;
  }, [pillWidth, pillHeight, margin]);


  // Close the menu when a session starts.
  //
  // `blur` cannot do this job here, however much it looks like it should: this
  // window never takes focus (`WS_EX_NOACTIVATE`), so it never loses it either,
  // and the listener below only ever fires in the component sheet, where the same
  // component runs in an ordinary browser window. Without the state check, right-
  // clicking the bar and then dictating left the 226px menu panel open behind the
  // pill — an opaque rectangle around a window whose entire job is to be a pill,
  // and one the user cannot dismiss by clicking away from, because there is
  // nowhere to click that this window can see.
  useEffect(() => {
    if (!menu) return;
    if (live || working) {
      setMenu(false);
      return;
    }
    const close = () => setMenu(false);
    window.addEventListener("blur", close);
    return () => window.removeEventListener("blur", close);
  }, [menu, live, working]);

  // State changes are announced. A person using a screen reader gets no benefit
  // from a waveform, and the whole point of this window is knowing whether the
  // microphone is open. Lost in an earlier rewrite; restored here.
  const spoken = live
    ? "Listening. Press Escape to discard."
    : working
      ? "Writing your words"
      : alerting
        ? alertText
        : "Ready. Hold the shortcut to dictate.";

  return (
    // `data-fit` is what turns on reading the pill's size off the window. It is
    // true only in the real overlay window: the component sheet renders this same
    // component in an ordinary browser window, where the viewport is the whole
    // page and a pill sized to it would be absurd.
    <div className="overlay-root" data-menu={menu} data-fit={inTauri()}>
      {/* `assertive` when something went wrong: a failure announced politely
          waits for the screen reader to finish whatever it was saying, which
          for a message about text that did not land is too late to be useful. */}
      <span className="sr-only" role="status" aria-live={failed ? "assertive" : "polite"}>
        {spoken}
      </span>
      <div
        className="overlay-hit"
        ref={hit}
        data-snapping={snapping}
        // Not a width. A fallback for the browser preview, where there is no
        // window to read a size off; in the real overlay window the rule keyed on
        // `data-fit` outranks it and this is never consulted. Setting it as a
        // custom property rather than a width is what keeps that true: an inline
        // `width` would beat every stylesheet rule and reintroduce exactly the
        // JavaScript-owned pill size this change exists to remove.
        style={{ "--pill-w": `${pillWidth}px` } as React.CSSProperties}
        onMouseDown={startDrag}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu((m) => !m);
        }}
        title="Drag to move · right-click for options"
      >
        <FlowBar
          live={live}
          levelRef={levelRef}
          elapsed={elapsed(view.elapsedMs)}
          hint={hint}
          working={working}
          failed={failed && alerting}
          message={alerting ? alertText : undefined}
          confirm={confirm}
          publish={hit}
          onCancel={() => void call("cancel_session")}
        />
      </div>

      {menu && (
        <div className="overlay-menu" role="menu">
          <button role="menuitem" onClick={() => { call("show_hub_cmd"); setMenu(false); }}>
            Open OpenVoice
          </button>
          <button role="menuitem" onClick={() => { call("paste_last"); setMenu(false); }}>
            Paste last transcript
          </button>
          <div className="overlay-menu-sep" />
          <button role="menuitem" onClick={() => { call("overlay_snooze", { minutes: 60 }); setMenu(false); }}>
            Hide for an hour
          </button>
          <button role="menuitem" onClick={() => { call("overlay_always_visible", { on: false }); setMenu(false); }}>
            Only show while dictating
          </button>
        </div>
      )}
    </div>
  );
}
